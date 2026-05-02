use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

use crate::editor::autocomplete::AutocompleteState;
use crate::file_dialog::{DialogKind, DialogResult};
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
    detect_mouse_zone, point_in_rect, win_pos, App, AppState, HoverTarget, WindowControlRects,
    DOUBLE_CLICK_MS, SCROLL_LINE_PIXELS,
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
            is_dragging_split: false,
            is_dragging_v_scroll: false,
            scroll_drag_offset: 0.0,
            is_dragging_editor: false,
            editor_drag_cell: None,
            is_dragging_cell_resize: false,
            cell_resize_index: None,
            cell_resize_start_y: 0.0,
            cell_resize_start_h: 0.0,
            is_dragging_cell_h_scroll: false,
            cell_h_scroll_index: None,
            cell_h_scroll_drag_offset: 0.0,
            is_dragging_cell_v_scroll: false,
            cell_v_scroll_index: None,
            cell_v_scroll_drag_offset: 0.0,
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

            WindowEvent::Moved(_) => {
                let cur = state.window.inner_size();
                let size_changed = cur.width != state.render_area.prev_win_size.0
                    || cur.height != state.render_area.prev_win_size.1;
                if !size_changed {
                    state.render_area.prev_win_pos = win_pos(&state.window);
                }
            }

            WindowEvent::Resized(size) => {
                state.renderer.resize(size);
                let (w, h) = (size.width as f32, size.height as f32);
                state.split_left_width = state.layout.clamp_left_width(state.split_left_width, w);
                state.cached_layout = state.layout.compute(w, h);
                let wp = win_pos(&state.window);
                state.render_area.adjust_for_pane_change(
                    &state.cached_layout.right_pane,
                    wp,
                    (size.width, size.height),
                );
                state.recompute_hover();
                state.sync_active_tab();
                state.dismiss_autocomplete();
            }

            WindowEvent::CursorMoved { position, .. } => {
                state.cursor_position = (position.x as f32, position.y as f32);

                if state.is_dragging_v_scroll {
                    state
                        .renderer
                        .set_v_scroll_from_drag(state.cursor_position.1, state.scroll_drag_offset);
                    state.sync_active_tab();
                    state.dismiss_autocomplete();
                    state.window.request_redraw();
                } else if state.is_dragging_split {
                    let content_x = state.cached_layout.left_pane.x;
                    let desired_width = state.cursor_position.0 - content_x;

                    let size = state.window.inner_size();
                    let (w, h) = (size.width as f32, size.height as f32);
                    state.split_left_width = state.layout.clamp_left_width(desired_width, w);
                    state.cached_layout = state.layout.compute(w, h);
                    let wp = win_pos(&state.window);
                    state.render_area.adjust_for_pane_change(
                        &state.cached_layout.right_pane,
                        wp,
                        (size.width, size.height),
                    );
                    state.sync_active_tab();
                    state.dismiss_autocomplete();
                } else if state.render_area.is_dragging {
                    let (mx, my) = state.cursor_position;
                    let dx = mx - state.render_area.last_drag_pos.0;
                    let dy = my - state.render_area.last_drag_pos.1;

                    let rp = state.cached_layout.right_pane;
                    let wpp_x = if rp.w > 0.0 {
                        (state.render_area.axis_x_max - state.render_area.axis_x_min) / rp.w
                    } else {
                        state.render_area.world_per_pixel
                    };
                    let wpp_y = if rp.h > 0.0 {
                        (state.render_area.axis_y_max - state.render_area.axis_y_min) / rp.h
                    } else {
                        state.render_area.world_per_pixel
                    };

                    match state.render_area.mouse_zone {
                        MouseZone::Center => {
                            let world_dx = -dx * wpp_x;
                            let world_dy = dy * wpp_y;
                            state.render_area.axis_x_min += world_dx;
                            state.render_area.axis_x_max += world_dx;
                            state.render_area.axis_y_min += world_dy;
                            state.render_area.axis_y_max += world_dy;
                        }
                        MouseZone::XAxisEdge => {
                            let world_dx = -dx * wpp_x;
                            state.render_area.axis_x_min += world_dx;
                            state.render_area.axis_x_max += world_dx;
                        }
                        MouseZone::YAxisEdge => {
                            let world_dy = dy * wpp_y;
                            state.render_area.axis_y_min += world_dy;
                            state.render_area.axis_y_max += world_dy;
                        }
                    }

                    state.render_area.last_drag_pos = (mx, my);
                    state.window.request_redraw();
                } else if state.is_dragging_cell_resize {
                    if let Some(idx) = state.cell_resize_index {
                        let delta_y = state.cursor_position.1 - state.cell_resize_start_y;
                        let new_h = state.cell_resize_start_h + delta_y;
                        let text_pad = spacing::sm();
                        let min_h = fonts::editor_line_height() + text_pad * 2.0;
                        let content_h = state
                            .cell_layouts
                            .iter()
                            .find(|cl| cl.cell_index == idx)
                            .map(|cl| cl.content_height)
                            .unwrap_or(new_h);
                        let natural_h = content_h + text_pad * 2.0;
                        let clamped = new_h.clamp(min_h, natural_h);
                        let tab = state.session.active_tab_mut();
                        if idx < tab.cells().len() {
                            if (clamped - natural_h).abs() < 1.0 {
                                tab.cell_mut(idx).contracted_editor_h = None;
                            } else {
                                tab.cell_mut(idx).contracted_editor_h = Some(clamped);
                            }
                        }
                        state.sync_active_tab();
                        state.window.request_redraw();
                    }
                } else if state.is_dragging_cell_h_scroll {
                    if let Some(idx) = state.cell_h_scroll_index {
                        state.renderer.set_editor_h_scroll_from_drag(
                            idx,
                            state.cursor_position.0,
                            state.cell_h_scroll_drag_offset,
                        );
                        state.sync_active_tab();
                        state.window.request_redraw();
                    }
                } else if state.is_dragging_cell_v_scroll {
                    if let Some(idx) = state.cell_v_scroll_index {
                        state.renderer.set_editor_v_scroll_from_drag(
                            idx,
                            state.cursor_position.1,
                            state.cell_v_scroll_drag_offset,
                        );
                        state.sync_active_tab();
                        state.window.request_redraw();
                    }
                } else if state.is_dragging_editor {
                    if let Some(cell_idx) = state.editor_drag_cell {
                        let (mx, my) = state.cursor_position;
                        if let Some(byte_offset) = state.renderer.hit_test_cell(cell_idx, mx, my) {
                            state
                                .session
                                .active_tab_mut()
                                .cell_mut(cell_idx)
                                .buffer
                                .set_cursor_byte_extend(byte_offset);
                            state.sync_active_tab();
                        }
                    }
                } else {
                    state.recompute_hover();
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => {
                        (x * SCROLL_LINE_PIXELS, y * SCROLL_LINE_PIXELS)
                    }
                    winit::event::MouseScrollDelta::PixelDelta(pos) => (pos.x as f32, pos.y as f32),
                };

                let (mx, my) = state.cursor_position;

                let rp = state.cached_layout.right_pane;
                if point_in_rect(mx, my, &rp) && dy.abs() > 0.001 {
                    let factor = if dy > 0.0 { 0.97 } else { 1.03 };
                    let zone = detect_mouse_zone(mx, my, &rp);
                    match zone {
                        MouseZone::Center => state.render_area.zoom_uniform(factor, (mx, my), &rp),
                        MouseZone::XAxisEdge => state.render_area.zoom_x(factor, (mx, my), &rp),
                        MouseZone::YAxisEdge => state.render_area.zoom_y(factor, (mx, my), &rp),
                    }
                    state.window.request_redraw();
                } else {
                    let lp = state.cached_layout.left_pane;
                    if point_in_rect(mx, my, &lp) {
                        let mut handled = false;

                        for cl in &state.cell_layouts {
                            if cl.output.h > 0.0 && point_in_rect(mx, my, &cl.output) {
                                let h_delta = if dx.abs() > 0.001 { dx } else { -dy };
                                state.renderer.scroll_output_x(cl.cell_index, -h_delta);
                                handled = true;
                                state.window.request_redraw();
                                break;
                            }
                        }

                        if !handled {
                            let mut editor_cell = None;
                            for cl in &state.cell_layouts {
                                if point_in_rect(mx, my, &cl.editor) {
                                    editor_cell = Some(cl.cell_index);
                                    break;
                                }
                            }
                            if let Some(idx) = editor_cell {
                                if dx.abs() > 0.001 {
                                    state.renderer.scroll_editor_x(idx, -dx);
                                }
                                if dy.abs() > 0.001 {
                                    if state.renderer.cell_is_contracted(idx) {
                                        let unconsumed = state.renderer.scroll_editor_y(idx, -dy);
                                        if unconsumed.abs() > 0.001 {
                                            state.renderer.scroll_by(0.0, unconsumed);
                                            state.dismiss_autocomplete();
                                        }
                                    } else {
                                        state.renderer.scroll_by(0.0, -dy);
                                        state.dismiss_autocomplete();
                                    }
                                }
                                state.sync_active_tab();
                                state.recompute_hover();
                                state.window.request_redraw();
                                handled = true;
                            }
                        }

                        if !handled {
                            state.renderer.scroll_by(0.0, -dy);
                            state.cell_layouts = state.renderer.cell_layouts().to_vec();
                            state.dismiss_autocomplete();
                            state.recompute_hover();
                            state.window.request_redraw();
                        }
                    }
                }
            }

            // Right-click on the color button steps the seed backward.
            // The color button is the only right-click consumer right now.
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => {
                if let HoverTarget::CellColorButton(i) = state.hover_target {
                    state.cycle_cell_color(i, -1);
                }
            }

            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                state.mouse_press_target = state.hover_target;

                match state.hover_target {
                    HoverTarget::WindowEdge(dir) => {
                        if let Err(e) = state.window.drag_resize_window(dir) {
                            log::warn!("drag_resize_window failed: {}", e);
                        }
                    }
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
                                state
                                    .session
                                    .active_tab_mut()
                                    .cell_mut(i)
                                    .buffer
                                    .set_cursor_byte_extend(byte_offset);
                            } else {
                                state
                                    .session
                                    .active_tab_mut()
                                    .cell_mut(i)
                                    .buffer
                                    .set_cursor_byte(byte_offset);
                            }
                        }
                        state.session.active_tab_mut().set_active_cell(i);
                        state.is_dragging_editor = true;
                        state.editor_drag_cell = Some(i);
                        event_loop.set_control_flow(ControlFlow::Poll);
                        state.sync_active_tab();
                    }
                    HoverTarget::CellOutputToggle(_) => {
                        state.close_menu();
                        state.dismiss_autocomplete();
                    }
                    HoverTarget::CellOutputCopyButton(i) => {
                        state.close_menu();
                        state.dismiss_autocomplete();
                        let text_to_copy = state
                            .session
                            .active_tab()
                            .cell(i)
                            .outcome
                            .message
                            .as_ref()
                            .map(|m| m.display_text().to_string());
                        if let Some(text) = text_to_copy {
                            if let Some(cb) = state.clipboard.as_mut() {
                                let _ = cb.set_text(&text);
                            }
                        }
                    }
                    HoverTarget::CellResizeHandle(i) => {
                        state.close_menu();
                        state.is_dragging_cell_resize = true;
                        state.cell_resize_index = Some(i);
                        state.cell_resize_start_y = state.cursor_position.1;
                        let editor_h = state
                            .cell_layouts
                            .iter()
                            .find(|cl| cl.cell_index == i)
                            .map(|cl| cl.editor.h)
                            .unwrap_or(100.0);
                        state.cell_resize_start_h = editor_h;
                        state.window.set_cursor(CursorIcon::NsResize);
                        event_loop.set_control_flow(ControlFlow::Poll);
                    }
                    HoverTarget::CellEditorHScrollThumb(i) => {
                        state.close_menu();
                        state.is_dragging_cell_h_scroll = true;
                        state.cell_h_scroll_index = Some(i);
                        let thumb_x = state
                            .cell_layouts
                            .iter()
                            .find(|cl| cl.cell_index == i)
                            .map(|cl| cl.editor_h_scrollbar_thumb.x)
                            .unwrap_or(0.0);
                        state.cell_h_scroll_drag_offset = state.cursor_position.0 - thumb_x;
                        event_loop.set_control_flow(ControlFlow::Poll);
                    }
                    HoverTarget::CellEditorVScrollThumb(i) => {
                        state.close_menu();
                        state.is_dragging_cell_v_scroll = true;
                        state.cell_v_scroll_index = Some(i);
                        let thumb_y = state
                            .cell_layouts
                            .iter()
                            .find(|cl| cl.cell_index == i)
                            .map(|cl| cl.editor_v_scrollbar_thumb.y)
                            .unwrap_or(0.0);
                        state.cell_v_scroll_drag_offset = state.cursor_position.1 - thumb_y;
                        event_loop.set_control_flow(ControlFlow::Poll);
                    }
                    _ => {
                        if state.open_menu.is_some() {
                            state.close_menu();
                        }
                    }
                }
            }

            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                if state.is_any_drag_active() {
                    let was_v_scroll = state.is_dragging_v_scroll;
                    state.is_dragging_split = false;
                    state.is_dragging_v_scroll = false;
                    state.render_area.is_dragging = false;
                    state.is_dragging_editor = false;
                    state.editor_drag_cell = None;
                    state.is_dragging_cell_resize = false;
                    state.cell_resize_index = None;
                    state.is_dragging_cell_h_scroll = false;
                    state.cell_h_scroll_index = None;
                    state.is_dragging_cell_v_scroll = false;
                    state.cell_v_scroll_index = None;
                    if state.pending_dialog.is_none() {
                        event_loop.set_control_flow(ControlFlow::Wait);
                    }
                    if was_v_scroll {
                        state.sync_active_tab();
                    }
                    state.recompute_hover();
                    return;
                }

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

                if matches!(
                    state.hover_target,
                    HoverTarget::MenuItem(_) | HoverTarget::DropdownItem(_)
                ) {
                    return;
                }

                let (mx, my) = state.cursor_position;

                for (i, hit) in state.tab_hit_rects.iter().enumerate() {
                    if point_in_rect(mx, my, &hit.close) {
                        state.close_tab_at(i);
                        return;
                    }
                }

                for (i, hit) in state.tab_hit_rects.iter().enumerate() {
                    if point_in_rect(mx, my, &hit.full) {
                        let old = state.session.active_index;
                        if old != i {
                            let old_id = state.session.tabs[old].tab_id;
                            let new_id = state.session.tabs[i].tab_id;
                            state.renderer.stash_tab_shaders(old_id);
                            state.renderer.restore_tab_shaders(new_id);
                            state.switch_tab_axis(old, i);
                        }
                        state.session.set_active(i);
                        // Clear in-flight REDUCE bookkeeping on both ends —
                        // shared service and the (now-inactive) tab's notebook.
                        state.reduce_service.borrow_mut().clear_pending();
                        if let Some(prev) = state.session.tabs.get_mut(old) {
                            prev.notebook.clear_pending();
                        }
                        state.invalidate_lang_cache();
                        state.sync_active_tab();
                        return;
                    }
                }

                if point_in_rect(mx, my, &state.plus_button_rect) {
                    let old = state.session.active_index;
                    let old_id = state.session.tabs[old].tab_id;
                    state.renderer.stash_tab_shaders(old_id);
                    let new_idx = state.session.new_tab();
                    state.switch_tab_axis(old, new_idx);
                    state.sync_active_tab();
                    return;
                }

                if let HoverTarget::AutocompleteItem(i) = state.hover_target {
                    state.autocomplete.selected_index = i;
                    state.accept_autocomplete();
                    return;
                }
                state.dismiss_autocomplete();

                let click_target = match state.mouse_press_target {
                    HoverTarget::CellPlayButton(_)
                    | HoverTarget::CellColorButton(_)
                    | HoverTarget::CellCopyButton(_)
                    | HoverTarget::CellOutputCopyButton(_)
                    | HoverTarget::CellOutputToggle(_)
                    | HoverTarget::CellDeleteButton(_)
                    | HoverTarget::AddCellButton => state.mouse_press_target,
                    _ => state.hover_target,
                };
                match click_target {
                    HoverTarget::CellPlayButton(i) => {
                        let is_playing =
                            state.session.active_tab().cell(i).state == CellState::Playing;
                        if is_playing {
                            state.trigger_cell_stop(i);
                        } else {
                            state.trigger_cell_play(i);
                        }
                    }
                    HoverTarget::CellColorButton(i) => {
                        state.cycle_cell_color(i, 1);
                    }
                    HoverTarget::CellEditor(_) => {}
                    HoverTarget::CellCopyButton(i) => {
                        let text = state
                            .session
                            .active_tab()
                            .cell(i)
                            .buffer
                            .text()
                            .to_string();
                        if let Some(cb) = state.clipboard.as_mut() {
                            let _ = cb.set_text(&text);
                        }
                    }
                    HoverTarget::CellOutputCopyButton(i) => {
                        let output_text = state
                            .session
                            .active_tab()
                            .cell(i)
                            .outcome
                            .message
                            .as_ref()
                            .map(|m| m.display_text().to_string())
                            .unwrap_or_default();
                        if let Some(cb) = state.clipboard.as_mut() {
                            let _ = cb.set_text(&output_text);
                        }
                    }
                    HoverTarget::CellOutputToggle(i) => {
                        let cell = state.session.active_tab_mut().cell_mut(i);
                        cell.output_collapsed = !cell.output_collapsed;
                        state.sync_active_tab();
                    }
                    HoverTarget::CellDeleteButton(i) => {
                        let cell = state.session.active_tab().cell(i);
                        if cell.state == CellState::Playing {
                            state.renderer.remove_cell_shader(cell.id);
                        }
                        state.session.active_tab_mut().remove_cell(i);
                        state.invalidate_lang_cache();
                        state.sync_active_tab();
                    }
                    HoverTarget::AddCellButton => {
                        state.session.active_tab_mut().add_cell();
                        state.invalidate_lang_cache();
                        state.sync_active_tab();
                    }
                    _ => {}
                }
            }

            WindowEvent::KeyboardInput {
                event: key_event, ..
            } => {
                if key_event.state != ElementState::Pressed {
                    return;
                }

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

                if state.autocomplete.active {
                    match &key_event.logical_key {
                        Key::Named(NamedKey::ArrowUp) => {
                            state.autocomplete.select_prev();
                            state
                                .renderer
                                .update_autocomplete_selection(state.autocomplete.selected_index);
                            state.window.request_redraw();
                            return;
                        }
                        Key::Named(NamedKey::ArrowDown) => {
                            state.autocomplete.select_next();
                            state
                                .renderer
                                .update_autocomplete_selection(state.autocomplete.selected_index);
                            state.window.request_redraw();
                            return;
                        }
                        Key::Named(NamedKey::Tab) => {
                            state.accept_autocomplete();
                            return;
                        }
                        Key::Named(NamedKey::Enter) => {
                            state.accept_autocomplete();
                            return;
                        }
                        Key::Named(NamedKey::Escape) => {
                            state.dismiss_autocomplete();
                            state.window.request_redraw();
                            return;
                        }
                        _ => {}
                    }
                }

                if state.handle_shortcut(event_loop, &key_event.logical_key) {
                    return;
                }

                let shift = state.modifiers.shift_key();
                let changed = match key_event.logical_key {
                    Key::Named(NamedKey::Backspace) => state
                        .session
                        .active_tab_mut()
                        .active_cell_mut()
                        .buffer
                        .backspace(),
                    Key::Named(NamedKey::Delete) => state
                        .session
                        .active_tab_mut()
                        .active_cell_mut()
                        .buffer
                        .delete(),
                    Key::Named(NamedKey::Enter) => {
                        state
                            .session
                            .active_tab_mut()
                            .active_cell_mut()
                            .buffer
                            .insert_newline_auto_indent();
                        true
                    }
                    Key::Named(NamedKey::Tab) => {
                        state
                            .session
                            .active_tab_mut()
                            .active_cell_mut()
                            .buffer
                            .insert_tab();
                        true
                    }
                    Key::Named(NamedKey::ArrowLeft) => state
                        .session
                        .active_tab_mut()
                        .active_cell_mut()
                        .buffer
                        .move_left(shift),
                    Key::Named(NamedKey::ArrowRight) => state
                        .session
                        .active_tab_mut()
                        .active_cell_mut()
                        .buffer
                        .move_right(shift),
                    Key::Named(NamedKey::ArrowUp) => state
                        .session
                        .active_tab_mut()
                        .active_cell_mut()
                        .buffer
                        .move_up(shift),
                    Key::Named(NamedKey::ArrowDown) => state
                        .session
                        .active_tab_mut()
                        .active_cell_mut()
                        .buffer
                        .move_down(shift),
                    Key::Named(NamedKey::Home) => state
                        .session
                        .active_tab_mut()
                        .active_cell_mut()
                        .buffer
                        .move_home(shift),
                    Key::Named(NamedKey::End) => state
                        .session
                        .active_tab_mut()
                        .active_cell_mut()
                        .buffer
                        .move_end(shift),
                    _ => {
                        if state.modifiers.control_key() {
                            return;
                        }
                        if let Some(ref text) = key_event.text {
                            for c in text.chars() {
                                if !c.is_control() {
                                    state
                                        .session
                                        .active_tab_mut()
                                        .active_cell_mut()
                                        .buffer
                                        .insert(c);
                                }
                            }
                            true
                        } else {
                            false
                        }
                    }
                };

                if changed {
                    let key_start = Instant::now();
                    state.session.active_tab_mut().mark_modified();
                    log::debug!(
                        "[perf] --- KEYSTROKE --- (1st sync: text changed, no highlight spans yet)"
                    );
                    state.sync_active_tab();
                    let t_sync = key_start.elapsed().as_micros();
                    let ac_start = Instant::now();
                    state.update_autocomplete();
                    let t_ac = ac_start.elapsed().as_micros();
                    let t_total = key_start.elapsed().as_micros();
                    log::debug!(
                        "[perf] keystroke done: {}us total | sync={}us autocomplete={}us",
                        t_total,
                        t_sync,
                        t_ac
                    );
                    state.window.request_redraw();
                } else {
                    match &key_event.logical_key {
                        Key::Named(NamedKey::ArrowLeft)
                        | Key::Named(NamedKey::ArrowRight)
                        | Key::Named(NamedKey::ArrowUp)
                        | Key::Named(NamedKey::ArrowDown)
                        | Key::Named(NamedKey::Home)
                        | Key::Named(NamedKey::End) => {
                            state.dismiss_autocomplete();
                        }
                        _ => {}
                    }
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

        if needs_redraw {
            state.sync_active_tab();
            if state.autocomplete.active {
                state.update_autocomplete();
            }
            state.window.request_redraw();
        }

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
                            let old = state.session.active_index;
                            let old_id = state.session.tabs[old].tab_id;
                            state.renderer.stash_tab_shaders(old_id);
                            if let Err(e) = state.session.open_file(&path) {
                                log::error!("Failed to open file: {}", e);
                            } else {
                                let new_idx = state.session.active_index;
                                state.switch_tab_axis(old, new_idx);
                            }
                        }
                        DialogKind::Save => {
                            if let Err(e) = state.session.active_tab_mut().save_as(&path) {
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
