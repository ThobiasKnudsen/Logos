// Centralized visual theme — colors, spacing, fonts, syntax themes.
//
// Themes are loaded from `themes.json` next to the binary. The JSON uses a
// compact format (~20 fields per theme); all other UI colors are derived.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};

// ---------------------------------------------------------------------------
// Rgba — with hex-string JSON serialization ("#rrggbb" / "#rrggbbaa")
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }
    pub const fn hex(rgb: u32) -> Self {
        Self {
            r: ((rgb >> 16) & 0xFF) as u8,
            g: ((rgb >> 8) & 0xFF) as u8,
            b: (rgb & 0xFF) as u8,
            a: 255,
        }
    }
    pub fn to_wgpu(self) -> wgpu::Color {
        wgpu::Color {
            r: self.r as f64 / 255.0,
            g: self.g as f64 / 255.0,
            b: self.b as f64 / 255.0,
            a: self.a as f64 / 255.0,
        }
    }
    pub fn to_f32_array(self) -> [f32; 4] {
        [
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            self.a as f32 / 255.0,
        ]
    }
    pub fn to_glyphon(self) -> glyphon::Color {
        glyphon::Color::rgba(self.r, self.g, self.b, self.a)
    }
}

impl Serialize for Rgba {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if self.a == 255 {
            s.serialize_str(&format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b))
        } else {
            s.serialize_str(&format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a))
        }
    }
}

impl<'de> Deserialize<'de> for Rgba {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        let s = s.trim_start_matches('#');
        let byte = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).map_err(serde::de::Error::custom);
        match s.len() {
            6 => Ok(Rgba { r: byte(0)?, g: byte(2)?, b: byte(4)?, a: 255 }),
            8 => Ok(Rgba { r: byte(0)?, g: byte(2)?, b: byte(4)?, a: byte(6)? }),
            _ => Err(serde::de::Error::custom("expected #rrggbb or #rrggbbaa")),
        }
    }
}

// ---------------------------------------------------------------------------
// Color helpers
// ---------------------------------------------------------------------------

fn lighten(c: Rgba, n: u8) -> Rgba {
    Rgba::rgb(c.r.saturating_add(n), c.g.saturating_add(n), c.b.saturating_add(n))
}
fn darken(c: Rgba, n: u8) -> Rgba {
    Rgba::rgb(c.r.saturating_sub(n), c.g.saturating_sub(n), c.b.saturating_sub(n))
}
fn mid(a: Rgba, b: Rgba) -> Rgba {
    Rgba::rgb(
        ((a.r as u16 + b.r as u16) / 2) as u8,
        ((a.g as u16 + b.g as u16) / 2) as u8,
        ((a.b as u16 + b.b as u16) / 2) as u8,
    )
}
fn alpha(c: Rgba, a: u8) -> Rgba {
    Rgba::new(c.r, c.g, c.b, a)
}

// ---------------------------------------------------------------------------
// Full Theme — used internally by the renderer (55 fields, not serialized)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub bg_primary: Rgba,
    pub bg_secondary: Rgba,
    pub bg_elevated: Rgba,
    pub bg_hover: Rgba,
    pub border: Rgba,
    pub border_focus: Rgba,
    pub text_primary: Rgba,
    pub text_secondary: Rgba,
    pub text_muted: Rgba,
    pub accent_primary: Rgba,
    pub accent_secondary: Rgba,
    pub accent_info: Rgba,
    pub tab_active: Rgba,
    pub tab_inactive: Rgba,
    pub tab_hover: Rgba,
    pub editor_bg: Rgba,
    pub editor_gutter: Rgba,
    pub editor_selection: Rgba,
    pub graph_bg: Rgba,
    pub graph_grid: Rgba,
    pub graph_axis: Rgba,
    pub axis_zone_bg: Rgba,
    pub axis_label: Rgba,
    pub toolbar_bg: Rgba,
    pub toolbar_button: Rgba,
    pub toolbar_button_hover: Rgba,
    pub toolbar_button_active: Rgba,
    pub split_handle: Rgba,
    pub split_handle_hover: Rgba,
    pub close_button_hover: Rgba,
    pub dropdown_bg: Rgba,
    pub dropdown_hover: Rgba,
    pub dropdown_separator: Rgba,
    pub menu_item_hover: Rgba,
    pub scrollbar_track: Rgba,
    pub scrollbar_thumb: Rgba,
    pub scrollbar_thumb_hover: Rgba,
    pub cursor: Rgba,
    pub play_button: Rgba,
    pub play_button_hover: Rgba,
    pub stop_button: Rgba,
    pub stop_button_hover: Rgba,
    pub tooltip_bg: Rgba,
    pub tooltip_border: Rgba,
    pub keyword: Rgba,
    pub identifier: Rgba,
    pub math_variable: Rgba,
    pub number: Rgba,
    pub operator: Rgba,
    pub string: Rgba,
    pub comment: Rgba,
    pub punctuation: Rgba,
    pub whitespace: Rgba,
    pub builtin: Rgba,
    pub axis: Rgba,
    pub type_name: Rgba,
    pub unknown: Rgba,
}

