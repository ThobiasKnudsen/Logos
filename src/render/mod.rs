pub mod rects;
pub mod shader_pipeline;

use std::sync::Arc;

use glyphon::{
    Attrs, Buffer as TextBuffer, Cache, Color as GlyphonColor, Family, FontSystem, Metrics,
    Resolution, Shaping, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use wgpu::{
    CommandEncoderDescriptor, DeviceDescriptor, Instance, InstanceDescriptor, LoadOp,
    MultisampleState, Operations, PresentMode, RenderPassColorAttachment, RenderPassDescriptor,
    RequestAdapterOptions, StoreOp, SurfaceConfiguration, TextureUsages, TextureViewDescriptor,
};
use winit::dpi::PhysicalSize;
use winit::window::Window;

use crate::app::{self, HoverTarget, MenuItemDef, WindowControlRects};
use crate::editor::autocomplete::CandidateKind;
use crate::ui::layout::{LayoutResult, Rect};
use crate::ui::theme::{self, fonts, spacing, Rgba};
use rects::{RectInstance, RectRenderer};
use shader_pipeline::ShaderPipelineManager;

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
    pub play_button: Rect,
    pub copy_button: Rect,
    pub delete_button: Rect,
    pub separator: Rect,
    pub editor: Rect,
    /// Output area below the editor (for REDUCE results). Zero-sized if no output.
    pub output: Rect,
    /// Full-width separator line between editor and output row. Zero-sized if no output.
    pub output_separator: Rect,
    /// Chevron toggle button (collapse/expand output). Zero-sized if no output.
    pub output_toggle: Rect,
    /// Copy button in the output toolbar row. Zero-sized if no output.
    pub output_copy_button: Rect,
    /// The full output toolbar row rect (toggle + label + copy). For clipping.
    pub output_toolbar: Rect,
}

/// Info about a single cell, passed from AppState to the renderer.
pub struct CellInfo {
    pub text: String,
    pub cursor_byte: usize,
    pub is_playing: bool,
    /// Selection range as (start_byte, end_byte), if any.
    pub selection: Option<(usize, usize)>,
    /// Output text to display below the cell (simplified result, error, etc.)
    pub output_text: Option<String>,
    /// Whether this output is an error (for red coloring).
    pub is_error: bool,
    /// Whether the output area is collapsed (hidden).
    pub output_collapsed: bool,
}

/// Parameters for the render area (right pane) passed from AppState.
pub struct RenderAreaParams {
    pub axis_x_min: f32,
    pub axis_x_max: f32,
    pub axis_y_min: f32,
    pub axis_y_max: f32,
    pub mouse_uv: [f32; 2],
}

const MAX_AXIS_LABELS: usize = 12;

/// Compute a "nice" tick step for a given range and max ticks using the 1-2-5 rule.
fn compute_nice_step(range: f32, max_ticks: usize) -> f32 {
    if range <= f32::EPSILON || max_ticks < 2 {
        return 1.0;
    }
    let rough_step = range / max_ticks as f32;
    let mag = 10.0_f32.powf(rough_step.log10().floor());
    let normalized = rough_step / mag;
    if normalized <= 1.5 {
        mag
    } else if normalized <= 3.5 {
        2.0 * mag
    } else if normalized <= 7.5 {
        5.0 * mag
    } else {
        10.0 * mag
    }
}

/// Generate tick positions for an axis given a fixed step.
fn generate_ticks(axis_min: f32, axis_max: f32, step: f32) -> Vec<f32> {
    if step <= f32::EPSILON {
        return vec![];
    }
    let first = (axis_min / step).ceil() * step;
    let mut ticks = Vec::new();
    let mut v = first;
    while v <= axis_max + step * 0.001 {
        ticks.push(v);
        v += step;
    }
    ticks
}

/// Format a tick value for axis labels.
fn format_tick(v: f32, step: f32) -> String {
    if v.abs() < step * 0.01 {
        return "0".to_string();
    }
    let decimals = if step >= 1.0 {
        0
    } else {
        ((-step.log10()).ceil() as usize).min(6)
    };
    format!("{:.prec$}", v, prec = decimals)
}

// Base values — multiply by fonts::scale() at use-sites via helper fns below.
const BASE_TAB_PAD_H: f32 = 12.0;
const BASE_TAB_CLOSE_SIZE: f32 = 20.0;
const BASE_TAB_CLOSE_PAD: f32 = 6.0;
const BASE_TAB_GAP: f32 = 2.0;
const BASE_TAB_DOT_PAD: f32 = 6.0;
const BASE_MENU_ITEM_PAD: f32 = 10.0;
const BASE_CELL_HEADER_HEIGHT: f32 = 28.0;
const BASE_CELL_DELETE_SIZE: f32 = 22.0;
const BASE_OUTPUT_TOGGLE_HEIGHT: f32 = 20.0;

