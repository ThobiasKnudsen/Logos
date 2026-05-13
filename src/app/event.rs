use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

use crate::editor::autocomplete::AutocompleteState;
use crate::file_dialog::{self, DialogKind, DialogResult};
use crate::lang::lang_service::LangService;
use crate::lang::reduce::service::ReduceService;
use crate::notebook::CellState;
use crate::render::{RenderAreaParams, Renderer};
use crate::session::Session;
use crate::ui::layout::{Rect, UiLayout};
use crate::ui::theme::{fonts, spacing, split};

use super::cas::WgpuGpuDispatch;
use super::render_area::{MouseZone, RenderAreaState};
use super::{
    detect_mouse_zone, point_in_rect, win_pos, ActiveDrag, App, AppState, HoverTarget,
    WindowControlRects, DOUBLE_CLICK_MS, SCROLL_LINE_PIXELS,
};

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

        let reduce_service = std::rc::Rc::new(std::cell::RefCell::new(ReduceService::new()));
        // GPU factory: each notebook's `parallel for`/array dispatch goes
        // through a `WgpuGpuDispatch` holding shared `Arc`s of the
        // renderer's device & queue.
        let (gpu_device, gpu_queue) = renderer.gpu_arcs();
        let gpu_factory: crate::session::GpuFactory = Box::new(move || {
            Box::new(WgpuGpuDispatch::new(gpu_device.clone(), gpu_queue.clone()))
        });
        let session = Session::new(Some(reduce_service.clone()), Some(gpu_factory));

        let mut layout = UiLayout::new();
        let size = window.inner_size();
        let cached_layout = layout.compute(size.width as f32, size.height as f32);

        let mut state = AppState {
            renderer,
            session,
            layout,
            cached_layout,
            window,
            modifiers: ModifiersState::empty(),
            pending_dialog: None,
            cursor_position: (0.0, 0.0),
            tab_hit_rects: Vec::new(),
            plus_button_rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
            },
            cell_layouts: Vec::new(),
            hover_target: HoverTarget::None,
            mouse_press_target: HoverTarget::None,
            split_left_width: split::DEFAULT_LEFT_WIDTH,
            active_drag: ActiveDrag::None,
            win_control_rects: WindowControlRects::default(),
            is_maximized: false,
            last_title_click: None,
            menu_item_rects: Vec::new(),
            open_menu: None,
            dropdown_item_rects: Vec::new(),
            feedback_button_rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
            },
            open_color_picker: None,
            color_picker_hover_left_at: None,
            render_area: RenderAreaState::default(),
            clipboard: arboard::Clipboard::new().ok(),
            autocomplete: AutocompleteState::new(),
            autocomplete_item_rects: Vec::new(),
            reduce_service,
            lang_service: LangService::new(),
            cached_user_symbols: Vec::new(),
            last_submitted_texts: Vec::new(),
            last_frame_time: Instant::now(),
        };

        let wp = win_pos(&state.window);
        let ws = state.window.inner_size();
        state
            .render_area
            .adjust_for_pane_change(&state.cached_layout.right_pane, wp, ws.into());
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

            WindowEvent::Moved(_) => state.handle_window_moved(),

            WindowEvent::Resized(size) => state.handle_resize(size),

            WindowEvent::CursorMoved { position, .. } => state.handle_cursor_moved(position),

            WindowEvent::MouseWheel { delta, .. } => state.handle_mouse_wheel(delta),

            // Right-click on the color button opens the RGBA slider popup
            // anchored under that button. The color button is the only
            // right-click consumer right now.
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => state.handle_mouse_pressed_right(),

            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => state.handle_mouse_pressed_left(event_loop),

            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => state.handle_mouse_released_left(event_loop),

            WindowEvent::KeyboardInput {
                event: key_event, ..
            } => state.handle_keyboard_input(key_event, event_loop),

            WindowEvent::RedrawRequested => state.handle_redraw_requested(),

            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(state) = &mut self.state else {
            return;
        };

        let mut needs_redraw = false;

        while let Some(resp) = state.lang_service.try_recv() {
            state.cached_user_symbols = resp.user_symbols;
        }

        // REDUCE: drain responses through the active notebook.
        let updated = state.session.active_tab_mut().notebook.tick();
        if !updated.is_empty() {
            for cell_idx in &updated {
                state.sync_cell_from_notebook(*cell_idx);
            }
            needs_redraw = true;
        }

        // Auto-rerun: any cell that's been edited and idle long enough
        // gets replayed without the user pressing play.
        let now = Instant::now();
        let auto_played = state
            .session
            .active_tab_mut()
            .notebook
            .tick_auto_rerun(now);
        if !auto_played.is_empty() {
            for cell_idx in &auto_played {
                state.sync_cell_from_notebook(*cell_idx);
            }
            needs_redraw = true;
        }
        let auto_rerun_deadline = state
            .session
            .active_tab()
            .notebook
            .next_auto_rerun_deadline(now);

        if needs_redraw {
            state.sync_active_tab();
            if state.autocomplete.active {
                state.update_autocomplete();
            }
            state.window.request_redraw();
        }

        // Color-picker hover-out grace timer. Run before the wait/poll
        // arbitration so a still-pending deadline can override `Wait`.
        let picker_deadline = state.tick_color_picker_timer();

        if state.renderer.has_active_shaders() || state.reduce_service.borrow().has_pending() {
            const TARGET_FRAME_TIME: std::time::Duration = std::time::Duration::from_micros(16_667);
            let elapsed = state.last_frame_time.elapsed();
            if elapsed >= TARGET_FRAME_TIME {
                state.last_frame_time = Instant::now();
                state.window.request_redraw();
            } else {
                let wait_until = Instant::now() + (TARGET_FRAME_TIME - elapsed);
                event_loop.set_control_flow(ControlFlow::WaitUntil(wait_until));
            }
        } else if !state.is_any_drag_active() && state.pending_dialog.is_none() {
            let earliest = [picker_deadline, auto_rerun_deadline]
                .into_iter()
                .flatten()
                .min();
            match earliest {
                Some(deadline) => event_loop.set_control_flow(ControlFlow::WaitUntil(deadline)),
                None => event_loop.set_control_flow(ControlFlow::Wait),
            }
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
                            let old = state.session.active_index();
                            let old_id = state.session.tabs()[old].tab_id;
                            state.renderer.stash_tab_shaders(old_id);
                            if let Err(e) = state.session.open_file(&path) {
                                log::error!("Failed to open file: {}", e);
                                file_dialog::show_error(
                                    "Cannot open file",
                                    &format!("{}\n\n{}", path.display(), e),
                                );
                            } else {
                                let new_idx = state.session.active_index();
                                state.switch_tab_axis(old, new_idx);
                            }
                        }
                        DialogKind::Save => {
                            if let Err(e) = state.session.active_tab_mut().save_as(&path) {
                                log::error!("Failed to save file: {}", e);
                                file_dialog::show_error(
                                    "Cannot save file",
                                    &format!("{}\n\n{}", path.display(), e),
                                );
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

// Per-`WindowEvent` handler methods. `window_event` above is a thin
// dispatcher that picks the right handler based on the variant; the bodies
// live here so each event's logic is self-contained and easy to navigate.
impl AppState {
    pub(super) fn handle_window_moved(&mut self) {
        let cur = self.window.inner_size();
        let size_changed = cur.width != self.render_area.prev_win_size.0
            || cur.height != self.render_area.prev_win_size.1;
        if !size_changed {
            self.render_area.prev_win_pos = win_pos(&self.window);
        }
    }

    pub(super) fn handle_resize(&mut self, size: PhysicalSize<u32>) {
        self.renderer.resize(size);
        let (w, h) = (size.width as f32, size.height as f32);
        self.split_left_width = self.layout.clamp_left_width(self.split_left_width, w);
        self.cached_layout = self.layout.compute(w, h);
        let wp = win_pos(&self.window);
        self.render_area.adjust_for_pane_change(
            &self.cached_layout.right_pane,
            wp,
            (size.width, size.height),
        );
        self.recompute_hover();
        self.sync_active_tab();
        self.dismiss_autocomplete();
    }

    pub(super) fn handle_cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        self.cursor_position = (position.x as f32, position.y as f32);

        if let ActiveDrag::ColorPickerSlider { channel } = self.active_drag {
            self.update_color_picker_value_at(channel, self.cursor_position.1);
            self.window.request_redraw();
            return;
        } else if let ActiveDrag::VScroll { offset } = self.active_drag {
            self.renderer
                .set_v_scroll_from_drag(self.cursor_position.1, offset);
            self.sync_active_tab();
            self.dismiss_autocomplete();
            self.window.request_redraw();
        } else if matches!(self.active_drag, ActiveDrag::Split) {
            let content_x = self.cached_layout.left_pane.x;
            let desired_width = self.cursor_position.0 - content_x;

            let size = self.window.inner_size();
            let (w, h) = (size.width as f32, size.height as f32);
            self.split_left_width = self.layout.clamp_left_width(desired_width, w);
            self.cached_layout = self.layout.compute(w, h);
            let wp = win_pos(&self.window);
            self.render_area.adjust_for_pane_change(
                &self.cached_layout.right_pane,
                wp,
                (size.width, size.height),
            );
            self.sync_active_tab();
            self.dismiss_autocomplete();
        } else if self.render_area.is_dragging {
            let (mx, my) = self.cursor_position;
            let dx = mx - self.render_area.last_drag_pos.0;
            let dy = my - self.render_area.last_drag_pos.1;

            let rp = self.cached_layout.right_pane;
            let wpp_x = if rp.w > 0.0 {
                (self.render_area.axis_x_max - self.render_area.axis_x_min) / rp.w
            } else {
                self.render_area.world_per_pixel
            };
            let wpp_y = if rp.h > 0.0 {
                (self.render_area.axis_y_max - self.render_area.axis_y_min) / rp.h
            } else {
                self.render_area.world_per_pixel
            };

            match self.render_area.mouse_zone {
                MouseZone::Center => {
                    let world_dx = -dx * wpp_x;
                    let world_dy = dy * wpp_y;
                    self.render_area.axis_x_min += world_dx;
                    self.render_area.axis_x_max += world_dx;
                    self.render_area.axis_y_min += world_dy;
                    self.render_area.axis_y_max += world_dy;
                }
                MouseZone::XAxisEdge => {
                    let world_dx = -dx * wpp_x;
                    self.render_area.axis_x_min += world_dx;
                    self.render_area.axis_x_max += world_dx;
                }
                MouseZone::YAxisEdge => {
                    let world_dy = dy * wpp_y;
                    self.render_area.axis_y_min += world_dy;
                    self.render_area.axis_y_max += world_dy;
                }
            }

            self.render_area.last_drag_pos = (mx, my);
            self.window.request_redraw();
        } else if let ActiveDrag::CellResize { cell: idx, start_y, start_h } = self.active_drag {
            let delta_y = self.cursor_position.1 - start_y;
            let new_h = start_h + delta_y;
            let text_pad = spacing::sm();
            let min_h = fonts::editor_line_height() + text_pad * 2.0;
            let content_h = self
                .cell_layouts
                .iter()
                .find(|cl| cl.cell_index == idx)
                .map(|cl| cl.content_height)
                .unwrap_or(new_h);
            let natural_h = content_h + text_pad * 2.0;
            let clamped = new_h.clamp(min_h, natural_h);
            let tab = self.session.active_tab_mut();
            if idx < tab.cells().len() {
                if (clamped - natural_h).abs() < 1.0 {
                    tab.cell_mut(idx).contracted_editor_h = None;
                } else {
                    tab.cell_mut(idx).contracted_editor_h = Some(clamped);
                }
            }
            self.sync_active_tab();
            self.window.request_redraw();
        } else if let ActiveDrag::CellHScroll { cell: idx, offset } = self.active_drag {
            self.renderer
                .set_editor_h_scroll_from_drag(idx, self.cursor_position.0, offset);
            self.sync_active_tab();
            self.window.request_redraw();
        } else if let ActiveDrag::CellVScroll { cell: idx, offset } = self.active_drag {
            self.renderer
                .set_editor_v_scroll_from_drag(idx, self.cursor_position.1, offset);
            self.sync_active_tab();
            self.window.request_redraw();
        } else if let ActiveDrag::Editor { cell: cell_idx } = self.active_drag {
            let (mx, my) = self.cursor_position;
            if let Some(byte_offset) = self.renderer.hit_test_cell(cell_idx, mx, my) {
                self.session
                    .active_tab_mut()
                    .cell_mut(cell_idx)
                    .buffer
                    .set_cursor_byte_extend(byte_offset);
                self.sync_active_tab();
            }
        } else {
            self.recompute_hover();
            self.maybe_close_color_picker_on_hover();
        }
    }

    pub(super) fn handle_mouse_wheel(&mut self, delta: MouseScrollDelta) {
        let (dx, dy) = match delta {
            MouseScrollDelta::LineDelta(x, y) => (x * SCROLL_LINE_PIXELS, y * SCROLL_LINE_PIXELS),
            MouseScrollDelta::PixelDelta(pos) => (pos.x as f32, pos.y as f32),
        };

        let (mx, my) = self.cursor_position;

        let rp = self.cached_layout.right_pane;
        if point_in_rect(mx, my, &rp) && dy.abs() > 0.001 {
            let factor = if dy > 0.0 { 0.97 } else { 1.03 };
            let zone = detect_mouse_zone(mx, my, &rp);
            match zone {
                MouseZone::Center => self.render_area.zoom_uniform(factor, (mx, my), &rp),
                MouseZone::XAxisEdge => self.render_area.zoom_x(factor, (mx, my), &rp),
                MouseZone::YAxisEdge => self.render_area.zoom_y(factor, (mx, my), &rp),
            }
            self.window.request_redraw();
        } else {
            let lp = self.cached_layout.left_pane;
            if point_in_rect(mx, my, &lp) {
                let mut handled = false;

                for cl in &self.cell_layouts {
                    if cl.output.h > 0.0 && point_in_rect(mx, my, &cl.output) {
                        let h_delta = if dx.abs() > 0.001 { dx } else { -dy };
                        self.renderer.scroll_output_x(cl.cell_index, -h_delta);
                        handled = true;
                        self.window.request_redraw();
                        break;
                    }
                }

                if !handled {
                    let mut editor_cell = None;
                    for cl in &self.cell_layouts {
                        if point_in_rect(mx, my, &cl.editor) {
                            editor_cell = Some(cl.cell_index);
                            break;
                        }
                    }
                    if let Some(idx) = editor_cell {
                        if dx.abs() > 0.001 {
                            self.renderer.scroll_editor_x(idx, -dx);
                        }
                        if dy.abs() > 0.001 {
                            if self.renderer.cell_is_contracted(idx) {
                                let unconsumed = self.renderer.scroll_editor_y(idx, -dy);
                                if unconsumed.abs() > 0.001 {
                                    self.renderer.scroll_by(0.0, unconsumed);
                                    self.dismiss_autocomplete();
                                }
                            } else {
                                self.renderer.scroll_by(0.0, -dy);
                                self.dismiss_autocomplete();
                            }
                        }
                        self.sync_active_tab();
                        self.recompute_hover();
                        self.window.request_redraw();
                        handled = true;
                    }
                }

                if !handled {
                    self.renderer.scroll_by(0.0, -dy);
                    self.cell_layouts = self.renderer.cell_layouts().to_vec();
                    self.dismiss_autocomplete();
                    self.recompute_hover();
                    self.window.request_redraw();
                }
            }
        }
    }

    pub(super) fn handle_mouse_pressed_right(&mut self) {
        if let HoverTarget::CellColorButton(i) = self.hover_target {
            self.open_color_picker = Some(i);
            self.active_drag = ActiveDrag::None;
            self.sync_active_tab();
        }
    }

    pub(super) fn handle_mouse_pressed_left(&mut self, event_loop: &ActiveEventLoop) {
        self.mouse_press_target = self.hover_target;

        match self.hover_target {
            HoverTarget::ColorPickerSlider(channel) => {
                self.active_drag = ActiveDrag::ColorPickerSlider { channel };
                self.update_color_picker_value_at(channel, self.cursor_position.1);
                event_loop.set_control_flow(ControlFlow::Poll);
            }
            HoverTarget::ColorPickerArea => {
                // Click on the picker bg but outside any slider — no-op,
                // just keeps the popup open.
            }
            HoverTarget::WindowEdge(dir) => {
                if let Err(e) = self.window.drag_resize_window(dir) {
                    log::warn!("drag_resize_window failed: {}", e);
                }
            }
            HoverTarget::SplitHandle => {
                self.close_menu();
                self.active_drag = ActiveDrag::Split;
                self.window.set_cursor(CursorIcon::ColResize);
                event_loop.set_control_flow(ControlFlow::Poll);
            }
            HoverTarget::VScrollThumb => {
                self.close_menu();
                if let Some(thumb) = self.renderer.v_thumb_rect() {
                    self.active_drag = ActiveDrag::VScroll {
                        offset: self.cursor_position.1 - thumb.y,
                    };
                    event_loop.set_control_flow(ControlFlow::Poll);
                }
            }
            HoverTarget::TitleBar => {
                self.close_menu();
                let now = Instant::now();
                if let Some(last) = self.last_title_click {
                    if now.duration_since(last).as_millis() < DOUBLE_CLICK_MS {
                        self.toggle_maximize();
                        self.last_title_click = None;
                        return;
                    }
                }
                self.last_title_click = Some(now);
                if let Err(e) = self.window.drag_window() {
                    log::warn!("drag_window failed: {}", e);
                }
            }
            HoverTarget::MenuItem(i) => {
                self.open_menu(i);
            }
            HoverTarget::DropdownItem(i) => {
                if let Some(menu_idx) = self.open_menu {
                    self.handle_menu_action(event_loop, menu_idx, i);
                }
            }
            HoverTarget::RenderArea => {
                self.close_menu();
                self.render_area.is_dragging = true;
                self.render_area.last_drag_pos = self.cursor_position;
                let rp = self.cached_layout.right_pane;
                let (mx, my) = self.cursor_position;
                self.render_area.mouse_zone = detect_mouse_zone(mx, my, &rp);
                self.window.set_cursor(match self.render_area.mouse_zone {
                    MouseZone::Center => CursorIcon::Grabbing,
                    MouseZone::XAxisEdge => CursorIcon::EwResize,
                    MouseZone::YAxisEdge => CursorIcon::NsResize,
                });
                event_loop.set_control_flow(ControlFlow::Poll);
            }
            HoverTarget::CellEditor(i) => {
                self.close_menu();
                self.dismiss_autocomplete();
                let (mx, my) = self.cursor_position;
                if let Some(byte_offset) = self.renderer.hit_test_cell(i, mx, my) {
                    if self.modifiers.shift_key() {
                        self.session
                            .active_tab_mut()
                            .cell_mut(i)
                            .buffer
                            .set_cursor_byte_extend(byte_offset);
                    } else {
                        self.session
                            .active_tab_mut()
                            .cell_mut(i)
                            .buffer
                            .set_cursor_byte(byte_offset);
                    }
                }
                self.session.active_tab_mut().set_active_cell(i);
                self.active_drag = ActiveDrag::Editor { cell: i };
                event_loop.set_control_flow(ControlFlow::Poll);
                self.sync_active_tab();
            }
            HoverTarget::CellOutputToggle(_) => {
                self.close_menu();
                self.dismiss_autocomplete();
            }
            HoverTarget::RenderAreaToggle(_) => {
                // Press just records the target; the actual toggle
                // fires on release in `handle_mouse_released_left` so
                // dragging off the button before releasing cancels
                // the click (matching every other button in the UI).
                self.close_menu();
                self.dismiss_autocomplete();
            }
            HoverTarget::CellOutputCopyButton(i) => {
                self.close_menu();
                self.dismiss_autocomplete();
                let text_to_copy = self
                    .session
                    .active_tab()
                    .cell(i)
                    .outcome
                    .message
                    .as_ref()
                    .map(|m| m.display_text().to_string());
                if let Some(text) = text_to_copy {
                    if let Some(cb) = self.clipboard.as_mut() {
                        let _ = cb.set_text(&text);
                    }
                }
            }
            HoverTarget::CellResizeHandle(i) => {
                self.close_menu();
                let editor_h = self
                    .cell_layouts
                    .iter()
                    .find(|cl| cl.cell_index == i)
                    .map(|cl| cl.editor.h)
                    .unwrap_or(100.0);
                self.active_drag = ActiveDrag::CellResize {
                    cell: i,
                    start_y: self.cursor_position.1,
                    start_h: editor_h,
                };
                self.window.set_cursor(CursorIcon::NsResize);
                event_loop.set_control_flow(ControlFlow::Poll);
            }
            HoverTarget::CellEditorHScrollThumb(i) => {
                self.close_menu();
                let thumb_x = self
                    .cell_layouts
                    .iter()
                    .find(|cl| cl.cell_index == i)
                    .map(|cl| cl.editor_h_scrollbar_thumb.x)
                    .unwrap_or(0.0);
                self.active_drag = ActiveDrag::CellHScroll {
                    cell: i,
                    offset: self.cursor_position.0 - thumb_x,
                };
                event_loop.set_control_flow(ControlFlow::Poll);
            }
            HoverTarget::CellEditorVScrollThumb(i) => {
                self.close_menu();
                let thumb_y = self
                    .cell_layouts
                    .iter()
                    .find(|cl| cl.cell_index == i)
                    .map(|cl| cl.editor_v_scrollbar_thumb.y)
                    .unwrap_or(0.0);
                self.active_drag = ActiveDrag::CellVScroll {
                    cell: i,
                    offset: self.cursor_position.1 - thumb_y,
                };
                event_loop.set_control_flow(ControlFlow::Poll);
            }
            _ => {
                if self.open_menu.is_some() {
                    self.close_menu();
                }
            }
        }
    }

    pub(super) fn handle_mouse_released_left(&mut self, event_loop: &ActiveEventLoop) {
        if matches!(self.active_drag, ActiveDrag::ColorPickerSlider { .. }) {
            self.active_drag = ActiveDrag::None;
            if self.pending_dialog.is_none() {
                event_loop.set_control_flow(ControlFlow::Wait);
            }
            self.recompute_hover();
            return;
        }
        if self.is_any_drag_active() {
            let was_v_scroll = matches!(self.active_drag, ActiveDrag::VScroll { .. });
            self.active_drag = ActiveDrag::None;
            self.render_area.is_dragging = false;
            if self.pending_dialog.is_none() {
                event_loop.set_control_flow(ControlFlow::Wait);
            }
            if was_v_scroll {
                self.sync_active_tab();
            }
            self.recompute_hover();
            return;
        }

        match self.hover_target {
            HoverTarget::WinBtnClose => {
                event_loop.exit();
                return;
            }
            HoverTarget::WinBtnMinimize => {
                self.window.set_minimized(true);
                return;
            }
            HoverTarget::WinBtnMaximize => {
                self.toggle_maximize();
                return;
            }
            HoverTarget::FeedbackButton => {
                self.do_open_feedback_url();
                return;
            }
            _ => {}
        }

        if matches!(
            self.hover_target,
            HoverTarget::MenuItem(_) | HoverTarget::DropdownItem(_)
        ) {
            return;
        }

        let (mx, my) = self.cursor_position;

        for (i, hit) in self.tab_hit_rects.iter().enumerate() {
            if point_in_rect(mx, my, &hit.close) {
                self.close_tab_at(i);
                return;
            }
        }

        for (i, hit) in self.tab_hit_rects.iter().enumerate() {
            if point_in_rect(mx, my, &hit.full) {
                let old = self.session.active_index();
                if old != i {
                    let old_id = self.session.tabs()[old].tab_id;
                    let new_id = self.session.tabs()[i].tab_id;
                    self.renderer.stash_tab_shaders(old_id);
                    self.renderer.restore_tab_shaders(new_id);
                    self.switch_tab_axis(old, i);
                }
                self.session.set_active(i);
                // Clear in-flight REDUCE bookkeeping on both ends —
                // shared service and the (now-inactive) tab's notebook.
                self.reduce_service.borrow_mut().clear_pending();
                if let Some(prev) = self.session.tab_mut(old) {
                    prev.notebook.clear_pending();
                }
                self.invalidate_lang_cache();
                self.sync_active_tab();
                return;
            }
        }

        if point_in_rect(mx, my, &self.plus_button_rect) {
            let old = self.session.active_index();
            let old_id = self.session.tabs()[old].tab_id;
            self.renderer.stash_tab_shaders(old_id);
            let new_idx = self.session.new_tab();
            self.switch_tab_axis(old, new_idx);
            self.sync_active_tab();
            return;
        }

        if let HoverTarget::AutocompleteItem(i) = self.hover_target {
            self.autocomplete.selected_index = i;
            self.accept_autocomplete();
            return;
        }
        self.dismiss_autocomplete();

        let click_target = match self.mouse_press_target {
            HoverTarget::CellPlayButton(_)
            | HoverTarget::CellColorButton(_)
            | HoverTarget::CellCopyButton(_)
            | HoverTarget::CellOutputCopyButton(_)
            | HoverTarget::CellOutputToggle(_)
            | HoverTarget::CellDeleteButton(_)
            | HoverTarget::AddCellButton
            | HoverTarget::RenderAreaToggle(_) => self.mouse_press_target,
            _ => self.hover_target,
        };
        match click_target {
            HoverTarget::CellPlayButton(i) => {
                let is_playing =
                    self.session.active_tab().cell(i).state == CellState::Playing;
                if is_playing {
                    self.trigger_cell_stop(i);
                } else {
                    self.trigger_cell_play(i);
                }
            }
            HoverTarget::CellColorButton(i) => {
                self.cycle_cell_color(i, 1);
                // If the picker is open, follow the click to this
                // cell so the sliders mirror the new color and the
                // popup re-anchors to the right button.
                if self.open_color_picker.is_some() {
                    self.open_color_picker = Some(i);
                    self.sync_active_tab();
                }
            }
            HoverTarget::CellEditor(_) => {}
            HoverTarget::CellCopyButton(i) => {
                let text = self
                    .session
                    .active_tab()
                    .cell(i)
                    .buffer
                    .text()
                    .to_string();
                if let Some(cb) = self.clipboard.as_mut() {
                    let _ = cb.set_text(&text);
                }
            }
            HoverTarget::CellOutputCopyButton(i) => {
                let output_text = self
                    .session
                    .active_tab()
                    .cell(i)
                    .outcome
                    .message
                    .as_ref()
                    .map(|m| m.display_text().to_string())
                    .unwrap_or_default();
                if let Some(cb) = self.clipboard.as_mut() {
                    let _ = cb.set_text(&output_text);
                }
            }
            HoverTarget::CellOutputToggle(i) => {
                let cell = self.session.active_tab_mut().cell_mut(i);
                cell.output_collapsed = !cell.output_collapsed;
                self.sync_active_tab();
            }
            HoverTarget::CellDeleteButton(i) => {
                let cell = self.session.active_tab().cell(i);
                if cell.state == CellState::Playing {
                    self.renderer.remove_cell_shader(cell.id);
                }
                self.session.active_tab_mut().remove_cell(i);
                // Cell removal shifts indices, so the picker can no
                // longer trust its anchor — drop it.
                self.close_color_picker();
                self.invalidate_lang_cache();
                self.sync_active_tab();
            }
            HoverTarget::AddCellButton => {
                self.session.active_tab_mut().add_cell();
                self.invalidate_lang_cache();
                self.sync_active_tab();
            }
            HoverTarget::RenderAreaToggle(kind) => {
                // Flip the toggle and copy the result into the active
                // tab so it survives a tab switch. Each toggle is
                // independent (issue #27 acceptance #1); the renderer
                // re-reads `RenderAreaParams.toggles` on the next
                // frame and gates the corresponding visual.
                self.render_area.toggles.toggle(kind);
                if let Some(tab) = self.session.tab_mut(self.session.active_index()) {
                    tab.view_toggles = self.render_area.toggles;
                }
                self.window.request_redraw();
            }
            _ => {}
        }
    }

    pub(super) fn handle_keyboard_input(
        &mut self,
        key_event: KeyEvent,
        event_loop: &ActiveEventLoop,
    ) {
        if key_event.state != ElementState::Pressed {
            return;
        }

        if key_event.logical_key == Key::Named(NamedKey::Escape) {
            if self.open_color_picker.is_some() {
                self.close_color_picker();
                return;
            }
            if self.autocomplete.active {
                self.dismiss_autocomplete();
                self.window.request_redraw();
                return;
            }
            if self.open_menu.is_some() {
                self.close_menu();
                return;
            }
        }

        if self.autocomplete.active {
            match &key_event.logical_key {
                Key::Named(NamedKey::ArrowUp) => {
                    self.autocomplete.select_prev();
                    self.renderer
                        .update_autocomplete_selection(self.autocomplete.selected_index);
                    self.window.request_redraw();
                    return;
                }
                Key::Named(NamedKey::ArrowDown) => {
                    self.autocomplete.select_next();
                    self.renderer
                        .update_autocomplete_selection(self.autocomplete.selected_index);
                    self.window.request_redraw();
                    return;
                }
                Key::Named(NamedKey::Tab) => {
                    self.accept_autocomplete();
                    return;
                }
                Key::Named(NamedKey::Enter) => {
                    self.accept_autocomplete();
                    return;
                }
                Key::Named(NamedKey::Escape) => {
                    self.dismiss_autocomplete();
                    self.window.request_redraw();
                    return;
                }
                _ => {}
            }
        }

        if self.handle_shortcut(event_loop, &key_event.logical_key) {
            return;
        }

        let shift = self.modifiers.shift_key();
        let changed = match key_event.logical_key {
            Key::Named(NamedKey::Backspace) => self
                .session
                .active_tab_mut()
                .active_cell_mut()
                .buffer
                .backspace(),
            Key::Named(NamedKey::Delete) => self
                .session
                .active_tab_mut()
                .active_cell_mut()
                .buffer
                .delete(),
            Key::Named(NamedKey::Enter) => {
                self.session
                    .active_tab_mut()
                    .active_cell_mut()
                    .buffer
                    .insert_newline_auto_indent();
                true
            }
            Key::Named(NamedKey::Tab) => {
                self.session
                    .active_tab_mut()
                    .active_cell_mut()
                    .buffer
                    .insert_tab();
                true
            }
            Key::Named(NamedKey::ArrowLeft) => self
                .session
                .active_tab_mut()
                .active_cell_mut()
                .buffer
                .move_left(shift),
            Key::Named(NamedKey::ArrowRight) => self
                .session
                .active_tab_mut()
                .active_cell_mut()
                .buffer
                .move_right(shift),
            Key::Named(NamedKey::ArrowUp) => self
                .session
                .active_tab_mut()
                .active_cell_mut()
                .buffer
                .move_up(shift),
            Key::Named(NamedKey::ArrowDown) => self
                .session
                .active_tab_mut()
                .active_cell_mut()
                .buffer
                .move_down(shift),
            Key::Named(NamedKey::Home) => self
                .session
                .active_tab_mut()
                .active_cell_mut()
                .buffer
                .move_home(shift),
            Key::Named(NamedKey::End) => self
                .session
                .active_tab_mut()
                .active_cell_mut()
                .buffer
                .move_end(shift),
            _ => {
                if self.modifiers.control_key() {
                    return;
                }
                if let Some(ref text) = key_event.text {
                    for c in text.chars() {
                        if c.is_control() {
                            continue;
                        }
                        self.session
                            .active_tab_mut()
                            .active_cell_mut()
                            .buffer
                            .insert(c);
                        // After each char, see if the user just
                        // completed a `\command` followed by a
                        // non-identifier delimiter — if so, convert
                        // the command to its Unicode symbol in
                        // place. Means `\integral(` produces `∫(`
                        // without ever showing or accepting the
                        // autocomplete popup.
                        self.try_auto_complete_latex_command();
                    }
                    true
                } else {
                    false
                }
            }
        };

        if changed {
            let key_start = Instant::now();
            self.session.active_tab_mut().mark_modified();
            // Stamp the active cell so the auto-rerun loop knows the
            // user just typed. The actual replay fires from
            // AboutToWait after 200ms of further silence.
            let active_cell_idx = self.session.active_tab().active_cell_index;
            self.session
                .active_tab_mut()
                .notebook
                .mark_edited(active_cell_idx, key_start);
            log::debug!(
                "[perf] --- KEYSTROKE --- (1st sync: text changed, no highlight spans yet)"
            );
            self.sync_active_tab();
            let t_sync = key_start.elapsed().as_micros();
            let ac_start = Instant::now();
            self.update_autocomplete();
            let t_ac = ac_start.elapsed().as_micros();
            let t_total = key_start.elapsed().as_micros();
            log::debug!(
                "[perf] keystroke done: {}us total | sync={}us autocomplete={}us",
                t_total,
                t_sync,
                t_ac
            );
            self.window.request_redraw();
        } else {
            match &key_event.logical_key {
                Key::Named(NamedKey::ArrowLeft)
                | Key::Named(NamedKey::ArrowRight)
                | Key::Named(NamedKey::ArrowUp)
                | Key::Named(NamedKey::ArrowDown)
                | Key::Named(NamedKey::Home)
                | Key::Named(NamedKey::End) => {
                    self.dismiss_autocomplete();
                }
                _ => {}
            }
            self.window.request_redraw();
        }
    }

    pub(super) fn handle_redraw_requested(&mut self) {
        let rp = self.cached_layout.right_pane;
        let (mx, my) = self.cursor_position;
        let mouse_uv = if rp.w > 0.0 && rp.h > 0.0 {
            [
                ((mx - rp.x) / rp.w).clamp(0.0, 1.0),
                (1.0 - (my - rp.y) / rp.h).clamp(0.0, 1.0),
            ]
        } else {
            [0.0, 0.0]
        };
        let render_params = RenderAreaParams {
            axis_x_min: self.render_area.axis_x_min,
            axis_x_max: self.render_area.axis_x_max,
            axis_y_min: self.render_area.axis_y_min,
            axis_y_max: self.render_area.axis_y_max,
            mouse_uv,
            toggles: self.render_area.toggles,
        };

        let picker_color = self.open_color_picker.and_then(|i| {
            self.session
                .active_tab()
                .cells()
                .get(i)
                .map(|c| c.plot_color)
        });
        self.renderer.render(
            &self.cached_layout,
            self.hover_target,
            &self.win_control_rects,
            matches!(self.active_drag, ActiveDrag::Split),
            self.open_menu,
            &render_params,
            picker_color,
        );
    }
}
