// Centralized visual theme — colors, spacing, fonts, syntax themes.
//
// Ported from the Zig `theme.zig`. Every visual constant lives here so the
// rest of the codebase can `use crate::ui::theme::*` and stay DRY.
#![allow(dead_code)]

// ---------------------------------------------------------------------------
// Rgba
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

    /// Construct from a 24-bit hex literal, e.g. `Rgba::hex(0xc678dd)`.
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

// ---------------------------------------------------------------------------
// Color palette — deep blue-gray base with warm accents
// ---------------------------------------------------------------------------

pub mod colors {
    use super::Rgba;

    // Backgrounds
    pub const BG_PRIMARY: Rgba = Rgba::rgb(16, 16, 16);
    pub const BG_SECONDARY: Rgba = Rgba::rgb(24, 24, 24);
    pub const BG_ELEVATED: Rgba = Rgba::rgb(40, 40, 40);
    pub const BG_HOVER: Rgba = Rgba::rgb(96, 96, 96);

    // Borders & dividers
    pub const BORDER: Rgba = Rgba::rgb(86, 86, 86);
    pub const BORDER_FOCUS: Rgba = Rgba::rgb(180, 180, 180);

    // Text
    pub const TEXT_PRIMARY: Rgba = Rgba::rgb(242, 242, 247);
    pub const TEXT_SECONDARY: Rgba = Rgba::rgb(192, 192, 192);
    pub const TEXT_MUTED: Rgba = Rgba::rgb(128, 128, 128);

    // Accents
    pub const ACCENT_PRIMARY: Rgba = Rgba::rgb(236, 167, 107); // #eca76b warm orange
    pub const ACCENT_SECONDARY: Rgba = Rgba::rgb(129, 199, 132); // #81c784 soft green
    pub const ACCENT_INFO: Rgba = Rgba::rgb(100, 181, 246); // #64b5f6 blue

    // Tab states
    pub const TAB_ACTIVE: Rgba = Rgba::rgb(40, 49, 61);
    pub const TAB_INACTIVE: Rgba = Rgba::rgb(22, 27, 34);
    pub const TAB_HOVER: Rgba = Rgba::rgb(35, 43, 54);

    // Editor
    pub const EDITOR_BG: Rgba = Rgba::rgb(18, 22, 28);
    pub const EDITOR_GUTTER: Rgba = Rgba::rgb(30, 37, 46);
    pub const EDITOR_SELECTION: Rgba = Rgba::new(68, 85, 108, 128);

    // Graph area
    pub const GRAPH_BG: Rgba = Rgba::rgb(15, 18, 23);
    pub const GRAPH_GRID: Rgba = Rgba::new(40, 49, 61, 80);
    pub const GRAPH_AXIS: Rgba = Rgba::rgb(113, 128, 150);

    // Toolbar
    pub const TOOLBAR_BG: Rgba = Rgba::rgb(28, 34, 42);
    pub const TOOLBAR_BUTTON: Rgba = Rgba::rgb(45, 55, 68);
    pub const TOOLBAR_BUTTON_HOVER: Rgba = Rgba::rgb(60, 72, 88);
    pub const TOOLBAR_BUTTON_ACTIVE: Rgba = Rgba::rgb(75, 165, 95); // green play

    // Separator / split handle
    pub const SPLIT_HANDLE: Rgba = Rgba::rgb(50, 50, 55);
    pub const SPLIT_HANDLE_HOVER: Rgba = Rgba::rgb(80, 80, 90);

    // Window control buttons
    pub const CLOSE_BUTTON_HOVER: Rgba = Rgba::rgb(196, 43, 28);

    // Menu / dropdown
    pub const DROPDOWN_BG: Rgba = Rgba::rgb(30, 30, 35);
    pub const DROPDOWN_HOVER: Rgba = Rgba::rgb(55, 62, 75);
    pub const DROPDOWN_SEPARATOR: Rgba = Rgba::rgb(60, 60, 65);
    pub const MENU_ITEM_HOVER: Rgba = Rgba::rgb(50, 55, 65);