// ---------------------------------------------------------------------------
// JsonTheme — compact representation stored in themes.json
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonTheme {
    pub name: String,
    // UI base (7 colors — everything else derived)
    pub primary_bg: Rgba,
    pub secondary_bg: Rgba,
    pub tertiary_bg: Rgba,
    pub primary_text: Rgba,
    pub secondary_text: Rgba,
    pub primary_border: Rgba,
    pub secondary_border: Rgba,
    // Functional (3 colors)
    pub accent: Rgba,
    pub red: Rgba,
    pub green: Rgba,
    // Syntax (10 colors)
    pub keyword: Rgba,
    pub identifier: Rgba,
    pub math_variable: Rgba,
    pub number: Rgba,
    pub operator: Rgba,
    pub string: Rgba,
    pub comment: Rgba,
    pub builtin: Rgba,
    pub type_name: Rgba,
    pub axis: Rgba,
}

impl JsonTheme {
    pub fn to_theme(&self) -> Theme {
        let pb = self.primary_bg;
        let sb = self.secondary_bg;
        let tb = self.tertiary_bg;
        let pt = self.primary_text;
        let st = self.secondary_text;
        let b1 = self.primary_border;
        let b2 = self.secondary_border;
        let ac = self.accent;
        let rd = self.red;
        let gn = self.green;

        Theme {
            bg_primary:            pb,
            bg_secondary:          sb,
            bg_elevated:           tb,
            bg_hover:              lighten(tb, 12),
            border:                b1,
            border_focus:          ac,
            text_primary:          pt,
            text_secondary:        st,
            text_muted:            darken(st, 20),
            accent_primary:        ac,
            accent_secondary:      gn,
            accent_info:           ac,
            tab_active:            tb,
            tab_inactive:          pb,
            tab_hover:             mid(pb, tb),
            editor_bg:             pb,
            editor_gutter:         sb,
            editor_selection:      alpha(ac, 60),
            graph_bg:              darken(pb, 4),
            graph_grid:            alpha(b1, 80),
            graph_axis:            st,
            axis_zone_bg:          alpha(pb, 210),
            axis_label:            st,
            toolbar_bg:            sb,
            toolbar_button:        tb,
            toolbar_button_hover:  lighten(tb, 12),
            toolbar_button_active: gn,
            split_handle:          b1,
            split_handle_hover:    b2,
            close_button_hover:    rd,
            dropdown_bg:           sb,
            dropdown_hover:        tb,
            dropdown_separator:    b1,
            menu_item_hover:       tb,
            scrollbar_track:       alpha(sb, 120),
            scrollbar_thumb:       alpha(b2, 190),
            scrollbar_thumb_hover: alpha(lighten(b2, 20), 230),
            cursor:                pt,
            play_button:           gn,
            play_button_hover:     lighten(gn, 25),
            stop_button:           rd,
            stop_button_hover:     lighten(rd, 25),
            tooltip_bg:            sb,
            tooltip_border:        b1,
            keyword:               self.keyword,
            identifier:            self.identifier,
            math_variable:         self.math_variable,
            number:                self.number,
            operator:              self.operator,
            string:                self.string,
            comment:               self.comment,
            punctuation:           st,
            whitespace:            st,
            builtin:               self.builtin,
            axis:                  self.axis,
            type_name:             self.type_name,
            unknown:               rd,
        }
    }
}

// ---------------------------------------------------------------------------
// Default themes (compiled-in, written to themes.json on first run)
// ---------------------------------------------------------------------------

