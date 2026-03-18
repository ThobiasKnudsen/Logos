use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

use crate::editor::autocomplete::{self, AutocompleteState};
use crate::editor::cell::CellOutput;
use crate::file_dialog::{DialogKind, DialogResult, FileDialog};
use crate::lang;
use crate::lang::reduce::service::ReduceService;
use crate::lang::reduce::translate;
use crate::render::{CellInfo, CellLayout, Renderer, TabHitRect, TabInfo};
use crate::session::TabManager;
use crate::ui::layout::{LayoutResult, Rect, UiLayout};
use crate::render::RenderAreaParams;
use crate::ui::theme::{self, fonts, spacing, split};

/// Debounce interval before sending an expression to REDUCE.
const REDUCE_DEBOUNCE: Duration = Duration::from_millis(300);

// ---------------------------------------------------------------------------
// Menu definitions
// ---------------------------------------------------------------------------

pub(crate) struct MenuItemDef {
    pub label: &'static str,
    pub shortcut: &'static str,
}

pub(crate) const MENU_NAMES: &[&str] = &["File", "Edit", "View", "Theme", "Help"];

const MENU_FILE_ITEMS: &[MenuItemDef] = &[
    MenuItemDef { label: "New Tab", shortcut: "Ctrl+N" },
    MenuItemDef { label: "Open...", shortcut: "Ctrl+O" },
    MenuItemDef { label: "Save", shortcut: "Ctrl+S" },
    MenuItemDef { label: "Save As...", shortcut: "Ctrl+Shift+S" },
    MenuItemDef { label: "Close Tab", shortcut: "Ctrl+W" },
    MenuItemDef { label: "Quit", shortcut: "Ctrl+Q" },
];

const MENU_EDIT_ITEMS: &[MenuItemDef] = &[
    MenuItemDef { label: "Cut", shortcut: "Ctrl+X" },
    MenuItemDef { label: "Copy", shortcut: "Ctrl+C" },
    MenuItemDef { label: "Paste", shortcut: "Ctrl+V" },
    MenuItemDef { label: "Select All", shortcut: "Ctrl+A" },
];

const MENU_VIEW_ITEMS: &[MenuItemDef] = &[
    MenuItemDef { label: "Zoom In", shortcut: "Ctrl+=" },
    MenuItemDef { label: "Zoom Out", shortcut: "Ctrl+-" },
    MenuItemDef { label: "Reset Zoom", shortcut: "Ctrl+0" },
];

const MENU_THEME_ITEMS: &[MenuItemDef] = &[
    MenuItemDef { label: "Catppuccin", shortcut: "" },
    MenuItemDef { label: "One Dark", shortcut: "" },
    MenuItemDef { label: "Monokai", shortcut: "" },
    MenuItemDef { label: "Dracula", shortcut: "" },
    MenuItemDef { label: "Gruvbox", shortcut: "" },
    MenuItemDef { label: "Nord", shortcut: "" },
    MenuItemDef { label: "Solarized", shortcut: "" },
];

pub(crate) fn menu_items(index: usize) -> &'static [MenuItemDef] {
    match index {
        0 => MENU_FILE_ITEMS,
        1 => MENU_EDIT_ITEMS,
        2 => MENU_VIEW_ITEMS,
        3 => MENU_THEME_ITEMS,
        _ => &[],
    }
}

/// Returns the index of the currently active theme (for highlighting in the Theme menu).
pub(crate) fn active_theme_index() -> usize {
    theme::BUILTIN_THEMES
        .iter()
        .position(|(name, _)| *name == theme::active_theme_name())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Hover target
// ---------------------------------------------------------------------------

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
    CellCopyButton(usize),
    CellDeleteButton(usize),
    AddCellButton,
    AutocompleteItem(usize),
    RenderArea,
}

// ---------------------------------------------------------------------------
// Mouse zone for render area interaction
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum MouseZone {
    Center,
    XAxisEdge, // bottom edge — X axis only
    YAxisEdge, // left edge — Y axis only
}

struct RenderAreaState {
    axis_x_min: f32,
    axis_x_max: f32,
    axis_y_min: f32,
    axis_y_max: f32,
    is_dragging: bool,
    last_drag_pos: (f32, f32),
    mouse_zone: MouseZone,
}

impl Default for RenderAreaState {
    fn default() -> Self {
        Self {
            axis_x_min: -5.0,
            axis_x_max: 5.0,
            axis_y_min: -5.0,
            axis_y_max: 5.0,
            is_dragging: false,
            last_drag_pos: (0.0, 0.0),
            mouse_zone: MouseZone::Center,
        }
    }
}

// ---------------------------------------------------------------------------
// Window control button rects
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub(crate) struct WindowControlRects {
    pub minimize: Rect,
    pub maximize: Rect,
    pub close: Rect,
}

impl Default for WindowControlRects {
    fn default() -> Self {
        let z = Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 };
        Self { minimize: z, maximize: z, close: z }
    }
}

// ---------------------------------------------------------------------------
// App shell
// ---------------------------------------------------------------------------

struct App {
    state: Option<AppState>,
}

struct AppState {
    renderer: Renderer,
    tab_manager: TabManager,
    layout: UiLayout,
    cached_layout: LayoutResult,
    window: Arc<Window>,
    modifiers: ModifiersState,
    pending_dialog: Option<FileDialog>,
    cursor_position: (f32, f32),
    tab_hit_rects: Vec<TabHitRect>,
    plus_button_rect: Rect,

    // Cell layouts for hit-testing
    cell_layouts: Vec<CellLayout>,

    // Hover
    hover_target: HoverTarget,
    /// Hover target captured at mouse-press time, used for click resolution.
    mouse_press_target: HoverTarget,

    // Split dragging
    split_left_width: f32,
    is_dragging_split: bool,

    // Scrollbar dragging
    is_dragging_v_scroll: bool,
    scroll_drag_offset: f32,

    // Editor text selection dragging
    is_dragging_editor: bool,
    editor_drag_cell: Option<usize>,

    // Window controls
    win_control_rects: WindowControlRects,
    is_maximized: bool,

    // Double-click detection for title bar
    last_title_click: Option<Instant>,

    // Menu state
    menu_item_rects: Vec<Rect>,
    open_menu: Option<usize>,
    dropdown_item_rects: Vec<Rect>,

    // Render area (right pane) interaction state
    render_area: RenderAreaState,

    // Clipboard
    clipboard: Option<arboard::Clipboard>,

    // Autocomplete
    autocomplete: AutocompleteState,
    autocomplete_item_rects: Vec<Rect>,

