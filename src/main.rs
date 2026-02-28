mod app;
mod editor;
mod render;
mod ui;

fn main() {
    env_logger::init();
    app::run();
}