fn default_themes() -> Vec<JsonTheme> {
    vec![
        JsonTheme {
            name: "Catppuccin".into(),
            primary_bg: Rgba::hex(0x0e0e16), secondary_bg: Rgba::hex(0x13131e),
            tertiary_bg: Rgba::hex(0x1c1c2a), primary_text: Rgba::hex(0xcdd6f4),
            secondary_text: Rgba::hex(0xa6adc8), primary_border: Rgba::hex(0x262636),
            secondary_border: Rgba::hex(0x46485a), accent: Rgba::hex(0x89b4fa),
            red: Rgba::hex(0xc43c32), green: Rgba::hex(0x4ba55f),
            keyword: Rgba::hex(0xcba6f7), identifier: Rgba::hex(0xa6e3a1),
            math_variable: Rgba::hex(0xf38ba8), number: Rgba::hex(0xfab387),
            operator: Rgba::hex(0x89dceb), string: Rgba::hex(0xa6e3a1),
            comment: Rgba::hex(0x6c7086), builtin: Rgba::hex(0x89b4fa),
            type_name: Rgba::hex(0x94e2d5), axis: Rgba::hex(0xf9e2af),
        },
        JsonTheme {
            name: "One Dark".into(),
            primary_bg: Rgba::hex(0x0f1116), secondary_bg: Rgba::hex(0x14171e),
            tertiary_bg: Rgba::hex(0x1e2128), primary_text: Rgba::hex(0xabb2bf),
            secondary_text: Rgba::hex(0x8891a5), primary_border: Rgba::hex(0x242832),
            secondary_border: Rgba::hex(0x3c424e), accent: Rgba::hex(0x61afef),
            red: Rgba::hex(0xc43c32), green: Rgba::hex(0x4ba55f),
            keyword: Rgba::hex(0xc678dd), identifier: Rgba::hex(0xe06c75),
            math_variable: Rgba::hex(0xe5c07b), number: Rgba::hex(0xd19a66),
            operator: Rgba::hex(0x56b6c2), string: Rgba::hex(0x98c379),
            comment: Rgba::hex(0x5c6370), builtin: Rgba::hex(0x61afef),
            type_name: Rgba::hex(0xe5c07b), axis: Rgba::hex(0xd19a66),
        },
        JsonTheme {
            name: "Monokai".into(),
            primary_bg: Rgba::hex(0x10110c), secondary_bg: Rgba::hex(0x161610),
            tertiary_bg: Rgba::hex(0x202018), primary_text: Rgba::hex(0xf8f8f2),
            secondary_text: Rgba::hex(0xb0ae9e), primary_border: Rgba::hex(0x26271e),
            secondary_border: Rgba::hex(0x444436), accent: Rgba::hex(0xa6e22e),
            red: Rgba::hex(0xc43c32), green: Rgba::hex(0x4ba55f),
            keyword: Rgba::hex(0xff6188), identifier: Rgba::hex(0xa9dc76),
            math_variable: Rgba::hex(0xffd866), number: Rgba::hex(0xab9df2),
            operator: Rgba::hex(0xff6188), string: Rgba::hex(0xffd866),
            comment: Rgba::hex(0x727072), builtin: Rgba::hex(0x78dce8),
            type_name: Rgba::hex(0x78dce8), axis: Rgba::hex(0xfc9867),
        },
        JsonTheme {
            name: "Dracula".into(),
            primary_bg: Rgba::hex(0x0e0f16), secondary_bg: Rgba::hex(0x14151e),
            tertiary_bg: Rgba::hex(0x1e1f2a), primary_text: Rgba::hex(0xf8f8f2),
            secondary_text: Rgba::hex(0xb0b2c0), primary_border: Rgba::hex(0x242632),
            secondary_border: Rgba::hex(0x44465c), accent: Rgba::hex(0xbd93f9),
            red: Rgba::hex(0xff5555), green: Rgba::hex(0x50fa7b),
            keyword: Rgba::hex(0xff79c6), identifier: Rgba::hex(0x50fa7b),
            math_variable: Rgba::hex(0xf8f8f2), number: Rgba::hex(0xbd93f9),
            operator: Rgba::hex(0xff79c6), string: Rgba::hex(0xf1fa8c),
            comment: Rgba::hex(0x6272a4), builtin: Rgba::hex(0x8be9fd),
            type_name: Rgba::hex(0x8be9fd), axis: Rgba::hex(0xffb86c),
        },
        JsonTheme {
            name: "Gruvbox".into(),
            primary_bg: Rgba::hex(0x100f0e), secondary_bg: Rgba::hex(0x161412),
            tertiary_bg: Rgba::hex(0x201d1a), primary_text: Rgba::hex(0xebdbb2),
            secondary_text: Rgba::hex(0xbdae93), primary_border: Rgba::hex(0x262320),
            secondary_border: Rgba::hex(0x443e36), accent: Rgba::hex(0xfe8019),
            red: Rgba::hex(0xcc241d), green: Rgba::hex(0x4ba55f),
            keyword: Rgba::hex(0xfb4934), identifier: Rgba::hex(0x83a598),
            math_variable: Rgba::hex(0xfabd2f), number: Rgba::hex(0xd3869b),
            operator: Rgba::hex(0xfe8019), string: Rgba::hex(0xb8bb26),
            comment: Rgba::hex(0x928374), builtin: Rgba::hex(0x8ec07c),
            type_name: Rgba::hex(0x83a598), axis: Rgba::hex(0xfabd2f),
        },
        JsonTheme {
            name: "Nord".into(),
            primary_bg: Rgba::hex(0x0e1218), secondary_bg: Rgba::hex(0x131820),
            tertiary_bg: Rgba::hex(0x1c222c), primary_text: Rgba::hex(0xeceff4),
            secondary_text: Rgba::hex(0xb0b8c8), primary_border: Rgba::hex(0x232a36),
            secondary_border: Rgba::hex(0x3e4856), accent: Rgba::hex(0x88c0d0),
            red: Rgba::hex(0xbf616a), green: Rgba::hex(0x4ba55f),
            keyword: Rgba::hex(0x81a1c1), identifier: Rgba::hex(0x88c0d0),
            math_variable: Rgba::hex(0xd8dee9), number: Rgba::hex(0xb48ead),
            operator: Rgba::hex(0x81a1c1), string: Rgba::hex(0xa3be8c),
            comment: Rgba::hex(0x616e88), builtin: Rgba::hex(0x88c0d0),
            type_name: Rgba::hex(0x8fbcbb), axis: Rgba::hex(0xebcb8b),
        },
        JsonTheme {
            name: "Solarized".into(),
            primary_bg: Rgba::hex(0x000c11), secondary_bg: Rgba::hex(0x00161e),
            tertiary_bg: Rgba::hex(0x031e26), primary_text: Rgba::hex(0xfdf6e3),
            secondary_text: Rgba::hex(0x93a1a1), primary_border: Rgba::hex(0x05242e),
            secondary_border: Rgba::hex(0x0a3a48), accent: Rgba::hex(0x268bd2),
            red: Rgba::hex(0xdc322f), green: Rgba::hex(0x4ba55f),
            keyword: Rgba::hex(0x859900), identifier: Rgba::hex(0x268bd2),
            math_variable: Rgba::hex(0xcb4b16), number: Rgba::hex(0xd33682),
            operator: Rgba::hex(0x93a1a1), string: Rgba::hex(0x2aa198),
            comment: Rgba::hex(0x586e75), builtin: Rgba::hex(0x268bd2),
            type_name: Rgba::hex(0xcb4b16), axis: Rgba::hex(0xb58900),
        },
    ]
}

