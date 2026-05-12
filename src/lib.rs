//! Logos — math-native code editor.
//!
//! The binary in `src/main.rs` is a thin wrapper around `app::run()`.
//! Integration tests in `tests/` consume the lib through this public API.

pub mod app;
pub mod editor;
pub mod file_dialog;
pub mod lang;
pub mod notebook;
pub mod render;
pub mod session;
pub mod ui;
