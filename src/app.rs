use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

use crate::file_dialog::{DialogKind, DialogResult, FileDialog};
use crate::render::{Renderer, TabHitRect, TabInfo};
use crate::session::TabManager;
use crate::ui::layout::{LayoutResult, Rect, UiLayout};
use crate::ui::theme::{fonts, spacing, split};

// ---------------------------------------------------------------------------
// Menu definitions
// ---------------------------------------------------------------------------

pub(crate) struct MenuItemDef {
    pub label: &'static str,
    pub shortcut: &'static str,
}

pub(crate) const MENU_NAMES: &[&str] = &["File", "Edit", "View", "Help"];

const MENU_FILE_ITEMS: &[MenuItemDef] = &[
    MenuItemDef { label: "New Tab", shortcut: "Ctrl+N" },
    MenuItemDef { label: "Open...", shortcut: "Ctrl+O" },
    MenuItemDef { label: "Save", shortcut: "Ctrl+S" },
    MenuItemDef { label: "Save As...", shortcut: "Ctrl+Shift+S" },
    MenuItemDef { label: "Close Tab", shortcut: "Ctrl+W" },
    MenuItemDef { label: "Quit", shortcut: "Ctrl+Q" },
];

const MENU_VIEW_ITEMS: &[MenuItemDef] = &[
    MenuItemDef { label: "Zoom In", shortcut: "Ctrl+=" },
    MenuItemDef { label: "Zoom Out", shortcut: "Ctrl+-" },
    MenuItemDef { label: "Reset Zoom", shortcut: "Ctrl+0" },
];

pub(crate) fn menu_items(index: usize) -> &'static [MenuItemDef] {
    match index {
        0 => MENU_FILE_ITEMS,
        2 => MENU_VIEW_ITEMS,
        _ => &[],
    }
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
    HScrollThumb,
    VScrollThumb,
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

    // Hover
    hover_target: HoverTarget,

    // Split dragging
    split_ratio: f32,
    is_dragging_split: bool,

    // Scrollbar dragging
    is_dragging_h_scroll: bool,
    is_dragging_v_scroll: bool,
    scroll_drag_offset: f32,

    // Window controls
    win_control_rects: WindowControlRects,
    is_maximized: bool,

    // Double-click detection for title bar
    last_title_click: Option<Instant>,

    // Menu state
    menu_item_rects: Vec<Rect>,
    open_menu: Option<usize>,
    dropdown_item_rects: Vec<Rect>,
}

const DOUBLE_CLICK_MS: u128 = 400;
const SCROLL_LINE_PIXELS: f32 = 40.0;

impl AppState {
    /// Sync renderer with the active tab's buffer and all tab infos.
    fn sync_active_tab(&mut self) {
        let lp = self.cached_layout.left_pane;
        let tab = self.tab_manager.active_tab();
        self.renderer.update_text(
            tab.buffer.text(),
            tab.buffer.cursor_byte_offset(),
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

        let (hit_rects, plus_rect) = self
            .renderer
            .update_tab_bar(&tab_infos, self.cached_layout.tab_bar);
        self.tab_hit_rects = hit_rects;
        self.plus_button_rect = plus_rect;

        // Compute window control rects from title bar
        self.win_control_rects = compute_win_control_rects(&self.cached_layout.title_bar);

        // Update menu item rects
        self.menu_item_rects = self
            .renderer
            .update_menu_items(self.cached_layout.title_bar, self.win_control_rects.minimize.x);

        // Update status bar
        let tab = self.tab_manager.active_tab();
        let (line, col) = line_col_from(tab.buffer.text(), tab.buffer.cursor_byte_offset());
        let line_count = tab.buffer.text().lines().count().max(1);
        let name = tab
            .file_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| tab.name.clone());
        let modified = if tab.is_modified { " [modified]" } else { "" };
        self.renderer.update_status(&format!(
            "{}{} \u{2502} {} lines \u{2502} Ln {}, Col {}",
            name,
            modified,
            line_count,
            line + 1,
            col + 1,
        ));

        self.window.request_redraw();
    }

