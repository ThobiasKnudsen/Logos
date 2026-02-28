use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

use crate::file_dialog::{DialogKind, DialogResult, FileDialog};
use crate::render::{Renderer, TabHitRect, TabInfo};
use crate::session::TabManager;
use crate::ui::layout::{LayoutResult, Rect, UiLayout};

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
}

impl AppState {
    /// Sync renderer with the active tab's buffer and all tab infos.
    fn sync_active_tab(&mut self) {
        let lp = self.cached_layout.left_pane;
        let tab = self.tab_manager.active_tab();
        self.renderer.update_text(
            tab.buffer.text(),
            tab.buffer.cursor_byte_offset(),
            lp.x,
            lp.y,
            lp.w,
            lp.h,
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

    fn handle_shortcut(&mut self, event_loop: &ActiveEventLoop, key: &Key) -> bool {
        let ctrl = self.modifiers.control_key();
        let shift = self.modifiers.shift_key();

        if !ctrl {
            return false;
        }

        match key {
            Key::Character(c) if c.as_str() == "n" => {
                self.tab_manager.new_tab();
                self.sync_active_tab();
                true
            }
            Key::Character(c) if c.as_str() == "o" => {
                if self.pending_dialog.is_none() {
                    self.pending_dialog = Some(FileDialog::spawn(DialogKind::Open));
                }
                true
            }
            Key::Character(c) if c.as_str() == "s" || c.as_str() == "S" => {
                if shift {
                    // Ctrl+Shift+S: always Save As
                    if self.pending_dialog.is_none() {
                        self.pending_dialog = Some(FileDialog::spawn(DialogKind::Save));
                    }
                } else {
                    // Ctrl+S: save if path exists, otherwise Save As
                    if self.tab_manager.active_tab().file_path.is_some() {
                        if let Err(e) = self.tab_manager.active_tab_mut().save() {
                            log::error!("Save failed: {}", e);
                        }
                        self.sync_active_tab();
                    } else if self.pending_dialog.is_none() {
                        self.pending_dialog = Some(FileDialog::spawn(DialogKind::Save));
                    }
                }
                true
            }
            Key::Character(c) if c.as_str() == "w" => {
                let idx = self.tab_manager.active_index;
                self.tab_manager.close_tab(idx);
                self.sync_active_tab();
                true
            }
            Key::Character(c) if c.as_str() == "q" => {
                event_loop.exit();
                true
            }
            _ => false,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let window_attrs = Window::default_attributes()
            .with_inner_size(LogicalSize::new(1200.0, 800.0))
            .with_title("Logos");
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
            plus_button_rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
            },
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
                state.sync_active_tab();
            }

            WindowEvent::CursorMoved { position, .. } => {
                state.cursor_position = (position.x as f32, position.y as f32);
            }

            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                let (mx, my) = state.cursor_position;

                // Check close buttons first (they're inside tab rects)
                for (i, hit) in state.tab_hit_rects.iter().enumerate() {
                    if point_in_rect(mx, my, &hit.close) {
                        state.tab_manager.close_tab(i);
                        state.sync_active_tab();
                        return;
                    }
                }

                // Check tab selection
                for (i, hit) in state.tab_hit_rects.iter().enumerate() {
                    if point_in_rect(mx, my, &hit.full) {
                        state.tab_manager.set_active(i);
                        state.sync_active_tab();
                        return;
                    }
                }

                // Check plus button
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

                // Try shortcuts first
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
                        // Don't insert text when Ctrl is held (shortcuts)
                        if state.modifiers.control_key() {
                            return;
                        }
                        if let Some(ref text) = key_event.text {
                            for c in text.chars() {
                                // Filter out control characters
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
                state.renderer.render(&state.cached_layout);
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
                    event_loop.set_control_flow(ControlFlow::Wait);

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
                    event_loop.set_control_flow(ControlFlow::Wait);
                }
            }
        }
    }
}

pub fn run() {
    let event_loop = EventLoop::new().unwrap();
    event_loop.run_app(&mut App { state: None }).unwrap();
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