fn tab_pad_h() -> f32 { BASE_TAB_PAD_H * fonts::scale() }
fn tab_close_size() -> f32 { BASE_TAB_CLOSE_SIZE * fonts::scale() }
fn tab_close_pad() -> f32 { BASE_TAB_CLOSE_PAD * fonts::scale() }
fn tab_gap() -> f32 { BASE_TAB_GAP * fonts::scale() }
fn tab_dot_pad() -> f32 { BASE_TAB_DOT_PAD * fonts::scale() }
fn menu_item_pad() -> f32 { BASE_MENU_ITEM_PAD * fonts::scale() }
fn cell_header_height() -> f32 { BASE_CELL_HEADER_HEIGHT * fonts::scale() }
fn cell_delete_size() -> f32 { BASE_CELL_DELETE_SIZE * fonts::scale() }
fn output_toggle_height() -> f32 { BASE_OUTPUT_TOGGLE_HEIGHT * fonts::scale() }

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
    /// Cached cell text for dirty-checking (skip reshaping unchanged cells).
    cell_texts: Vec<String>,
    /// Output text buffers (one per cell, for REDUCE results).
    cell_output_buffers: Vec<TextBuffer>,
    /// Cached output text for dirty-checking.
    cell_output_texts: Vec<String>,
    /// Whether each cell's output is an error (for red coloring).
    cell_output_is_error: Vec<bool>,
    /// Computed cell layouts for hit-testing and rendering.
    cell_layouts: Vec<CellLayout>,
    /// Which cell is currently active (receives keyboard input).
    active_cell_index: usize,
    /// Cursor position within the active cell (content-relative x, y) + line height.
    cursor_content_pos: (f32, f32, f32),
    /// Selection highlight rectangles for the active cell (content-relative).
    selection_content_rects: Vec<(f32, f32, f32, f32)>,
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
    /// Copy button label for cells.
    cell_copy_label: TextBuffer,
    /// Play button label (▶).
    cell_play_label: TextBuffer,
    /// Stop button label (■).
    cell_stop_label: TextBuffer,
    /// Tooltip label for play/stop button.
    tooltip_label: TextBuffer,
    /// Which cells are currently playing (indexed by cell position in current view).
    cell_playing: Vec<bool>,
    /// Per-cell horizontal scroll offset for output text.
    cell_output_scroll_x: Vec<f32>,
    /// Chevron label for collapsed output (▶).
    cell_chevron_right: TextBuffer,
    /// Chevron label for expanded output (▼).
    cell_chevron_down: TextBuffer,
    /// "Output" label for the output toolbar row.
    output_label: TextBuffer,
    /// Previous active cell + cursor byte, to detect when auto-scroll is needed.
    prev_active_cell: usize,
    prev_cursor_byte: usize,

    // Scrollbar geometry for hit-testing (vertical only for cell container)
    v_track_rect: Option<Rect>,
    v_thumb_rect: Option<Rect>,

    // UI label buffers
    status_label: TextBuffer,
    /// Cached status bar text for dirty-checking.
    cached_status_text: String,

    // Individual menu item labels
    menu_item_labels: Vec<TextBuffer>,
    menu_item_rects: Vec<Rect>,

    // Dropdown state
    dropdown_item_labels: Vec<TextBuffer>,
    dropdown_shortcut_labels: Vec<TextBuffer>,
    dropdown_bg: Rect,
    dropdown_item_rects: Vec<Rect>,
    dropdown_active: bool,
    dropdown_active_item: Option<usize>,

    // Dynamic tab bar (cached to avoid rebuilding on every sync)
    cached_tab_info: Vec<(String, bool, bool)>, // (name, is_active, is_modified)
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

    // Autocomplete popup
    ac_active: bool,
    ac_bg: Rect,
    ac_item_rects: Vec<Rect>,
    ac_item_labels: Vec<TextBuffer>,
    ac_kind_labels: Vec<TextBuffer>,
    ac_selected_index: usize,

    // Batched rect renderer
    rect_renderer: RectRenderer,

    // Shader pipeline for user code rendering
    shader_pipeline: ShaderPipelineManager,

    // Axis overlay resources (separate GPU resources to avoid atlas corruption)
    overlay_rect_renderer: RectRenderer,
    axis_atlas: TextAtlas,
    axis_text_renderer: TextRenderer,
    axis_label_buffers: Vec<TextBuffer>,
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Self {
        let physical_size = window.inner_size();

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
            .find(|f| !f.is_srgb())
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
        let plus_label = Self::create_label(&mut font_system, fonts::ui_size(), "+");
        let dot_label = Self::create_label(&mut font_system, fonts::ui_size(), "\u{25CF}");

        let win_min_label = Self::create_label(&mut font_system, fonts::menu_size(), "\u{2500}");
        let win_max_label = Self::create_label(&mut font_system, fonts::menu_size(), "\u{25A1}");
        let win_close_label = Self::create_label(&mut font_system, fonts::menu_size(), "\u{00D7}");

        let rect_renderer = RectRenderer::new(&device, swapchain_format);
        let shader_pipeline = ShaderPipelineManager::new(&device, swapchain_format);

        // Axis overlay: separate Cache + Atlas + TextRenderer for isolation
        let axis_cache = Cache::new(&device);
        let mut axis_atlas = TextAtlas::new(&device, &queue, &axis_cache, swapchain_format);
        let axis_text_renderer =
            TextRenderer::new(&mut axis_atlas, &device, MultisampleState::default(), None);
        let overlay_rect_renderer = RectRenderer::new(&device, swapchain_format);
        let axis_label_buffers: Vec<TextBuffer> = (0..MAX_AXIS_LABELS * 2)
            .map(|_| Self::create_label(&mut font_system, fonts::small_size(), "0"))
            .collect();

        let add_cell_label = Self::create_label(&mut font_system, fonts::ui_size(), "+");
        let cell_delete_label = Self::create_label(&mut font_system, fonts::ui_size(), "\u{2715}");
        let cell_copy_label = Self::create_label(&mut font_system, fonts::ui_size(), "\u{2398}");
        let cell_play_label = Self::create_label(&mut font_system, fonts::ui_size(), "\u{25B6}");
        let cell_stop_label = Self::create_label(&mut font_system, fonts::ui_size(), "\u{25A0}");
        let cell_chevron_right = Self::create_label(&mut font_system, fonts::small_size(), "\u{25B6}");
        let cell_chevron_down = Self::create_label(&mut font_system, fonts::small_size(), "\u{25BC}");
        let output_label = Self::create_label(&mut font_system, fonts::small_size(), "Output");
        let tooltip_label = Self::create_label(&mut font_system, fonts::small_size(), "Ctrl+Enter");

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
            cell_texts: Vec::new(),
            cell_output_buffers: Vec::new(),
            cell_output_texts: Vec::new(),
            cell_output_is_error: Vec::new(),
            cell_layouts: Vec::new(),
            active_cell_index: 0,
            cursor_content_pos: (0.0, 0.0, fonts::editor_line_height()),
            selection_content_rects: Vec::new(),
            cell_scroll_y: 0.0,
            cells_total_height: 0.0,
            cached_editor_pane: zero_rect,
            add_cell_label,
            add_cell_rect: zero_rect,
            cell_delete_label,
            cell_copy_label,
            cell_play_label,
            cell_stop_label,
            cell_chevron_right,
            cell_chevron_down,
            output_label,
            tooltip_label,
            cell_playing: Vec::new(),
            cell_output_scroll_x: Vec::new(),
            prev_active_cell: usize::MAX,
            prev_cursor_byte: usize::MAX,
            v_track_rect: None,
            v_thumb_rect: None,
            status_label,
            cached_status_text: String::new(),
            menu_item_labels,
            menu_item_rects: Vec::new(),
            dropdown_item_labels: Vec::new(),
            dropdown_shortcut_labels: Vec::new(),
            dropdown_bg: zero_rect,
            dropdown_item_rects: Vec::new(),
            dropdown_active: false,
            dropdown_active_item: None,
            cached_tab_info: Vec::new(),
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
            ac_active: false,
            ac_bg: zero_rect,
            ac_item_rects: Vec::new(),
            ac_item_labels: Vec::new(),
            ac_kind_labels: Vec::new(),
            ac_selected_index: 0,
            rect_renderer,
            shader_pipeline,
            overlay_rect_renderer,
            axis_atlas,
            axis_text_renderer,
            axis_label_buffers,
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
        for buf in &mut self.cell_output_buffers {
            buf.set_metrics(
                &mut self.font_system,
                Metrics::new(fonts::editor_size(), fonts::editor_line_height()),
            );
            buf.shape_until_scroll(&mut self.font_system, false);
        }

        self.add_cell_label =
            Self::create_label(&mut self.font_system, fonts::ui_size(), "+");
        self.cell_delete_label =
            Self::create_label(&mut self.font_system, fonts::ui_size(), "\u{2715}");
        self.cell_copy_label =
            Self::create_label(&mut self.font_system, fonts::ui_size(), "\u{2398}");
        self.cell_play_label =
            Self::create_label(&mut self.font_system, fonts::ui_size(), "\u{25B6}");
        self.cell_stop_label =
            Self::create_label(&mut self.font_system, fonts::ui_size(), "\u{25A0}");
        self.cell_chevron_right =
            Self::create_label(&mut self.font_system, fonts::small_size(), "\u{25B6}");
        self.cell_chevron_down =
            Self::create_label(&mut self.font_system, fonts::small_size(), "\u{25BC}");
        self.output_label =
            Self::create_label(&mut self.font_system, fonts::small_size(), "Output");
        self.tooltip_label =
            Self::create_label(&mut self.font_system, fonts::small_size(), "Ctrl+Enter");

        // Update axis label buffer metrics + size constraint so tick numbers scale with zoom
        let axis_size = fonts::small_size();
        let axis_metrics = Metrics::new(axis_size, axis_size * 1.4);
        for buf in &mut self.axis_label_buffers {
            buf.set_metrics(&mut self.font_system, axis_metrics);
            buf.set_size(&mut self.font_system, Some(2000.0), Some(axis_size * 2.0));
            buf.shape_until_scroll(&mut self.font_system, false);
        }

        // Invalidate caches so tab bar and cells get re-laid-out at new scale
        self.cached_tab_info.clear();
        self.cell_texts.clear();
        self.cell_output_texts.clear();
        self.cell_output_is_error.clear();

        // Close any open dropdown/autocomplete since label sizes changed
        self.close_dropdown();
        self.close_autocomplete();
    }

    /// Invalidate cached cell texts so the next sync forces re-highlighting.
    /// Used when the syntax theme changes without the text itself changing.
    pub fn invalidate_cell_texts(&mut self) {
        self.cell_texts.clear();
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
        let mut x = title_bar.x + spacing::sm();
        let y = title_bar.y;
        let h = title_bar.h;

        for label in &self.menu_item_labels {
            let text_w = Self::measure_label_width(label);
            let item_w = menu_item_pad() * 2.0 + text_w;
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

    /// Open a dropdown menu. `active_item` highlights one item (e.g. current theme).
    /// Returns item rects for hit testing.
    pub fn open_dropdown(
        &mut self,
        menu_index: usize,
        menu_rect: Rect,
        active_item: Option<usize>,
    ) -> Vec<Rect> {
        // Build label/shortcut pairs — theme menu (index 3) is dynamic from JSON
        let static_items: &[MenuItemDef] = app::menu_items(menu_index);
        let is_theme_menu = menu_index == 3;
        let item_count = if is_theme_menu {
            app::theme_menu_count()
        } else {
            static_items.len()
        };
        if item_count == 0 {
            self.dropdown_active = false;
            return Vec::new();
        }

        let item_h = spacing::dropdown_item_height();
        let pad = spacing::dropdown_padding();

        // Create labels and measure widths
        self.dropdown_item_labels.clear();
        self.dropdown_shortcut_labels.clear();
        self.dropdown_active_item = active_item;
        let mut max_label_w = 0.0_f32;
        let mut max_shortcut_w = 0.0_f32;

        for i in 0..item_count {
            let (item_label, item_shortcut) = if is_theme_menu {
                (app::theme_menu_label(i), "")
            } else {
                (static_items[i].label.to_string(), static_items[i].shortcut)
            };
            // Prefix active item with a checkmark
            let label_text = if active_item == Some(i) {
                format!("\u{2713} {}", item_label)
            } else {
                format!("   {}", item_label)
            };
            let label =
                Self::create_label(&mut self.font_system, fonts::menu_size(), &label_text);
            let shortcut =
                Self::create_label(&mut self.font_system, fonts::small_size(), item_shortcut);
            max_label_w = max_label_w.max(Self::measure_label_width(&label));
            max_shortcut_w = max_shortcut_w.max(Self::measure_label_width(&shortcut));
            self.dropdown_item_labels.push(label);
            self.dropdown_shortcut_labels.push(shortcut);
        }

        let dropdown_w = (max_label_w + max_shortcut_w + menu_item_pad() * 4.0)
            .max(spacing::dropdown_min_width());
        let dropdown_h = item_count as f32 * item_h + pad * 2.0;

        let x = menu_rect.x;
        let y = menu_rect.y + menu_rect.h;

        self.dropdown_bg = Rect {
            x,
            y,
            w: dropdown_w,
            h: dropdown_h,
        };

        let mut item_rects = Vec::with_capacity(item_count);
        for i in 0..item_count {
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

    // ----- Autocomplete popup public API -----

    /// Open the autocomplete popup at the given position with candidates.
    /// Returns item rects for hit-testing.
    pub fn open_autocomplete(
        &mut self,
        x: f32,
        y: f32,
        candidates: &[(String, CandidateKind)],
        selected_index: usize,
        pane: Rect,
    ) -> Vec<Rect> {
        self.ac_item_labels.clear();
        self.ac_kind_labels.clear();
        self.ac_item_rects.clear();

        if candidates.is_empty() {
            self.ac_active = false;
            return Vec::new();
        }

        let item_h = spacing::dropdown_item_height();
        let pad = spacing::dropdown_padding();

        let mut max_label_w = 0.0_f32;
        let mut max_kind_w = 0.0_f32;

        for (label, kind) in candidates {
            let lbl = Self::create_label(&mut self.font_system, fonts::editor_size(), label);
            let badge = Self::create_label(&mut self.font_system, fonts::small_size(), kind.badge());
            max_label_w = max_label_w.max(Self::measure_label_width(&lbl));
            max_kind_w = max_kind_w.max(Self::measure_label_width(&badge));
            self.ac_item_labels.push(lbl);
            self.ac_kind_labels.push(badge);
        }

        let popup_w = (max_kind_w + spacing::sm() + max_label_w + menu_item_pad() * 2.0)
            .max(120.0 * fonts::scale());
        let popup_h = candidates.len() as f32 * item_h + pad * 2.0;

        // Position below cursor; flip above if it would exceed pane bottom
        let mut popup_x = x;
        let mut popup_y = y;

        if popup_y + popup_h > pane.y + pane.h {
            // Flip above cursor (subtract one line height + popup height)
            popup_y = y - fonts::editor_line_height() - popup_h;
        }

        // Clamp right edge
        if popup_x + popup_w > pane.x + pane.w {
            popup_x = (pane.x + pane.w - popup_w).max(pane.x);
        }

        self.ac_bg = Rect {
            x: popup_x,
            y: popup_y,
            w: popup_w,
            h: popup_h,
        };

        let mut item_rects = Vec::with_capacity(candidates.len());
        for i in 0..candidates.len() {
            item_rects.push(Rect {
                x: popup_x,
                y: popup_y + pad + i as f32 * item_h,
                w: popup_w,
                h: item_h,
            });
        }

        self.ac_item_rects = item_rects.clone();
        self.ac_selected_index = selected_index;
        self.ac_active = true;
        item_rects
    }

    pub fn close_autocomplete(&mut self) {
        self.ac_active = false;
        self.ac_item_labels.clear();
        self.ac_kind_labels.clear();
        self.ac_item_rects.clear();
    }

    pub fn update_autocomplete_selection(&mut self, index: usize) {
        self.ac_selected_index = index;
    }

    pub fn autocomplete_bg_rect(&self) -> Option<Rect> {
        if self.ac_active {
            Some(self.ac_bg)
        } else {
            None
        }
    }

    /// Returns the cursor position (content-relative x, y, line_height) for the active cell.
    pub fn cursor_content_pos(&self) -> (f32, f32, f32) {
        self.cursor_content_pos
    }

    /// Returns the dropdown background rect if a dropdown is active.
    pub fn dropdown_bg_rect(&self) -> Option<Rect> {
        if self.dropdown_active {
            Some(self.dropdown_bg)
        } else {
            None
        }
    }

    // ----- Shader pipeline public API -----

    pub fn compile_cell_shader(&mut self, cell_id: usize, wgsl_source: &str) -> Result<(), String> {
        self.shader_pipeline.compile_and_add(&self.device, cell_id, wgsl_source)
    }

    pub fn remove_cell_shader(&mut self, cell_id: usize) {
        self.shader_pipeline.remove(cell_id);
    }

    pub fn has_active_shaders(&self) -> bool {
        self.shader_pipeline.has_active()
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
            shift_cell_layouts(&mut self.cell_layouts, &mut self.add_cell_rect, delta);
        }

        self.update_scrollbar_rects(pane);
    }

    /// Scroll a cell's output text horizontally.
    pub fn scroll_output_x(&mut self, cell_index: usize, delta: f32) {
        if cell_index >= self.cell_output_scroll_x.len() {
            return;
        }
        let content_w = Self::measure_output_content_width(&self.cell_output_buffers[cell_index]);
        let visible_w = self
            .cell_layouts
            .iter()
            .find(|cl| cl.cell_index == cell_index)
            .map(|cl| cl.output.w - spacing::sm() * 2.0)
            .unwrap_or(0.0);
        let max_scroll = (content_w - visible_w).max(0.0);
        self.cell_output_scroll_x[cell_index] =
            (self.cell_output_scroll_x[cell_index] + delta).clamp(0.0, max_scroll);
    }

    /// Measure the total content width of an output text buffer.
    fn measure_output_content_width(buf: &TextBuffer) -> f32 {
        let mut max_right = 0.0_f32;
        for run in buf.layout_runs() {
            for glyph in run.glyphs.iter() {
                max_right = max_right.max(glyph.x + glyph.w);
            }
        }
        max_right
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
    pub fn cell_layouts(&self) -> &[CellLayout] {
        &self.cell_layouts
    }

    /// Hit-test a screen position against a cell's text buffer, returning the
    /// byte offset into the cell's text. Returns `None` if the cell index is
    /// out of range or has no layout.
    pub fn hit_test_cell(&self, cell_index: usize, screen_x: f32, screen_y: f32) -> Option<usize> {
        let cl = self.cell_layouts.iter().find(|c| c.cell_index == cell_index)?;
        if cell_index >= self.cell_buffers.len() {
            return None;
        }
        let buf = &self.cell_buffers[cell_index];
        let text_pad = crate::ui::theme::spacing::sm();

        // Convert screen coords to content-relative coords
        let cx = screen_x - cl.editor.x - text_pad;
        let cy = screen_y - cl.editor.y - text_pad;

        // Collect layout runs to find the target line
        let mut best_run = None;
        let mut last_run_line_top = 0.0_f32;
        let mut last_run_line_height = crate::ui::theme::fonts::editor_line_height();

        for run in buf.layout_runs() {
            last_run_line_top = run.line_top;
            last_run_line_height = run.line_height;

            if cy >= run.line_top && cy < run.line_top + run.line_height {
                best_run = Some((run.line_i, run.glyphs.to_vec()));
                break;
            }
            // Track the closest run above cursor
            if run.line_top + run.line_height <= cy {
                best_run = Some((run.line_i, run.glyphs.to_vec()));
            }
        }

        // If click is below all runs, snap to end of last line
        if cy > last_run_line_top + last_run_line_height {
            let text = if cell_index < self.cell_texts.len() {
                &self.cell_texts[cell_index]
            } else {
                return Some(0);
            };
            return Some(text.len());
        }

        // If click is above all runs (cy < 0), return offset 0
        if cy < 0.0 {
            return Some(0);
        }

        let (line_i, glyphs) = best_run?;

        let text = if cell_index < self.cell_texts.len() {
            &self.cell_texts[cell_index]
        } else {
            return Some(0);
        };

        // Compute the byte offset of the start of this line
        let line_start_byte: usize = text
            .split('\n')
            .take(line_i)
            .map(|l| l.len() + 1) // +1 for the '\n'
            .sum();

        if glyphs.is_empty() {
            return Some(line_start_byte);
        }

        // If click is to the left of the first glyph, snap to line start
        if cx <= glyphs[0].x {
            return Some(line_start_byte);
        }

        // Find the glyph whose midpoint is closest to cx
        for glyph in &glyphs {
            let mid = glyph.x + glyph.w / 2.0;
            if cx < mid {
                return Some(line_start_byte + glyph.start);
            }
        }

        // Past the last glyph — snap to end of line
        if let Some(last) = glyphs.last() {
            Some(line_start_byte + last.end)
        } else {
            Some(line_start_byte)
        }
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
            let sb_w = spacing::scrollbar_width();
            let track_h = pane.h;
            let sb_x = pane.x + pane.w - sb_w;
            self.v_track_rect = Some(Rect { x: sb_x, y: pane.y, w: sb_w, h: track_h });

            let ratio = visible_h / self.cells_total_height;
            let thumb_h = (track_h * ratio).max(spacing::scrollbar_thumb_min_h());
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
        self.cell_playing = cells.iter().map(|c| c.is_playing).collect();

        // Sync cell_buffers count with cells count
        while self.cell_buffers.len() < cells.len() {
            let mut buf = TextBuffer::new(
                &mut self.font_system,
                Metrics::new(fonts::editor_size(), fonts::editor_line_height()),
            );
            buf.set_tab_width(&mut self.font_system, 4);
            self.cell_buffers.push(buf);
        }
        self.cell_buffers.truncate(cells.len());
        self.cell_texts.truncate(cells.len());

        // Sync output buffers count
        while self.cell_output_buffers.len() < cells.len() {
            let buf = TextBuffer::new(
                &mut self.font_system,
                Metrics::new(fonts::editor_size(), fonts::editor_line_height()),
            );
            self.cell_output_buffers.push(buf);
        }
        self.cell_output_buffers.truncate(cells.len());
        self.cell_output_texts.truncate(cells.len());
        self.cell_output_is_error.resize(cells.len(), false);

        // Sync output scroll offsets
        while self.cell_output_scroll_x.len() < cells.len() {
            self.cell_output_scroll_x.push(0.0);
        }
        self.cell_output_scroll_x.truncate(cells.len());

        // Set text + shape each buffer, measure heights
        let cell_pad = spacing::cell_padding();
        let cell_spacing = spacing::cell_spacing();
        let header_h = cell_header_height();
        let sep_h = 1.0;
        let text_pad = spacing::sm();
        let container_pad = spacing::sm(); // internal padding within cell container

        let cell_area_width = pane.w - cell_pad * 2.0;
        // Account for scrollbar width
        let effective_width = if self.v_track_rect.is_some() {
            cell_area_width - spacing::scrollbar_width()
        } else {
            cell_area_width
        };

        let mut layouts = Vec::with_capacity(cells.len());
        let mut y_offset = cell_pad; // accumulates from top of cell container

        for (i, cell_info) in cells.iter().enumerate() {
            // Only reshape if text actually changed (cosmic-text shaping is expensive)
            let text_changed = self.cell_texts.get(i).map_or(true, |prev| *prev != cell_info.text);
            if text_changed {
                self.cell_buffers[i].set_size(&mut self.font_system, None, None);

                // Syntax-highlighted rich text
                let spans = crate::lang::highlight::highlight(&cell_info.text);
                let default_attrs = Attrs::new().family(Family::Monospace);
                let rich_spans: Vec<(&str, Attrs)> = spans
                    .iter()
                    .map(|s| {
                        let text_slice = &cell_info.text[s.start..s.end];
                        let attrs = default_attrs.color(GlyphonColor::rgba(
                            s.color.r, s.color.g, s.color.b, s.color.a,
                        ));
                        (text_slice, attrs)
                    })
                    .collect();
                self.cell_buffers[i].set_rich_text(
                    &mut self.font_system,
                    rich_spans,
                    default_attrs,
                    Shaping::Advanced,
                );

                self.cell_buffers[i].shape_until_scroll(&mut self.font_system, false);
                // Update cached text
                if i < self.cell_texts.len() {
                    self.cell_texts[i].clone_from(&cell_info.text);
                } else {
                    self.cell_texts.push(cell_info.text.clone());
                }
            }

            // Measure content height
            let mut content_h = Self::measure_content_height(&self.cell_buffers[i])
                .max(fonts::editor_line_height());
            // Trailing newline creates an empty line that layout_runs() doesn't report
            if cell_info.text.ends_with('\n') {
                content_h += fonts::editor_line_height();
            }
            let editor_h = content_h + text_pad * 2.0;

            // Update output buffer text if changed
            let has_output = cell_info.output_text.is_some();
            let output_text_ref = cell_info.output_text.as_deref().unwrap_or("");
            let output_changed = self
                .cell_output_texts
                .get(i)
                .map_or(true, |prev| prev != output_text_ref);
            self.cell_output_is_error[i] = cell_info.is_error;
            if output_changed {
                if has_output {
                    // No width constraint — no wrapping
                    self.cell_output_buffers[i].set_size(&mut self.font_system, None, None);
                    self.cell_output_buffers[i].set_text(
                        &mut self.font_system,
                        output_text_ref,
                        Attrs::new().family(Family::Monospace),
                        Shaping::Advanced,
                    );
                    self.cell_output_buffers[i]
                        .shape_until_scroll(&mut self.font_system, false);
                }
                if i < self.cell_output_texts.len() {
                    self.cell_output_texts[i] = output_text_ref.to_string();
                } else {
                    self.cell_output_texts.push(output_text_ref.to_string());
                }
                // Reset horizontal scroll when output text changes
                if i < self.cell_output_scroll_x.len() {
                    self.cell_output_scroll_x[i] = 0.0;
                }
            }

            // Output toolbar row height (visible when output exists)
            let output_toggle_h = if has_output { output_toggle_height() } else { 0.0 };

            // Output height: dynamic based on line count when expanded, 0 when collapsed
            let output_h = if has_output && !cell_info.output_collapsed {
                let line_count = output_text_ref.lines().count().max(1).min(10) as f32;
                fonts::editor_line_height() * line_count + text_pad * 2.0
            } else {
                0.0
            };

            // When has_output: sep_h (separator) + xs (margin) + toggle_h (toolbar row) + output_h
            let output_section_h = if has_output {
                sep_h + spacing::xs() + output_toggle_h + output_h
            } else {
                0.0
            };

            let container_h =
                container_pad + header_h + sep_h + editor_h + output_section_h + container_pad;

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
            // Center buttons between cell top edge and separator line
            let btn_y = container.y + (container_pad + header_h - cell_delete_size()) / 2.0;
            let play_button = Rect {
                x: header.x,
                y: btn_y,
                w: cell_delete_size(),
                h: cell_delete_size(),
            };
            let delete_button = Rect {
                x: header.x + header.w - cell_delete_size(),
                y: btn_y,
                w: cell_delete_size(),
                h: cell_delete_size(),
            };
            let copy_button = Rect {
                x: header.x + header.w - cell_delete_size() * 2.0 - spacing::xs(),
                y: btn_y,
                w: cell_delete_size(),
                h: cell_delete_size(),
            };
            // Separator spans full cell width
            let separator = Rect {
                x: container.x,
                y: header.y + header_h,
                w: container.w,
                h: sep_h,
            };
            let editor = Rect {
                x: container.x + container_pad,
                y: separator.y + sep_h,
                w: effective_width - container_pad * 2.0,
                h: editor_h,
            };
            let inner_w = effective_width - container_pad * 2.0;
            let zero_rect = Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 };

            // Output toolbar: separator line + margin + [chevron] [Output] ... [copy btn]
            let (output_separator, output_toggle, output_copy_button, output_toolbar, output) =
                if has_output {
                    let osep_y = editor.y + editor_h;
                    let output_sep = Rect {
                        x: container.x,
                        y: osep_y,
                        w: container.w,
                        h: sep_h,
                    };
                    let margin = spacing::xs();
                    let row_y = osep_y + sep_h + margin;
                    let row_h = output_toggle_h;
                    let inner_x = container.x + container_pad;

                    // Chevron toggle button — small square
                    let toggle_btn = Rect {
                        x: inner_x,
                        y: row_y,
                        w: row_h,
                        h: row_h,
                    };

                    // Copy button — right-aligned in the row
                    let ob_btn_size = cell_delete_size();
                    let copy_btn = Rect {
                        x: inner_x + inner_w - ob_btn_size,
                        y: row_y + (row_h - ob_btn_size) / 2.0,
                        w: ob_btn_size,
                        h: ob_btn_size,
                    };

                    // Full toolbar row rect (for clipping)
                    let toolbar = Rect {
                        x: inner_x,
                        y: row_y,
                        w: inner_w,
                        h: row_h,
                    };

                    let out_y = row_y + row_h;
                    let out = Rect {
                        x: inner_x,
                        y: out_y,
                        w: inner_w,
                        h: output_h,
                    };

                    (output_sep, toggle_btn, copy_btn, toolbar, out)
                } else {
                    (zero_rect, zero_rect, zero_rect, zero_rect, zero_rect)
                };

            layouts.push(CellLayout {
                cell_index: i,
                container,
                header,
                play_button,
                copy_button,
                delete_button,
                separator,
                editor,
                output,
                output_separator,
                output_toggle,
                output_copy_button,
                output_toolbar,
            });

            y_offset += container_h + cell_spacing;
        }

        // Add cell button (round, centered)
        let add_btn_size = 28.0 * fonts::scale();
        let add_btn_x = pane.x + cell_pad + effective_width / 2.0 - add_btn_size / 2.0;
        let add_btn_y = pane.y + y_offset - self.cell_scroll_y;
        self.add_cell_rect = Rect {
            x: add_btn_x,
            y: add_btn_y,
            w: add_btn_size,
            h: add_btn_size,
        };

        y_offset += add_btn_size + cell_pad;
        self.cells_total_height = y_offset;

        // Compute cursor position for active cell
        if active_cell_index < cells.len() {
            let (cx, cy, ch) = Self::compute_cursor_content_pos(
                &self.cell_buffers[active_cell_index],
                &cells[active_cell_index].text,
                cells[active_cell_index].cursor_byte,
            );
            self.cursor_content_pos = (cx, cy, ch);

            // Compute selection highlight rects
            self.selection_content_rects = if let Some((sel_start, sel_end)) =
                cells[active_cell_index].selection
            {
                Self::compute_selection_rects(
                    &self.cell_buffers[active_cell_index],
                    &cells[active_cell_index].text,
                    sel_start,
                    sel_end,
                )
            } else {
                Vec::new()
            };

            // Auto-scroll to keep active cell cursor visible, but only when
            // the cursor or active cell actually changed (e.g. typing,
            // keyboard navigation, switching cells). Without this guard,
            // every sync_active_tab() call (play button, copy, etc.) would
            // force-scroll to the cursor at the bottom of a long cell.
            let cursor_byte = cells[active_cell_index].cursor_byte;
            let cursor_moved = active_cell_index != self.prev_active_cell
                || cursor_byte != self.prev_cursor_byte;
            self.prev_active_cell = active_cell_index;
            self.prev_cursor_byte = cursor_byte;

            if cursor_moved {
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
                        shift_cell_layouts(&mut layouts, &mut self.add_cell_rect, scroll_delta);
                    }
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

    /// Compute content-relative (x, y, w, h) rectangles for a text selection.
    fn compute_selection_rects(
        text_buffer: &TextBuffer,
        text: &str,
        sel_start: usize,
        sel_end: usize,
    ) -> Vec<(f32, f32, f32, f32)> {
        let start = sel_start.min(text.len());
        let end = sel_end.min(text.len());
        if start >= end {
            return Vec::new();
        }

        // Compute line and column-byte for start and end
        let before_start = &text[..start];
        let start_line = before_start.matches('\n').count();
        let start_line_begin = before_start.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let start_col_byte = start - start_line_begin;

        let before_end = &text[..end];
        let end_line = before_end.matches('\n').count();
        let end_line_begin = before_end.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let end_col_byte = end - end_line_begin;

        let mut rects = Vec::new();
        let min_sel_width = 6.0; // minimum width for empty-looking lines

        for run in text_buffer.layout_runs() {
            if run.line_i < start_line || run.line_i > end_line {
                continue;
            }

            // Determine the byte-column range to highlight on this line
            let col_start = if run.line_i == start_line {
                start_col_byte
            } else {
                0
            };
            let col_end = if run.line_i == end_line {
                end_col_byte
            } else {
                usize::MAX // entire line
            };

            if col_start == col_end && run.line_i == start_line && run.line_i == end_line {
                continue;
            }

            // Find x position of col_start
            let x_start = if col_start == 0 {
                0.0
            } else {
                let mut x = run
                    .glyphs
                    .last()
                    .map(|g| g.x + g.w)
                    .unwrap_or(0.0);
                for glyph in run.glyphs.iter() {
                    if glyph.start >= col_start {
                        x = glyph.x;
                        break;
                    }
                }
                x
            };

            // Find x position of col_end
            let x_end = if col_end == usize::MAX {
                run.glyphs
                    .last()
                    .map(|g| g.x + g.w)
                    .unwrap_or(0.0)
                    + min_sel_width // extend past line end for visibility
            } else {
                let mut x = run
                    .glyphs
                    .last()
                    .map(|g| g.x + g.w)
                    .unwrap_or(0.0);
                for glyph in run.glyphs.iter() {
                    if glyph.start >= col_end {
                        x = glyph.x;
                        break;
                    }
                }
                x
            };

            let w = (x_end - x_start).max(if run.line_i != end_line {
                min_sel_width
            } else {
                0.0
            });
            if w > 0.0 {
                rects.push((x_start, run.line_top, w, run.line_height));
            }
        }

        rects
    }

    fn measure_content_height(buf: &TextBuffer) -> f32 {
        let mut max_bottom = 0.0_f32;
        for run in buf.layout_runs() {
            max_bottom = max_bottom.max(run.line_top + run.line_height);
        }
        max_bottom
    }

    pub fn update_status(&mut self, text: &str) {
        if self.cached_status_text == text {
            return;
        }
        self.cached_status_text = text.to_string();
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
    ) -> Option<(Vec<TabHitRect>, Rect)> {
        // Check if tab info has changed; skip expensive label recreation if not
        let new_info: Vec<(String, bool, bool)> = tabs
            .iter()
            .map(|t| (t.name.clone(), t.is_active, t.is_modified))
            .collect();
        if new_info == self.cached_tab_info {
            return None;
        }
        self.cached_tab_info = new_info;

        self.tab_labels.clear();
        self.tab_close_labels.clear();
        self.tab_modified.clear();
        self.tab_bg_rects.clear();
        self.tab_close_rects.clear();

        let tab_h = tab_bar_rect.h;
        let mut x = tab_bar_rect.x + tab_gap();
        let y = tab_bar_rect.y;
        let mut hit_rects = Vec::with_capacity(tabs.len());

        let dot_w = Self::measure_label_width(&self.dot_label);
        let dot_area = tab_dot_pad() + dot_w + tab_dot_pad();

        for tab in tabs {
            let label = Self::create_label(&mut self.font_system, fonts::ui_size(), &tab.name);
            let text_w = Self::measure_label_width(&label);
            let close_label =
                Self::create_label(&mut self.font_system, fonts::ui_size(), "\u{2715}");
            let left_pad = if tab.is_modified { dot_area } else { tab_pad_h() };
            let tab_w = left_pad + text_w + tab_close_pad() + tab_close_size() + tab_pad_h();
            let tab_rect = Rect { x, y, w: tab_w, h: tab_h };
            let close_rect = Rect {
                x: x + tab_w - tab_pad_h() - tab_close_size(),
                y: y + (tab_h - tab_close_size()) / 2.0,
                w: tab_close_size(),
                h: tab_close_size(),
            };

            self.tab_bg_rects.push((tab_rect, tab.is_active));
            self.tab_close_rects.push(close_rect);
            self.tab_labels.push(label);
            self.tab_close_labels.push(close_label);
            self.tab_modified.push(tab.is_modified);
            hit_rects.push(TabHitRect { full: tab_rect, close: close_rect });
            x += tab_w + tab_gap();
        }

        let plus_w = tab_pad_h() * 2.0 + Self::measure_label_width(&self.plus_label);
        self.plus_rect = Rect { x, y, w: plus_w, h: tab_h };
        Some((hit_rects, self.plus_rect))
    }

    pub fn render(
        &mut self,
        layout: &LayoutResult,
        hover: HoverTarget,
        win_controls: &WindowControlRects,
        is_dragging_split: bool,
        open_menu: Option<usize>,
        render_area: &RenderAreaParams,
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
        let text_pad = spacing::sm();
        let t = theme::theme();

        // Pane clip bounds
        let pane_left = lp.x as i32;
        let pane_top = lp.y as i32;
        let pane_right = if self.v_track_rect.is_some() {
            (lp.x + lp.w - spacing::scrollbar_width()) as i32
        } else {
            (lp.x + lp.w) as i32
        };
        let pane_bottom = (lp.y + lp.h) as i32;

        // -- Background rects --
        // Pane backgrounds first (cells render on top of these)
        let mut ui_rects = vec![
            rect_from(layout.left_pane, t.editor_bg),
            rect_from(layout.right_pane, t.graph_bg),
        ];

        // Split handle
        let split_color = if is_dragging_split || hover == HoverTarget::SplitHandle {
            t.split_handle_hover
        } else {
            t.split_handle
        };
        ui_rects.push(rect_from(layout.split_handle, split_color));

        // --- Cell container rects ---
        for (i, cl) in self.cell_layouts.iter().enumerate() {
            // Skip cells fully outside the visible pane
            if cl.container.y + cl.container.h < lp.y || cl.container.y > lp.y + lp.h {
                continue;
            }

            // Cell border (1px larger rect behind the cell bg)
            let border_color = if i == self.active_cell_index {
                t.border_focus
            } else {
                t.border
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
            ui_rects.push(rect_rounded(cl.container, t.bg_elevated, cell_radius));

            // Play/Stop button background
            {
                let is_playing = i < self.cell_playing.len() && self.cell_playing[i];
                let is_hovered = hover == HoverTarget::CellPlayButton(i);
                let btn_color = if is_playing {
                    if is_hovered { t.stop_button_hover } else { t.stop_button }
                } else {
                    if is_hovered { t.play_button_hover } else { t.play_button }
                };
                ui_rects.push(rect_rounded(cl.play_button, btn_color, 4.0 * fonts::scale()));
            }

            // Copy button hover
            if hover == HoverTarget::CellCopyButton(i) {
                ui_rects.push(rect_from(cl.copy_button, t.bg_hover));
            }

            // Delete button hover
            if hover == HoverTarget::CellDeleteButton(i) {
                ui_rects.push(rect_from(cl.delete_button, t.bg_hover));
            }

            // Separator line
            ui_rects.push(rect_from(cl.separator, t.border));

            // Output toolbar (shown when output exists, even if collapsed)
            if cl.output_toolbar.h > 0.0 {
                // Full-width separator line between editor and output
                ui_rects.push(rect_from(cl.output_separator, t.border));

                // Chevron toggle button hover
                if hover == HoverTarget::CellOutputToggle(i) {
                    ui_rects.push(rect_from(cl.output_toggle, t.bg_hover));
                }

                // Copy button hover
                if hover == HoverTarget::CellOutputCopyButton(i) {
                    ui_rects.push(rect_from(cl.output_copy_button, t.bg_hover));
                }
            }
        }

        // Selection highlight in active cell (clipped to editor bounds)
        if let Some(cl) = self.cell_layouts.get(self.active_cell_index) {
            let editor_right = cl.editor.x + cl.editor.w;
            let editor_bottom = cl.editor.y + cl.editor.h;
            for &(sx, sy, sw, sh) in &self.selection_content_rects {
                let screen_x = cl.editor.x + text_pad + sx;
                let screen_y = cl.editor.y + text_pad + sy;
                // Clip selection rect to cell editor bounds
                let clipped_x = screen_x.max(cl.editor.x);
                let clipped_y = screen_y.max(cl.editor.y);
                let clipped_w = (screen_x + sw).min(editor_right) - clipped_x;
                let clipped_h = (screen_y + sh).min(editor_bottom) - clipped_y;
                if clipped_w > 0.0
                    && clipped_h > 0.0
                    && clipped_x + clipped_w >= lp.x
                    && clipped_x < pane_right as f32
                    && clipped_y + clipped_h > lp.y
                    && clipped_y < pane_bottom as f32
                {
                    ui_rects.push(RectInstance {
                        x: clipped_x,
                        y: clipped_y,
                        w: clipped_w,
                        h: clipped_h,
                        color: t.editor_selection.to_f32_array(),
                        corner_radius: 2.0,
                        _padding: [0.0; 3],
                    });
                }
            }
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
                    color: t.cursor.to_f32_array(),
                    corner_radius: 0.0,
                    _padding: [0.0; 3],
                });
            }
        }

        // Add cell button (round)
        if self.add_cell_rect.y + self.add_cell_rect.h > lp.y
            && self.add_cell_rect.y < lp.y + lp.h
        {
            let add_color = if hover == HoverTarget::AddCellButton {
                t.bg_hover
            } else {
                t.bg_elevated
            };
            ui_rects.push(rect_rounded(self.add_cell_rect, add_color, self.add_cell_rect.w / 2.0));
        }

        // Scrollbars
        if let Some(track) = self.v_track_rect {
            ui_rects.push(rect_from(track, t.scrollbar_track));
        }
        if let Some(thumb) = self.v_thumb_rect {
            let color = if matches!(hover, HoverTarget::VScrollThumb) {
                t.scrollbar_thumb_hover
            } else {
                t.scrollbar_thumb
            };
            ui_rects.push(rect_from(thumb, color));
        }

        // --- Title bar, tab bar, status bar overlays ---
        // Drawn AFTER cells so cell content cannot overlap the UI chrome
        ui_rects.push(rect_from(layout.title_bar, t.bg_secondary));
        ui_rects.push(rect_from(layout.tab_bar, t.tab_inactive));
        ui_rects.push(rect_from(layout.status_bar, t.bg_secondary));

        // Menu item hover backgrounds
        for (idx, rect) in self.menu_item_rects.iter().enumerate() {
            let is_open = open_menu == Some(idx);
            let is_hovered = hover == HoverTarget::MenuItem(idx);
            if is_open || is_hovered {
                ui_rects.push(rect_from(*rect, t.menu_item_hover));
            }
        }

        // Per-tab backgrounds (hover-aware)
        for (idx, (rect, is_active)) in self.tab_bg_rects.iter().enumerate() {
            let color = if *is_active {
                t.tab_active
            } else if matches!(hover, HoverTarget::Tab(i) | HoverTarget::TabClose(i) if i == idx) {
                t.tab_hover
            } else {
                t.tab_inactive
            };
            ui_rects.push(rect_from(*rect, color));
        }

        // Tab close hover bg
        for (idx, close_rect) in self.tab_close_rects.iter().enumerate() {
            if hover == HoverTarget::TabClose(idx) {
                ui_rects.push(rect_from(*close_rect, t.bg_hover));
            }
        }

        // Plus button (tab bar)
        let plus_color = if hover == HoverTarget::PlusButton {
            t.tab_hover
        } else {
            t.tab_inactive
        };
        ui_rects.push(rect_from(self.plus_rect, plus_color));

        // Window control hover
        if hover == HoverTarget::WinBtnMinimize {
            ui_rects.push(rect_from(win_controls.minimize, t.bg_hover));
        }
        if hover == HoverTarget::WinBtnMaximize {
            ui_rects.push(rect_from(win_controls.maximize, t.bg_hover));
        }
        if hover == HoverTarget::WinBtnClose {
            ui_rects.push(rect_from(win_controls.close, t.close_button_hover));
        }

        // Dropdown background + item hovers (drawn last so they overlay tab bar)
        if self.dropdown_active {
            ui_rects.push(rect_from(self.dropdown_bg, t.dropdown_bg));
            let db = self.dropdown_bg;
            ui_rects.push(RectInstance {
                x: db.x, y: db.y, w: db.w, h: 1.0,
                color: t.dropdown_separator.to_f32_array(),
                corner_radius: 0.0, _padding: [0.0; 3],
            });
            ui_rects.push(RectInstance {
                x: db.x, y: db.y + db.h - 1.0, w: db.w, h: 1.0,
                color: t.dropdown_separator.to_f32_array(),
                corner_radius: 0.0, _padding: [0.0; 3],
            });
            ui_rects.push(RectInstance {
                x: db.x, y: db.y, w: 1.0, h: db.h,
                color: t.dropdown_separator.to_f32_array(),
                corner_radius: 0.0, _padding: [0.0; 3],
            });
            ui_rects.push(RectInstance {
                x: db.x + db.w - 1.0, y: db.y, w: 1.0, h: db.h,
                color: t.dropdown_separator.to_f32_array(),
                corner_radius: 0.0, _padding: [0.0; 3],
            });

            for (idx, rect) in self.dropdown_item_rects.iter().enumerate() {
                if hover == HoverTarget::DropdownItem(idx) {
                    ui_rects.push(rect_from(*rect, t.dropdown_hover));
                } else if self.dropdown_active_item == Some(idx) {
                    // Subtle highlight for the currently active item (e.g. selected theme)
                    ui_rects.push(rect_from(*rect, t.bg_elevated));
                }
            }
        }

        // Autocomplete popup (drawn after dropdown, overlays cell content)
        if self.ac_active {
            let ac_radius = 6.0 * fonts::scale();
            // Border
            ui_rects.push(rect_rounded(
                Rect {
                    x: self.ac_bg.x - 1.0,
                    y: self.ac_bg.y - 1.0,
                    w: self.ac_bg.w + 2.0,
                    h: self.ac_bg.h + 2.0,
                },
                t.dropdown_separator,
                ac_radius + 1.0,
            ));
            // Background
            ui_rects.push(rect_rounded(self.ac_bg, t.dropdown_bg, ac_radius));

            // Item highlights
            for (idx, rect) in self.ac_item_rects.iter().enumerate() {
                if idx == self.ac_selected_index {
                    ui_rects.push(rect_from(*rect, t.dropdown_hover));
                } else if hover == HoverTarget::AutocompleteItem(idx) {
                    ui_rects.push(rect_from(*rect, t.bg_elevated));
                }
            }
        }

        // -- Text areas --
        let mut text_areas: Vec<TextArea> = Vec::new();
        let editor_color = t.text_primary.to_glyphon();

        // Dropdown rect for clipping pane content around it
        let dd_clip = if self.dropdown_active {
            Some(self.dropdown_bg)
        } else {
            None
        };

        // Cell editor text + button labels
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

                let bounds = TextBounds {
                    left: clip_left,
                    top: clip_top,
                    right: clip_right,
                    bottom: clip_bottom,
                };

                // Multi-region clipping: split text around dropdown instead of hiding it
                let regions = if let Some(dd) = &dd_clip {
                    clip_bounds_around_dropdown(&bounds, dd)
                } else {
                    vec![bounds]
                };

                for region in regions {
                    if region.left < region.right && region.top < region.bottom {
                        text_areas.push(TextArea {
                            buffer: &self.cell_buffers[i],
                            left: cl.editor.x + text_pad,
                            top: cl.editor.y + text_pad,
                            scale: 1.0,
                            bounds: region,
                            default_color: editor_color,
                            custom_glyphs: &[],
                        });
                    }
                }
            }

            // Output toolbar labels: chevron, "Output", copy button
            if cl.output_toolbar.h > 0.0 {
                let tb_clip_top = (cl.output_toolbar.y as i32).max(pane_top);
                let tb_clip_bottom =
                    ((cl.output_toolbar.y + cl.output_toolbar.h) as i32).min(pane_bottom);
                if tb_clip_top < tb_clip_bottom {
                    let is_collapsed = cl.output.h <= 0.0;
                    let line_h = fonts::small_size() * 1.4;

                    // Chevron (▶ or ▼) centered in toggle button
                    let chevron_buf = if is_collapsed {
                        &self.cell_chevron_right
                    } else {
                        &self.cell_chevron_down
                    };
                    let chev_w = Self::measure_label_width(chevron_buf);
                    let chev_x = cl.output_toggle.x + (cl.output_toggle.w - chev_w) / 2.0;
                    let chev_y = cl.output_toggle.y + (cl.output_toggle.h - line_h) / 2.0;
                    let mut chev_bounds = TextBounds {
                        left: (cl.output_toggle.x as i32).max(pane_left),
                        top: tb_clip_top,
                        right: ((cl.output_toggle.x + cl.output_toggle.w) as i32)
                            .min(pane_right),
                        bottom: tb_clip_bottom,
                    };
                    let chev_visible = dd_clip
                        .as_ref()
                        .map_or(true, |dd| clip_bounds_under_dropdown(&mut chev_bounds, dd));
                    if chev_visible {
                        text_areas.push(TextArea {
                            buffer: chevron_buf,
                            left: chev_x,
                            top: chev_y,
                            scale: 1.0,
                            bounds: chev_bounds,
                            default_color: t.text_muted.to_glyphon(),
                            custom_glyphs: &[],
                        });
                    }

                    // "Output" label to the right of the chevron
                    let label_x = cl.output_toggle.x + cl.output_toggle.w + spacing::xs();
                    let label_y = cl.output_toolbar.y + (cl.output_toolbar.h - line_h) / 2.0;
                    let mut label_bounds = TextBounds {
                        left: (label_x as i32).max(pane_left),
                        top: tb_clip_top,
                        right: ((cl.output_toolbar.x + cl.output_toolbar.w) as i32)
                            .min(pane_right),
                        bottom: tb_clip_bottom,
                    };
                    let label_visible = dd_clip
                        .as_ref()
                        .map_or(true, |dd| clip_bounds_under_dropdown(&mut label_bounds, dd));
                    if label_visible {
                        text_areas.push(TextArea {
                            buffer: &self.output_label,
                            left: label_x,
                            top: label_y,
                            scale: 1.0,
                            bounds: label_bounds,
                            default_color: t.text_muted.to_glyphon(),
                            custom_glyphs: &[],
                        });
                    }

                    // Copy button label (⎘) right-aligned in toolbar
                    let ocb = &cl.output_copy_button;
                    let mut ocb_bounds = TextBounds {
                        left: (ocb.x as i32).max(pane_left),
                        top: tb_clip_top,
                        right: ((ocb.x + ocb.w) as i32).min(pane_right),
                        bottom: tb_clip_bottom,
                    };
                    let ocb_visible = dd_clip
                        .as_ref()
                        .map_or(true, |dd| clip_bounds_under_dropdown(&mut ocb_bounds, dd));
                    if ocb_visible {
                        let copy_w = Self::measure_label_width(&self.cell_copy_label);
                        let copy_line_h = fonts::ui_size() * 1.4;
                        let cx = ocb.x + (ocb.w - copy_w) / 2.0;
                        let cy = ocb.y + (ocb.h - copy_line_h) / 2.0;
                        text_areas.push(TextArea {
                            buffer: &self.cell_copy_label,
                            left: cx,
                            top: cy,
                            scale: 1.0,
                            bounds: ocb_bounds,
                            default_color: t.text_muted.to_glyphon(),
                            custom_glyphs: &[],
                        });
                    }
                }
            }

            // Output text (REDUCE result or error) below the toolbar
            if cl.output.h > 0.0 && i < self.cell_output_buffers.len() {
                let scroll_x = self
                    .cell_output_scroll_x
                    .get(i)
                    .copied()
                    .unwrap_or(0.0);
                let clip_left = (cl.output.x as i32).max(pane_left);
                let clip_top = (cl.output.y as i32).max(pane_top);
                let clip_right = ((cl.output.x + cl.output.w) as i32).min(pane_right);
                let clip_bottom = ((cl.output.y + cl.output.h) as i32).min(pane_bottom);

                let is_err = self.cell_output_is_error.get(i).copied().unwrap_or(false);
                let output_color = if is_err { t.stop_button } else { t.text_muted };

                if clip_left < clip_right && clip_top < clip_bottom {
                    text_areas.push(TextArea {
                        buffer: &self.cell_output_buffers[i],
                        left: cl.output.x + text_pad - scroll_x,
                        top: cl.output.y + text_pad,
                        scale: 1.0,
                        bounds: TextBounds {
                            left: clip_left,
                            top: clip_top,
                            right: clip_right,
                            bottom: clip_bottom,
                        },
                        default_color: output_color.to_glyphon(),
                        custom_glyphs: &[],
                    });
                }
            }

            // Play/Stop button label (▶ or ■)
            let play = &cl.play_button;
            let play_clip_top = (play.y as i32).max(pane_top);
            let play_clip_bottom = ((play.y + play.h) as i32).min(pane_bottom);
            if play_clip_top < play_clip_bottom {
                let mut bounds = TextBounds {
                    left: play.x as i32,
                    top: play_clip_top,
                    right: (play.x + play.w) as i32,
                    bottom: play_clip_bottom,
                };
                let visible = dd_clip
                    .as_ref()
                    .map_or(true, |dd| clip_bounds_under_dropdown(&mut bounds, dd));

                if visible {
                    let is_playing = i < self.cell_playing.len() && self.cell_playing[i];
                    let play_buf = if is_playing { &self.cell_stop_label } else { &self.cell_play_label };
                    let label_w = Self::measure_label_width(play_buf);
                    let line_h = fonts::ui_size() * 1.4;
                    let cx = play.x + (play.w - label_w) / 2.0;
                    let cy = play.y + (play.h - line_h) / 2.0;
                    text_areas.push(TextArea {
                        buffer: play_buf,
                        left: cx,
                        top: cy,
                        scale: 1.0,
                        bounds,
                        default_color: t.text_primary.to_glyphon(),
                        custom_glyphs: &[],
                    });
                }
            }

            // Copy button label (⎘)
            {
                let cb = &cl.copy_button;
                let cb_clip_top = (cb.y as i32).max(pane_top);
                let cb_clip_bottom = ((cb.y + cb.h) as i32).min(pane_bottom);
                if cb_clip_top < cb_clip_bottom {
                    let mut bounds = TextBounds {
                        left: cb.x as i32,
                        top: cb_clip_top,
                        right: (cb.x + cb.w) as i32,
                        bottom: cb_clip_bottom,
                    };
                    let visible = dd_clip
                        .as_ref()
                        .map_or(true, |dd| clip_bounds_under_dropdown(&mut bounds, dd));

                    if visible {
                        let label_w = Self::measure_label_width(&self.cell_copy_label);
                        let line_h = fonts::ui_size() * 1.4;
                        let cx = cb.x + (cb.w - label_w) / 2.0;
                        let cy = cb.y + (cb.h - line_h) / 2.0;
                        text_areas.push(TextArea {
                            buffer: &self.cell_copy_label,
                            left: cx,
                            top: cy,
                            scale: 1.0,
                            bounds,
                            default_color: t.text_muted.to_glyphon(),
                            custom_glyphs: &[],
                        });
                    }
                }
            }

            // Delete button label (×)
            let del = &cl.delete_button;
            let del_clip_top = (del.y as i32).max(pane_top);
            let del_clip_bottom = ((del.y + del.h) as i32).min(pane_bottom);
            if del_clip_top < del_clip_bottom {
                let mut bounds = TextBounds {
                    left: del.x as i32,
                    top: del_clip_top,
                    right: (del.x + del.w) as i32,
                    bottom: del_clip_bottom,
                };
                let visible = dd_clip
                    .as_ref()
                    .map_or(true, |dd| clip_bounds_under_dropdown(&mut bounds, dd));

                if visible {
                    let label_w = Self::measure_label_width(&self.cell_delete_label);
                    let line_h = fonts::ui_size() * 1.4;
                    let cx = del.x + (del.w - label_w) / 2.0;
                    let cy = del.y + (del.h - line_h) / 2.0;
                    text_areas.push(TextArea {
                        buffer: &self.cell_delete_label,
                        left: cx,
                        top: cy,
                        scale: 1.0,
                        bounds,
                        default_color: t.text_muted.to_glyphon(),
                        custom_glyphs: &[],
                    });
                }
            }
        }

        // Tooltip for play/stop button hover
        for (i, cl) in self.cell_layouts.iter().enumerate() {
            if !matches!(hover, HoverTarget::CellPlayButton(idx) if idx == i) {
                continue;
            }
            // Position tooltip below the play button
            let tip_w = Self::measure_label_width(&self.tooltip_label) + spacing::sm() * 2.0;
            let tip_h = fonts::small_size() * 1.4 + spacing::xs() * 2.0;
            let tip_x = cl.play_button.x + cl.play_button.w / 2.0 - tip_w / 2.0;
            let tip_y = cl.play_button.y + cl.play_button.h + spacing::xs();
            let tip_rect = Rect { x: tip_x, y: tip_y, w: tip_w, h: tip_h };

            // Tooltip background + border
            ui_rects.push(rect_rounded(
                Rect { x: tip_rect.x - 1.0, y: tip_rect.y - 1.0, w: tip_rect.w + 2.0, h: tip_rect.h + 2.0 },
                t.tooltip_border,
                4.0 * fonts::scale(),
            ));
            ui_rects.push(rect_rounded(tip_rect, t.tooltip_bg, 4.0 * fonts::scale()));

            // Tooltip text
            text_areas.push(TextArea {
                buffer: &self.tooltip_label,
                left: tip_rect.x + spacing::sm(),
                top: tip_rect.y + spacing::xs(),
                scale: 1.0,
                bounds: TextBounds {
                    left: tip_rect.x as i32,
                    top: tip_rect.y as i32,
                    right: (tip_rect.x + tip_rect.w) as i32,
                    bottom: (tip_rect.y + tip_rect.h) as i32,
                },
                default_color: t.text_primary.to_glyphon(),
                custom_glyphs: &[],
            });
        }

        // Add cell button label (centered "+" icon)
        if self.add_cell_rect.y + self.add_cell_rect.h > lp.y
            && self.add_cell_rect.y < lp.y + lp.h
        {
            let clip_top = (self.add_cell_rect.y as i32).max(pane_top);
            let clip_bottom = ((self.add_cell_rect.y + self.add_cell_rect.h) as i32).min(pane_bottom);
            let mut bounds = TextBounds {
                left: self.add_cell_rect.x as i32,
                top: clip_top,
                right: (self.add_cell_rect.x + self.add_cell_rect.w) as i32,
                bottom: clip_bottom,
            };
            let visible = dd_clip
                .as_ref()
                .map_or(true, |dd| clip_bounds_under_dropdown(&mut bounds, dd));

            if visible && bounds.top < bounds.bottom {
                let label_w = Self::measure_label_width(&self.add_cell_label);
                let line_h = fonts::ui_size() * 1.4;
                let cx = self.add_cell_rect.x + (self.add_cell_rect.w - label_w) / 2.0;
                let cy = self.add_cell_rect.y + (self.add_cell_rect.h - line_h) / 2.0;
                text_areas.push(TextArea {
                    buffer: &self.add_cell_label,
                    left: cx,
                    top: cy,
                    scale: 1.0,
                    bounds,
                    default_color: t.text_muted.to_glyphon(),
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
                left: rect.x + menu_item_pad(),
                top: rect.y + spacing::xs(),
                scale: 1.0,
                bounds: TextBounds {
                    left: rect.x as i32,
                    top: rect.y as i32,
                    right: menu_right as i32,
                    bottom: (rect.y + rect.h) as i32,
                },
                default_color: t.text_primary.to_glyphon(),
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
            let cy = rect.y + spacing::xs();
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
                default_color: t.text_primary.to_glyphon(),
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
                tab_rect.x + tab_dot_pad() + Self::measure_label_width(&self.dot_label) + tab_dot_pad()
            } else {
                tab_rect.x + tab_pad_h()
            };
            text_areas.push(TextArea {
                buffer: label,
                left: text_left,
                top: tab_rect.y + spacing::sm(),
                scale: 1.0,
                bounds,
                default_color: t.text_primary.to_glyphon(),
                custom_glyphs: &[],
            });
        }

        // Modified dot indicators (text-based \u{25CF} for round dot)
        for (i, (tab_rect, _)) in self.tab_bg_rects.iter().enumerate() {
            if i < self.tab_modified.len() && self.tab_modified[i] {
                let Some(bounds) = clip_bounds_for_dropdown(tab_rect, &tab_bar, dropdown_clip.as_ref()) else {
                    continue;
                };

                let dot_x = tab_rect.x + tab_dot_pad();
                // Same baseline as tab text, nudged up slightly because ● sits
                // lower than regular text glyphs in the line box
                let dot_y = tab_rect.y + spacing::sm() - 1.0;
                text_areas.push(TextArea {
                    buffer: &self.dot_label,
                    left: dot_x,
                    top: dot_y,
                    scale: 1.0,
                    bounds,
                    default_color: t.text_primary.to_glyphon(),
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
                default_color: t.text_muted.to_glyphon(),
                custom_glyphs: &[],
            });
        }

        // Plus button label
        if let Some(bounds) = clip_bounds_for_dropdown(&self.plus_rect, &tab_bar, dropdown_clip.as_ref()) {
            text_areas.push(TextArea {
                buffer: &self.plus_label,
                left: self.plus_rect.x + tab_pad_h(),
                top: self.plus_rect.y + spacing::sm(),
                scale: 1.0,
                bounds,
                default_color: t.text_muted.to_glyphon(),
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
                    left: rect.x + menu_item_pad(),
                    top: rect.y + spacing::xs(),
                    scale: 1.0,
                    bounds: TextBounds {
                        left: rect.x as i32,
                        top: rect.y as i32,
                        right: (rect.x + rect.w) as i32,
                        bottom: (rect.y + rect.h) as i32,
                    },
                    default_color: t.text_primary.to_glyphon(),
                    custom_glyphs: &[],
                });
                // Shortcut label (right-aligned)
                let shortcut_w = Self::measure_label_width(shortcut);
                text_areas.push(TextArea {
                    buffer: shortcut,
                    left: rect.x + rect.w - menu_item_pad() - shortcut_w,
                    top: rect.y + spacing::xs(),
                    scale: 1.0,
                    bounds: TextBounds {
                        left: rect.x as i32,
                        top: rect.y as i32,
                        right: (rect.x + rect.w) as i32,
                        bottom: (rect.y + rect.h) as i32,
                    },
                    default_color: t.text_muted.to_glyphon(),
                    custom_glyphs: &[],
                });
            }
        }

        // Autocomplete item labels
        if self.ac_active {
            for (i, (label, kind_label)) in self
                .ac_item_labels
                .iter()
                .zip(self.ac_kind_labels.iter())
                .enumerate()
            {
                if i >= self.ac_item_rects.len() {
                    break;
                }
                let rect = &self.ac_item_rects[i];
                let bounds = TextBounds {
                    left: rect.x as i32,
                    top: rect.y as i32,
                    right: (rect.x + rect.w) as i32,
                    bottom: (rect.y + rect.h) as i32,
                };
                // Kind badge (left-aligned)
                text_areas.push(TextArea {
                    buffer: kind_label,
                    left: rect.x + spacing::sm(),
                    top: rect.y + spacing::xs(),
                    scale: 1.0,
                    bounds,
                    default_color: t.text_muted.to_glyphon(),
                    custom_glyphs: &[],
                });
                // Label (after badge)
                let badge_w = Self::measure_label_width(kind_label);
                text_areas.push(TextArea {
                    buffer: label,
                    left: rect.x + spacing::sm() + badge_w + spacing::sm(),
                    top: rect.y + spacing::xs(),
                    scale: 1.0,
                    bounds,
                    default_color: t.text_primary.to_glyphon(),
                    custom_glyphs: &[],
                });
            }
        }

        // Status label
        text_areas.push(TextArea {
            buffer: &self.status_label,
            left: layout.status_bar.x + spacing::md(),
            top: layout.status_bar.y + spacing::xs(),
            scale: 1.0,
            bounds: TextBounds {
                left: layout.status_bar.x as i32,
                top: layout.status_bar.y as i32,
                right: (layout.status_bar.x + layout.status_bar.w) as i32,
                bottom: (layout.status_bar.y + layout.status_bar.h) as i32,
            },
            default_color: t.text_secondary.to_glyphon(),
            custom_glyphs: &[],
        });

        // (render area placeholder removed — empty pane is intentional)

        // -- Axis overlay computation --
        // Labels are drawn directly on the plot (no reserved margin).
        let rp = layout.right_pane;
        let scale = fonts::scale();
        let label_pad = 4.0_f32 * scale;

        let x_range = render_area.axis_x_max - render_area.axis_x_min;
        let y_range = render_area.axis_y_max - render_area.axis_y_min;

        // Compute max ticks per axis based on pixel density, scaled with zoom,
        // capped at MAX_AXIS_LABELS so every grid line gets a label with uniform spacing.
        let min_px_spacing = 80.0_f32 * scale;
        let max_ticks_x = (rp.w / min_px_spacing).max(2.0) as usize;
        let max_ticks_y = (rp.h / min_px_spacing).max(2.0) as usize;
        let max_ticks_x = max_ticks_x.min(MAX_AXIS_LABELS);
        let max_ticks_y = max_ticks_y.min(MAX_AXIS_LABELS);

        // Compute nice step independently per axis so zooming one axis
        // doesn't reduce line count on the other.
        let mut x_step = compute_nice_step(x_range, max_ticks_x);
        let mut y_step = compute_nice_step(y_range, max_ticks_y);

        // Bump step if generate_ticks overshoots MAX_AXIS_LABELS due to boundary rounding.
        let x_ticks = loop {
            let xt = generate_ticks(render_area.axis_x_min, render_area.axis_x_max, x_step);
            if xt.len() <= MAX_AXIS_LABELS { break xt; }
            x_step *= 2.0;
        };
        let y_ticks = loop {
            let yt = generate_ticks(render_area.axis_y_min, render_area.axis_y_max, y_step);
            if yt.len() <= MAX_AXIS_LABELS { break yt; }
            y_step *= 2.0;
        };

        // Update axis label buffer text
        for (i, tick) in x_ticks.iter().enumerate() {
            let text = format_tick(*tick, x_step);
            self.axis_label_buffers[i].set_text(
                &mut self.font_system,
                &text,
                Attrs::new().family(Family::Monospace),
                Shaping::Advanced,
            );
            self.axis_label_buffers[i].shape_until_scroll(&mut self.font_system, false);
        }
        for (i, tick) in y_ticks.iter().enumerate() {
            let text = format_tick(*tick, y_step);
            self.axis_label_buffers[MAX_AXIS_LABELS + i].set_text(
                &mut self.font_system,
                &text,
                Attrs::new().family(Family::Monospace),
                Shaping::Advanced,
            );
            self.axis_label_buffers[MAX_AXIS_LABELS + i]
                .shape_until_scroll(&mut self.font_system, false);
        }

        // Build axis overlay rects (grid lines + label backing rects)
        let mut axis_rects: Vec<RectInstance> = Vec::new();
        let label_h = fonts::small_size() * 1.4;

        // Grid lines at every tick position
        let grid_color = theme::theme().graph_grid.to_f32_array();
        let zero_color = {
            let g = theme::theme().graph_grid;
            Rgba::new(g.r, g.g, g.b, (g.a as u16 * 2).min(255) as u8).to_f32_array()
        };
        if x_range > f32::EPSILON {
            for tick in &x_ticks {
                let t = (tick - render_area.axis_x_min) / x_range;
                let sx = rp.x + t * rp.w;
                if sx >= rp.x && sx <= rp.x + rp.w {
                    let is_zero = tick.abs() < f32::EPSILON;
                    axis_rects.push(RectInstance {
                        x: if is_zero { sx - 0.5 } else { sx },
                        y: rp.y,
                        w: if is_zero { 2.0 } else { 1.0 },
                        h: rp.h,
                        color: if is_zero { zero_color } else { grid_color },
                        corner_radius: 0.0, _padding: [0.0; 3],
                    });
                }
            }
        }
        if y_range > f32::EPSILON {
            for tick in &y_ticks {
                let t = (tick - render_area.axis_y_min) / y_range;
                let sy = rp.y + rp.h - t * rp.h;
                if sy >= rp.y && sy <= rp.y + rp.h {
                    let is_zero = tick.abs() < f32::EPSILON;
                    axis_rects.push(RectInstance {
                        x: rp.x,
                        y: if is_zero { sy - 0.5 } else { sy },
                        w: rp.w,
                        h: if is_zero { 2.0 } else { 1.0 },
                        color: if is_zero { zero_color } else { grid_color },
                        corner_radius: 0.0, _padding: [0.0; 3],
                    });
                }
            }
        }

        // Label backing rects + text areas (on-plot, with semi-transparent bg)
        let mut axis_text_areas: Vec<TextArea> = Vec::new();
        let label_color = t.axis_label.to_glyphon();
        let rp_bounds = TextBounds {
            left: rp.x as i32,
            top: rp.y as i32,
            right: (rp.x + rp.w) as i32,
            bottom: (rp.y + rp.h) as i32,
        };

        // X axis labels (along bottom edge)
        if x_range > f32::EPSILON {
            for (i, tick) in x_ticks.iter().enumerate() {
                let t = (tick - render_area.axis_x_min) / x_range;
                let sx = rp.x + t * rp.w;
                let lw = Self::measure_label_width(&self.axis_label_buffers[i]);
                let lx = (sx - lw / 2.0).max(rp.x + label_pad);
                let ly = rp.y + rp.h - label_h - label_pad;
                axis_text_areas.push(TextArea {
                    buffer: &self.axis_label_buffers[i],
                    left: lx, top: ly, scale: 1.0,
                    bounds: rp_bounds,
                    default_color: label_color,
                    custom_glyphs: &[],
                });
            }
        }

        // Y axis labels (along left edge)
        if y_range > f32::EPSILON {
            for (i, tick) in y_ticks.iter().enumerate() {
                let t = (tick - render_area.axis_y_min) / y_range;
                let sy = rp.y + rp.h - t * rp.h;
                let lx = rp.x + label_pad;
                let ly = (sy - label_h / 2.0).max(rp.y + label_pad);
                axis_text_areas.push(TextArea {
                    buffer: &self.axis_label_buffers[MAX_AXIS_LABELS + i],
                    left: lx, top: ly, scale: 1.0,
                    bounds: rp_bounds,
                    default_color: label_color,
                    custom_glyphs: &[],
                });
            }
        }

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

        self.axis_text_renderer
            .prepare(
                &self.device,
                &self.queue,
                &mut self.font_system,
                &mut self.axis_atlas,
                &self.viewport,
                axis_text_areas,
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

        // Pass 1: Clear + UI rects + UI text
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("main_pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(t.bg_primary.to_wgpu()),
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

        // Pass 2: Shader pipelines into right pane
        if self.shader_pipeline.has_active() {
            self.shader_pipeline.render(
                &mut encoder,
                &view,
                &self.queue,
                &layout.right_pane,
                (self.surface_config.width, self.surface_config.height),
                [
                    render_area.axis_x_min,
                    render_area.axis_y_max,
                    render_area.axis_x_max,
                    render_area.axis_y_min,
                ],
                render_area.mouse_uv,
            );
        }

        // Pass 3: Axis overlay (zone backgrounds, grid lines, tick labels)
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("axis_overlay_pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Load,
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            self.overlay_rect_renderer
                .draw(&self.queue, &mut pass, sw, sh, &axis_rects);
            self.axis_text_renderer
                .render(&self.axis_atlas, &self.viewport, &mut pass)
                .unwrap();
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        self.atlas.trim();
        self.axis_atlas.trim();
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

/// Shift all cell layout rects and the add-cell button by a scroll delta.
fn shift_cell_layouts(layouts: &mut [CellLayout], add_cell_rect: &mut Rect, delta: f32) {
    for cl in layouts.iter_mut() {
        cl.container.y -= delta;
        cl.header.y -= delta;
        cl.play_button.y -= delta;
        cl.copy_button.y -= delta;
        cl.delete_button.y -= delta;
        cl.separator.y -= delta;
        cl.editor.y -= delta;
        cl.output_separator.y -= delta;
        cl.output_toggle.y -= delta;
        cl.output_copy_button.y -= delta;
        cl.output_toolbar.y -= delta;
        cl.output.y -= delta;
    }
    add_cell_rect.y -= delta;
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

/// Clip TextBounds so they don't overlap a dropdown that hangs from above.
/// Returns false if the bounds become empty (fully occluded).
fn clip_bounds_under_dropdown(bounds: &mut TextBounds, dropdown: &Rect) -> bool {
    let dd_left = dropdown.x as i32;
    let dd_right = (dropdown.x + dropdown.w) as i32;
    let dd_bottom = (dropdown.y + dropdown.h) as i32;

    // No vertical overlap → no clipping needed
    if bounds.top >= dd_bottom || bounds.bottom <= dropdown.y as i32 {
        return true;
    }
    // No horizontal overlap → no clipping needed
    if bounds.left >= dd_right || bounds.right <= dd_left {
        return true;
    }
    // There's overlap. Push top below the dropdown.
    bounds.top = bounds.top.max(dd_bottom);
    bounds.top < bounds.bottom
}

/// Split TextBounds into multiple visible regions around a dropdown overlay.
/// Returns up to 4 regions (above, left-of, right-of, below) so that text
/// remains visible everywhere except directly behind the dropdown.
fn clip_bounds_around_dropdown(bounds: &TextBounds, dropdown: &Rect) -> Vec<TextBounds> {
    let dd_left = dropdown.x as i32;
    let dd_right = (dropdown.x + dropdown.w) as i32;
    let dd_top = dropdown.y as i32;
    let dd_bottom = (dropdown.y + dropdown.h) as i32;

    // No vertical overlap → no clipping needed
    if bounds.top >= dd_bottom || bounds.bottom <= dd_top {
        return vec![*bounds];
    }
    // No horizontal overlap → no clipping needed
    if bounds.left >= dd_right || bounds.right <= dd_left {
        return vec![*bounds];
    }

    let mut result = Vec::with_capacity(4);

    // Part above dropdown (full width)
    if bounds.top < dd_top {
        result.push(TextBounds {
            left: bounds.left,
            top: bounds.top,
            right: bounds.right,
            bottom: dd_top,
        });
    }

    // Part to the left of dropdown (within dropdown's vertical range)
    if bounds.left < dd_left {
        let clip_top = bounds.top.max(dd_top);
        let clip_bottom = bounds.bottom.min(dd_bottom);
        if clip_top < clip_bottom {
            result.push(TextBounds {
                left: bounds.left,
                top: clip_top,
                right: dd_left,
                bottom: clip_bottom,
            });
        }
    }

    // Part to the right of dropdown (within dropdown's vertical range)
    if bounds.right > dd_right {
        let clip_top = bounds.top.max(dd_top);
        let clip_bottom = bounds.bottom.min(dd_bottom);
        if clip_top < clip_bottom {
            result.push(TextBounds {
                left: dd_right,
                top: clip_top,
                right: bounds.right,
                bottom: clip_bottom,
            });
        }
    }

    // Part below dropdown (full width)
    if bounds.bottom > dd_bottom {
        result.push(TextBounds {
            left: bounds.left,
            top: dd_bottom,
            right: bounds.right,
            bottom: bounds.bottom,
        });
    }

    result
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