    // Scrollbar
    pub const SCROLLBAR_TRACK: Rgba = Rgba::new(40, 40, 45, 100);
    pub const SCROLLBAR_THUMB: Rgba = Rgba::new(100, 100, 110, 180);
    pub const SCROLLBAR_THUMB_HOVER: Rgba = Rgba::new(140, 140, 150, 220);

    // Cursor
    pub const CURSOR: Rgba = Rgba::rgb(230, 230, 230);

    // Play/Stop button
    pub const PLAY_BUTTON: Rgba = Rgba::rgb(75, 165, 95);
    pub const PLAY_BUTTON_HOVER: Rgba = Rgba::rgb(95, 195, 115);
    pub const STOP_BUTTON: Rgba = Rgba::rgb(196, 60, 50);
    pub const STOP_BUTTON_HOVER: Rgba = Rgba::rgb(220, 80, 70);

    // Tooltip
    pub const TOOLTIP_BG: Rgba = Rgba::rgb(60, 60, 65);
    pub const TOOLTIP_BORDER: Rgba = Rgba::rgb(80, 80, 85);
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
    pub const CELL_PADDING: f32 = 12.0;
    pub const CELL_SPACING: f32 = 8.0;
    pub const BUTTON_SIZE: f32 = 24.0;
    pub const HEADER_HEIGHT: f32 = 32.0;
    pub const TEXT_PADDING: f32 = 24.0;

    // Scaled versions — multiply by current zoom factor.
    pub fn menu_height() -> f32 { MENU_HEIGHT * super::fonts::scale() }
    pub fn tab_height() -> f32 { TAB_HEIGHT * super::fonts::scale() }
    pub fn status_height() -> f32 { STATUS_HEIGHT * super::fonts::scale() }
    pub fn split_handle_width() -> f32 { SPLIT_HANDLE_WIDTH * super::fonts::scale() }
    pub fn window_control_width() -> f32 { WINDOW_CONTROL_WIDTH * super::fonts::scale() }
    pub fn dropdown_item_height() -> f32 { DROPDOWN_ITEM_HEIGHT * super::fonts::scale() }
}

// ---------------------------------------------------------------------------
// Border radius
// ---------------------------------------------------------------------------

pub mod radius {
    pub const NONE: f32 = 0.0;
    pub const SM: f32 = 2.0;
    pub const MD: f32 = 4.0;
    pub const LG: f32 = 6.0;
    pub const XL: f32 = 8.0;
}

// ---------------------------------------------------------------------------
// Fonts — base sizes + zoom
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

    /// Zoom state stored as atomic bits (safe, no `unsafe` needed).
    static CURRENT_SCALE: AtomicU32 = AtomicU32::new(1.0_f32.to_bits());

    pub fn scale() -> f32 {
        f32::from_bits(CURRENT_SCALE.load(Ordering::Relaxed))
    }

    pub fn zoom_in() {
        let new = (scale() + SCALE_STEP).min(MAX_SCALE);
        CURRENT_SCALE.store(new.to_bits(), Ordering::Relaxed);
    }

    pub fn zoom_out() {
        let new = (scale() - SCALE_STEP).max(MIN_SCALE);
        CURRENT_SCALE.store(new.to_bits(), Ordering::Relaxed);
    }

    pub fn reset_zoom() {
        CURRENT_SCALE.store(1.0_f32.to_bits(), Ordering::Relaxed);
    }

    pub fn editor_size() -> f32 {
        BASE_EDITOR * scale()
    }
    pub fn editor_line_height() -> f32 {
        editor_size() * LINE_HEIGHT_FACTOR
    }
    pub fn ui_size() -> f32 {
        BASE_UI * scale()
    }
    pub fn small_size() -> f32 {
        BASE_SMALL * scale()
    }
    pub fn tooltip_size() -> f32 {
        BASE_TOOLTIP * scale()
    }
    pub fn status_size() -> f32 {
        BASE_STATUS * scale()
    }
    pub fn menu_size() -> f32 {
        BASE_MENU * scale()
    }
}