    // REDUCE CAS integration
    reduce_service: ReduceService,
    /// Last time the active cell was edited (for debounce).
    last_edit_time: Option<Instant>,
    /// Cell index that was last edited.
    last_edited_cell: Option<usize>,
}

const DOUBLE_CLICK_MS: u128 = 400;
const SCROLL_LINE_PIXELS: f32 = 40.0;

impl AppState {
    /// Sync renderer with the active tab's cells and all tab infos.
    fn sync_active_tab(&mut self) {
        let lp = self.cached_layout.left_pane;
        let tab = self.tab_manager.active_tab();
        let cell_infos: Vec<CellInfo> = tab
            .cells
            .iter()
            .map(|c| CellInfo {
                text: c.buffer.text().to_string(),
                cursor_byte: c.buffer.cursor_byte_offset(),
                is_playing: c.is_playing,
                selection: c.buffer.selection_range(),
                output_text: match &c.output {
                    CellOutput::None => None,
                    CellOutput::Error(e) => Some(format!("Error: {}", e)),
                    CellOutput::Simplifying => Some("Simplifying...".to_string()),
                    CellOutput::Simplified(s) => Some(s.clone()),
                },
            })
            .collect();
        self.cell_layouts = self.renderer.update_cells(
            &cell_infos,
            tab.active_cell_index,
            lp,
        );

        // Update tab bar
        let tab_infos: Vec<TabInfo> = self
            .tab_manager
            .tabs
            .iter()
            .enumerate()
            .map(|(i, t)| TabInfo {
                name: t.name.clone(),
                is_active: i == self.tab_manager.active_index,
                is_modified: t.is_modified,
            })
            .collect();

        if let Some((hit_rects, plus_rect)) = self
            .renderer
            .update_tab_bar(&tab_infos, self.cached_layout.tab_bar)
        {
            self.tab_hit_rects = hit_rects;
            self.plus_button_rect = plus_rect;
        }

        // Compute window control rects from title bar
        self.win_control_rects = compute_win_control_rects(&self.cached_layout.title_bar);

        // Update menu item rects
        self.menu_item_rects = self
            .renderer
            .update_menu_items(self.cached_layout.title_bar, self.win_control_rects.minimize.x);

        // Update status bar
        let tab = self.tab_manager.active_tab();
        let cell = tab.active_cell();
        let (line, col) = line_col_from(cell.buffer.text(), cell.buffer.cursor_byte_offset());
        let line_count = cell.buffer.text().lines().count().max(1);
        let name = tab
            .file_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| tab.name.clone());
        let modified = if tab.is_modified { " [modified]" } else { "" };
        self.renderer.update_status(&format!(
            "{}{} \u{2502} {} lines \u{2502} Ln {}, Col {} \u{2502} {}",
            name,
            modified,
            line_count,
            line + 1,
            col + 1,
            theme::active_theme_name(),
        ));

