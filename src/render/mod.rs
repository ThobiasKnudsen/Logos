pub mod rects;

use std::sync::Arc;

use glyphon::{
    Attrs, Buffer as TextBuffer, Cache, Family, FontSystem, Metrics, Resolution, Shaping,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use wgpu::{
    CommandEncoderDescriptor, DeviceDescriptor, Instance, InstanceDescriptor, LoadOp,
    MultisampleState, Operations, PresentMode, RenderPassColorAttachment, RenderPassDescriptor,
    RequestAdapterOptions, StoreOp, SurfaceConfiguration, TextureUsages, TextureViewDescriptor,
};
use winit::dpi::PhysicalSize;
use winit::window::Window;

use crate::app::{self, HoverTarget, MenuItemDef, WindowControlRects};
use crate::ui::layout::{LayoutResult, Rect};
use crate::ui::theme::{colors, fonts, spacing, Rgba};
use rects::{RectInstance, RectRenderer};

/// Info about a single tab, passed from AppState to the renderer.
pub struct TabInfo {
    pub name: String,
    pub is_active: bool,
    pub is_modified: bool,
}

/// Hit-test rectangles returned by update_tab_bar for mouse handling.
#[derive(Debug, Clone, Copy)]
pub struct TabHitRect {
    pub full: Rect,
    pub close: Rect,
}

/// Layout rects for a single cell, used for hit-testing and rendering.
#[derive(Debug, Clone, Copy)]
pub struct CellLayout {
    pub cell_index: usize,
    pub container: Rect,
    pub header: Rect,
    pub delete_button: Rect,
    pub separator: Rect,
    pub editor: Rect,
}

/// Info about a single cell, passed from AppState to the renderer.
pub struct CellInfo {
    pub text: String,
    pub cursor_byte: usize,
}

const TAB_PAD_H: f32 = 12.0;
const TAB_CLOSE_SIZE: f32 = 20.0;
const TAB_CLOSE_PAD: f32 = 6.0;
const TAB_GAP: f32 = 2.0;
const TAB_DOT_PAD: f32 = 6.0; // horizontal margin around the modified dot
const MENU_ITEM_PAD: f32 = 10.0;
const CELL_HEADER_HEIGHT: f32 = 28.0;
const CELL_DELETE_SIZE: f32 = 22.0;

/// Handles all GPU rendering: wgpu setup, text via glyphon, rects via instanced draw.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: SurfaceConfiguration,

    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    text_renderer: TextRenderer,

    // Multi-cell editor buffers (one glyphon buffer per cell)
    cell_buffers: Vec<TextBuffer>,
    /// Computed cell layouts for hit-testing and rendering.
    cell_layouts: Vec<CellLayout>,
    /// Which cell is currently active (receives keyboard input).
    active_cell_index: usize,
    /// Cursor position within the active cell (content-relative x, y) + line height.
    cursor_content_pos: (f32, f32, f32),
    /// Vertical scroll offset for the cell container.
    cell_scroll_y: f32,
    /// Total height of all cells stacked.
    cells_total_height: f32,
    /// Cached editor pane rect for scroll calculations.
    cached_editor_pane: Rect,
    /// "+" button below cells to add new cell.
    add_cell_label: TextBuffer,
    add_cell_rect: Rect,
    /// Delete button label for cells.
    cell_delete_label: TextBuffer,

    // Scrollbar geometry for hit-testing (vertical only for cell container)
    v_track_rect: Option<Rect>,
    v_thumb_rect: Option<Rect>,

    // UI label buffers
    status_label: TextBuffer,
    graph_label: TextBuffer,

    // Individual menu item labels
    menu_item_labels: Vec<TextBuffer>,
    menu_item_rects: Vec<Rect>,

    // Dropdown state
    dropdown_item_labels: Vec<TextBuffer>,
    dropdown_shortcut_labels: Vec<TextBuffer>,
    dropdown_bg: Rect,
    dropdown_item_rects: Vec<Rect>,
    dropdown_active: bool,

    // Dynamic tab bar
    tab_labels: Vec<TextBuffer>,
    tab_close_labels: Vec<TextBuffer>,
    tab_modified: Vec<bool>,
    dot_label: TextBuffer,
    tab_bg_rects: Vec<(Rect, bool)>,
    tab_close_rects: Vec<Rect>,
    plus_label: TextBuffer,
    plus_rect: Rect,

    // Window control button labels
    win_min_label: TextBuffer,
    win_max_label: TextBuffer,
    win_close_label: TextBuffer,

    // Batched rect renderer
    rect_renderer: RectRenderer,
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Self {
        let physical_size = window.inner_size();
        let scale_factor = window.scale_factor();

        let instance = Instance::new(InstanceDescriptor::default());
        let adapter = instance
            .request_adapter(&RequestAdapterOptions::default())
            .await
            .unwrap();
        let (device, queue) = adapter
            .request_device(&DeviceDescriptor::default(), None)
            .await
            .unwrap();

        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");

        let caps = surface.get_capabilities(&adapter);
        let swapchain_format = caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(caps.formats[0]);
        let surface_config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: swapchain_format,
            width: physical_size.width,
            height: physical_size.height,
            present_mode: PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        let mut font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let cache = Cache::new(&device);
        let viewport = Viewport::new(&device, &cache);
        let mut atlas = TextAtlas::new(&device, &queue, &cache, swapchain_format);
        let text_renderer =
            TextRenderer::new(&mut atlas, &device, MultisampleState::default(), None);

        let _ = (physical_size.height as f64 * scale_factor) as f32;

        // Menu item labels
        let menu_item_labels: Vec<TextBuffer> = app::MENU_NAMES
            .iter()
            .map(|name| Self::create_label(&mut font_system, fonts::menu_size(), name))
            .collect();

        let status_label = Self::create_label(
            &mut font_system,
            fonts::status_size(),
            "Ready \u{2502} Ln 1, Col 1",
        );
        let graph_label = Self::create_label(&mut font_system, fonts::ui_size(), "Graph");
        let plus_label = Self::create_label(&mut font_system, fonts::ui_size(), "+");
        let dot_label = Self::create_label(&mut font_system, fonts::ui_size(), "\u{25CF}");

        let win_min_label = Self::create_label(&mut font_system, fonts::menu_size(), "\u{2500}");
        let win_max_label = Self::create_label(&mut font_system, fonts::menu_size(), "\u{25A1}");
        let win_close_label = Self::create_label(&mut font_system, fonts::menu_size(), "\u{00D7}");

        let rect_renderer = RectRenderer::new(&device, swapchain_format);

        let add_cell_label = Self::create_label(&mut font_system, fonts::ui_size(), "+ Add Cell");
        let cell_delete_label = Self::create_label(&mut font_system, fonts::ui_size(), "\u{2715}");

        let zero_rect = Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 };

        Self {
            device,
            queue,
            surface,
            surface_config,
            font_system,
            swash_cache,
            viewport,
            atlas,
            text_renderer,
            cell_buffers: Vec::new(),
            cell_layouts: Vec::new(),
            active_cell_index: 0,
            cursor_content_pos: (0.0, 0.0, fonts::editor_line_height()),
            cell_scroll_y: 0.0,
            cells_total_height: 0.0,
            cached_editor_pane: zero_rect,
            add_cell_label,
            add_cell_rect: zero_rect,
            cell_delete_label,
            v_track_rect: None,
            v_thumb_rect: None,
            status_label,
            graph_label,
            menu_item_labels,
            menu_item_rects: Vec::new(),
            dropdown_item_labels: Vec::new(),
            dropdown_shortcut_labels: Vec::new(),
            dropdown_bg: zero_rect,
            dropdown_item_rects: Vec::new(),
            dropdown_active: false,
            tab_labels: Vec::new(),
            tab_close_labels: Vec::new(),
            tab_modified: Vec::new(),
            dot_label,
            tab_bg_rects: Vec::new(),
            tab_close_rects: Vec::new(),
            plus_label,
            plus_rect: zero_rect,
            win_min_label,
            win_max_label,
            win_close_label,
            rect_renderer,
        }
    }

    fn create_label(font_system: &mut FontSystem, size: f32, text: &str) -> TextBuffer {
        let mut buf = TextBuffer::new(font_system, Metrics::new(size, size * 1.4));
        buf.set_size(font_system, Some(2000.0), Some(size * 2.0));
        buf.set_text(
            font_system,
            text,
            Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
        );
        buf.shape_until_scroll(font_system, false);
        buf
    }

    fn measure_label_width(buf: &TextBuffer) -> f32 {
        let mut max_x = 0.0_f32;
        for run in buf.layout_runs() {
            if let Some(last) = run.glyphs.last() {
                max_x = max_x.max(last.x + last.w);
            }
        }
        max_x
    }

    pub fn resize(&mut self, new_size: PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }
        self.surface_config.width = new_size.width;
        self.surface_config.height = new_size.height;
        self.surface.configure(&self.device, &self.surface_config);
    }

    /// Recreate all label buffers at current font sizes (after zoom change).
    pub fn rebuild_labels(&mut self) {
        self.menu_item_labels = app::MENU_NAMES
            .iter()
            .map(|name| Self::create_label(&mut self.font_system, fonts::menu_size(), name))
            .collect();
        self.status_label = Self::create_label(&mut self.font_system, fonts::status_size(), "");
        self.graph_label = Self::create_label(&mut self.font_system, fonts::ui_size(), "Graph");
        self.plus_label = Self::create_label(&mut self.font_system, fonts::ui_size(), "+");
        self.dot_label = Self::create_label(&mut self.font_system, fonts::ui_size(), "\u{25CF}");
        self.win_min_label =
            Self::create_label(&mut self.font_system, fonts::menu_size(), "\u{2500}");
        self.win_max_label =
            Self::create_label(&mut self.font_system, fonts::menu_size(), "\u{25A1}");
        self.win_close_label =
            Self::create_label(&mut self.font_system, fonts::menu_size(), "\u{00D7}");

        // Update metrics for all cell buffers
        for buf in &mut self.cell_buffers {
            buf.set_metrics(
                &mut self.font_system,
                Metrics::new(fonts::editor_size(), fonts::editor_line_height()),
            );
            buf.shape_until_scroll(&mut self.font_system, false);
        }

        self.add_cell_label =
            Self::create_label(&mut self.font_system, fonts::ui_size(), "+ Add Cell");
        self.cell_delete_label =
            Self::create_label(&mut self.font_system, fonts::ui_size(), "\u{2715}");

        // Close any open dropdown since label sizes changed
        self.close_dropdown();
    }

    pub fn set_maximized_icon(&mut self, is_maximized: bool) {
        let icon = if is_maximized { "\u{274F}" } else { "\u{25A1}" };
        self.win_max_label.set_text(
            &mut self.font_system,
            icon,
            Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
        );
        self.win_max_label
            .shape_until_scroll(&mut self.font_system, false);
    }

    /// Update menu item positions from title bar rect. Returns hit rects.
    pub fn update_menu_items(&mut self, title_bar: Rect, win_ctrl_start_x: f32) -> Vec<Rect> {
        let mut rects = Vec::with_capacity(app::MENU_NAMES.len());
        let mut x = title_bar.x + spacing::SM;
        let y = title_bar.y;
        let h = title_bar.h;

        for label in &self.menu_item_labels {
            let text_w = Self::measure_label_width(label);
            let item_w = MENU_ITEM_PAD * 2.0 + text_w;
            // Don't extend past window controls
            if x + item_w > win_ctrl_start_x {
                break;
            }
            rects.push(Rect { x, y, w: item_w, h });
            x += item_w;
        }

        self.menu_item_rects = rects.clone();
        rects
    }

    /// Open a dropdown menu. Returns item rects for hit testing.
    pub fn open_dropdown(&mut self, menu_index: usize, menu_rect: Rect) -> Vec<Rect> {
        let items: &[MenuItemDef] = app::menu_items(menu_index);
        if items.is_empty() {
            self.dropdown_active = false;
            return Vec::new();
        }

        let item_h = spacing::dropdown_item_height();
        let pad = spacing::DROPDOWN_PADDING;

        // Create labels and measure widths
        self.dropdown_item_labels.clear();
        self.dropdown_shortcut_labels.clear();
        let mut max_label_w = 0.0_f32;
        let mut max_shortcut_w = 0.0_f32;

        for item in items {
            let label = Self::create_label(&mut self.font_system, fonts::menu_size(), item.label);
            let shortcut =
                Self::create_label(&mut self.font_system, fonts::small_size(), item.shortcut);
            max_label_w = max_label_w.max(Self::measure_label_width(&label));
            max_shortcut_w = max_shortcut_w.max(Self::measure_label_width(&shortcut));
            self.dropdown_item_labels.push(label);
            self.dropdown_shortcut_labels.push(shortcut);
        }

        let dropdown_w = (max_label_w + max_shortcut_w + MENU_ITEM_PAD * 4.0)
            .max(spacing::DROPDOWN_MIN_WIDTH);
        let dropdown_h = items.len() as f32 * item_h + pad * 2.0;

        let x = menu_rect.x;
        let y = menu_rect.y + menu_rect.h;

        self.dropdown_bg = Rect {
            x,
            y,
            w: dropdown_w,
            h: dropdown_h,
        };

        let mut item_rects = Vec::with_capacity(items.len());
        for i in 0..items.len() {
            item_rects.push(Rect {
                x,
                y: y + pad + i as f32 * item_h,
                w: dropdown_w,
                h: item_h,
            });
        }

        self.dropdown_item_rects = item_rects.clone();
        self.dropdown_active = true;
        item_rects
    }

    pub fn close_dropdown(&mut self) {
        self.dropdown_active = false;
        self.dropdown_item_labels.clear();
        self.dropdown_shortcut_labels.clear();
        self.dropdown_item_rects.clear();
    }

    /// Returns the dropdown background rect if a dropdown is active.
    pub fn dropdown_bg_rect(&self) -> Option<Rect> {
        if self.dropdown_active {
            Some(self.dropdown_bg)
        } else {
            None
        }
    }

    // ----- Scroll public API -----

    pub fn v_thumb_rect(&self) -> Option<Rect> {
        self.v_thumb_rect
    }

    /// Scroll the cell container by pixel deltas. Clamps to valid range.
    /// Also repositions cell layouts and scrollbar to match the new scroll.
    pub fn scroll_by(&mut self, _dx: f32, dy: f32) {
        let pane = self.cached_editor_pane;
        let visible_h = pane.h;

        let old_scroll = self.cell_scroll_y;
        self.cell_scroll_y += dy;
        let max_sy = (self.cells_total_height - visible_h).max(0.0);
        self.cell_scroll_y = self.cell_scroll_y.clamp(0.0, max_sy);

        let delta = self.cell_scroll_y - old_scroll;
        if delta.abs() > 0.001 {
            // Shift all cell layouts and add-cell button by the scroll delta
            for cl in &mut self.cell_layouts {
                cl.container.y -= delta;
                cl.header.y -= delta;
                cl.delete_button.y -= delta;
                cl.separator.y -= delta;
                cl.editor.y -= delta;
            }
            self.add_cell_rect.y -= delta;
        }

        self.update_scrollbar_rects(pane);
    }

    /// Set vertical scroll position from scrollbar thumb drag.
    pub fn set_v_scroll_from_drag(&mut self, mouse_y: f32, drag_offset: f32) {
        if let (Some(track), Some(thumb)) = (self.v_track_rect, self.v_thumb_rect) {
            let pane = self.cached_editor_pane;
            let visible_h = pane.h;
            let max_scroll = (self.cells_total_height - visible_h).max(0.0);

            let new_thumb_y = mouse_y - drag_offset;
            let range = track.h - thumb.h;
            if range > 0.0 {
                let ratio = ((new_thumb_y - track.y) / range).clamp(0.0, 1.0);
                self.cell_scroll_y = ratio * max_scroll;
            }

            self.update_scrollbar_rects(pane);
        }
    }

    /// Returns the cell layouts for hit-testing.
    #[allow(dead_code)]
    pub fn cell_layouts(&self) -> &[CellLayout] {
        &self.cell_layouts
    }

    /// Returns the add-cell button rect for hit-testing.
    pub fn add_cell_button_rect(&self) -> Rect {
        self.add_cell_rect
    }

    // ----- Scroll internals -----

    /// Recompute vertical scrollbar from cell container state.
    fn update_scrollbar_rects(&mut self, pane: Rect) {
        let visible_h = pane.h;
        let need_v = self.cells_total_height > visible_h;

        if need_v && visible_h > 0.0 && self.cells_total_height > 0.0 {
            let sb_w = spacing::SCROLLBAR_WIDTH;
            let track_h = pane.h;
            let sb_x = pane.x + pane.w - sb_w;
            self.v_track_rect = Some(Rect { x: sb_x, y: pane.y, w: sb_w, h: track_h });

            let ratio = visible_h / self.cells_total_height;
            let thumb_h = (track_h * ratio).max(spacing::SCROLLBAR_THUMB_MIN_H);
            let max_scroll = (self.cells_total_height - visible_h).max(0.0);
            let thumb_y = if max_scroll > 0.0 {
                pane.y + (self.cell_scroll_y / max_scroll) * (track_h - thumb_h)
            } else {
                pane.y
            };
            self.v_thumb_rect = Some(Rect { x: sb_x, y: thumb_y, w: sb_w, h: thumb_h });
        } else {
            self.v_track_rect = None;
            self.v_thumb_rect = None;
        }
    }

    // ----- Cell update -----

    /// Update all cell buffers and compute cell layouts.
    /// Returns the computed cell layouts for hit-testing.
    pub fn update_cells(
        &mut self,
        cells: &[CellInfo],
        active_cell_index: usize,
        pane: Rect,
    ) -> Vec<CellLayout> {
        self.cached_editor_pane = pane;
        self.active_cell_index = active_cell_index;

        // Sync cell_buffers count with cells count
        while self.cell_buffers.len() < cells.len() {
            let buf = TextBuffer::new(
                &mut self.font_system,
                Metrics::new(fonts::editor_size(), fonts::editor_line_height()),
            );
            self.cell_buffers.push(buf);
        }
        self.cell_buffers.truncate(cells.len());

        // Set text + shape each buffer, measure heights
        let cell_pad = spacing::CELL_PADDING;
        let cell_spacing = spacing::CELL_SPACING;
        let header_h = CELL_HEADER_HEIGHT;
        let sep_h = 1.0;
        let text_pad = spacing::SM;
        let container_pad = spacing::SM; // internal padding within cell container

        let cell_area_width = pane.w - cell_pad * 2.0;
        // Account for scrollbar width
        let effective_width = if self.v_track_rect.is_some() {
            cell_area_width - spacing::SCROLLBAR_WIDTH
        } else {
            cell_area_width
        };

        let mut layouts = Vec::with_capacity(cells.len());
        let mut y_offset = cell_pad; // accumulates from top of cell container

        for (i, cell_info) in cells.iter().enumerate() {
            // Set text
            self.cell_buffers[i].set_size(&mut self.font_system, None, None);
            self.cell_buffers[i].set_text(
                &mut self.font_system,
                &cell_info.text,
                Attrs::new().family(Family::Monospace),
                Shaping::Advanced,
            );
            self.cell_buffers[i].shape_until_scroll(&mut self.font_system, false);

            // Measure content height
            let content_h = Self::measure_content_height(&self.cell_buffers[i])
                .max(fonts::editor_line_height());
            let editor_h = content_h + text_pad * 2.0;

            let container_h = container_pad + header_h + sep_h + editor_h + container_pad;

            let screen_y = pane.y + y_offset - self.cell_scroll_y;

            let container = Rect {
                x: pane.x + cell_pad,
                y: screen_y,
                w: effective_width,
                h: container_h,
            };
            let header = Rect {
                x: container.x + container_pad,
                y: container.y + container_pad,
                w: effective_width - container_pad * 2.0,
                h: header_h,
            };
            let delete_button = Rect {
                x: header.x + header.w - CELL_DELETE_SIZE,
                y: header.y + (header_h - CELL_DELETE_SIZE) / 2.0,
                w: CELL_DELETE_SIZE,
                h: CELL_DELETE_SIZE,
            };
            let separator = Rect {
                x: container.x + container_pad,
                y: header.y + header_h,
                w: effective_width - container_pad * 2.0,
                h: sep_h,
            };
            let editor = Rect {
                x: container.x + container_pad,
                y: separator.y + sep_h,
                w: effective_width - container_pad * 2.0,
                h: editor_h,
            };

            layouts.push(CellLayout {
                cell_index: i,
                container,
                header,
                delete_button,
                separator,
                editor,
            });

            y_offset += container_h + cell_spacing;
        }

        // Add cell button
        let add_btn_h = CELL_HEADER_HEIGHT;
        let add_btn_w = Self::measure_label_width(&self.add_cell_label) + spacing::MD * 2.0;
        let add_btn_x = pane.x + cell_pad;
        let add_btn_y = pane.y + y_offset - self.cell_scroll_y;
        self.add_cell_rect = Rect {
            x: add_btn_x,
            y: add_btn_y,
            w: add_btn_w,
            h: add_btn_h,
        };

        y_offset += add_btn_h + cell_pad;
        self.cells_total_height = y_offset;

        // Compute cursor position for active cell
        if active_cell_index < cells.len() {
            let (cx, cy, ch) = Self::compute_cursor_content_pos(
                &self.cell_buffers[active_cell_index],
                &cells[active_cell_index].text,
                cells[active_cell_index].cursor_byte,
            );
            self.cursor_content_pos = (cx, cy, ch);

            // Auto-scroll to keep active cell cursor visible
            if let Some(active_layout) = layouts.get(active_cell_index) {
                let cursor_screen_y = active_layout.editor.y + text_pad + cy;
                let cursor_bottom = cursor_screen_y + ch;

                let old_scroll = self.cell_scroll_y;
                if cursor_screen_y < pane.y {
                    self.cell_scroll_y += cursor_screen_y - pane.y;
                } else if cursor_bottom > pane.y + pane.h {
                    self.cell_scroll_y += cursor_bottom - (pane.y + pane.h);
                }
                let max_sy = (self.cells_total_height - pane.h).max(0.0);
                self.cell_scroll_y = self.cell_scroll_y.clamp(0.0, max_sy);

                // If scroll changed, shift all layouts by the delta
                let scroll_delta = self.cell_scroll_y - old_scroll;
                if scroll_delta.abs() > 0.001 {
                    for layout in layouts.iter_mut() {
                        layout.container.y -= scroll_delta;
                        layout.header.y -= scroll_delta;
                        layout.delete_button.y -= scroll_delta;
                        layout.separator.y -= scroll_delta;
                        layout.editor.y -= scroll_delta;
                    }
                    self.add_cell_rect.y -= scroll_delta;
                }
            }
        }

        // Update scrollbar
        self.update_scrollbar_rects(pane);

        self.cell_layouts = layouts.clone();
        layouts
    }

    /// Returns (content_x, content_y, line_height) relative to buffer origin.
    fn compute_cursor_content_pos(
        text_buffer: &TextBuffer,
        text: &str,
        cursor_byte: usize,
    ) -> (f32, f32, f32) {
        let clamped = cursor_byte.min(text.len());
        let before = &text[..clamped];
        let line_idx = before.matches('\n').count();
        let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let col_byte = clamped - line_start;

        for run in text_buffer.layout_runs() {
            if run.line_i == line_idx {
                for glyph in run.glyphs.iter() {
                    if glyph.start >= col_byte {
                        return (glyph.x, run.line_top, run.line_height);
                    }
                }
                let x = run
                    .glyphs
                    .last()
                    .map(|g| g.x + g.w)
                    .unwrap_or(0.0);
                return (x, run.line_top, run.line_height);
            }
        }

        let mut last_top = 0.0_f32;
        let mut last_height = fonts::editor_line_height();
        let mut last_line_i = 0;
        for run in text_buffer.layout_runs() {
            last_top = run.line_top;
            last_height = run.line_height;
            last_line_i = run.line_i;
        }
        let extra = (line_idx.saturating_sub(last_line_i)) as f32;
        (0.0, last_top + last_height * extra, last_height)
    }

    #[allow(dead_code)]
    fn measure_content_width(buf: &TextBuffer) -> f32 {
        let mut max_w = 0.0_f32;
        for run in buf.layout_runs() {
            if let Some(last) = run.glyphs.last() {
                max_w = max_w.max(last.x + last.w);
            }
        }
        max_w
    }

    fn measure_content_height(buf: &TextBuffer) -> f32 {
        let mut max_bottom = 0.0_f32;
        for run in buf.layout_runs() {
            max_bottom = max_bottom.max(run.line_top + run.line_height);
        }
        max_bottom
    }

    pub fn update_status(&mut self, text: &str) {
        self.status_label.set_text(
            &mut self.font_system,
            text,
            Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
        );
        self.status_label
            .shape_until_scroll(&mut self.font_system, false);
    }

    pub fn update_tab_bar(
        &mut self,
        tabs: &[TabInfo],
        tab_bar_rect: Rect,
    ) -> (Vec<TabHitRect>, Rect) {
        self.tab_labels.clear();
        self.tab_close_labels.clear();
        self.tab_modified.clear();
        self.tab_bg_rects.clear();
        self.tab_close_rects.clear();

        let tab_h = tab_bar_rect.h;
        let mut x = tab_bar_rect.x + TAB_GAP;
        let y = tab_bar_rect.y;
        let mut hit_rects = Vec::with_capacity(tabs.len());

        let dot_w = Self::measure_label_width(&self.dot_label);
        let dot_area = TAB_DOT_PAD + dot_w + TAB_DOT_PAD;

        for tab in tabs {
            let label = Self::create_label(&mut self.font_system, fonts::ui_size(), &tab.name);
            let text_w = Self::measure_label_width(&label);
            let close_label =
                Self::create_label(&mut self.font_system, fonts::ui_size(), "\u{2715}");
            let left_pad = if tab.is_modified { dot_area } else { TAB_PAD_H };
            let tab_w = left_pad + text_w + TAB_CLOSE_PAD + TAB_CLOSE_SIZE + TAB_PAD_H;
            let tab_rect = Rect { x, y, w: tab_w, h: tab_h };
            let close_rect = Rect {
                x: x + tab_w - TAB_PAD_H - TAB_CLOSE_SIZE,
                y: y + (tab_h - TAB_CLOSE_SIZE) / 2.0,
                w: TAB_CLOSE_SIZE,
                h: TAB_CLOSE_SIZE,
            };

            self.tab_bg_rects.push((tab_rect, tab.is_active));
            self.tab_close_rects.push(close_rect);
            self.tab_labels.push(label);
            self.tab_close_labels.push(close_label);
            self.tab_modified.push(tab.is_modified);
            hit_rects.push(TabHitRect { full: tab_rect, close: close_rect });
            x += tab_w + TAB_GAP;
        }

        let plus_w = TAB_PAD_H * 2.0 + 10.0;
        self.plus_rect = Rect { x, y, w: plus_w, h: tab_h };
        (hit_rects, self.plus_rect)
    }

    pub fn render(
        &mut self,
        layout: &LayoutResult,
        hover: HoverTarget,
        win_controls: &WindowControlRects,
        is_dragging_split: bool,
        open_menu: Option<usize>,
    ) {
        self.viewport.update(
            &self.queue,
            Resolution {
                width: self.surface_config.width,
                height: self.surface_config.height,
            },
        );

        let sw = self.surface_config.width as f32;
        let sh = self.surface_config.height as f32;
        let lp = layout.left_pane;
        let text_pad = spacing::SM;

        // Pane clip bounds
        let pane_left = lp.x as i32;
        let pane_top = lp.y as i32;
        let pane_right = if self.v_track_rect.is_some() {
            (lp.x + lp.w - spacing::SCROLLBAR_WIDTH) as i32
        } else {
            (lp.x + lp.w) as i32
        };
        let pane_bottom = (lp.y + lp.h) as i32;

        // -- Background rects --
        let mut ui_rects = vec![
            rect_from(layout.title_bar, colors::BG_SECONDARY),
            rect_from(layout.tab_bar, colors::TAB_INACTIVE),
        ];

        // Menu item hover backgrounds
        for (idx, rect) in self.menu_item_rects.iter().enumerate() {
            let is_open = open_menu == Some(idx);
            let is_hovered = hover == HoverTarget::MenuItem(idx);
            if is_open || is_hovered {
                ui_rects.push(rect_from(*rect, colors::MENU_ITEM_HOVER));
            }
        }

        // Per-tab backgrounds (hover-aware)
        for (idx, (rect, is_active)) in self.tab_bg_rects.iter().enumerate() {
            let color = if *is_active {
                colors::TAB_ACTIVE
            } else if matches!(hover, HoverTarget::Tab(i) | HoverTarget::TabClose(i) if i == idx) {
                colors::TAB_HOVER
            } else {
                colors::TAB_INACTIVE
            };
            ui_rects.push(rect_from(*rect, color));
        }

        // Tab close hover bg
        for (idx, close_rect) in self.tab_close_rects.iter().enumerate() {
            if hover == HoverTarget::TabClose(idx) {
                ui_rects.push(rect_from(*close_rect, colors::BG_HOVER));
            }
        }

        // Plus button (tab bar)
        let plus_color = if hover == HoverTarget::PlusButton {
            colors::TAB_HOVER
        } else {
            colors::TAB_INACTIVE
        };
        ui_rects.push(rect_from(self.plus_rect, plus_color));

        // Split handle
        let split_color = if is_dragging_split || hover == HoverTarget::SplitHandle {
            colors::SPLIT_HANDLE_HOVER
        } else {
            colors::SPLIT_HANDLE
        };
        ui_rects.push(rect_from(layout.split_handle, split_color));

        // Main panes
        ui_rects.push(rect_from(layout.left_pane, colors::EDITOR_BG));
        ui_rects.push(rect_from(layout.right_pane, colors::GRAPH_BG));
        ui_rects.push(rect_from(layout.status_bar, colors::BG_SECONDARY));

        // --- Cell container rects ---
        for (i, cl) in self.cell_layouts.iter().enumerate() {
            // Skip cells fully outside the visible pane
            if cl.container.y + cl.container.h < lp.y || cl.container.y > lp.y + lp.h {
                continue;
            }

            // Cell border (1px larger rect behind the cell bg)
            let border_color = if i == self.active_cell_index {
                colors::BORDER_FOCUS
            } else {
                colors::BORDER
            };
            let cell_radius = 12.0 * fonts::scale();
            ui_rects.push(rect_rounded(
                Rect {
                    x: cl.container.x - 1.0,
                    y: cl.container.y - 1.0,
                    w: cl.container.w + 2.0,
                    h: cl.container.h + 2.0,
                },
                border_color,
                cell_radius + 1.0,
            ));

            // Cell container background
            ui_rects.push(rect_rounded(cl.container, colors::BG_ELEVATED, cell_radius));

            // Header background (slightly different shade for active)
            if i == self.active_cell_index {
                ui_rects.push(rect_from(cl.header, colors::BG_SECONDARY));
            }

            // Delete button hover
            if hover == HoverTarget::CellDeleteButton(i) {
                ui_rects.push(rect_from(cl.delete_button, colors::BG_HOVER));
            }

            // Separator line
            ui_rects.push(rect_from(cl.separator, colors::BORDER));
        }

        // Cursor in active cell
        if let Some(cl) = self.cell_layouts.get(self.active_cell_index) {
            let cursor_screen_x = cl.editor.x + text_pad + self.cursor_content_pos.0;
            let cursor_screen_y = cl.editor.y + text_pad + self.cursor_content_pos.1;
            let ch = self.cursor_content_pos.2;

            // Only draw if within pane bounds
            if cursor_screen_x >= lp.x
                && cursor_screen_x < pane_right as f32
                && cursor_screen_y + ch > lp.y
                && cursor_screen_y < pane_bottom as f32
            {
                ui_rects.push(RectInstance {
                    x: cursor_screen_x,
                    y: cursor_screen_y,
                    w: fonts::CURSOR_WIDTH,
                    h: ch,
                    color: colors::CURSOR.to_f32_array(),
                    corner_radius: 0.0,
                    _padding: [0.0; 3],
                });
            }
        }

        // Add cell button
        if self.add_cell_rect.y + self.add_cell_rect.h > lp.y
            && self.add_cell_rect.y < lp.y + lp.h
        {
            let add_color = if hover == HoverTarget::AddCellButton {
                colors::BG_HOVER
            } else {
                colors::BG_ELEVATED
            };
            ui_rects.push(rect_rounded(self.add_cell_rect, add_color, 6.0 * fonts::scale()));
        }

        // Scrollbars
        if let Some(track) = self.v_track_rect {
            ui_rects.push(rect_from(track, colors::SCROLLBAR_TRACK));
        }
        if let Some(thumb) = self.v_thumb_rect {
            let color = if matches!(hover, HoverTarget::VScrollThumb) {
                colors::SCROLLBAR_THUMB_HOVER
            } else {
                colors::SCROLLBAR_THUMB
            };
            ui_rects.push(rect_from(thumb, color));
        }

        // Window control hover
        if hover == HoverTarget::WinBtnMinimize {
            ui_rects.push(rect_from(win_controls.minimize, colors::BG_HOVER));
        }
        if hover == HoverTarget::WinBtnMaximize {
            ui_rects.push(rect_from(win_controls.maximize, colors::BG_HOVER));
        }
        if hover == HoverTarget::WinBtnClose {
            ui_rects.push(rect_from(win_controls.close, colors::CLOSE_BUTTON_HOVER));
        }

        // Dropdown background + item hovers (drawn last so they overlay tab bar)
        if self.dropdown_active {
            ui_rects.push(rect_from(self.dropdown_bg, colors::DROPDOWN_BG));
            let db = self.dropdown_bg;
            ui_rects.push(RectInstance {
                x: db.x, y: db.y, w: db.w, h: 1.0,
                color: colors::DROPDOWN_SEPARATOR.to_f32_array(),
                corner_radius: 0.0, _padding: [0.0; 3],
            });
            ui_rects.push(RectInstance {
                x: db.x, y: db.y + db.h - 1.0, w: db.w, h: 1.0,
                color: colors::DROPDOWN_SEPARATOR.to_f32_array(),
                corner_radius: 0.0, _padding: [0.0; 3],
            });
            ui_rects.push(RectInstance {
                x: db.x, y: db.y, w: 1.0, h: db.h,
                color: colors::DROPDOWN_SEPARATOR.to_f32_array(),
                corner_radius: 0.0, _padding: [0.0; 3],
            });
            ui_rects.push(RectInstance {
                x: db.x + db.w - 1.0, y: db.y, w: 1.0, h: db.h,
                color: colors::DROPDOWN_SEPARATOR.to_f32_array(),
                corner_radius: 0.0, _padding: [0.0; 3],
            });

            for (idx, rect) in self.dropdown_item_rects.iter().enumerate() {
                if hover == HoverTarget::DropdownItem(idx) {
                    ui_rects.push(rect_from(*rect, colors::DROPDOWN_HOVER));
                }
            }
        }

        // -- Text areas --
        let mut text_areas: Vec<TextArea> = Vec::new();
        let editor_color = colors::TEXT_PRIMARY.to_glyphon();

        // Cell editor text + delete button labels
        for (i, cl) in self.cell_layouts.iter().enumerate() {
            // Skip cells fully outside the visible pane
            if cl.container.y + cl.container.h < lp.y || cl.container.y > lp.y + lp.h {
                continue;
            }

            if i < self.cell_buffers.len() {
                // Clip text to both the cell editor rect and the pane
                let clip_left = (cl.editor.x as i32).max(pane_left);
                let clip_top = (cl.editor.y as i32).max(pane_top);
                let clip_right = ((cl.editor.x + cl.editor.w) as i32).min(pane_right);
                let clip_bottom = ((cl.editor.y + cl.editor.h) as i32).min(pane_bottom);

                if clip_left < clip_right && clip_top < clip_bottom {
                    text_areas.push(TextArea {
                        buffer: &self.cell_buffers[i],
                        left: cl.editor.x + text_pad,
                        top: cl.editor.y + text_pad,
                        scale: 1.0,
                        bounds: TextBounds {
                            left: clip_left,
                            top: clip_top,
                            right: clip_right,
                            bottom: clip_bottom,
                        },
                        default_color: editor_color,
                        custom_glyphs: &[],
                    });
                }
            }

            // Delete button label (×)
            let del = &cl.delete_button;
            let del_clip_top = (del.y as i32).max(pane_top);
            let del_clip_bottom = ((del.y + del.h) as i32).min(pane_bottom);
            if del_clip_top < del_clip_bottom {
                let label_w = Self::measure_label_width(&self.cell_delete_label);
                let line_h = fonts::ui_size() * 1.4;
                let cx = del.x + (del.w - label_w) / 2.0;
                let cy = del.y + (del.h - line_h) / 2.0;
                text_areas.push(TextArea {
                    buffer: &self.cell_delete_label,
                    left: cx,
                    top: cy,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: del.x as i32,
                        top: del_clip_top,
                        right: (del.x + del.w) as i32,
                        bottom: del_clip_bottom,
                    },
                    default_color: colors::TEXT_MUTED.to_glyphon(),
                    custom_glyphs: &[],
                });
            }
        }

        // Add cell button label
        if self.add_cell_rect.y + self.add_cell_rect.h > lp.y
            && self.add_cell_rect.y < lp.y + lp.h
        {
            let clip_top = (self.add_cell_rect.y as i32).max(pane_top);
            let clip_bottom = ((self.add_cell_rect.y + self.add_cell_rect.h) as i32).min(pane_bottom);
            if clip_top < clip_bottom {
                text_areas.push(TextArea {
                    buffer: &self.add_cell_label,
                    left: self.add_cell_rect.x + spacing::MD,
                    top: self.add_cell_rect.y + spacing::XS,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: self.add_cell_rect.x as i32,
                        top: clip_top,
                        right: (self.add_cell_rect.x + self.add_cell_rect.w) as i32,
                        bottom: clip_bottom,
                    },
                    default_color: colors::TEXT_MUTED.to_glyphon(),
                    custom_glyphs: &[],
                });
            }
        }

        // Menu item labels in title bar
        let menu_right = win_controls.minimize.x;
        for (i, label) in self.menu_item_labels.iter().enumerate() {
            if i >= self.menu_item_rects.len() {
                break;
            }
            let rect = &self.menu_item_rects[i];
            text_areas.push(TextArea {
                buffer: label,
                left: rect.x + MENU_ITEM_PAD,
                top: rect.y + spacing::XS,
                scale: 1.0,
                bounds: TextBounds {
                    left: rect.x as i32,
                    top: rect.y as i32,
                    right: menu_right as i32,
                    bottom: (rect.y + rect.h) as i32,
                },
                default_color: colors::TEXT_PRIMARY.to_glyphon(),
                custom_glyphs: &[],
            });
        }

        // Window control labels
        let win_ctrl_pairs: [(&TextBuffer, &Rect); 3] = [
            (&self.win_min_label, &win_controls.minimize),
            (&self.win_max_label, &win_controls.maximize),
            (&self.win_close_label, &win_controls.close),
        ];
        for (label, rect) in &win_ctrl_pairs {
            let label_w = Self::measure_label_width(label);
            let cx = rect.x + (rect.w - label_w) / 2.0;
            let cy = rect.y + spacing::XS;
            text_areas.push(TextArea {
                buffer: label,
                left: cx,
                top: cy,
                scale: 1.0,
                bounds: TextBounds {
                    left: rect.x as i32,
                    top: rect.y as i32,
                    right: (rect.x + rect.w) as i32,
                    bottom: (rect.y + rect.h) as i32,
                },
                default_color: colors::TEXT_PRIMARY.to_glyphon(),
                custom_glyphs: &[],
            });
        }

        // Tab labels — clip with TextBounds to avoid showing through dropdown
        let tab_bar = layout.tab_bar;
        let dropdown_clip = if self.dropdown_active {
            Some(self.dropdown_bg)
        } else {
            None
        };

        for (i, label) in self.tab_labels.iter().enumerate() {
            let (tab_rect, _) = &self.tab_bg_rects[i];
            let Some(bounds) = clip_bounds_for_dropdown(tab_rect, &tab_bar, dropdown_clip.as_ref()) else {
                continue;
            };

            let is_modified = i < self.tab_modified.len() && self.tab_modified[i];
            let text_left = if is_modified {
                tab_rect.x + TAB_DOT_PAD + Self::measure_label_width(&self.dot_label) + TAB_DOT_PAD
            } else {
                tab_rect.x + TAB_PAD_H
            };
            text_areas.push(TextArea {
                buffer: label,
                left: text_left,
                top: tab_rect.y + spacing::SM,
                scale: 1.0,
                bounds,
                default_color: colors::TEXT_PRIMARY.to_glyphon(),
                custom_glyphs: &[],
            });
        }

        // Modified dot indicators (text-based \u{25CF} for round dot)
        for (i, (tab_rect, _)) in self.tab_bg_rects.iter().enumerate() {
            if i < self.tab_modified.len() && self.tab_modified[i] {
                let Some(bounds) = clip_bounds_for_dropdown(tab_rect, &tab_bar, dropdown_clip.as_ref()) else {
                    continue;
                };

                let dot_x = tab_rect.x + TAB_DOT_PAD;
                // Same baseline as tab text, nudged up slightly because ● sits
                // lower than regular text glyphs in the line box
                let dot_y = tab_rect.y + spacing::SM - 1.0;
                text_areas.push(TextArea {
                    buffer: &self.dot_label,
                    left: dot_x,
                    top: dot_y,
                    scale: 1.0,
                    bounds,
                    default_color: colors::TEXT_PRIMARY.to_glyphon(),
                    custom_glyphs: &[],
                });
            }
        }

        // Tab close labels — centered in close rect
        for (i, close_label) in self.tab_close_labels.iter().enumerate() {
            let close_rect = &self.tab_close_rects[i];
            let Some(mut bounds) = clip_bounds_for_dropdown(close_rect, &tab_bar, dropdown_clip.as_ref()) else {
                continue;
            };
            // Tighten bounds to close rect itself
            bounds.left = bounds.left.max(close_rect.x as i32);
            bounds.right = bounds.right.min((close_rect.x + close_rect.w) as i32);
            bounds.top = close_rect.y as i32;
            bounds.bottom = (close_rect.y + close_rect.h) as i32;

            // Center the × glyph in the close rect
            let label_w = Self::measure_label_width(close_label);
            let line_h = fonts::ui_size() * 1.4;
            let cx = close_rect.x + (close_rect.w - label_w) / 2.0;
            let cy = close_rect.y + (close_rect.h - line_h) / 2.0;

            text_areas.push(TextArea {
                buffer: close_label,
                left: cx,
                top: cy,
                scale: 1.0,
                bounds,
                default_color: colors::TEXT_MUTED.to_glyphon(),
                custom_glyphs: &[],
            });
        }

        // Plus button label
        if let Some(bounds) = clip_bounds_for_dropdown(&self.plus_rect, &tab_bar, dropdown_clip.as_ref()) {
            text_areas.push(TextArea {
                buffer: &self.plus_label,
                left: self.plus_rect.x + TAB_PAD_H,
                top: self.plus_rect.y + spacing::SM,
                scale: 1.0,
                bounds,
                default_color: colors::TEXT_MUTED.to_glyphon(),
                custom_glyphs: &[],
            });
        }

        // Dropdown item labels
        if self.dropdown_active {
            for (i, (label, shortcut)) in self
                .dropdown_item_labels
                .iter()
                .zip(self.dropdown_shortcut_labels.iter())
                .enumerate()
            {
                if i >= self.dropdown_item_rects.len() {
                    break;
                }
                let rect = &self.dropdown_item_rects[i];
                // Item label (left-aligned)
                text_areas.push(TextArea {
                    buffer: label,
                    left: rect.x + MENU_ITEM_PAD,
                    top: rect.y + spacing::XS,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: rect.x as i32,
                        top: rect.y as i32,
                        right: (rect.x + rect.w) as i32,
                        bottom: (rect.y + rect.h) as i32,
                    },
                    default_color: colors::TEXT_PRIMARY.to_glyphon(),
                    custom_glyphs: &[],
                });
                // Shortcut label (right-aligned)
                let shortcut_w = Self::measure_label_width(shortcut);
                text_areas.push(TextArea {
                    buffer: shortcut,
                    left: rect.x + rect.w - MENU_ITEM_PAD - shortcut_w,
                    top: rect.y + spacing::XS,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: rect.x as i32,
                        top: rect.y as i32,
                        right: (rect.x + rect.w) as i32,
                        bottom: (rect.y + rect.h) as i32,
                    },
                    default_color: colors::TEXT_MUTED.to_glyphon(),
                    custom_glyphs: &[],
                });
            }
        }

        // Status label
        text_areas.push(TextArea {
            buffer: &self.status_label,
            left: layout.status_bar.x + spacing::MD,
            top: layout.status_bar.y + spacing::XS,
            scale: 1.0,
            bounds: TextBounds {
                left: layout.status_bar.x as i32,
                top: layout.status_bar.y as i32,
                right: (layout.status_bar.x + layout.status_bar.w) as i32,
                bottom: (layout.status_bar.y + layout.status_bar.h) as i32,
            },
            default_color: colors::TEXT_SECONDARY.to_glyphon(),
            custom_glyphs: &[],
        });

        // Graph placeholder
        text_areas.push(TextArea {
            buffer: &self.graph_label,
            left: layout.right_pane.x + layout.right_pane.w / 2.0 - 20.0,
            top: layout.right_pane.y + layout.right_pane.h / 2.0 - 10.0,
            scale: 1.0,
            bounds: TextBounds {
                left: layout.right_pane.x as i32,
                top: layout.right_pane.y as i32,
                right: (layout.right_pane.x + layout.right_pane.w) as i32,
                bottom: (layout.right_pane.y + layout.right_pane.h) as i32,
            },
            default_color: colors::TEXT_MUTED.to_glyphon(),
            custom_glyphs: &[],
        });

        // -- GPU submit --
        self.text_renderer
            .prepare(
                &self.device,
                &self.queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                text_areas,
                &mut self.swash_cache,
            )
            .unwrap();

        let frame = self.surface.get_current_texture().unwrap();
        let view = frame
            .texture
            .create_view(&TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor { label: None });

        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("main_pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(colors::BG_PRIMARY.to_wgpu()),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            self.rect_renderer
                .draw(&self.queue, &mut pass, sw, sh, &ui_rects);
            self.text_renderer
                .render(&self.atlas, &self.viewport, &mut pass)
                .unwrap();
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        self.atlas.trim();
    }
}