// ---------------------------------------------------------------------------
// Runtime theme store
// ---------------------------------------------------------------------------

struct ThemeStore {
    names: Vec<String>,
    themes: Vec<Theme>,
}

static THEME_STORE: OnceLock<Mutex<ThemeStore>> = OnceLock::new();

fn store() -> &'static Mutex<ThemeStore> {
    THEME_STORE.get_or_init(|| {
        let json_themes = load_or_create_themes_json();
        let names = json_themes.iter().map(|t| t.name.clone()).collect();
        let themes = json_themes.iter().map(|t| t.to_theme()).collect();
        Mutex::new(ThemeStore { names, themes })
    })
}

fn themes_json_path() -> std::path::PathBuf {
    std::env::current_exe()
        .unwrap_or_default()
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("themes.json")
}

fn load_or_create_themes_json() -> Vec<JsonTheme> {
    let path = themes_json_path();
    if path.exists() {
        if let Ok(json) = std::fs::read_to_string(&path) {
            match serde_json::from_str::<Vec<JsonTheme>>(&json) {
                Ok(t) if !t.is_empty() => {
                    log::info!("Loaded {} themes from {}", t.len(), path.display());
                    return t;
                }
                Ok(_) => log::warn!("themes.json is empty, using defaults"),
                Err(e) => log::warn!("Failed to parse themes.json: {e}, using defaults"),
            }
        }
    }
    let defaults = default_themes();
    if let Ok(json) = serde_json::to_string_pretty(&defaults) {
        if let Err(e) = std::fs::write(&path, &json) {
            log::warn!("Failed to write themes.json: {e}");
        } else {
            log::info!("Created themes.json at {}", path.display());
        }
    }
    defaults
}

