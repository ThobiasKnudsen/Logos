use std::sync::Arc;
use std::time::Instant;

use winit::event_loop::EventLoop;
use winit::keyboard::ModifiersState;
use winit::window::{ResizeDirection, Window};

use crate::editor::autocomplete::{self, AutocompleteState};
use crate::file_dialog::FileDialog;
use crate::lang::lang_service::LangService;
use crate::lang::reduce::service::ReduceService;
use crate::render::{CellLayout, Renderer, TabHitRect};
use crate::session::Session;
use crate::ui::layout::{LayoutResult, Rect, UiLayout};
use crate::ui::theme::spacing;

mod cas;
mod event;
mod menus;
pub(crate) mod render_area;
mod state;

pub(crate) use menus::{
    dynamic_menu_count, dynamic_menu_label, menu_items, MenuItemDef, FONTS_MENU_INDEX,
    HELP_MENU_INDEX, MENU_NAMES, THEME_MENU_INDEX,
};
use render_area::RenderAreaState;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum HoverTarget {
    None,
    Tab(usize),
    TabClose(usize),
    PlusButton,
    SplitHandle,
    TitleBar,
    WinBtnMinimize,
    WinBtnMaximize,
    WinBtnClose,
    MenuItem(usize),
    DropdownItem(usize),
    FeedbackButton,
    /// One of the four vertical sliders inside the open color-picker popup.
    /// Channel index: 0 = R, 1 = G, 2 = B, 3 = A.
    ColorPickerSlider(u8),
    /// Inside the color-picker popup but not on a slider — keeps clicks from
    /// dismissing the popup.
    ColorPickerArea,
    VScrollThumb,
    CellEditor(usize),
    CellPlayButton(usize),
    CellColorButton(usize),
    CellCopyButton(usize),
    CellOutputCopyButton(usize),
    CellOutputToggle(usize),
    CellDeleteButton(usize),
    AddCellButton,
    AutocompleteItem(usize),
    CellResizeHandle(usize),
    CellEditorHScrollThumb(usize),
    CellEditorVScrollThumb(usize),
    RenderArea,
    /// A render-area chrome toggle button (issue #27). Payload picks
    /// which of the four buttons (grid / cursor / axis numbers /
    /// 2D-3D) is under the cursor so both hover-styling and click
    /// dispatch can identify it without re-running hit-tests.
    RenderAreaToggle(render_area::ToggleKind),
    WindowEdge(ResizeDirection),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct WindowControlRects {
    pub minimize: Rect,
    pub maximize: Rect,
    pub close: Rect,
}

impl Default for WindowControlRects {
    fn default() -> Self {
        let z = Rect {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        };
        Self {
            minimize: z,
            maximize: z,
            close: z,
        }
    }
}

struct App {
    state: Option<AppState>,
}

/// Which interactive drag the user is currently performing, if any.
///
/// Only one drag is active at a time, so encoding the choice as a single
/// enum makes that invariant unrepresentable as a bug: the seven previous
/// `is_dragging_*` bools and their companion index/offset fields could in
/// principle all be `true` simultaneously, which never made sense. Variant
/// payloads carry the per-drag state inline.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ActiveDrag {
    None,
    /// Pane splitter (between left and right panes).
    Split,
    /// Left-pane vertical scrollbar thumb. `offset` is the cursor's y-delta
    /// from the thumb top at mouse-down.
    VScroll { offset: f32 },
    /// Selecting text inside a cell editor.
    Editor { cell: usize },
    /// Resizing a cell's bottom edge. `start_y` is the cursor y at
    /// mouse-down; `start_h` is the editor height at that instant.
    CellResize { cell: usize, start_y: f32, start_h: f32 },
    /// Horizontal scrollbar within a cell. `offset` is the cursor's x-delta
    /// from the thumb left at mouse-down.
    CellHScroll { cell: usize, offset: f32 },
    /// Vertical scrollbar within a cell. `offset` is the cursor's y-delta
    /// from the thumb top at mouse-down.
    CellVScroll { cell: usize, offset: f32 },
    /// Color-picker slider thumb. `channel`: 0 = R, 1 = G, 2 = B, 3 = A.
    ColorPickerSlider { channel: u8 },
}

impl ActiveDrag {
    pub(crate) fn is_active(self) -> bool {
        !matches!(self, Self::None)
    }
}

struct AppState {
    renderer: Renderer,
    session: Session,
    layout: UiLayout,
    cached_layout: LayoutResult,
    window: Arc<Window>,
    modifiers: ModifiersState,
    pending_dialog: Option<FileDialog>,
    cursor_position: (f32, f32),
    tab_hit_rects: Vec<TabHitRect>,
    plus_button_rect: Rect,

    cell_layouts: Vec<CellLayout>,

    hover_target: HoverTarget,
    /// Hover target captured at mouse-press time, used for click resolution.
    mouse_press_target: HoverTarget,

    split_left_width: f32,