fn rect_from(r: Rect, color: Rgba) -> RectInstance {
    RectInstance {
        x: r.x,
        y: r.y,
        w: r.w,
        h: r.h,
        color: color.to_f32_array(),
        corner_radius: 0.0,
        _padding: [0.0; 3],
    }
}

fn rect_rounded(r: Rect, color: Rgba, radius: f32) -> RectInstance {
    RectInstance {
        x: r.x,
        y: r.y,
        w: r.w,
        h: r.h,
        color: color.to_f32_array(),
        corner_radius: radius,
        _padding: [0.0; 3],
    }
}

fn rects_overlap(a: &Rect, b: &Rect) -> bool {
    a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y
}

/// Compute clipped TextBounds for an element in the tab bar, excluding the dropdown area.
/// Returns None if the element is fully occluded by the dropdown.
fn clip_bounds_for_dropdown(
    elem: &Rect,
    tab_bar: &Rect,
    dropdown: Option<&Rect>,
) -> Option<TextBounds> {
    let mut left = tab_bar.x as i32;
    let mut right = (tab_bar.x + tab_bar.w) as i32;
    let top = tab_bar.y as i32;
    let bottom = (tab_bar.y + tab_bar.h) as i32;

    if let Some(dd) = dropdown {
        if rects_overlap(elem, dd) {
            let dd_left = dd.x;
            let dd_right = dd.x + dd.w;

            // Fully inside dropdown: skip entirely
            if elem.x >= dd_left && elem.x + elem.w <= dd_right {
                return None;
            }

            // Element starts left of dropdown: clip right edge to dropdown left
            if elem.x < dd_left {
                right = right.min(dd_left as i32);
            }

            // Element starts inside dropdown: clip left edge to dropdown right
            if elem.x >= dd_left && elem.x < dd_right {
                left = left.max(dd_right as i32);
            }
        }
    }

    Some(TextBounds { left, top, right, bottom })
}