// ---------------------------------------------------------------------------
// Split defaults
// ---------------------------------------------------------------------------

pub mod split {
    pub const DEFAULT_RATIO: f32 = 0.45;
    pub const MIN_PANE_SIZE: f32 = 200.0;
}

// ---------------------------------------------------------------------------
// Syntax themes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct SyntaxTheme {
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

pub const THEME_ONE_DARK: SyntaxTheme = SyntaxTheme {
    keyword: Rgba::hex(0xc678dd),
    identifier: Rgba::hex(0xe06c75),
    math_variable: Rgba::hex(0xe5c07b),
    number: Rgba::hex(0xd19a66),
    operator: Rgba::hex(0x56b6c2),
    string: Rgba::hex(0x98c379),
    comment: Rgba::hex(0x5c6370),
    punctuation: Rgba::hex(0xabb2bf),
    whitespace: Rgba::hex(0xabb2bf),
    builtin: Rgba::hex(0x61afef),
    axis: Rgba::hex(0xd19a66),
    type_name: Rgba::hex(0xe5c07b),
    unknown: Rgba::hex(0xe06c75),
};

pub const THEME_MONOKAI: SyntaxTheme = SyntaxTheme {
    keyword: Rgba::hex(0xff6188),
    identifier: Rgba::hex(0xa9dc76),
    math_variable: Rgba::hex(0xffd866),
    number: Rgba::hex(0xab9df2),
    operator: Rgba::hex(0xff6188),
    string: Rgba::hex(0xffd866),
    comment: Rgba::hex(0x727072),
    punctuation: Rgba::hex(0x939293),
    whitespace: Rgba::hex(0x939293),
    builtin: Rgba::hex(0x78dce8),
    axis: Rgba::hex(0xfc9867),
    type_name: Rgba::hex(0x78dce8),
    unknown: Rgba::hex(0xfc9867),
};

pub const THEME_DRACULA: SyntaxTheme = SyntaxTheme {
    keyword: Rgba::hex(0xff79c6),
    identifier: Rgba::hex(0x50fa7b),
    math_variable: Rgba::hex(0xf8f8f2),
    number: Rgba::hex(0xbd93f9),
    operator: Rgba::hex(0xff79c6),
    string: Rgba::hex(0xf1fa8c),
    comment: Rgba::hex(0x6272a4),
    punctuation: Rgba::hex(0xf8f8f2),
    whitespace: Rgba::hex(0xf8f8f2),
    builtin: Rgba::hex(0x8be9fd),
    axis: Rgba::hex(0xffb86c),
    type_name: Rgba::hex(0x8be9fd),
    unknown: Rgba::hex(0xff5555),
};

pub const THEME_CATPPUCCIN: SyntaxTheme = SyntaxTheme {
    keyword: Rgba::hex(0xcba6f7),
    identifier: Rgba::hex(0xa6e3a1),
    math_variable: Rgba::hex(0xf38ba8),
    number: Rgba::hex(0xfab387),
    operator: Rgba::hex(0x89dceb),
    string: Rgba::hex(0xa6e3a1),
    comment: Rgba::hex(0x6c7086),
    punctuation: Rgba::hex(0xbac2de),
    whitespace: Rgba::hex(0xbac2de),
    builtin: Rgba::hex(0x89b4fa),
    axis: Rgba::hex(0xf9e2af),
    type_name: Rgba::hex(0x94e2d5),
    unknown: Rgba::hex(0xf38ba8),
};