    /// The current drag operation, if any. Replaces seven previous
    /// `is_dragging_*` bools plus their companion index/offset fields.
    active_drag: ActiveDrag,

    win_control_rects: WindowControlRects,
    is_maximized: bool,

    last_title_click: Option<Instant>,

    menu_item_rects: Vec<Rect>,
    open_menu: Option<usize>,
    dropdown_item_rects: Vec<Rect>,
    feedback_button_rect: Rect,

    /// `Some(cell_index)` while the RGBA color-picker popup is anchored to
    /// that cell's color button. Independent of `active_drag` — the popup
    /// can be open without the user actively dragging a slider.
    open_color_picker: Option<usize>,
    /// Timestamp at which the cursor first left both the picker and any
    /// color button. The picker auto-closes after `COLOR_PICKER_HOVER_GRACE`
    /// has elapsed since this moment, giving the user time to traverse the
    /// gap between button and popup. Reset to `None` whenever hover lands
    /// back inside.
    color_picker_hover_left_at: Option<Instant>,

    render_area: RenderAreaState,

    clipboard: Option<arboard::Clipboard>,

    autocomplete: AutocompleteState,
    autocomplete_item_rects: Vec<Rect>,

    /// REDUCE CAS integration. Shared with every `NotebookView`'s `Notebook`
    /// so all notebooks route through the single process-wide REDUCE worker.
    reduce_service: std::rc::Rc<std::cell::RefCell<ReduceService>>,

    lang_service: LangService,
    cached_user_symbols: Vec<autocomplete::Candidate>,
    /// Track what text was last submitted to lang_service per cell,
    /// so we don't re-submit unchanged text.
    last_submitted_texts: Vec<String>,

    last_frame_time: Instant,
}

const DOUBLE_CLICK_MS: u128 = 400;
const SCROLL_LINE_PIXELS: f32 = 40.0;
/// Grace period between the cursor leaving the picker/color-button area
/// and the picker auto-closing. Long enough to traverse the visual gap
/// between the button and the popup without the popup vanishing.
pub(crate) const COLOR_PICKER_HOVER_GRACE: std::time::Duration =
    std::time::Duration::from_millis(200);

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    event_loop.run_app(&mut App { state: None })?;
    Ok(())
}

fn compute_win_control_rects(title_bar: &Rect) -> WindowControlRects {
    let w = spacing::window_control_width();
    let h = title_bar.h;
    let right = title_bar.x + title_bar.w;
    let y = title_bar.y;

    WindowControlRects {
        close: Rect {
            x: right - w,
            y,
            w,
            h,
        },
        maximize: Rect {
            x: right - w * 2.0,
            y,
            w,
            h,
        },
        minimize: Rect {
            x: right - w * 3.0,
            y,
            w,
            h,
        },
    }
}

fn line_col_from(text: &str, cursor_byte: usize) -> (usize, usize) {
    let clamped = cursor_byte.min(text.len());
    let before = &text[..clamped];
    let line = before.matches('\n').count();
    let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let col = before[line_start..].chars().count();
    (line, col)
}

fn point_in_rect(x: f32, y: f32, r: &Rect) -> bool {
    x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h
}

const RESIZE_BORDER: f32 = 6.0;

fn edge_resize_direction(mx: f32, my: f32, win_w: f32, win_h: f32) -> Option<ResizeDirection> {
    let left = mx < RESIZE_BORDER;
    let right = mx > win_w - RESIZE_BORDER;
    let top = my < RESIZE_BORDER;
    let bottom = my > win_h - RESIZE_BORDER;

    match (left, right, top, bottom) {
        (true, _, true, _) => Some(ResizeDirection::NorthWest),
        (true, _, _, true) => Some(ResizeDirection::SouthWest),
        (_, true, true, _) => Some(ResizeDirection::NorthEast),
        (_, true, _, true) => Some(ResizeDirection::SouthEast),
        (true, _, _, _) => Some(ResizeDirection::West),
        (_, true, _, _) => Some(ResizeDirection::East),
        (_, _, true, _) => Some(ResizeDirection::North),
        (_, _, _, true) => Some(ResizeDirection::South),
        _ => None,
    }
}

/// Get the window's position on screen in physical pixels.
fn win_pos(window: &Window) -> (f32, f32) {
    window
        .outer_position()
        .map(|p| (p.x as f32, p.y as f32))
        .unwrap_or((0.0, 0.0))
}

/// Detect which interaction zone the mouse is in within the render pane.
fn detect_mouse_zone(mx: f32, my: f32, pane: &Rect) -> render_area::MouseZone {
    let rel_y = my - pane.y;
    let rel_x = mx - pane.x;
    if rel_y > pane.h - spacing::axis_zone_size() {
        render_area::MouseZone::XAxisEdge
    } else if rel_x < spacing::axis_zone_size() {
        render_area::MouseZone::YAxisEdge
    } else {
        render_area::MouseZone::Center
    }
}
