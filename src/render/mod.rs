pub mod compute_pipeline;
pub mod rects;
pub mod shader_pipeline;

mod cells;
mod chrome;
mod frame;
mod hit;
mod init;
mod menus;
mod scroll;
mod shaders;
mod text;

use std::sync::Arc;

use glyphon::{
    Buffer as TextBuffer, FontSystem, SwashCache, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use wgpu::SurfaceConfiguration;

use crate::ui::layout::Rect;
use crate::ui::theme::{fonts, Rgba};
use rects::{RectInstance, RectRenderer};
use shader_pipeline::ShaderPipelineManager;

pub(crate) const COMPOSITE_SHADER_WGSL: &str = r#"
struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    var pos: array<vec2<f32>, 3> = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    let p = pos[vi];
    var out: VsOut;
    out.position = vec4<f32>(p.x, p.y, 0.0, 1.0);
    out.uv = vec2<f32>((p.x + 1.0) * 0.5, 1.0 - (p.y + 1.0) * 0.5);
    return out;
}

@group(0) @binding(0) var scene_tex: texture_2d<f32>;
@group(0) @binding(1) var overlay_tex: texture_2d<f32>;
@group(0) @binding(2) var tex_sampler: sampler;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let scene = textureSample(scene_tex, tex_sampler, in.uv);
    let overlay = textureSample(overlay_tex, tex_sampler, in.uv);
    let inverted = fract(scene.rgb - vec3<f32>(0.5));
    let blended = mix(scene.rgb, inverted, overlay.a);
    return vec4<f32>(blended, 1.0);
}
"#;

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
    pub color_button: Rect,
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
    /// Horizontal scrollbar track at the bottom of the editor. Zero-sized if no overflow.
    pub editor_h_scrollbar_track: Rect,
    /// Horizontal scrollbar thumb. Zero-sized if no overflow.
    pub editor_h_scrollbar_thumb: Rect,
    /// Vertical scrollbar track at the right of the editor. Zero-sized if not contracted.
    pub editor_v_scrollbar_track: Rect,
    /// Vertical scrollbar thumb. Zero-sized if not contracted.
    pub editor_v_scrollbar_thumb: Rect,
    /// Resize handle zone at the bottom edge of the cell container.
    pub resize_handle: Rect,
    /// The full (unconstrained) content height of this cell's editor text.
    pub content_height: f32,
    /// Cell's plot color — used to draw the swatch on the color button.
    pub plot_color: Rgba,
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
    /// Contracted editor height override (None = auto-fit).
    pub contracted_editor_h: Option<f32>,
    /// Color used by this cell's plot shaders (and the color-button swatch).
    pub plot_color: Rgba,
}

/// Parameters for the render area (right pane) passed from AppState.
pub struct RenderAreaParams {
    pub axis_x_min: f32,
    pub axis_x_max: f32,
    pub axis_y_min: f32,
    pub axis_y_max: f32,
    pub mouse_uv: [f32; 2],
}

pub(crate) const MAX_AXIS_LABELS: usize = 12;

