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
    MenuItemDef {
        label: "Waves",
        shortcut: "",
    },
];

/// File names of each shipped example, parallel to `MENU_EXAMPLES_ITEMS`.
/// Resolved to a real path at click-time by `resolve_example_path`, which
/// looks for an `examples/` folder deployed next to the binary (and falls
/// back to a few dev-tree locations so `cargo run` still works).
pub(super) const EXAMPLE_FILENAMES: &[&str] = &[
    "gradient.txt",
    "ripple.txt",
    "mandlebrot.txt",
    "warp.txt",
    "monte_carlo.txt",
    "waves",
];

/// Resolve an example file name to an absolute path. The expected production
/// layout is `<exe_dir>/examples/<name>`; for `cargo run` the binary lives at
/// `target/<profile>/logos`, so we also try walking up to the repo root and
/// the current working directory.
pub(super) fn resolve_example_path(filename: &str) -> Option<std::path::PathBuf> {
    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.to_path_buf());
            // `target/<profile>/logos` → repo root is two parents up.
            if let Some(p) = dir.parent().and_then(|p| p.parent()) {
                roots.push(p.to_path_buf());
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd);
    }
    for root in roots {
        let candidate = root.join("examples").join(filename);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_menu_example_resolves_to_a_real_file() {
        for &filename in EXAMPLE_FILENAMES {
            let resolved = resolve_example_path(filename)
                .unwrap_or_else(|| panic!("could not resolve {filename}"));
            assert!(
                resolved.is_file(),
                "{filename} resolved to {} which is not a file",
                resolved.display()
            );
        }
        assert_eq!(EXAMPLE_FILENAMES.len(), MENU_EXAMPLES_ITEMS.len());
    }
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