// ---------------------------------------------------------------------------
// Active theme selection + public API
// ---------------------------------------------------------------------------

mod theme_state {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static CURRENT: AtomicUsize = AtomicUsize::new(0);
    pub fn index() -> usize { CURRENT.load(Ordering::Relaxed) }
    pub fn set(i: usize) { CURRENT.store(i, Ordering::Relaxed); }
}

pub fn theme() -> &'static Theme {
    let s = store().lock().unwrap();
    let idx = theme_state::index().min(s.themes.len() - 1);
    unsafe { &*(&s.themes[idx] as *const Theme) }
}

pub fn active_theme_name() -> String {
    let s = store().lock().unwrap();
    let idx = theme_state::index().min(s.names.len() - 1);
    s.names[idx].clone()
}

pub fn theme_names() -> Vec<String> {
    store().lock().unwrap().names.clone()
}

pub fn cycle_theme() -> String {
    let s = store().lock().unwrap();
    let next = (theme_state::index() + 1) % s.themes.len();
    theme_state::set(next);
    s.names[next].clone()
}

pub fn set_theme(idx: usize) {
    let s = store().lock().unwrap();
    if idx < s.themes.len() { theme_state::set(idx); }
}

pub fn theme_count() -> usize {
    store().lock().unwrap().themes.len()
}

pub fn reload_themes() -> usize {
    let jt = load_or_create_themes_json();
    let names: Vec<String> = jt.iter().map(|t| t.name.clone()).collect();
    let count = names.len();
    let themes = jt.iter().map(|t| t.to_theme()).collect();
    let mut s = store().lock().unwrap();
    s.names = names;
    s.themes = themes;
    if theme_state::index() >= count { theme_state::set(0); }
    count
}

// ---------------------------------------------------------------------------
// Spacing
// ---------------------------------------------------------------------------

pub mod spacing {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const MENU_HEIGHT: f32 = 28.0;
    pub const TAB_HEIGHT: f32 = 36.0;
    pub const STATUS_HEIGHT: f32 = 24.0;
    pub const SPLIT_HANDLE_WIDTH: f32 = 6.0;
    pub const WINDOW_CONTROL_WIDTH: f32 = 46.0;
    pub const DROPDOWN_ITEM_HEIGHT: f32 = 28.0;
    pub const DROPDOWN_PADDING: f32 = 4.0;
    pub const DROPDOWN_MIN_WIDTH: f32 = 220.0;
    pub const SCROLLBAR_HEIGHT: f32 = 8.0;
    pub const SCROLLBAR_WIDTH: f32 = 8.0;
    pub const SCROLLBAR_THUMB_MIN_W: f32 = 30.0;
    pub const SCROLLBAR_THUMB_MIN_H: f32 = 30.0;
    pub const GUTTER_WIDTH: f32 = 48.0;
    pub const AXIS_ZONE_SIZE: f32 = 40.0;
    pub const CELL_PADDING: f32 = 12.0;
    pub const CELL_SPACING: f32 = 8.0;
    pub const BUTTON_SIZE: f32 = 24.0;
    pub const HEADER_HEIGHT: f32 = 32.0;
    pub const TEXT_PADDING: f32 = 24.0;