/// Compute a "nice" tick step for a given range and max ticks using the 1-2-5 rule.
pub(crate) fn compute_nice_step(range: f32, max_ticks: usize) -> f32 {
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
pub(crate) fn generate_ticks(axis_min: f32, axis_max: f32, step: f32) -> Vec<f32> {
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

/// Compute decimal places for cursor labels based on axis range.
pub(crate) fn cursor_decimals(range: f32) -> usize {
    if range <= f32::EPSILON {
        return 6;
    }
    (5 - range.log10().ceil() as i32).clamp(0, 10) as usize
}

/// Format a tick value for axis labels.
pub(crate) fn format_tick(v: f32, step: f32) -> String {
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

const BASE_TAB_PAD_H: f32 = 12.0;
const BASE_TAB_CLOSE_SIZE: f32 = 20.0;
const BASE_TAB_CLOSE_PAD: f32 = 6.0;
const BASE_TAB_GAP: f32 = 2.0;
const BASE_TAB_DOT_PAD: f32 = 6.0;
const BASE_MENU_ITEM_PAD: f32 = 10.0;
const BASE_CELL_HEADER_HEIGHT: f32 = 28.0;
const BASE_CELL_DELETE_SIZE: f32 = 22.0;
const BASE_OUTPUT_TOGGLE_HEIGHT: f32 = 20.0;

pub(crate) fn tab_pad_h() -> f32 {
    BASE_TAB_PAD_H * fonts::scale()
}
pub(crate) fn tab_close_size() -> f32 {
    BASE_TAB_CLOSE_SIZE * fonts::scale()
}
pub(crate) fn tab_close_pad() -> f32 {
    BASE_TAB_CLOSE_PAD * fonts::scale()
}
pub(crate) fn tab_gap() -> f32 {
    BASE_TAB_GAP * fonts::scale()
}
pub(crate) fn tab_dot_pad() -> f32 {
    BASE_TAB_DOT_PAD * fonts::scale()
}
pub(crate) fn menu_item_pad() -> f32 {
    BASE_MENU_ITEM_PAD * fonts::scale()
}
pub(crate) fn cell_header_height() -> f32 {
    BASE_CELL_HEADER_HEIGHT * fonts::scale()
}
pub(crate) fn cell_delete_size() -> f32 {
    BASE_CELL_DELETE_SIZE * fonts::scale()
}
pub(crate) fn output_toggle_height() -> f32 {
    BASE_OUTPUT_TOGGLE_HEIGHT * fonts::scale()
}

/// Handles all GPU rendering: wgpu setup, text via glyphon, rects via instanced draw.
pub struct Renderer {
    pub(crate) device: Arc<wgpu::Device>,
    pub(crate) queue: Arc<wgpu::Queue>,
    pub(crate) surface: wgpu::Surface<'static>,
    pub(crate) surface_config: SurfaceConfiguration,

    pub(crate) font_system: FontSystem,
    pub(crate) swash_cache: SwashCache,
    pub(crate) viewport: Viewport,
    pub(crate) atlas: TextAtlas,
    pub(crate) text_renderer: TextRenderer,

    pub(crate) cell_buffers: Vec<TextBuffer>,
    pub(crate) cell_texts: Vec<String>,
    pub(crate) cell_output_buffers: Vec<TextBuffer>,
    pub(crate) cell_output_texts: Vec<String>,
    pub(crate) cell_output_is_error: Vec<bool>,
    pub(crate) cell_layouts: Vec<CellLayout>,
    pub(crate) active_cell_index: usize,
    pub(crate) cursor_content_pos: (f32, f32, f32),
    pub(crate) selection_content_rects: Vec<(f32, f32, f32, f32)>,
    pub(crate) cell_scroll_y: f32,
    pub(crate) cells_total_height: f32,
    pub(crate) cached_editor_pane: Rect,
    pub(crate) add_cell_label: TextBuffer,
    pub(crate) add_cell_rect: Rect,
    pub(crate) cell_delete_label: TextBuffer,
    pub(crate) cell_copy_label: TextBuffer,
    pub(crate) cell_play_label: TextBuffer,
    pub(crate) cell_stop_label: TextBuffer,
    pub(crate) tooltip_label: TextBuffer,
    pub(crate) cell_playing: Vec<bool>,
    pub(crate) cell_output_scroll_x: Vec<f32>,
    pub(crate) cell_editor_scroll_x: Vec<f32>,
    pub(crate) cell_editor_scroll_y: Vec<f32>,
    pub(crate) cell_content_heights: Vec<f32>,
    pub(crate) cell_chevron_right: TextBuffer,
    pub(crate) cell_chevron_down: TextBuffer,
    pub(crate) output_label: TextBuffer,
    pub(crate) prev_active_cell: usize,
    pub(crate) prev_cursor_byte: usize,

    pub(crate) v_track_rect: Option<Rect>,
    pub(crate) v_thumb_rect: Option<Rect>,

    pub(crate) status_label: TextBuffer,
    pub(crate) cached_status_text: String,

    pub(crate) menu_item_labels: Vec<TextBuffer>,
    pub(crate) menu_item_rects: Vec<Rect>,

    pub(crate) dropdown_item_labels: Vec<TextBuffer>,
    pub(crate) dropdown_shortcut_labels: Vec<TextBuffer>,
    pub(crate) dropdown_bg: Rect,
    pub(crate) dropdown_item_rects: Vec<Rect>,
    pub(crate) dropdown_active: bool,
    pub(crate) dropdown_active_item: Option<usize>,

    pub(crate) cached_tab_info: Vec<(String, bool, bool)>,
    pub(crate) tab_labels: Vec<TextBuffer>,
    pub(crate) tab_close_labels: Vec<TextBuffer>,
    pub(crate) tab_modified: Vec<bool>,
    pub(crate) dot_label: TextBuffer,
    pub(crate) tab_bg_rects: Vec<(Rect, bool)>,
    pub(crate) tab_close_rects: Vec<Rect>,
    pub(crate) plus_label: TextBuffer,
    pub(crate) plus_rect: Rect,

    pub(crate) win_min_label: TextBuffer,
    pub(crate) win_max_label: TextBuffer,
    pub(crate) win_close_label: TextBuffer,

    pub(crate) ac_active: bool,
    pub(crate) ac_bg: Rect,
    pub(crate) ac_item_rects: Vec<Rect>,
    pub(crate) ac_item_labels: Vec<TextBuffer>,
    pub(crate) ac_kind_labels: Vec<TextBuffer>,
    pub(crate) ac_selected_index: usize,

    pub(crate) rect_renderer: RectRenderer,

    pub(crate) shader_pipeline: ShaderPipelineManager,

    pub(crate) overlay_rect_renderer: RectRenderer,
    pub(crate) axis_atlas: TextAtlas,
    pub(crate) axis_text_renderer: TextRenderer,
    pub(crate) axis_label_buffers: Vec<TextBuffer>,

    pub(crate) cursor_x_label: TextBuffer,
    pub(crate) cursor_y_label: TextBuffer,
    pub(crate) cursor_rect_renderer: RectRenderer,
    pub(crate) cursor_text_atlas: TextAtlas,
    pub(crate) cursor_text_renderer: TextRenderer,

    pub(crate) scene_texture: wgpu::Texture,
    pub(crate) scene_view: wgpu::TextureView,
    pub(crate) overlay_texture: wgpu::Texture,
    pub(crate) overlay_view: wgpu::TextureView,
    pub(crate) composite_pipeline: wgpu::RenderPipeline,
    pub(crate) composite_bind_group_layout: wgpu::BindGroupLayout,
    pub(crate) composite_bind_group: wgpu::BindGroup,
    pub(crate) composite_sampler: wgpu::Sampler,
}

pub(crate) fn rect_from(r: Rect, color: Rgba) -> RectInstance {
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
pub(crate) fn shift_cell_layouts(layouts: &mut [CellLayout], add_cell_rect: &mut Rect, delta: f32) {
    for cl in layouts.iter_mut() {
        cl.container.y -= delta;
        cl.header.y -= delta;
        cl.play_button.y -= delta;
        cl.color_button.y -= delta;
        cl.copy_button.y -= delta;
        cl.delete_button.y -= delta;
        cl.separator.y -= delta;
        cl.editor.y -= delta;
        cl.output_separator.y -= delta;
        cl.output_toggle.y -= delta;
        cl.output_copy_button.y -= delta;
        cl.output_toolbar.y -= delta;
        cl.output.y -= delta;
        cl.editor_h_scrollbar_track.y -= delta;
        cl.editor_h_scrollbar_thumb.y -= delta;
        cl.editor_v_scrollbar_track.y -= delta;
        cl.editor_v_scrollbar_thumb.y -= delta;
        cl.resize_handle.y -= delta;
    }
    add_cell_rect.y -= delta;
}

pub(crate) fn rect_rounded(r: Rect, color: Rgba, radius: f32) -> RectInstance {
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
pub(crate) fn clip_bounds_under_dropdown(bounds: &mut TextBounds, dropdown: &Rect) -> bool {
    let dd_left = dropdown.x as i32;
    let dd_right = (dropdown.x + dropdown.w) as i32;
    let dd_bottom = (dropdown.y + dropdown.h) as i32;

    if bounds.top >= dd_bottom || bounds.bottom <= dropdown.y as i32 {
        return true;
    }
    if bounds.left >= dd_right || bounds.right <= dd_left {
        return true;
    }
    bounds.top = bounds.top.max(dd_bottom);
    bounds.top < bounds.bottom
}

/// Split TextBounds into multiple visible regions around a dropdown overlay.
pub(crate) fn clip_bounds_around_dropdown(bounds: &TextBounds, dropdown: &Rect) -> Vec<TextBounds> {
    let dd_left = dropdown.x as i32;
    let dd_right = (dropdown.x + dropdown.w) as i32;
    let dd_top = dropdown.y as i32;
    let dd_bottom = (dropdown.y + dropdown.h) as i32;

    if bounds.top >= dd_bottom || bounds.bottom <= dd_top {
        return vec![*bounds];
    }
    if bounds.left >= dd_right || bounds.right <= dd_left {
        return vec![*bounds];
    }

    let mut result = Vec::with_capacity(4);

    if bounds.top < dd_top {
        result.push(TextBounds {
            left: bounds.left,
            top: bounds.top,
            right: bounds.right,
            bottom: dd_top,
        });
    }

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
pub(crate) fn clip_bounds_for_dropdown(
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

            if elem.x >= dd_left && elem.x + elem.w <= dd_right {
                return None;
            }

            if elem.x < dd_left {
                right = right.min(dd_left as i32);
            }

            if elem.x >= dd_left && elem.x < dd_right {
                left = left.max(dd_right as i32);
            }
        }
    }

    Some(TextBounds {
        left,
        top,
        right,
        bottom,
    })
}