pub const THEME_GRUVBOX: SyntaxTheme = SyntaxTheme {
    keyword: Rgba::hex(0xfb4934),
    identifier: Rgba::hex(0x83a598),
    math_variable: Rgba::hex(0xfabd2f),
    number: Rgba::hex(0xd3869b),
    operator: Rgba::hex(0xfe8019),
    string: Rgba::hex(0xb8bb26),
    comment: Rgba::hex(0x928374),
    punctuation: Rgba::hex(0xa89984),
    whitespace: Rgba::hex(0xa89984),
    builtin: Rgba::hex(0x8ec07c),
    axis: Rgba::hex(0xfabd2f),
    type_name: Rgba::hex(0x83a598),
    unknown: Rgba::hex(0xfb4934),
};

pub const THEME_NORD: SyntaxTheme = SyntaxTheme {
    keyword: Rgba::hex(0x81a1c1),
    identifier: Rgba::hex(0x88c0d0),
    math_variable: Rgba::hex(0xd8dee9),
    number: Rgba::hex(0xb48ead),
    operator: Rgba::hex(0x81a1c1),
    string: Rgba::hex(0xa3be8c),
    comment: Rgba::hex(0x616e88),
    punctuation: Rgba::hex(0xd8dee9),
    whitespace: Rgba::hex(0xd8dee9),
    builtin: Rgba::hex(0x88c0d0),
    axis: Rgba::hex(0xebcb8b),
    type_name: Rgba::hex(0x8fbcbb),
    unknown: Rgba::hex(0xbf616a),
};

pub const THEME_SOLARIZED: SyntaxTheme = SyntaxTheme {
    keyword: Rgba::hex(0x859900),
    identifier: Rgba::hex(0x268bd2),
    math_variable: Rgba::hex(0xcb4b16),
    number: Rgba::hex(0xd33682),
    operator: Rgba::hex(0x93a1a1),
    string: Rgba::hex(0x2aa198),
    comment: Rgba::hex(0x586e75),
    punctuation: Rgba::hex(0x839496),
    whitespace: Rgba::hex(0x839496),
    builtin: Rgba::hex(0x268bd2),
    axis: Rgba::hex(0xb58900),
    type_name: Rgba::hex(0xcb4b16),
    unknown: Rgba::hex(0xdc322f),
};

pub const BUILTIN_THEMES: &[(&str, SyntaxTheme)] = &[
    ("Catppuccin", THEME_CATPPUCCIN),
    ("One Dark", THEME_ONE_DARK),
    ("Monokai", THEME_MONOKAI),
    ("Dracula", THEME_DRACULA),
    ("Gruvbox", THEME_GRUVBOX),
    ("Nord", THEME_NORD),
    ("Solarized", THEME_SOLARIZED),
];

// ---------------------------------------------------------------------------
// Active theme selection — atomic index into BUILTIN_THEMES
// ---------------------------------------------------------------------------

mod theme_state {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static CURRENT_THEME: AtomicUsize = AtomicUsize::new(0); // 0 = Catppuccin

    pub fn index() -> usize {
        CURRENT_THEME.load(Ordering::Relaxed)
    }

    pub fn set(idx: usize) {
        CURRENT_THEME.store(idx, Ordering::Relaxed);
    }
}

/// Get the currently active syntax theme.
pub fn active_syntax_theme() -> SyntaxTheme {
    let idx = theme_state::index().min(BUILTIN_THEMES.len() - 1);
    BUILTIN_THEMES[idx].1
}

/// Get the name of the currently active syntax theme.
pub fn active_theme_name() -> &'static str {
    let idx = theme_state::index().min(BUILTIN_THEMES.len() - 1);
    BUILTIN_THEMES[idx].0
}

/// Cycle to the next built-in theme. Returns the new theme name.
pub fn cycle_theme() -> &'static str {
    let next = (theme_state::index() + 1) % BUILTIN_THEMES.len();
    theme_state::set(next);
    BUILTIN_THEMES[next].0
}

/// Set theme by index (0-based into BUILTIN_THEMES).
pub fn set_theme(idx: usize) {
    if idx < BUILTIN_THEMES.len() {
        theme_state::set(idx);
    }
}

/// Number of built-in themes available.
pub fn theme_count() -> usize {
    BUILTIN_THEMES.len()
}
