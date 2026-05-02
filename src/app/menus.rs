use crate::ui::theme;

pub(crate) struct MenuItemDef {
    pub label: &'static str,
    pub shortcut: &'static str,
}

pub(crate) const MENU_NAMES: &[&str] = &["File", "Edit", "View", "Examples", "Theme", "Help"];

const MENU_FILE_ITEMS: &[MenuItemDef] = &[
    MenuItemDef {
        label: "New Tab",
        shortcut: "Ctrl+N",
    },
    MenuItemDef {
        label: "Open...",
        shortcut: "Ctrl+O",
    },
    MenuItemDef {
        label: "Save",
        shortcut: "Ctrl+S",
    },
    MenuItemDef {
        label: "Save As...",
        shortcut: "Ctrl+Shift+S",
    },
    MenuItemDef {
        label: "Close Tab",
        shortcut: "Ctrl+W",
    },
    MenuItemDef {
        label: "Quit",
        shortcut: "Ctrl+Q",
    },
];

const MENU_EDIT_ITEMS: &[MenuItemDef] = &[
    MenuItemDef {
        label: "Cut",
        shortcut: "Ctrl+X",
    },
    MenuItemDef {
        label: "Copy",
        shortcut: "Ctrl+C",
    },
    MenuItemDef {
        label: "Paste",
        shortcut: "Ctrl+V",
    },
    MenuItemDef {
        label: "Select All",
        shortcut: "Ctrl+A",
    },
];

const MENU_VIEW_ITEMS: &[MenuItemDef] = &[
    MenuItemDef {
        label: "Zoom In",
        shortcut: "Ctrl+=",
    },
    MenuItemDef {
        label: "Zoom Out",
        shortcut: "Ctrl+-",
    },
    MenuItemDef {
        label: "Reset Zoom",
        shortcut: "Ctrl+0",
    },
];

pub(super) const MENU_EXAMPLES_ITEMS: &[MenuItemDef] = &[
    MenuItemDef {
        label: "Gradient",
        shortcut: "",
    },
    MenuItemDef {
        label: "Ripple",
        shortcut: "",
    },
    MenuItemDef {
        label: "Mandelbrot",
        shortcut: "",
    },
    MenuItemDef {
        label: "Warp",
        shortcut: "",
    },
    MenuItemDef {
        label: "Monte Carlo",
        shortcut: "",
    },
];

pub(super) const EXAMPLE_SOURCES: &[&str] = &[
    include_str!("../../examples/gradient.txt"),
    include_str!("../../examples/ripple.txt"),
    include_str!("../../examples/mandlebrot.txt"),
    include_str!("../../examples/warp.txt"),
    include_str!("../../examples/monte_carlo.txt"),
];

/// Dynamic theme menu items built from themes.json at runtime.
pub(crate) struct DynMenuItemDef {
    pub label: String,
}

static THEME_MENU_CACHE: std::sync::OnceLock<std::sync::Mutex<Vec<DynMenuItemDef>>> =
    std::sync::OnceLock::new();

fn theme_menu_items() -> &'static std::sync::Mutex<Vec<DynMenuItemDef>> {
    THEME_MENU_CACHE.get_or_init(|| {
        let names = theme::theme_names();
        let items = names
            .into_iter()
            .map(|n| DynMenuItemDef { label: n })
            .collect();
        std::sync::Mutex::new(items)
    })
}

pub(crate) fn menu_items(index: usize) -> &'static [MenuItemDef] {
    match index {
        0 => MENU_FILE_ITEMS,
        1 => MENU_EDIT_ITEMS,
        2 => MENU_VIEW_ITEMS,
        3 => MENU_EXAMPLES_ITEMS,
        _ => &[],
    }
}

pub(crate) fn theme_menu_count() -> usize {
    theme_menu_items().lock().unwrap().len()
}

pub(crate) fn theme_menu_label(idx: usize) -> String {
    let items = theme_menu_items().lock().unwrap();
    items.get(idx).map(|i| i.label.clone()).unwrap_or_default()
}

pub(super) fn active_theme_index() -> usize {
    let name = theme::active_theme_name();
    let items = theme_menu_items().lock().unwrap();
    items.iter().position(|i| i.label == name).unwrap_or(0)
}
