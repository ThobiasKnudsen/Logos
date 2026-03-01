mod app;
mod editor;
mod file_dialog;
mod lang;
mod render;
mod session;
mod ui;

fn main() {
    env_logger::init();
    app::run();
}
