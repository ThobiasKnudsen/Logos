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

const TAB_PAD_H: f32 = 12.0;
const TAB_CLOSE_SIZE: f32 = 20.0;
const TAB_CLOSE_PAD: f32 = 6.0;
const TAB_GAP: f32 = 2.0;
const TAB_DOT_PAD: f32 = 6.0; // horizontal margin around the modified dot
const MENU_ITEM_PAD: f32 = 10.0;

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

    // Editor text buffer (left pane)
    editor_buffer: TextBuffer,
    /// Cursor position within the text buffer (content-relative x, y) + line height.
    cursor_content_pos: (f32, f32, f32),
    /// Horizontal scroll offset for the editor.
    editor_scroll_x: f32,
    /// Vertical scroll offset for the editor.
    editor_scroll_y: f32,
    /// Total content width of the editor (longest line).
    editor_content_width: f32,
    /// Total content height of the editor (all lines).
    editor_content_height: f32,
    /// Cached editor pane rect for scroll calculations.
    cached_editor_pane: Rect,

    // Scrollbar geometry for hit-testing
    h_track_rect: Option<Rect>,
    v_track_rect: Option<Rect>,
    h_thumb_rect: Option<Rect>,
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

        let mut editor_buffer = TextBuffer::new(
            &mut font_system,
            Metrics::new(fonts::editor_size(), fonts::editor_line_height()),
        );

        let physical_height = (physical_size.height as f64 * scale_factor) as f32;
        // No constraints = no wrapping, all lines laid out.
        editor_buffer.set_size(&mut font_system, None, None);
        editor_buffer.shape_until_scroll(&mut font_system, false);

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

        let _ = physical_height; // used only for initial sizing context

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
            editor_buffer,
            cursor_content_pos: (0.0, 0.0, fonts::editor_line_height()),
            editor_scroll_x: 0.0,
            editor_scroll_y: 0.0,
            editor_content_width: 0.0,
            editor_content_height: 0.0,
            cached_editor_pane: zero_rect,
            h_track_rect: None,
            v_track_rect: None,
            h_thumb_rect: None,
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

        self.editor_buffer.set_metrics(
            &mut self.font_system,
            Metrics::new(fonts::editor_size(), fonts::editor_line_height()),
        );
        self.editor_buffer
            .shape_until_scroll(&mut self.font_system, false);

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

    pub fn h_thumb_rect(&self) -> Option<Rect> {
        self.h_thumb_rect
    }

    pub fn v_thumb_rect(&self) -> Option<Rect> {
        self.v_thumb_rect
    }

    /// Scroll the editor view by pixel deltas. Clamps to valid range.
    pub fn scroll_by(&mut self, dx: f32, dy: f32) {
        let pane = self.cached_editor_pane;
        let (visible_w, visible_h, _, _) = self.compute_visible_area(pane);

        self.editor_scroll_x += dx;
        self.editor_scroll_y += dy;

        let max_sx = (self.editor_content_width - visible_w).max(0.0);
        let max_sy = (self.editor_content_height - visible_h).max(0.0);
        self.editor_scroll_x = self.editor_scroll_x.clamp(0.0, max_sx);
        self.editor_scroll_y = self.editor_scroll_y.clamp(0.0, max_sy);

        let has_h = self.h_track_rect.is_some();
        let has_v = self.v_track_rect.is_some();
        self.update_scrollbar_rects(pane, has_h, has_v, visible_w, visible_h);
    }

    /// Set horizontal scroll position from scrollbar thumb drag.
    pub fn set_h_scroll_from_drag(&mut self, mouse_x: f32, drag_offset: f32) {
        if let (Some(track), Some(thumb)) = (self.h_track_rect, self.h_thumb_rect) {
            let pane = self.cached_editor_pane;
            let (visible_w, visible_h, _, _) = self.compute_visible_area(pane);
            let max_scroll = (self.editor_content_width - visible_w).max(0.0);

            let new_thumb_x = mouse_x - drag_offset;
            let range = track.w - thumb.w;
            if range > 0.0 {
                let ratio = ((new_thumb_x - track.x) / range).clamp(0.0, 1.0);
                self.editor_scroll_x = ratio * max_scroll;
            }

            let has_h = true;
            let has_v = self.v_track_rect.is_some();
            self.update_scrollbar_rects(pane, has_h, has_v, visible_w, visible_h);
        }
    }

    /// Set vertical scroll position from scrollbar thumb drag.
    pub fn set_v_scroll_from_drag(&mut self, mouse_y: f32, drag_offset: f32) {
        if let (Some(track), Some(thumb)) = (self.v_track_rect, self.v_thumb_rect) {
            let pane = self.cached_editor_pane;
            let (visible_w, visible_h, _, _) = self.compute_visible_area(pane);
            let max_scroll = (self.editor_content_height - visible_h).max(0.0);

            let new_thumb_y = mouse_y - drag_offset;
            let range = track.h - thumb.h;
            if range > 0.0 {
                let ratio = ((new_thumb_y - track.y) / range).clamp(0.0, 1.0);
                self.editor_scroll_y = ratio * max_scroll;
            }

            let has_h = self.h_track_rect.is_some();
            let has_v = true;
            self.update_scrollbar_rects(pane, has_h, has_v, visible_w, visible_h);
        }
    }

    // ----- Scroll internals -----

    /// Determine visible editor area accounting for scrollbars.
    /// Returns (visible_w, visible_h, needs_h_scrollbar, needs_v_scrollbar).
    fn compute_visible_area(&self, pane: Rect) -> (f32, f32, bool, bool) {
        let pad = spacing::TEXT_PADDING;
        let base_w = pane.w - pad * 2.0;
        let base_h = pane.h - pad * 2.0;

        // First pass
        let need_v = self.editor_content_height > base_h;
        let w1 = if need_v { base_w - spacing::SCROLLBAR_WIDTH } else { base_w };
        let need_h = self.editor_content_width > w1;
        let h1 = if need_h { base_h - spacing::SCROLLBAR_HEIGHT } else { base_h };
        // Second pass: recheck V with reduced height
        let need_v = self.editor_content_height > h1;
        let w2 = if need_v { base_w - spacing::SCROLLBAR_WIDTH } else { base_w };

        (w2.max(0.0), h1.max(0.0), need_h, need_v)
    }

    /// Recompute scrollbar track and thumb rects from current scroll state.
    fn update_scrollbar_rects(
        &mut self,
        pane: Rect,
        has_h: bool,
        has_v: bool,
        visible_w: f32,
        visible_h: f32,
    ) {
        // Horizontal scrollbar
        if has_h && visible_w > 0.0 && self.editor_content_width > 0.0 {
            let sb_h = spacing::SCROLLBAR_HEIGHT;
            let track_w = if has_v { pane.w - spacing::SCROLLBAR_WIDTH } else { pane.w };
            let sb_y = pane.y + pane.h - sb_h;
            self.h_track_rect = Some(Rect { x: pane.x, y: sb_y, w: track_w, h: sb_h });

            let ratio = visible_w / self.editor_content_width;
            let thumb_w = (track_w * ratio).max(spacing::SCROLLBAR_THUMB_MIN_W);
            let max_scroll = (self.editor_content_width - visible_w).max(0.0);
            let thumb_x = if max_scroll > 0.0 {
                pane.x + (self.editor_scroll_x / max_scroll) * (track_w - thumb_w)
            } else {
                pane.x
            };
            self.h_thumb_rect = Some(Rect { x: thumb_x, y: sb_y, w: thumb_w, h: sb_h });
        } else {
            self.h_track_rect = None;
            self.h_thumb_rect = None;
        }

        // Vertical scrollbar
        if has_v && visible_h > 0.0 && self.editor_content_height > 0.0 {
            let sb_w = spacing::SCROLLBAR_WIDTH;
            let track_h = if has_h { pane.h - spacing::SCROLLBAR_HEIGHT } else { pane.h };
            let sb_x = pane.x + pane.w - sb_w;
            self.v_track_rect = Some(Rect { x: sb_x, y: pane.y, w: sb_w, h: track_h });

            let ratio = visible_h / self.editor_content_height;
            let thumb_h = (track_h * ratio).max(spacing::SCROLLBAR_THUMB_MIN_H);
            let max_scroll = (self.editor_content_height - visible_h).max(0.0);
            let thumb_y = if max_scroll > 0.0 {
                pane.y + (self.editor_scroll_y / max_scroll) * (track_h - thumb_h)
            } else {
                pane.y
            };
            self.v_thumb_rect = Some(Rect { x: sb_x, y: thumb_y, w: sb_w, h: thumb_h });
        } else {
            self.v_track_rect = None;
            self.v_thumb_rect = None;
        }
    }

    // ----- Text update -----

    /// Update editor text. No wrapping, no height limit — all content is laid out.
    /// Auto-scrolls both axes to keep cursor visible.
    pub fn update_text(
        &mut self,
        text: &str,
        cursor_byte: usize,
        pane: Rect,
    ) {
        self.cached_editor_pane = pane;

        // No constraints = no wrapping, all lines laid out
        self.editor_buffer.set_size(
            &mut self.font_system,
            None,
            None,
        );
        self.editor_buffer.set_text(
            &mut self.font_system,
            text,
            Attrs::new().family(Family::Monospace),
            Shaping::Advanced,
        );
        self.editor_buffer
            .shape_until_scroll(&mut self.font_system, false);

        // Compute content-relative cursor position
        let (cx, cy, ch) =
            Self::compute_cursor_content_pos(&self.editor_buffer, text, cursor_byte);
        self.cursor_content_pos = (cx, cy, ch);

        // Measure total content dimensions
        self.editor_content_width = Self::measure_content_width(&self.editor_buffer);
        self.editor_content_height = Self::measure_content_height(&self.editor_buffer);

        // Determine scrollbar visibility (affects visible area)
        let (visible_w, visible_h, has_h, has_v) = self.compute_visible_area(pane);

        // Auto-scroll horizontally to keep cursor visible
        if visible_w > 0.0 {
            if cx < self.editor_scroll_x {
                self.editor_scroll_x = cx;
            }
            if cx > self.editor_scroll_x + visible_w {
                self.editor_scroll_x = cx - visible_w;
            }
            let max_sx = (self.editor_content_width - visible_w).max(0.0);
            self.editor_scroll_x = self.editor_scroll_x.clamp(0.0, max_sx);
        }

        // Auto-scroll vertically to keep cursor visible
        if visible_h > 0.0 {
            let cursor_bottom = cy + ch;
            if cy < self.editor_scroll_y {
                self.editor_scroll_y = cy;
            }
            if cursor_bottom > self.editor_scroll_y + visible_h {
                self.editor_scroll_y = cursor_bottom - visible_h;
            }
            let max_sy = (self.editor_content_height - visible_h).max(0.0);
            self.editor_scroll_y = self.editor_scroll_y.clamp(0.0, max_sy);
        }

        // Compute scrollbar rects for hit-testing
        self.update_scrollbar_rects(pane, has_h, has_v, visible_w, visible_h);
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
        let pad = spacing::TEXT_PADDING;
        let lp = layout.left_pane;

        // Compute editor clip bounds (excluding scrollbar areas)
        let editor_right = if self.v_track_rect.is_some() {
            (lp.x + lp.w - spacing::SCROLLBAR_WIDTH) as i32
        } else {
            (lp.x + lp.w) as i32
        };
        let editor_bottom = if self.h_track_rect.is_some() {
            (lp.y + lp.h - spacing::SCROLLBAR_HEIGHT) as i32
        } else {
            (lp.y + lp.h) as i32
        };

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

        // (Modified dot indicators are drawn as text below)

        // Plus button
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

        // Cursor (adjusted for both scroll axes)
        let cursor_screen_x = lp.x + pad + self.cursor_content_pos.0 - self.editor_scroll_x;
        let cursor_screen_y = lp.y + pad + self.cursor_content_pos.1 - self.editor_scroll_y;
        // Only draw cursor if within visible bounds
        if cursor_screen_x >= lp.x as f32
            && cursor_screen_x < editor_right as f32
            && cursor_screen_y + self.cursor_content_pos.2 > lp.y
            && cursor_screen_y < editor_bottom as f32
        {
            ui_rects.push(RectInstance {
                x: cursor_screen_x,
                y: cursor_screen_y,
                w: fonts::CURSOR_WIDTH,
                h: self.cursor_content_pos.2,
                color: colors::CURSOR.to_f32_array(),
            });
        }

        // Scrollbars
        if let Some(track) = self.h_track_rect {
            ui_rects.push(rect_from(track, colors::SCROLLBAR_TRACK));
        }
        if let Some(thumb) = self.h_thumb_rect {
            let color = if matches!(hover, HoverTarget::HScrollThumb) {
                colors::SCROLLBAR_THUMB_HOVER
            } else {
                colors::SCROLLBAR_THUMB
            };
            ui_rects.push(rect_from(thumb, color));
        }
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
        // Corner piece when both scrollbars visible
        if self.h_track_rect.is_some() && self.v_track_rect.is_some() {
            ui_rects.push(RectInstance {
                x: lp.x + lp.w - spacing::SCROLLBAR_WIDTH,
                y: lp.y + lp.h - spacing::SCROLLBAR_HEIGHT,
                w: spacing::SCROLLBAR_WIDTH,
                h: spacing::SCROLLBAR_HEIGHT,
                color: colors::SCROLLBAR_TRACK.to_f32_array(),
            });
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
            // Dropdown border (1px lines)
            let db = self.dropdown_bg;
            ui_rects.push(RectInstance {
                x: db.x, y: db.y, w: db.w, h: 1.0,
                color: colors::DROPDOWN_SEPARATOR.to_f32_array(),
            });
            ui_rects.push(RectInstance {
                x: db.x, y: db.y + db.h - 1.0, w: db.w, h: 1.0,
                color: colors::DROPDOWN_SEPARATOR.to_f32_array(),
            });
            ui_rects.push(RectInstance {
                x: db.x, y: db.y, w: 1.0, h: db.h,
                color: colors::DROPDOWN_SEPARATOR.to_f32_array(),
            });
            ui_rects.push(RectInstance {
                x: db.x + db.w - 1.0, y: db.y, w: 1.0, h: db.h,
                color: colors::DROPDOWN_SEPARATOR.to_f32_array(),
            });

            for (idx, rect) in self.dropdown_item_rects.iter().enumerate() {
                if hover == HoverTarget::DropdownItem(idx) {
                    ui_rects.push(rect_from(*rect, colors::DROPDOWN_HOVER));
                }
            }
        }

        // -- Text areas --
        let mut text_areas: Vec<TextArea> = Vec::new();

        // Editor text (scrolled in both axes)
        // When dropdown is active and overlaps the editor, split into regions
        // around the dropdown so text doesn't render through it.
        let ed_left = lp.x as i32;
        let ed_top = lp.y as i32;
        let editor_text_left = lp.x + pad - self.editor_scroll_x;
        let editor_text_top = lp.y + pad - self.editor_scroll_y;
        let editor_color = colors::TEXT_PRIMARY.to_glyphon();

        let dd_overlaps_editor = self.dropdown_active && {
            let db = self.dropdown_bg;
            (db.y + db.h) as i32 > ed_top
                && (db.x as i32) < editor_right
                && (db.x + db.w) as i32 > ed_left
        };

        if dd_overlaps_editor {
            let db = self.dropdown_bg;
            let dd_left = db.x as i32;
            let dd_right = (db.x + db.w) as i32;
            let dd_bottom = (db.y + db.h) as i32;

            // Region: to the left of dropdown (if dropdown doesn't start at editor left)
            if dd_left > ed_left {
                text_areas.push(TextArea {
                    buffer: &self.editor_buffer,
                    left: editor_text_left,
                    top: editor_text_top,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: ed_left,
                        top: ed_top,
                        right: dd_left,
                        bottom: dd_bottom.min(editor_bottom),
                    },
                    default_color: editor_color,
                    custom_glyphs: &[],
                });
            }
            // Region: to the right of dropdown
            if dd_right < editor_right {
                text_areas.push(TextArea {
                    buffer: &self.editor_buffer,
                    left: editor_text_left,
                    top: editor_text_top,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: dd_right,
                        top: ed_top,
                        right: editor_right,
                        bottom: dd_bottom.min(editor_bottom),
                    },
                    default_color: editor_color,
                    custom_glyphs: &[],
                });
            }
            // Region: full width below dropdown
            if dd_bottom < editor_bottom {
                text_areas.push(TextArea {
                    buffer: &self.editor_buffer,
                    left: editor_text_left,
                    top: editor_text_top,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: ed_left,
                        top: dd_bottom,
                        right: editor_right,
                        bottom: editor_bottom,
                    },
                    default_color: editor_color,
                    custom_glyphs: &[],
                });
            }
        } else {
            // No dropdown overlap — full editor bounds
            text_areas.push(TextArea {
                buffer: &self.editor_buffer,
                left: editor_text_left,
                top: editor_text_top,
                scale: 1.0,
                bounds: TextBounds {
                    left: ed_left,
                    top: ed_top,
                    right: editor_right,
                    bottom: editor_bottom,
                },
                default_color: editor_color,
                custom_glyphs: &[],
            });
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