        self.window.request_redraw();
    }

    /// Determine what the cursor is hovering over and set the cursor icon.
    fn recompute_hover(&mut self) {
        let (mx, my) = self.cursor_position;
        let wc = &self.win_control_rects;

        // Autocomplete items (highest priority)
        if self.autocomplete.active {
            for (i, rect) in self.autocomplete_item_rects.iter().enumerate() {
                if point_in_rect(mx, my, rect) {
                    self.set_hover(HoverTarget::AutocompleteItem(i));
                    return;
                }
            }
            if let Some(bg) = self.renderer.autocomplete_bg_rect() {
                if point_in_rect(mx, my, &bg) {
                    self.set_hover(HoverTarget::None);
                    return;
                }
            }
        }

        // If a dropdown is open, check dropdown items first
        if self.open_menu.is_some() {
            // Check dropdown items
            for (i, rect) in self.dropdown_item_rects.iter().enumerate() {
                if point_in_rect(mx, my, rect) {
                    self.set_hover(HoverTarget::DropdownItem(i));
                    return;
                }
            }
            // Check dropdown background (don't fall through to things behind it)
            if let Some(bg) = self.renderer.dropdown_bg_rect() {
                if point_in_rect(mx, my, &bg) {
                    self.set_hover(HoverTarget::None);
                    return;
                }
            }
        }

        // Menu items in title bar
        for (i, rect) in self.menu_item_rects.iter().enumerate() {
            if point_in_rect(mx, my, rect) {
                self.set_hover(HoverTarget::MenuItem(i));
                return;
            }
        }

        // Window control buttons
        if point_in_rect(mx, my, &wc.close) {
            self.set_hover(HoverTarget::WinBtnClose);
            return;
        }
        if point_in_rect(mx, my, &wc.maximize) {
            self.set_hover(HoverTarget::WinBtnMaximize);
            return;
        }
        if point_in_rect(mx, my, &wc.minimize) {
            self.set_hover(HoverTarget::WinBtnMinimize);
            return;
        }

        // Title bar (remaining area)
        if point_in_rect(mx, my, &self.cached_layout.title_bar) {
            self.set_hover(HoverTarget::TitleBar);
            return;
        }

        // Tab close buttons
        for (i, hit) in self.tab_hit_rects.iter().enumerate() {
            if point_in_rect(mx, my, &hit.close) {
                self.set_hover(HoverTarget::TabClose(i));
                return;
            }
        }

        // Tab body
        for (i, hit) in self.tab_hit_rects.iter().enumerate() {
            if point_in_rect(mx, my, &hit.full) {
                self.set_hover(HoverTarget::Tab(i));
                return;
            }
        }

        // Plus button
        if point_in_rect(mx, my, &self.plus_button_rect) {
            self.set_hover(HoverTarget::PlusButton);
            return;
        }

        // Split handle
        if point_in_rect(mx, my, &self.cached_layout.split_handle) {
            self.set_hover(HoverTarget::SplitHandle);
            return;
        }

        // Scrollbar thumb
        if let Some(thumb) = self.renderer.v_thumb_rect() {
            if point_in_rect(mx, my, &thumb) {
                self.set_hover(HoverTarget::VScrollThumb);
                return;
            }
        }

        // Cell areas (only if cursor is within the left pane)
        let lp = self.cached_layout.left_pane;
        if point_in_rect(mx, my, &lp) {
            // Check add-cell button
            let add_rect = self.renderer.add_cell_button_rect();
            if point_in_rect(mx, my, &add_rect) {
                self.set_hover(HoverTarget::AddCellButton);
                return;
            }

            // Check cells (play button, copy button, delete button, then editor area)
            for cl in &self.cell_layouts {
                if point_in_rect(mx, my, &cl.play_button) {
                    self.set_hover(HoverTarget::CellPlayButton(cl.cell_index));
                    return;
                }
                if point_in_rect(mx, my, &cl.copy_button) {
                    self.set_hover(HoverTarget::CellCopyButton(cl.cell_index));
                    return;
                }
                if point_in_rect(mx, my, &cl.delete_button) {
                    self.set_hover(HoverTarget::CellDeleteButton(cl.cell_index));
                    return;
                }
                if point_in_rect(mx, my, &cl.editor) {
                    self.set_hover(HoverTarget::CellEditor(cl.cell_index));
                    return;
                }
            }
        }

        // Right pane (render area)
        let rp = self.cached_layout.right_pane;
        if point_in_rect(mx, my, &rp) {
            self.render_area.mouse_zone = detect_mouse_zone(mx, my, &rp);
            self.set_hover(HoverTarget::RenderArea);
            return;
        }

        self.set_hover(HoverTarget::None);
    }

    fn set_hover(&mut self, target: HoverTarget) {
        // For RenderArea, also check if the zone changed
        let zone_changed = target == HoverTarget::RenderArea
            && self.hover_target == HoverTarget::RenderArea;

        if self.hover_target != target || zone_changed {
            self.hover_target = target;
            let icon = match target {
                HoverTarget::SplitHandle => CursorIcon::ColResize,
                HoverTarget::CellEditor(_) => CursorIcon::Text,
                HoverTarget::CellPlayButton(_) | HoverTarget::CellCopyButton(_)
                | HoverTarget::AutocompleteItem(_) => CursorIcon::Pointer,
                HoverTarget::RenderArea => match self.render_area.mouse_zone {
                    MouseZone::Center => {
                        if self.render_area.is_dragging {
                            CursorIcon::Grabbing
                        } else {
                            CursorIcon::Crosshair
                        }
                    }
                    MouseZone::XAxisEdge => CursorIcon::EwResize,
                    MouseZone::YAxisEdge => CursorIcon::NsResize,
                },
                _ => CursorIcon::Default,
            };
            self.window.set_cursor(icon);
            self.window.request_redraw();
        }
    }

    fn open_menu(&mut self, index: usize) {
        self.dismiss_autocomplete();
        if self.open_menu == Some(index) {
            self.close_menu();
            return;
        }
        let menu_rect = self.menu_item_rects[index];
        // Theme menu (index 3) highlights the currently active theme
        let active_item = if index == 3 {
            Some(active_theme_index())
        } else {
            None
        };
        self.dropdown_item_rects = self.renderer.open_dropdown(index, menu_rect, active_item);
        self.open_menu = Some(index);
        self.window.request_redraw();
    }

    fn close_menu(&mut self) {
        if self.open_menu.is_some() {
            self.open_menu = None;
            self.dropdown_item_rects.clear();
            self.renderer.close_dropdown();
            self.window.request_redraw();
        }
    }

    fn dismiss_autocomplete(&mut self) {
        if self.autocomplete.active {
            self.autocomplete.dismiss();
            self.autocomplete_item_rects.clear();
            self.renderer.close_autocomplete();
        }
    }

    fn update_autocomplete(&mut self) {
        let tab = self.tab_manager.active_tab();
        let cell = tab.active_cell();
        let text = cell.buffer.text();
        let cursor = cell.buffer.cursor_byte_offset();

        if let Some((prefix, prefix_start)) = autocomplete::prefix_at_cursor(text, cursor) {
            // Gather candidates
            let mut all = autocomplete::static_candidates();

            // When prefix starts with `\`, include LaTeX symbol candidates
            if prefix.starts_with('\\') {
                all.extend(autocomplete::symbol_candidates());
            }

            // Try to parse and extract user symbols (best-effort)
            let mut lex = crate::lang::lexer::Lexer::new(text);
            if let Ok(tokens) = lex.tokenize() {
                let mut parser = crate::lang::parser::Parser::new(tokens);
                if let Ok(ast) = parser.parse() {
                    all.extend(autocomplete::extract_user_symbols(&ast));
                }
            }

            self.autocomplete.update(prefix, prefix_start, &all);

            if self.autocomplete.active {
                // Position popup below cursor
                let (cx, cy, ch) = self.renderer.cursor_content_pos();
                let active_idx = tab.active_cell_index;
                if let Some(cl) = self.cell_layouts.get(active_idx) {
                    let text_pad = crate::ui::theme::spacing::sm();
                    let popup_x = cl.editor.x + text_pad + cx;
                    let popup_y = cl.editor.y + text_pad + cy + ch;
                    let pane = self.cached_layout.left_pane;

                    let candidates: Vec<(String, crate::editor::autocomplete::CandidateKind)> =
                        self.autocomplete.candidates.iter()
                            .map(|c| {
                                let text = c.display.as_deref().unwrap_or(&c.label);
                                (text.to_string(), c.kind)
                            })
                            .collect();

                    self.autocomplete_item_rects = self.renderer.open_autocomplete(
                        popup_x,
                        popup_y,
                        &candidates,
                        self.autocomplete.selected_index,
                        pane,
                    );
                } else {
                    self.dismiss_autocomplete();
                }
            } else {
                self.dismiss_autocomplete();
            }
        } else {
            self.dismiss_autocomplete();
        }
    }

    fn accept_autocomplete(&mut self) {
        if let Some((label, prefix_start)) = self.autocomplete.accept() {
            let label = label.to_string();
            let cursor = self.tab_manager.active_tab().active_cell().buffer.cursor_byte_offset();
            self.tab_manager.active_tab_mut().active_cell_mut().buffer
                .replace_range(prefix_start, cursor, &label);
            self.tab_manager.active_tab_mut().mark_modified();
        }
        self.dismiss_autocomplete();
        self.sync_active_tab();
    }

    fn handle_menu_action(&mut self, event_loop: &ActiveEventLoop, menu_idx: usize, item_idx: usize) {
        self.close_menu();
        match (menu_idx, item_idx) {
            // File menu
            (0, 0) => { self.tab_manager.new_tab(); self.sync_active_tab(); }
            (0, 1) => {
                if self.pending_dialog.is_none() {
                    self.pending_dialog = Some(FileDialog::spawn(DialogKind::Open));
                }
            }
            (0, 2) => {
                if self.tab_manager.active_tab().file_path.is_some() {
                    if let Err(e) = self.tab_manager.active_tab_mut().save() {
                        log::error!("Save failed: {}", e);
                    }
                    self.sync_active_tab();
                } else if self.pending_dialog.is_none() {
                    self.pending_dialog = Some(FileDialog::spawn(DialogKind::Save));
                }
            }
            (0, 3) => {
                if self.pending_dialog.is_none() {
                    self.pending_dialog = Some(FileDialog::spawn(DialogKind::Save));
                }
            }
            (0, 4) => {
                let idx = self.tab_manager.active_index;
                self.tab_manager.close_tab(idx);
                self.sync_active_tab();
            }
            (0, 5) => { event_loop.exit(); }
            // Edit menu: Cut, Copy, Paste, Select All
            (1, 0) => { self.do_cut(); }
            (1, 1) => { self.do_copy(); }
            (1, 2) => { self.do_paste(); }
            (1, 3) => { self.do_select_all(); }
            // View menu
            (2, 0) => { self.do_zoom(|_| fonts::zoom_in()); }
            (2, 1) => { self.do_zoom(|_| fonts::zoom_out()); }
            (2, 2) => { self.do_zoom(|_| fonts::reset_zoom()); }
            // Theme menu — each item selects a theme by index
            (3, i) => { self.select_theme(i); }
            _ => {}
        }
    }

    fn do_cut(&mut self) {
        if let Some(text) = self.tab_manager.active_tab().active_cell().buffer.selected_text() {
            let owned = text.to_string();
            if let Some(cb) = self.clipboard.as_mut() { let _ = cb.set_text(&owned); }
            self.tab_manager.active_tab_mut().active_cell_mut().buffer.backspace();
            self.tab_manager.active_tab_mut().mark_modified();
            self.sync_active_tab();
        }
    }

    fn do_copy(&mut self) {
        if let Some(text) = self.tab_manager.active_tab().active_cell().buffer.selected_text() {
            if let Some(cb) = self.clipboard.as_mut() { let _ = cb.set_text(text); }
        }
    }

    fn do_paste(&mut self) {
        if let Some(cb) = self.clipboard.as_mut() {
            if let Ok(text) = cb.get_text() {
                self.tab_manager.active_tab_mut().active_cell_mut().buffer.insert_text(&text);
                self.tab_manager.active_tab_mut().mark_modified();
                self.sync_active_tab();
            }
        }
    }

    fn do_select_all(&mut self) {
        self.tab_manager.active_tab_mut().active_cell_mut().buffer.select_all();
        self.sync_active_tab();
    }

    fn do_zoom(&mut self, f: impl FnOnce(&mut Self)) {
        f(self);
        self.renderer.rebuild_labels();
        self.layout.apply_scale();
        let size = self.window.inner_size();
        self.cached_layout = self.layout.compute(size.width as f32, size.height as f32);
        self.sync_active_tab();
    }

    fn cycle_theme(&mut self) {
        let name = theme::cycle_theme();
        log::info!("Switched theme to: {}", name);
        self.renderer.invalidate_cell_texts();
        self.sync_active_tab();
    }

    fn select_theme(&mut self, index: usize) {
        theme::set_theme(index);
        log::info!("Selected theme: {}", theme::active_theme_name());
        self.renderer.invalidate_cell_texts();
        self.sync_active_tab();
    }

    fn handle_shortcut(&mut self, event_loop: &ActiveEventLoop, key: &Key) -> bool {
        let ctrl = self.modifiers.control_key();
        let shift = self.modifiers.shift_key();

        if !ctrl {
            return false;
        }

        match key {
            Key::Character(c) if c.as_str() == "n" => {
                self.close_menu();
                self.tab_manager.new_tab();
                self.sync_active_tab();
                true
            }
            Key::Character(c) if c.as_str() == "o" => {
                self.close_menu();
                if self.pending_dialog.is_none() {
                    self.pending_dialog = Some(FileDialog::spawn(DialogKind::Open));
                }
                true
            }
            Key::Character(c) if c.as_str() == "s" || c.as_str() == "S" => {
                self.close_menu();
                if shift {
                    if self.pending_dialog.is_none() {
                        self.pending_dialog = Some(FileDialog::spawn(DialogKind::Save));
                    }
                } else if self.tab_manager.active_tab().file_path.is_some() {
                    if let Err(e) = self.tab_manager.active_tab_mut().save() {
                        log::error!("Save failed: {}", e);
                    }
                    self.sync_active_tab();
                } else if self.pending_dialog.is_none() {
                    self.pending_dialog = Some(FileDialog::spawn(DialogKind::Save));
                }
                true
            }
            Key::Character(c) if c.as_str() == "w" => {
                self.close_menu();
                let idx = self.tab_manager.active_index;
                self.tab_manager.close_tab(idx);
                self.sync_active_tab();
                true
            }
            Key::Character(c) if c.as_str() == "q" => {
                event_loop.exit();
                true
            }
            // Select all
            Key::Character(c) if c.as_str() == "a" => {
                self.close_menu();
                self.do_select_all();
                true
            }
            // Copy
            Key::Character(c) if c.as_str() == "c" => {
                self.do_copy();
                true
            }
            // Cut
            Key::Character(c) if c.as_str() == "x" => {
                self.do_cut();
                true
            }
            // Paste
            Key::Character(c) if c.as_str() == "v" => {
                self.do_paste();
                true
            }
            // Zoom
            Key::Character(c) if c.as_str() == "=" || c.as_str() == "+" => {
                self.close_menu();
                self.do_zoom(|_| fonts::zoom_in());
                true
            }
            Key::Character(c) if c.as_str() == "-" => {
                self.close_menu();
                self.do_zoom(|_| fonts::zoom_out());
                true
            }
            Key::Character(c) if c.as_str() == "0" => {
                self.close_menu();
                self.do_zoom(|_| fonts::reset_zoom());
                true
            }
            // Cycle syntax theme
            Key::Character(c) if c.as_str() == "t" => {
                self.close_menu();
                self.cycle_theme();
                true
            }
            // Run/Stop active cell
            Key::Named(NamedKey::Enter) => {
                self.close_menu();
                let active = self.tab_manager.active_tab().active_cell_index;
                let is_playing = self.tab_manager.active_tab().cells[active].is_playing;
                if is_playing {
                    self.trigger_cell_stop(active);
                } else {
                    self.trigger_cell_play(active);
                }
                true
            }
            _ => false,
        }
    }

    fn toggle_maximize(&mut self) {
        self.is_maximized = !self.is_maximized;
        self.window.set_maximized(self.is_maximized);
        self.renderer.set_maximized_icon(self.is_maximized);
    }

    /// Returns true if any drag operation is in progress.
    fn is_any_drag_active(&self) -> bool {
        self.is_dragging_split
            || self.is_dragging_v_scroll
            || self.render_area.is_dragging
            || self.is_dragging_editor
    }

    /// Compile and start playing a cell's shader.
    fn trigger_cell_play(&mut self, cell_index: usize) {
        let tab = self.tab_manager.active_tab();

        // Concatenate all cell texts up to and including this cell
        // (so earlier cells can define functions used by later ones)
        let mut source = String::new();
        for (i, cell) in tab.cells.iter().enumerate() {
            if i > cell_index {
                break;
            }
            if !source.is_empty() {
                source.push('\n');
            }
            source.push_str(cell.buffer.text());
        }

        // Lex → Parse → Generate WGSL
        match lang::compile(&source) {
            Ok(wgsl) => {
                let cell_id = tab.cells[cell_index].id;
                match self.renderer.compile_cell_shader(cell_id, &wgsl) {
                    Ok(()) => {
                        self.tab_manager.active_tab_mut().cells[cell_index].is_playing = true;
                        self.tab_manager.active_tab_mut().cells[cell_index].output = CellOutput::None;
                    }
                    Err(e) => {
                        log::error!("Shader compilation failed: {}", e);
                        self.tab_manager.active_tab_mut().cells[cell_index].output =
                            CellOutput::Error(format!("GPU: {}", e));
                    }
                }
            }
            Err(e) => {
                log::error!("Language pipeline error: {}", e);
                self.tab_manager.active_tab_mut().cells[cell_index].output =
                    CellOutput::Error(e);
            }
        }
        self.sync_active_tab();
    }

    /// Stop playing a cell's shader.
    fn trigger_cell_stop(&mut self, cell_index: usize) {
        let cell_id = self.tab_manager.active_tab().cells[cell_index].id;
        self.renderer.remove_cell_shader(cell_id);
        self.tab_manager.active_tab_mut().cells[cell_index].is_playing = false;
        self.tab_manager.active_tab_mut().cells[cell_index].output = CellOutput::None;
        self.sync_active_tab();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let window_attrs = Window::default_attributes()
            .with_inner_size(LogicalSize::new(1200.0, 800.0))
            .with_title("Logos")
            .with_decorations(false);
        let window = Arc::new(event_loop.create_window(window_attrs).unwrap());
        let renderer = pollster::block_on(Renderer::new(window.clone()));

        let tab_manager = TabManager::new();

        let mut layout = UiLayout::new();
        let size = window.inner_size();
        let cached_layout = layout.compute(size.width as f32, size.height as f32);

        let mut state = AppState {
            renderer,
            tab_manager,
            layout,
            cached_layout,
            window,
            modifiers: ModifiersState::empty(),
            pending_dialog: None,
            cursor_position: (0.0, 0.0),
            tab_hit_rects: Vec::new(),
            plus_button_rect: Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 },
            cell_layouts: Vec::new(),
            hover_target: HoverTarget::None,
            mouse_press_target: HoverTarget::None,
            split_left_width: split::DEFAULT_LEFT_WIDTH,
            is_dragging_split: false,
            is_dragging_v_scroll: false,
            scroll_drag_offset: 0.0,
            is_dragging_editor: false,
            editor_drag_cell: None,
            win_control_rects: WindowControlRects::default(),
            is_maximized: false,
            last_title_click: None,
            menu_item_rects: Vec::new(),
            open_menu: None,
            dropdown_item_rects: Vec::new(),
            render_area: RenderAreaState::default(),
            clipboard: arboard::Clipboard::new().ok(),
            autocomplete: AutocompleteState::new(),
            autocomplete_item_rects: Vec::new(),
            reduce_service: ReduceService::new(),
            last_edit_time: None,
            last_edited_cell: None,
        };

        state.sync_active_tab();
        self.state = Some(state);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(state) = &mut self.state else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::ModifiersChanged(mods) => {
                state.modifiers = mods.state();
            }

            WindowEvent::Resized(size) => {
                state.renderer.resize(size);
                let (w, h) = (size.width as f32, size.height as f32);
                state.split_left_width =
                    state.layout.clamp_left_width(state.split_left_width, w);
                state.cached_layout = state.layout.compute(w, h);
                state.recompute_hover();
                state.sync_active_tab();
                state.dismiss_autocomplete();
            }

            WindowEvent::CursorMoved { position, .. } => {
                state.cursor_position = (position.x as f32, position.y as f32);

                if state.is_dragging_v_scroll {
                    state.renderer.set_v_scroll_from_drag(
                        state.cursor_position.1,
                        state.scroll_drag_offset,
                    );
                    state.sync_active_tab();
                    state.dismiss_autocomplete();
                    state.window.request_redraw();
                } else if state.is_dragging_split {
                    let content_x = state.cached_layout.left_pane.x;
                    let desired_width = state.cursor_position.0 - content_x;

                    let size = state.window.inner_size();
                    let (w, h) = (size.width as f32, size.height as f32);
                    state.split_left_width =
                        state.layout.clamp_left_width(desired_width, w);
                    state.cached_layout = state.layout.compute(w, h);
                    state.sync_active_tab();
                    // Dismiss autocomplete — pane layout changed
                    state.dismiss_autocomplete();
                } else if state.render_area.is_dragging {
                    // Pan the render area
                    let (mx, my) = state.cursor_position;
                    let rp = state.cached_layout.right_pane;
                    let dx = mx - state.render_area.last_drag_pos.0;
                    let dy = my - state.render_area.last_drag_pos.1;

                    let x_range = state.render_area.axis_x_max - state.render_area.axis_x_min;
                    let y_range = state.render_area.axis_y_max - state.render_area.axis_y_min;

                    match state.render_area.mouse_zone {
                        MouseZone::Center => {
                            let world_dx = -dx * x_range / rp.w;
                            let world_dy = dy * y_range / rp.h;
                            state.render_area.axis_x_min += world_dx;
                            state.render_area.axis_x_max += world_dx;
                            state.render_area.axis_y_min += world_dy;
                            state.render_area.axis_y_max += world_dy;
                        }
                        MouseZone::XAxisEdge => {
                            let world_dx = -dx * x_range / rp.w;
                            state.render_area.axis_x_min += world_dx;
                            state.render_area.axis_x_max += world_dx;
                        }
                        MouseZone::YAxisEdge => {
                            let world_dy = dy * y_range / rp.h;
                            state.render_area.axis_y_min += world_dy;
                            state.render_area.axis_y_max += world_dy;
                        }
                    }

                    state.render_area.last_drag_pos = (mx, my);
                    state.window.request_redraw();
                } else if state.is_dragging_editor {
                    // Drag-to-select: extend selection to current mouse position
                    if let Some(cell_idx) = state.editor_drag_cell {
                        let (mx, my) = state.cursor_position;
                        if let Some(byte_offset) = state.renderer.hit_test_cell(cell_idx, mx, my) {
                            state.tab_manager.active_tab_mut().cells[cell_idx]
                                .buffer
                                .set_cursor_byte_extend(byte_offset);
                            state.sync_active_tab();
                        }
                    }
                } else {
                    state.recompute_hover();
                }
            }

            // --- Mouse Wheel ---
            WindowEvent::MouseWheel { delta, .. } => {
                let (_dx, dy) = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => {
                        (x * SCROLL_LINE_PIXELS, y * SCROLL_LINE_PIXELS)
                    }
                    winit::event::MouseScrollDelta::PixelDelta(pos) => {
                        (pos.x as f32, pos.y as f32)
                    }
                };

                let (mx, my) = state.cursor_position;

                // Check if cursor is over the render area (right pane) — zoom
                let rp = state.cached_layout.right_pane;
                if point_in_rect(mx, my, &rp) && dy.abs() > 0.001 {
                    let zone = detect_mouse_zone(mx, my, &rp);
                    let factor = if dy > 0.0 { 0.97 } else { 1.03 };

                    // Mouse position as ratio within the pane
                    let rel_x = ((mx - rp.x) / rp.w).clamp(0.0, 1.0);
                    let rel_y = (1.0 - (my - rp.y) / rp.h).clamp(0.0, 1.0);

                    let ra = &mut state.render_area;
                    match zone {
                        MouseZone::Center => {
                            zoom_axis(
                                &mut ra.axis_x_min,
                                &mut ra.axis_x_max,
                                factor,
                                rel_x,
                            );
                            zoom_axis(
                                &mut ra.axis_y_min,
                                &mut ra.axis_y_max,
                                factor,
                                rel_y,
                            );
                        }
                        MouseZone::XAxisEdge => {
                            zoom_axis(
                                &mut ra.axis_x_min,
                                &mut ra.axis_x_max,
                                factor,
                                rel_x,
                            );
                        }
                        MouseZone::YAxisEdge => {
                            zoom_axis(
                                &mut ra.axis_y_min,
                                &mut ra.axis_y_max,
                                factor,
                                rel_y,
                            );
                        }
                    }
                    state.window.request_redraw();
                }
                // Check if cursor is over the editor pane — scroll
                else {
                    let lp = state.cached_layout.left_pane;
                    if point_in_rect(mx, my, &lp) {
                        state.renderer.scroll_by(0.0, -dy);
                        state.cell_layouts = state.renderer.cell_layouts().to_vec();
                        // Dismiss autocomplete — cell positions shifted by scroll
                        state.dismiss_autocomplete();
                        state.recompute_hover();
                        state.window.request_redraw();
                    }
                }
            }

            // --- Mouse Pressed ---
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                // Capture hover target at press time so that small trackpad
                // scroll events between press and release can't steal the click.
                state.mouse_press_target = state.hover_target;

                match state.hover_target {
                    HoverTarget::SplitHandle => {
                        state.close_menu();
                        state.is_dragging_split = true;
                        state.window.set_cursor(CursorIcon::ColResize);
                        event_loop.set_control_flow(ControlFlow::Poll);
                    }
                    HoverTarget::VScrollThumb => {
                        state.close_menu();
                        if let Some(thumb) = state.renderer.v_thumb_rect() {
                            state.is_dragging_v_scroll = true;
                            state.scroll_drag_offset = state.cursor_position.1 - thumb.y;
                            event_loop.set_control_flow(ControlFlow::Poll);
                        }
                    }
                    HoverTarget::TitleBar => {
                        state.close_menu();
                        let now = Instant::now();
                        if let Some(last) = state.last_title_click {
                            if now.duration_since(last).as_millis() < DOUBLE_CLICK_MS {
                                state.toggle_maximize();
                                state.last_title_click = None;
                                return;
                            }
                        }
                        state.last_title_click = Some(now);
                        if let Err(e) = state.window.drag_window() {
                            log::warn!("drag_window failed: {}", e);
                        }
                    }
                    HoverTarget::MenuItem(i) => {
                        state.open_menu(i);
                    }
                    HoverTarget::DropdownItem(i) => {
                        if let Some(menu_idx) = state.open_menu {
                            state.handle_menu_action(event_loop, menu_idx, i);
                        }
                    }
                    HoverTarget::RenderArea => {
                        state.close_menu();
                        state.render_area.is_dragging = true;
                        state.render_area.last_drag_pos = state.cursor_position;
                        // Lock the zone at drag start
                        let rp = state.cached_layout.right_pane;
                        let (mx, my) = state.cursor_position;
                        state.render_area.mouse_zone = detect_mouse_zone(mx, my, &rp);
                        state.window.set_cursor(match state.render_area.mouse_zone {
                            MouseZone::Center => CursorIcon::Grabbing,
                            MouseZone::XAxisEdge => CursorIcon::EwResize,
                            MouseZone::YAxisEdge => CursorIcon::NsResize,
                        });
                        event_loop.set_control_flow(ControlFlow::Poll);
                    }
                    HoverTarget::CellEditor(i) => {
                        state.close_menu();
                        state.dismiss_autocomplete();
                        let (mx, my) = state.cursor_position;
                        if let Some(byte_offset) = state.renderer.hit_test_cell(i, mx, my) {
                            if state.modifiers.shift_key() {
                                // Shift+click: extend selection to clicked position
                                state.tab_manager.active_tab_mut().cells[i]
                                    .buffer
                                    .set_cursor_byte_extend(byte_offset);
                            } else {
                                // Normal click: position cursor, start potential drag
                                state.tab_manager.active_tab_mut().cells[i]
                                    .buffer
                                    .set_cursor_byte(byte_offset);
                            }
                        }
                        state.tab_manager.active_tab_mut().set_active_cell(i);
                        state.is_dragging_editor = true;
                        state.editor_drag_cell = Some(i);
                        event_loop.set_control_flow(ControlFlow::Poll);
                        state.sync_active_tab();
                    }
                    _ => {
                        // Click outside menus closes dropdown
                        if state.open_menu.is_some() {
                            state.close_menu();
                        }
                    }
                }
            }

            // --- Mouse Released ---
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                // End any drag operation
                if state.is_any_drag_active() {
                    let was_split = state.is_dragging_split;
                    let was_v_scroll = state.is_dragging_v_scroll;
                    let was_editor = state.is_dragging_editor;
                    state.is_dragging_split = false;
                    state.is_dragging_v_scroll = false;
                    state.render_area.is_dragging = false;
                    state.is_dragging_editor = false;
                    state.editor_drag_cell = None;
                    if state.pending_dialog.is_none() {
                        event_loop.set_control_flow(ControlFlow::Wait);
                    }
                    if was_v_scroll {
                        // Rebuild cell layouts with updated scroll position
                        state.sync_active_tab();
                    }
                    state.recompute_hover();
                    if was_split || was_editor {
                        // Split/editor drag already synced during move
                    }
                    return;
                }

                // Window control buttons
                match state.hover_target {
                    HoverTarget::WinBtnClose => {
                        event_loop.exit();
                        return;
                    }
                    HoverTarget::WinBtnMinimize => {
                        state.window.set_minimized(true);
                        return;
                    }
                    HoverTarget::WinBtnMaximize => {
                        state.toggle_maximize();
                        return;
                    }
                    _ => {}
                }

                // Menu/dropdown clicks handled in Pressed
                if matches!(state.hover_target, HoverTarget::MenuItem(_) | HoverTarget::DropdownItem(_)) {
                    return;
                }

                let (mx, my) = state.cursor_position;

                for (i, hit) in state.tab_hit_rects.iter().enumerate() {
                    if point_in_rect(mx, my, &hit.close) {
                        state.tab_manager.close_tab(i);
                        state.sync_active_tab();
                        return;
                    }
                }

                for (i, hit) in state.tab_hit_rects.iter().enumerate() {
                    if point_in_rect(mx, my, &hit.full) {
                        state.tab_manager.set_active(i);
                        // Clear pending REDUCE requests — new tab has its own cells
                        state.reduce_service.clear_pending();
                        state.last_edit_time = None;
                        state.last_edited_cell = None;
                        state.sync_active_tab();
                        return;
                    }
                }

                if point_in_rect(mx, my, &state.plus_button_rect) {
                    state.tab_manager.new_tab();
                    state.sync_active_tab();
                    return;
                }

                // Autocomplete click
                if let HoverTarget::AutocompleteItem(i) = state.hover_target {
                    state.autocomplete.selected_index = i;
                    state.accept_autocomplete();
                    return;
                }
                // Dismiss autocomplete on any other click
                state.dismiss_autocomplete();

                // Cell interactions — use the press-time target so that small
                // trackpad scroll events between press and release don't steal
                // button clicks (the scroll shifts layout rects, changing
                // hover_target before release arrives).
                let click_target = match state.mouse_press_target {
                    HoverTarget::CellPlayButton(_)
                    | HoverTarget::CellCopyButton(_)
                    | HoverTarget::CellDeleteButton(_)
                    | HoverTarget::AddCellButton => state.mouse_press_target,
                    _ => state.hover_target,
                };
                match click_target {
                    HoverTarget::CellPlayButton(i) => {
                        let is_playing = state.tab_manager.active_tab().cells[i].is_playing;
                        if is_playing {
                            state.trigger_cell_stop(i);
                        } else {
                            state.trigger_cell_play(i);
                        }
                    }
                    HoverTarget::CellEditor(_) => {
                        // Handled on press (drag-to-select); release is a no-op.
                    }
                    HoverTarget::CellCopyButton(i) => {
                        let text = state.tab_manager.active_tab().cells[i].buffer.text().to_string();
                        if let Some(cb) = state.clipboard.as_mut() {
                            let _ = cb.set_text(&text);
                        }
                    }
                    HoverTarget::CellDeleteButton(i) => {
                        // Stop shader if playing before removing
                        let cell = &state.tab_manager.active_tab().cells[i];
                        if cell.is_playing {
                            state.renderer.remove_cell_shader(cell.id);
                        }
                        state.tab_manager.active_tab_mut().remove_cell(i);
                        state.sync_active_tab();
                    }
                    HoverTarget::AddCellButton => {
                        state.tab_manager.active_tab_mut().add_cell();
                        state.sync_active_tab();
                    }
                    _ => {}
                }
            }

            WindowEvent::KeyboardInput {
                event: key_event,
                ..
            } => {
                if key_event.state != ElementState::Pressed {
                    return;
                }

                // Escape closes menu or autocomplete
                if key_event.logical_key == Key::Named(NamedKey::Escape) {
                    if state.autocomplete.active {
                        state.dismiss_autocomplete();
                        state.window.request_redraw();
                        return;
                    }
                    if state.open_menu.is_some() {
                        state.close_menu();
                        return;
                    }
                }

                // Autocomplete keyboard interception
                if state.autocomplete.active {
                    match &key_event.logical_key {
                        Key::Named(NamedKey::ArrowUp) => {
                            state.autocomplete.select_prev();
                            state.renderer.update_autocomplete_selection(
                                state.autocomplete.selected_index,
                            );
                            state.window.request_redraw();
                            return;
                        }
                        Key::Named(NamedKey::ArrowDown) => {
                            state.autocomplete.select_next();
                            state.renderer.update_autocomplete_selection(
                                state.autocomplete.selected_index,
                            );
                            state.window.request_redraw();
                            return;
                        }
                        Key::Named(NamedKey::Tab) | Key::Named(NamedKey::Enter) => {
                            state.accept_autocomplete();
                            return;
                        }
                        _ => {
                            // Fall through to normal key handling;
                            // autocomplete will be updated after the keystroke
                        }
                    }
                }

                if state.handle_shortcut(event_loop, &key_event.logical_key) {
                    return;
                }

                let shift = state.modifiers.shift_key();
                let changed = match key_event.logical_key {
                    Key::Named(NamedKey::Backspace) => {
                        state.tab_manager.active_tab_mut().active_cell_mut().buffer.backspace()
                    }
                    Key::Named(NamedKey::Delete) => {
                        state.tab_manager.active_tab_mut().active_cell_mut().buffer.delete()
                    }
                    Key::Named(NamedKey::Enter) => {
                        state.tab_manager.active_tab_mut().active_cell_mut().buffer.insert('\n');
                        true
                    }
                    Key::Named(NamedKey::ArrowLeft) => {
                        state.tab_manager.active_tab_mut().active_cell_mut().buffer.move_left(shift)
                    }
                    Key::Named(NamedKey::ArrowRight) => {
                        state.tab_manager.active_tab_mut().active_cell_mut().buffer.move_right(shift)
                    }
                    Key::Named(NamedKey::ArrowUp) => {
                        state.tab_manager.active_tab_mut().active_cell_mut().buffer.move_up(shift)
                    }
                    Key::Named(NamedKey::ArrowDown) => {
                        state.tab_manager.active_tab_mut().active_cell_mut().buffer.move_down(shift)
                    }
                    Key::Named(NamedKey::Home) => {
                        state.tab_manager.active_tab_mut().active_cell_mut().buffer.move_home(shift)
                    }
                    Key::Named(NamedKey::End) => {
                        state.tab_manager.active_tab_mut().active_cell_mut().buffer.move_end(shift)
                    }
                    _ => {
                        if state.modifiers.control_key() {
                            return;
                        }
                        if let Some(ref text) = key_event.text {
                            for c in text.chars() {
                                if !c.is_control() {
                                    state.tab_manager.active_tab_mut().active_cell_mut().buffer.insert(c);
                                }
                            }
                            true
                        } else {
                            false
                        }
                    }
                };

                if changed {
                    state.tab_manager.active_tab_mut().mark_modified();
                    // Record edit time for REDUCE debounce
                    state.last_edit_time = Some(Instant::now());
                    state.last_edited_cell =
                        Some(state.tab_manager.active_tab().active_cell_index);
                    state.sync_active_tab();
                    state.update_autocomplete();
                    state.window.request_redraw();
                } else {
                    // Cursor-only movement (arrows, home, end) — dismiss autocomplete
                    state.dismiss_autocomplete();
                    state.window.request_redraw();
                }
            }

            WindowEvent::RedrawRequested => {
                let rp = state.cached_layout.right_pane;
                let (mx, my) = state.cursor_position;
                let mouse_uv = if rp.w > 0.0 && rp.h > 0.0 {
                    [
                        ((mx - rp.x) / rp.w).clamp(0.0, 1.0),
                        (1.0 - (my - rp.y) / rp.h).clamp(0.0, 1.0),
                    ]
                } else {
                    [0.0, 0.0]
                };
                let render_params = RenderAreaParams {
                    axis_x_min: state.render_area.axis_x_min,
                    axis_x_max: state.render_area.axis_x_max,
                    axis_y_min: state.render_area.axis_y_min,
                    axis_y_max: state.render_area.axis_y_max,
                    mouse_uv,
                };

                state.renderer.render(
                    &state.cached_layout,
                    state.hover_target,
                    &state.win_control_rects,
                    state.is_dragging_split,
                    state.open_menu,
                    &render_params,
                );
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(state) = &mut self.state else {
            return;
        };

        let mut needs_redraw = false;

        // --- REDUCE: poll for completed responses ---
        while let Some(resp) = state.reduce_service.try_recv() {
            // Find the cell by id across all cells in the active tab
            let tab = state.tab_manager.active_tab_mut();
            if let Some(cell) = tab.cells.iter_mut().find(|c| c.id == resp.cell_id) {
                cell.output = match resp.result {
                    Ok(text) => {
                        if text.is_empty() {
                            CellOutput::None
                        } else {
                            CellOutput::Simplified(translate::from_reduce(&text))
                        }
                    }
                    Err(e) => CellOutput::Error(e),
                };
            }
            needs_redraw = true;
        }

        // --- REDUCE: check debounce timer and submit requests ---
        if let (Some(edit_time), Some(cell_idx)) =
            (state.last_edit_time, state.last_edited_cell)
        {
            if edit_time.elapsed() >= REDUCE_DEBOUNCE {
                // Debounce expired — send the expression to REDUCE
                state.last_edit_time = None;
                state.last_edited_cell = None;

                let tab = state.tab_manager.active_tab();
                if cell_idx < tab.cells.len() {
                    let cell = &tab.cells[cell_idx];
                    let text = cell.buffer.text().trim().to_string();
                    if !text.is_empty() {
                        let cell_id = cell.id;
                        let reduce_expr = translate::to_reduce(&text);
                        state.reduce_service.submit(cell_id, reduce_expr);

                        // Mark as simplifying
                        let cell_mut =
                            &mut state.tab_manager.active_tab_mut().cells[cell_idx];
                        cell_mut.output = CellOutput::Simplifying;
                        needs_redraw = true;
                    }
                }
            } else {
                // Still debouncing — need to poll again soon
                event_loop.set_control_flow(ControlFlow::Poll);
            }
        }

        if needs_redraw {
            state.sync_active_tab();
            // Re-position autocomplete popup since cell layouts may have shifted
            // (e.g., REDUCE output changed cell height).
            if state.autocomplete.active {
                state.update_autocomplete();
            }
        }

        // Continuous animation when shaders are active
        if state.renderer.has_active_shaders() {
            event_loop.set_control_flow(ControlFlow::Poll);
            state.window.request_redraw();
        } else if !state.is_any_drag_active()
            && state.pending_dialog.is_none()
            && state.last_edit_time.is_none()
        {
            event_loop.set_control_flow(ControlFlow::Wait);
        }

        if let Some(dialog) = &state.pending_dialog {
            event_loop.set_control_flow(ControlFlow::Poll);
            match dialog.poll() {
                DialogResult::Pending => {}
                DialogResult::Selected(path) => {
                    let kind = dialog.kind;
                    state.pending_dialog = None;
                    if !state.is_any_drag_active() {
                        event_loop.set_control_flow(ControlFlow::Wait);
                    }
                    match kind {
                        DialogKind::Open => {
                            if let Err(e) = state.tab_manager.open_file(&path) {
                                log::error!("Failed to open file: {}", e);
                            }
                        }
                        DialogKind::Save => {
                            if let Err(e) = state.tab_manager.active_tab_mut().save_as(&path) {
                                log::error!("Failed to save file: {}", e);
                            }
                        }
                    }
                    state.sync_active_tab();
                }
                DialogResult::Cancelled => {
                    state.pending_dialog = None;
                    if !state.is_any_drag_active() {
                        event_loop.set_control_flow(ControlFlow::Wait);
                    }
                }
            }
        }
    }
}

