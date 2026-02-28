use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::editor::Buffer;
use crate::render::Renderer;
use crate::ui::layout::{LayoutResult, UiLayout};

struct App {
    state: Option<AppState>,
}

struct AppState {
    renderer: Renderer,
    buffer: Buffer,
    layout: UiLayout,
    cached_layout: LayoutResult,
    window: Arc<Window>,
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

        let mut buffer = Buffer::new();
        buffer.set_text(DEMO_TEXT);

        let mut layout = UiLayout::new();
        let size = window.inner_size();
        let cached_layout = layout.compute(size.width as f32, size.height as f32);

        let mut state = AppState {
            renderer,
            buffer,
            layout,
            cached_layout,
            window,
        };

        // Push initial text to the renderer and trigger first paint.
        let lp = state.cached_layout.left_pane;
        state.renderer.update_text(
            state.buffer.text(),
            state.buffer.cursor_byte_offset(),
            lp.x,
            lp.y,
            lp.w,
            lp.h,
        );
        state.window.request_redraw();

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

            WindowEvent::Resized(size) => {
                state.renderer.resize(size);
                state.cached_layout =
                    state.layout.compute(size.width as f32, size.height as f32);

                // Re-layout editor text within new pane dimensions
                let lp = state.cached_layout.left_pane;
                state.renderer.update_text(
                    state.buffer.text(),
                    state.buffer.cursor_byte_offset(),
                    lp.x,
                    lp.y,
                    lp.w,
                    lp.h,
                );

                // Update status bar with line/cursor info
                let (line, col) = line_col_from(state.buffer.text(), state.buffer.cursor_byte_offset());
                let line_count = state.buffer.text().lines().count().max(1);
                state.renderer.update_status(&format!(
                    "Ready \u{2502} {} lines \u{2502} Ln {}, Col {}",
                    line_count,
                    line + 1,
                    col + 1,
                ));

                state.window.request_redraw();
            }

            WindowEvent::KeyboardInput {
                event: key_event,
                ..
            } => {
                if key_event.state != ElementState::Pressed {
                    return;
                }

                let changed = match key_event.logical_key {
                    Key::Named(NamedKey::Backspace) => state.buffer.backspace(),
                    Key::Named(NamedKey::Delete) => state.buffer.delete(),
                    Key::Named(NamedKey::Enter) => {
                        state.buffer.insert('\n');
                        true
                    }
                    Key::Named(NamedKey::ArrowLeft) => state.buffer.move_left(),
                    Key::Named(NamedKey::ArrowRight) => state.buffer.move_right(),
                    Key::Named(NamedKey::ArrowUp) => state.buffer.move_up(),
                    Key::Named(NamedKey::ArrowDown) => state.buffer.move_down(),
                    Key::Named(NamedKey::Home) => state.buffer.move_home(),
                    Key::Named(NamedKey::End) => state.buffer.move_end(),
                    _ => {
                        if let Some(ref text) = key_event.text {
                            for c in text.chars() {
                                state.buffer.insert(c);
                            }
                            true
                        } else {
                            false
                        }
                    }
                };

                if changed {
                    let lp = state.cached_layout.left_pane;
                    state.renderer.update_text(
                        state.buffer.text(),
                        state.buffer.cursor_byte_offset(),
                        lp.x,
                        lp.y,
                        lp.w,
                        lp.h,
                    );

                    // Update status bar
                    let (line, col) = line_col_from(state.buffer.text(), state.buffer.cursor_byte_offset());
                    let line_count = state.buffer.text().lines().count().max(1);
                    state.renderer.update_status(&format!(
                        "Ready \u{2502} {} lines \u{2502} Ln {}, Col {}",
                        line_count,
                        line + 1,
                        col + 1,
                    ));

                    state.window.request_redraw();
                }
            }

            WindowEvent::RedrawRequested => {
                state.renderer.render(&state.cached_layout);
            }

            _ => {}
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

const DEMO_TEXT: &str = "\
f(x) = x\u{b2} + 2x + 1

\u{03c0} \u{2248} 3.14159265

\u{2211}(i, 0, n) = n\u{b7}(n+1)/2

g(x, y) = sin(x) \u{b7} cos(y)

// Type here...
";