    /// Determine what the cursor is hovering over and set the cursor icon.
    fn recompute_hover(&mut self) {
        let (mx, my) = self.cursor_position;
        let wc = &self.win_control_rects;

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

        // Scrollbar thumbs (before general editor area)
        if let Some(thumb) = self.renderer.v_thumb_rect() {
            if point_in_rect(mx, my, &thumb) {
                self.set_hover(HoverTarget::VScrollThumb);
                return;
            }
        }
        if let Some(thumb) = self.renderer.h_thumb_rect() {
            if point_in_rect(mx, my, &thumb) {
                self.set_hover(HoverTarget::HScrollThumb);
                return;
            }
        }

        self.set_hover(HoverTarget::None);
    }

    fn set_hover(&mut self, target: HoverTarget) {
        if self.hover_target != target {
            self.hover_target = target;
            let icon = match target {
                HoverTarget::SplitHandle => CursorIcon::ColResize,
                _ => CursorIcon::Default,
            };
            self.window.set_cursor(icon);
            self.window.request_redraw();
        }
    }

    fn open_menu(&mut self, index: usize) {
        if self.open_menu == Some(index) {
            self.close_menu();
            return;
        }
        let menu_rect = self.menu_item_rects[index];
        self.dropdown_item_rects = self.renderer.open_dropdown(index, menu_rect);
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
            // View menu
            (2, 0) => { self.do_zoom(|_| fonts::zoom_in()); }
            (2, 1) => { self.do_zoom(|_| fonts::zoom_out()); }
            (2, 2) => { self.do_zoom(|_| fonts::reset_zoom()); }
            _ => {}
        }
    }

    fn do_zoom(&mut self, f: impl FnOnce(&mut Self)) {
        f(self);
        self.renderer.rebuild_labels();
        self.layout.apply_scale();
        let size = self.window.inner_size();
        self.cached_layout = self.layout.compute(size.width as f32, size.height as f32);
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
        self.is_dragging_split || self.is_dragging_h_scroll || self.is_dragging_v_scroll
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
            hover_target: HoverTarget::None,
            split_ratio: split::DEFAULT_RATIO,
            is_dragging_split: false,
            is_dragging_h_scroll: false,
            is_dragging_v_scroll: false,
            scroll_drag_offset: 0.0,
            win_control_rects: WindowControlRects::default(),
            is_maximized: false,
            last_title_click: None,
            menu_item_rects: Vec::new(),
            open_menu: None,
            dropdown_item_rects: Vec::new(),
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
                state.cached_layout =
                    state.layout.compute(size.width as f32, size.height as f32);
                state.recompute_hover();
                state.sync_active_tab();
            }

            WindowEvent::CursorMoved { position, .. } => {
                state.cursor_position = (position.x as f32, position.y as f32);

                if state.is_dragging_h_scroll {
                    state.renderer.set_h_scroll_from_drag(
                        state.cursor_position.0,
                        state.scroll_drag_offset,
                    );
                    state.window.request_redraw();
                } else if state.is_dragging_v_scroll {
                    state.renderer.set_v_scroll_from_drag(
                        state.cursor_position.1,
                        state.scroll_drag_offset,
                    );
                    state.window.request_redraw();
                } else if state.is_dragging_split {
                    let total_w = state.cached_layout.left_pane.w
                        + state.cached_layout.split_handle.w
                        + state.cached_layout.right_pane.w;

                    if total_w > 0.0 {
                        let content_x = state.cached_layout.left_pane.x;
                        let cursor_in_content = state.cursor_position.0 - content_x;
                        let mut ratio = cursor_in_content / total_w;

                        let min_ratio = split::MIN_PANE_SIZE / total_w;
                        let max_ratio = 1.0 - min_ratio;
                        ratio = ratio.clamp(min_ratio, max_ratio);

                        state.split_ratio = ratio;
                        state.layout.set_split_ratio(ratio);
                        let size = state.window.inner_size();
                        state.cached_layout = state.layout.compute(
                            size.width as f32,
                            size.height as f32,
                        );
                        state.sync_active_tab();
                    }
                } else {
                    state.recompute_hover();
                }
            }

            // --- Mouse Wheel ---
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => {
                        (x * SCROLL_LINE_PIXELS, y * SCROLL_LINE_PIXELS)
                    }
                    winit::event::MouseScrollDelta::PixelDelta(pos) => {
                        (pos.x as f32, pos.y as f32)
                    }
                };

                // Check if cursor is over the editor pane
                let lp = state.cached_layout.left_pane;
                let (mx, my) = state.cursor_position;
                if mx >= lp.x && mx <= lp.x + lp.w && my >= lp.y && my <= lp.y + lp.h {
                    if state.modifiers.shift_key() {
                        // Shift+scroll = horizontal
                        state.renderer.scroll_by(-dy, 0.0);
                    } else {
                        // Normal scroll: vertical (negate Y because scroll up = negative delta)
                        state.renderer.scroll_by(-dx, -dy);
                    }
                    state.window.request_redraw();
                }
            }

            // --- Mouse Pressed ---
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                match state.hover_target {
                    HoverTarget::SplitHandle => {
                        state.close_menu();
                        state.is_dragging_split = true;
                        state.window.set_cursor(CursorIcon::ColResize);
                        event_loop.set_control_flow(ControlFlow::Poll);
                    }
                    HoverTarget::HScrollThumb => {
                        state.close_menu();
                        if let Some(thumb) = state.renderer.h_thumb_rect() {
                            state.is_dragging_h_scroll = true;
                            state.scroll_drag_offset = state.cursor_position.0 - thumb.x;
                            event_loop.set_control_flow(ControlFlow::Poll);
                        }
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
                    state.is_dragging_split = false;
                    state.is_dragging_h_scroll = false;
                    state.is_dragging_v_scroll = false;
                    if state.pending_dialog.is_none() {
                        event_loop.set_control_flow(ControlFlow::Wait);
                    }
                    state.recompute_hover();
                    if was_split {
                        // Split drag already synced during move
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
                        state.sync_active_tab();
                        return;
                    }
                }

                if point_in_rect(mx, my, &state.plus_button_rect) {
                    state.tab_manager.new_tab();
                    state.sync_active_tab();
                }
            }

            WindowEvent::KeyboardInput {
                event: key_event,
                ..
            } => {
                if key_event.state != ElementState::Pressed {
                    return;
                }

                // Escape closes menu
                if key_event.logical_key == Key::Named(NamedKey::Escape) {
                    if state.open_menu.is_some() {
                        state.close_menu();
                        return;
                    }
                }

                if state.handle_shortcut(event_loop, &key_event.logical_key) {
                    return;
                }

                let changed = match key_event.logical_key {
                    Key::Named(NamedKey::Backspace) => {
                        state.tab_manager.active_tab_mut().buffer.backspace()
                    }
                    Key::Named(NamedKey::Delete) => {
                        state.tab_manager.active_tab_mut().buffer.delete()
                    }
                    Key::Named(NamedKey::Enter) => {
                        state.tab_manager.active_tab_mut().buffer.insert('\n');
                        true
                    }
                    Key::Named(NamedKey::ArrowLeft) => {
                        state.tab_manager.active_tab_mut().buffer.move_left()
                    }
                    Key::Named(NamedKey::ArrowRight) => {
                        state.tab_manager.active_tab_mut().buffer.move_right()
                    }
                    Key::Named(NamedKey::ArrowUp) => {
                        state.tab_manager.active_tab_mut().buffer.move_up()
                    }
                    Key::Named(NamedKey::ArrowDown) => {
                        state.tab_manager.active_tab_mut().buffer.move_down()
                    }
                    Key::Named(NamedKey::Home) => {
                        state.tab_manager.active_tab_mut().buffer.move_home()
                    }
                    Key::Named(NamedKey::End) => {
                        state.tab_manager.active_tab_mut().buffer.move_end()
                    }
                    _ => {
                        if state.modifiers.control_key() {
                            return;
                        }
                        if let Some(ref text) = key_event.text {
                            for c in text.chars() {
                                if !c.is_control() {
                                    state.tab_manager.active_tab_mut().buffer.insert(c);
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
                    state.sync_active_tab();
                }
            }

            WindowEvent::RedrawRequested => {
                state.renderer.render(
                    &state.cached_layout,
                    state.hover_target,
                    &state.win_control_rects,
                    state.is_dragging_split,
                    state.open_menu,
                );
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(state) = &mut self.state else {
            return;
        };

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
