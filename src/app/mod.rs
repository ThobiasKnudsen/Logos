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
mod render_area;
mod state;

pub(crate) use menus::{
    dynamic_menu_count, dynamic_menu_label, menu_items, resolve_example_path, MenuItemDef,
    FONTS_MENU_INDEX, MENU_NAMES, THEME_MENU_INDEX,
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
    is_dragging_split: bool,

    is_dragging_v_scroll: bool,
    scroll_drag_offset: f32,

    is_dragging_editor: bool,
    editor_drag_cell: Option<usize>,

    is_dragging_cell_resize: bool,
    cell_resize_index: Option<usize>,
    cell_resize_start_y: f32,
    cell_resize_start_h: f32,

    is_dragging_cell_h_scroll: bool,
    cell_h_scroll_index: Option<usize>,
    cell_h_scroll_drag_offset: f32,

    is_dragging_cell_v_scroll: bool,
    cell_v_scroll_index: Option<usize>,
    cell_v_scroll_drag_offset: f32,

    win_control_rects: WindowControlRects,
    is_maximized: bool,

    last_title_click: Option<Instant>,

    menu_item_rects: Vec<Rect>,
    open_menu: Option<usize>,
    dropdown_item_rects: Vec<Rect>,

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
