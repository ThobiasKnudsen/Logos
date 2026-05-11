use crate::ui::theme::{self, font_family};

pub(crate) struct MenuItemDef {
    pub label: &'static str,
    pub shortcut: &'static str,
}

pub(crate) const MENU_NAMES: &[&str] =
    &["File", "Edit", "View", "Examples", "Theme", "Fonts", "Help"];

/// Index of the Examples menu in `MENU_NAMES` — populated dynamically from
/// the runtime examples directory (see `examples_menu_count` /
/// `examples_menu_label`). New `.logos` files dropped into `examples/`
/// appear automatically; no rebuild needed.
pub(crate) const EXAMPLES_MENU_INDEX: usize = 3;
/// Index of the Theme menu in `MENU_NAMES`.
pub(crate) const THEME_MENU_INDEX: usize = 4;
/// Index of the Fonts menu in `MENU_NAMES`.
pub(crate) const FONTS_MENU_INDEX: usize = 5;
/// Index of the Help menu in `MENU_NAMES`.
pub(crate) const HELP_MENU_INDEX: usize = 6;

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

const MENU_HELP_ITEMS: &[MenuItemDef] = &[
    MenuItemDef {
        label: "Copy Diagnostics",
        shortcut: "",
    },
];

/// One discovered example file, paired with its menu label and absolute path.
/// Built once at startup by walking the same root candidates as
/// `examples_dir()`; new `.logos` files appear only after restart.
struct ExampleEntry {
    label: String,
    path: std::path::PathBuf,
}

static EXAMPLES_CACHE: std::sync::OnceLock<Vec<ExampleEntry>> =
    std::sync::OnceLock::new();

/// Candidate `examples/` directories, in priority order. Production deploys
/// place the folder next to the binary; for `cargo run` the binary lives at
/// `target/<profile>/logos`, so the repo root (two parents up) is also tried;
/// finally we fall back to the current working directory.
fn examples_dir_candidates() -> Vec<std::path::PathBuf> {
    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.to_path_buf());
            if let Some(p) = dir.parent().and_then(|p| p.parent()) {
                roots.push(p.to_path_buf());
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd);
    }
    roots.into_iter().map(|r| r.join("examples")).collect()
}

/// "monte_carlo" → "Monte Carlo"; "gradient" → "Gradient". Splits on `_` and
/// title-cases each word so the menu reads naturally. Underscores are the
/// expected naming convention for multi-word example files.
fn stem_to_label(stem: &str) -> String {
    stem.split('_')
        .filter(|s| !s.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn discover_examples() -> Vec<ExampleEntry> {
    for dir in examples_dir_candidates() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut items: Vec<ExampleEntry> = entries
            .filter_map(Result::ok)
            .filter(|e| {
                e.file_type().map(|t| t.is_file()).unwrap_or(false)
                    && e.path().extension().is_some_and(|x| x == "logos")
            })
            .filter_map(|e| {
                let path = e.path();
                let stem = path.file_stem()?.to_string_lossy().to_string();
                Some(ExampleEntry {
                    label: stem_to_label(&stem),
                    path,
                })
            })
            .collect();
        if !items.is_empty() {
            items.sort_by(|a, b| a.label.cmp(&b.label));
            return items;
        }
    }
    Vec::new()
}

fn examples_cache() -> &'static [ExampleEntry] {
    EXAMPLES_CACHE.get_or_init(discover_examples).as_slice()
}

pub(crate) fn examples_menu_count() -> usize {
    examples_cache().len()
}

pub(crate) fn examples_menu_label(idx: usize) -> String {
    examples_cache()
        .get(idx)
        .map(|e| e.label.clone())
        .unwrap_or_default()
}

/// Absolute path to the example file at `idx`, ready to load through the
/// session's `open_file`. `None` if the index is out of range.
pub(crate) fn example_path(idx: usize) -> Option<std::path::PathBuf> {
    examples_cache().get(idx).map(|e| e.path.clone())
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
    fn examples_directory_is_discovered_and_nonempty_in_dev_tree() {
        // Sanity: running from a checkout, the `examples/` folder should be
        // discovered. Production may legitimately ship without it.
        let entries = examples_cache();
        assert!(
            !entries.is_empty(),
            "no .logos files found in any `examples/` candidate; \
             checked: {:?}",
            examples_dir_candidates(),
        );
        for entry in entries {
            assert!(
                entry.path.is_file(),
                "discovered entry {:?} is not a real file",
                entry.path,
            );
        }
    }

    #[test]
    fn stem_to_label_title_cases_snake_case() {
        assert_eq!(stem_to_label("gradient"), "Gradient");
        assert_eq!(stem_to_label("monte_carlo"), "Monte Carlo");
        assert_eq!(stem_to_label("a_b_c"), "A B C");
        assert_eq!(stem_to_label(""), "");
    }
}

pub(crate) fn menu_items(index: usize) -> &'static [MenuItemDef] {
    match index {
        0 => MENU_FILE_ITEMS,
        1 => MENU_EDIT_ITEMS,
        2 => MENU_VIEW_ITEMS,
        i if i == HELP_MENU_INDEX => MENU_HELP_ITEMS,
        // Examples is dynamic — see `examples_menu_count` /
        // `examples_menu_label`. `menu_items` for it returns &[] so the
        // renderer takes the dynamic branch in `dynamic_menu_count`.
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

pub(crate) fn font_menu_count() -> usize {
    font_family::count()
}

pub(crate) fn font_menu_label(idx: usize) -> String {
    font_family::name(idx).to_string()
}

pub(super) fn active_font_index() -> usize {
    font_family::active_index()
}

/// Number of items in a dynamic menu (Examples/Theme/Fonts) — used by the
/// renderer. Returns `None` for non-dynamic menus.
pub(crate) fn dynamic_menu_count(menu_index: usize) -> Option<usize> {
    match menu_index {
        EXAMPLES_MENU_INDEX => Some(examples_menu_count()),
        THEME_MENU_INDEX => Some(theme_menu_count()),
        FONTS_MENU_INDEX => Some(font_menu_count()),
        _ => None,
    }
}

/// Label for a dynamic-menu item. Caller must ensure `menu_index` is dynamic.
pub(crate) fn dynamic_menu_label(menu_index: usize, item_index: usize) -> String {
    match menu_index {
        EXAMPLES_MENU_INDEX => examples_menu_label(item_index),
        THEME_MENU_INDEX => theme_menu_label(item_index),
        FONTS_MENU_INDEX => font_menu_label(item_index),
        _ => String::new(),
    }
}