    pub fn scale() -> f32 { super::fonts::scale() }
    pub fn xs() -> f32 { XS * scale() }
    pub fn sm() -> f32 { SM * scale() }
    pub fn md() -> f32 { MD * scale() }
    pub fn lg() -> f32 { LG * scale() }
    pub fn xl() -> f32 { XL * scale() }
    pub fn menu_height() -> f32 { MENU_HEIGHT * scale() }
    pub fn tab_height() -> f32 { TAB_HEIGHT * scale() }
    pub fn status_height() -> f32 { STATUS_HEIGHT * scale() }
    pub fn split_handle_width() -> f32 { SPLIT_HANDLE_WIDTH * scale() }
    pub fn window_control_width() -> f32 { WINDOW_CONTROL_WIDTH * scale() }
    pub fn dropdown_item_height() -> f32 { DROPDOWN_ITEM_HEIGHT * scale() }
    pub fn dropdown_padding() -> f32 { DROPDOWN_PADDING * scale() }
    pub fn dropdown_min_width() -> f32 { DROPDOWN_MIN_WIDTH * scale() }
    pub fn scrollbar_height() -> f32 { SCROLLBAR_HEIGHT * scale() }
    pub fn scrollbar_width() -> f32 { SCROLLBAR_WIDTH * scale() }
    pub fn scrollbar_thumb_min_w() -> f32 { SCROLLBAR_THUMB_MIN_W * scale() }
    pub fn scrollbar_thumb_min_h() -> f32 { SCROLLBAR_THUMB_MIN_H * scale() }
    pub fn gutter_width() -> f32 { GUTTER_WIDTH * scale() }
    pub fn axis_zone_size() -> f32 { AXIS_ZONE_SIZE * scale() }
    pub fn cell_padding() -> f32 { CELL_PADDING * scale() }
    pub fn cell_spacing() -> f32 { CELL_SPACING * scale() }
    pub fn button_size() -> f32 { BUTTON_SIZE * scale() }
    pub fn header_height() -> f32 { HEADER_HEIGHT * scale() }
    pub fn text_padding() -> f32 { TEXT_PADDING * scale() }
}

// ---------------------------------------------------------------------------
// Fonts
// ---------------------------------------------------------------------------

pub mod fonts {
    use std::sync::atomic::{AtomicU32, Ordering};
    pub const BASE_EDITOR: f32 = 20.0;
    pub const BASE_UI: f32 = 14.0;
    pub const BASE_SMALL: f32 = 12.0;
    pub const BASE_TOOLTIP: f32 = 13.0;
    pub const BASE_STATUS: f32 = 13.0;
    pub const BASE_MENU: f32 = 14.0;
    pub const MIN_SCALE: f32 = 0.6;
    pub const MAX_SCALE: f32 = 2.0;
    pub const SCALE_STEP: f32 = 0.1;
    pub const LINE_HEIGHT_FACTOR: f32 = 1.4;
    pub const CURSOR_WIDTH: f32 = 2.0;
    static CURRENT_SCALE: AtomicU32 = AtomicU32::new(1.0_f32.to_bits());
    pub fn scale() -> f32 { f32::from_bits(CURRENT_SCALE.load(Ordering::Relaxed)) }
    pub fn zoom_in() { let n = (scale() + SCALE_STEP).min(MAX_SCALE); CURRENT_SCALE.store(n.to_bits(), Ordering::Relaxed); }
    pub fn zoom_out() { let n = (scale() - SCALE_STEP).max(MIN_SCALE); CURRENT_SCALE.store(n.to_bits(), Ordering::Relaxed); }
    pub fn reset_zoom() { CURRENT_SCALE.store(1.0_f32.to_bits(), Ordering::Relaxed); }
    pub fn editor_size() -> f32 { BASE_EDITOR * scale() }
    pub fn editor_line_height() -> f32 { editor_size() * LINE_HEIGHT_FACTOR }
    pub fn ui_size() -> f32 { BASE_UI * scale() }
    pub fn small_size() -> f32 { BASE_SMALL * scale() }
    pub fn tooltip_size() -> f32 { BASE_TOOLTIP * scale() }
    pub fn status_size() -> f32 { BASE_STATUS * scale() }
    pub fn menu_size() -> f32 { BASE_MENU * scale() }
}

// ---------------------------------------------------------------------------
// Split defaults
// ---------------------------------------------------------------------------

pub mod split {
    pub const DEFAULT_LEFT_WIDTH: f32 = 540.0;
    pub const MIN_PANE_SIZE: f32 = 200.0;
}
