mod app;
mod editor;
mod file_dialog;
mod lang;
mod notebook;
mod render;
mod session;
mod ui;

fn main() {
    env_logger::init();
    install_panic_hook();
    if let Err(e) = app::run() {
        log::error!("fatal: {e}");
        std::process::exit(1);
    }
}

fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "<unknown>".to_string());
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
            .unwrap_or("<no message>");
        log::error!("panic at {location}: {msg}");
        default(info);
    }));
}