pub fn run() {
    let event_loop = EventLoop::new().unwrap();
    event_loop.run_app(&mut App { state: None }).unwrap();
}

fn compute_win_control_rects(title_bar: &Rect) -> WindowControlRects {
    let w = spacing::window_control_width();
    let h = title_bar.h;
    let right = title_bar.x + title_bar.w;
    let y = title_bar.y;

    WindowControlRects {
        close: Rect { x: right - w, y, w, h },
        maximize: Rect { x: right - w * 2.0, y, w, h },
        minimize: Rect { x: right - w * 3.0, y, w, h },
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

/// Detect which interaction zone the mouse is in within the render pane.
/// Bottom edge → X axis, left edge → Y axis, everything else → center.
fn detect_mouse_zone(mx: f32, my: f32, pane: &Rect) -> MouseZone {
    let rel_y = my - pane.y;
    let rel_x = mx - pane.x;
    if rel_y > pane.h - spacing::axis_zone_size() {
        MouseZone::XAxisEdge
    } else if rel_x < spacing::axis_zone_size() {
        MouseZone::YAxisEdge
    } else {
        MouseZone::Center
    }
}

/// Zoom an axis range around a cursor ratio (0..1), preserving the world point under the cursor.
fn zoom_axis(axis_min: &mut f32, axis_max: &mut f32, factor: f32, cursor_ratio: f32) {
    let range = *axis_max - *axis_min;
    let cursor_world = *axis_min + cursor_ratio * range;
    let new_range = range * factor;
    *axis_min = cursor_world - cursor_ratio * new_range;
    *axis_max = cursor_world + (1.0 - cursor_ratio) * new_range;
}
