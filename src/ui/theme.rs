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
// Unified Theme — UI chrome + syntax highlighting in one struct
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    // UI chrome — backgrounds
    pub bg_primary: Rgba,
    pub bg_secondary: Rgba,
    pub bg_elevated: Rgba,
    pub bg_hover: Rgba,

    // Borders & dividers
    pub border: Rgba,
    pub border_focus: Rgba,

    // Text
    pub text_primary: Rgba,
    pub text_secondary: Rgba,
    pub text_muted: Rgba,

    // Accents
    pub accent_primary: Rgba,
    pub accent_secondary: Rgba,
    pub accent_info: Rgba,

    // Tab states
    pub tab_active: Rgba,
    pub tab_inactive: Rgba,
    pub tab_hover: Rgba,

    // Editor
    pub editor_bg: Rgba,
    pub editor_gutter: Rgba,
    pub editor_selection: Rgba,

    // Graph area
    pub graph_bg: Rgba,
    pub graph_grid: Rgba,
    pub graph_axis: Rgba,

    // Toolbar
    pub toolbar_bg: Rgba,
    pub toolbar_button: Rgba,
    pub toolbar_button_hover: Rgba,
    pub toolbar_button_active: Rgba,

    // Split handle
    pub split_handle: Rgba,
    pub split_handle_hover: Rgba,

    // Window controls
    pub close_button_hover: Rgba,

    // Menu / dropdown
    pub dropdown_bg: Rgba,
    pub dropdown_hover: Rgba,
    pub dropdown_separator: Rgba,
    pub menu_item_hover: Rgba,

    // Scrollbar
    pub scrollbar_track: Rgba,
    pub scrollbar_thumb: Rgba,
    pub scrollbar_thumb_hover: Rgba,

    // Cursor
    pub cursor: Rgba,

    // Play/Stop button
    pub play_button: Rgba,
    pub play_button_hover: Rgba,
    pub stop_button: Rgba,
    pub stop_button_hover: Rgba,

    // Tooltip
    pub tooltip_bg: Rgba,
    pub tooltip_border: Rgba,

    // Syntax highlighting
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
// Built-in themes — 7 complete themes with cohesive UI + syntax palettes
// ---------------------------------------------------------------------------

pub const THEME_CATPPUCCIN: Theme = Theme {
    // UI chrome — deep purple-navy (Catppuccin Mocha)
    bg_primary:           Rgba::rgb(30, 30, 46),
    bg_secondary:         Rgba::rgb(36, 36, 54),
    bg_elevated:          Rgba::rgb(49, 50, 68),
    bg_hover:             Rgba::rgb(69, 71, 90),
    border:               Rgba::rgb(69, 71, 90),
    border_focus:         Rgba::hex(0x89b4fa),
    text_primary:         Rgba::hex(0xcdd6f4),
    text_secondary:       Rgba::hex(0xbac2de),
    text_muted:           Rgba::hex(0xa6adc8),
    accent_primary:       Rgba::hex(0xfab387),
    accent_secondary:     Rgba::hex(0xa6e3a1),
    accent_info:          Rgba::hex(0x89b4fa),
    tab_active:           Rgba::rgb(49, 50, 68),
    tab_inactive:         Rgba::rgb(30, 30, 46),
    tab_hover:            Rgba::rgb(40, 40, 58),
    editor_bg:            Rgba::rgb(24, 24, 37),
    editor_gutter:        Rgba::rgb(30, 30, 46),
    editor_selection:     Rgba::new(69, 71, 90, 110),
    graph_bg:             Rgba::rgb(17, 17, 27),
    graph_grid:           Rgba::new(49, 50, 68, 80),
    graph_axis:           Rgba::hex(0xa6adc8),
    toolbar_bg:           Rgba::rgb(30, 30, 46),
    toolbar_button:       Rgba::rgb(49, 50, 68),
    toolbar_button_hover: Rgba::rgb(69, 71, 90),
    toolbar_button_active: Rgba::rgb(75, 165, 95),
    split_handle:         Rgba::rgb(45, 45, 63),
    split_handle_hover:   Rgba::rgb(69, 71, 90),
    close_button_hover:   Rgba::rgb(196, 43, 28),
    dropdown_bg:          Rgba::rgb(36, 36, 54),
    dropdown_hover:       Rgba::rgb(55, 55, 75),
    dropdown_separator:   Rgba::rgb(49, 50, 68),
    menu_item_hover:      Rgba::rgb(55, 55, 75),
    scrollbar_track:      Rgba::new(36, 36, 54, 120),
    scrollbar_thumb:      Rgba::new(108, 112, 134, 190),
    scrollbar_thumb_hover: Rgba::new(137, 140, 170, 230),
    cursor:               Rgba::hex(0xcdd6f4),
    play_button:          Rgba::rgb(75, 165, 95),
    play_button_hover:    Rgba::rgb(95, 195, 115),
    stop_button:          Rgba::rgb(196, 60, 50),
    stop_button_hover:    Rgba::rgb(220, 80, 70),
    tooltip_bg:           Rgba::rgb(49, 50, 68),
    tooltip_border:       Rgba::rgb(69, 71, 90),
    // Syntax
    keyword:       Rgba::hex(0xcba6f7),
    identifier:    Rgba::hex(0xa6e3a1),
    math_variable: Rgba::hex(0xf38ba8),
    number:        Rgba::hex(0xfab387),
    operator:      Rgba::hex(0x89dceb),
    string:        Rgba::hex(0xa6e3a1),
    comment:       Rgba::hex(0x6c7086),
    punctuation:   Rgba::hex(0xbac2de),
    whitespace:    Rgba::hex(0xbac2de),
    builtin:       Rgba::hex(0x89b4fa),
    axis:          Rgba::hex(0xf9e2af),
    type_name:     Rgba::hex(0x94e2d5),
    unknown:       Rgba::hex(0xf38ba8),
};

pub const THEME_ONE_DARK: Theme = Theme {
    // UI chrome — blue-gray
    bg_primary:           Rgba::hex(0x282c34),
    bg_secondary:         Rgba::hex(0x2c313a),
    bg_elevated:          Rgba::hex(0x373b43),
    bg_hover:             Rgba::hex(0x464b55),
    border:               Rgba::hex(0x3e4451),
    border_focus:         Rgba::hex(0x61afef),
    text_primary:         Rgba::hex(0xabb2bf),
    text_secondary:       Rgba::hex(0x9da5b4),
    text_muted:           Rgba::hex(0x8891a5),
    accent_primary:       Rgba::hex(0xd19a66),
    accent_secondary:     Rgba::hex(0x98c379),
    accent_info:          Rgba::hex(0x61afef),
    tab_active:           Rgba::hex(0x373b43),
    tab_inactive:         Rgba::hex(0x282c34),
    tab_hover:            Rgba::hex(0x2f3440),
    editor_bg:            Rgba::hex(0x21252b),
    editor_gutter:        Rgba::hex(0x282c34),
    editor_selection:     Rgba::new(55, 75, 120, 110),
    graph_bg:             Rgba::hex(0x1b1f27),
    graph_grid:           Rgba::new(62, 68, 81, 80),
    graph_axis:           Rgba::hex(0x8891a5),
    toolbar_bg:           Rgba::hex(0x282c34),
    toolbar_button:       Rgba::hex(0x373b43),
    toolbar_button_hover: Rgba::hex(0x464b55),
    toolbar_button_active: Rgba::rgb(75, 165, 95),
    split_handle:         Rgba::hex(0x343842),
    split_handle_hover:   Rgba::hex(0x464b55),
    close_button_hover:   Rgba::rgb(196, 43, 28),
    dropdown_bg:          Rgba::hex(0x2c313a),
    dropdown_hover:       Rgba::hex(0x3e4451),
    dropdown_separator:   Rgba::hex(0x373b43),
    menu_item_hover:      Rgba::hex(0x3e4451),
    scrollbar_track:      Rgba::new(44, 49, 58, 120),
    scrollbar_thumb:      Rgba::new(92, 99, 112, 190),
    scrollbar_thumb_hover: Rgba::new(120, 128, 142, 230),
    cursor:               Rgba::hex(0xabb2bf),
    play_button:          Rgba::rgb(75, 165, 95),
    play_button_hover:    Rgba::rgb(95, 195, 115),
    stop_button:          Rgba::rgb(196, 60, 50),
    stop_button_hover:    Rgba::rgb(220, 80, 70),
    tooltip_bg:           Rgba::hex(0x373b43),
    tooltip_border:       Rgba::hex(0x464b55),
    // Syntax
    keyword:       Rgba::hex(0xc678dd),
    identifier:    Rgba::hex(0xe06c75),
    math_variable: Rgba::hex(0xe5c07b),
    number:        Rgba::hex(0xd19a66),
    operator:      Rgba::hex(0x56b6c2),
    string:        Rgba::hex(0x98c379),
    comment:       Rgba::hex(0x5c6370),
    punctuation:   Rgba::hex(0xabb2bf),
    whitespace:    Rgba::hex(0xabb2bf),
    builtin:       Rgba::hex(0x61afef),
    axis:          Rgba::hex(0xd19a66),
    type_name:     Rgba::hex(0xe5c07b),
    unknown:       Rgba::hex(0xe06c75),
};

pub const THEME_MONOKAI: Theme = Theme {
    // UI chrome — warm charcoal
    bg_primary:           Rgba::hex(0x272822),
    bg_secondary:         Rgba::hex(0x2d2e27),
    bg_elevated:          Rgba::hex(0x3b3c32),
    bg_hover:             Rgba::hex(0x4b4c40),
    border:               Rgba::hex(0x45463a),
    border_focus:         Rgba::hex(0xa6e22e),
    text_primary:         Rgba::hex(0xf8f8f2),
    text_secondary:       Rgba::hex(0xd6d6ca),
    text_muted:           Rgba::hex(0xb0ae9e),
    accent_primary:       Rgba::hex(0xfd971f),
    accent_secondary:     Rgba::hex(0xa6e22e),
    accent_info:          Rgba::hex(0x66d9ef),
    tab_active:           Rgba::hex(0x3b3c32),
    tab_inactive:         Rgba::hex(0x272822),
    tab_hover:            Rgba::hex(0x323328),
    editor_bg:            Rgba::hex(0x222318),
    editor_gutter:        Rgba::hex(0x272822),
    editor_selection:     Rgba::new(73, 72, 62, 120),
    graph_bg:             Rgba::hex(0x1c1d14),
    graph_grid:           Rgba::new(59, 60, 50, 80),
    graph_axis:           Rgba::hex(0xb0ae9e),
    toolbar_bg:           Rgba::hex(0x272822),
    toolbar_button:       Rgba::hex(0x3b3c32),
    toolbar_button_hover: Rgba::hex(0x4b4c40),
    toolbar_button_active: Rgba::rgb(75, 165, 95),
    split_handle:         Rgba::hex(0x35362c),
    split_handle_hover:   Rgba::hex(0x4b4c40),
    close_button_hover:   Rgba::rgb(196, 43, 28),
    dropdown_bg:          Rgba::hex(0x2d2e27),
    dropdown_hover:       Rgba::hex(0x45463a),
    dropdown_separator:   Rgba::hex(0x3b3c32),
    menu_item_hover:      Rgba::hex(0x45463a),
    scrollbar_track:      Rgba::new(45, 46, 39, 120),
    scrollbar_thumb:      Rgba::new(110, 109, 100, 190),
    scrollbar_thumb_hover: Rgba::new(140, 138, 128, 230),
    cursor:               Rgba::hex(0xf8f8f2),
    play_button:          Rgba::rgb(75, 165, 95),
    play_button_hover:    Rgba::rgb(95, 195, 115),
    stop_button:          Rgba::rgb(196, 60, 50),
    stop_button_hover:    Rgba::rgb(220, 80, 70),
    tooltip_bg:           Rgba::hex(0x3b3c32),
    tooltip_border:       Rgba::hex(0x4b4c40),
    // Syntax
    keyword:       Rgba::hex(0xff6188),
    identifier:    Rgba::hex(0xa9dc76),
    math_variable: Rgba::hex(0xffd866),
    number:        Rgba::hex(0xab9df2),
    operator:      Rgba::hex(0xff6188),
    string:        Rgba::hex(0xffd866),
    comment:       Rgba::hex(0x727072),
    punctuation:   Rgba::hex(0x939293),
    whitespace:    Rgba::hex(0x939293),
    builtin:       Rgba::hex(0x78dce8),
    axis:          Rgba::hex(0xfc9867),
    type_name:     Rgba::hex(0x78dce8),
    unknown:       Rgba::hex(0xfc9867),
};

pub const THEME_DRACULA: Theme = Theme {
    // UI chrome — purple-dark
    bg_primary:           Rgba::hex(0x282a36),
    bg_secondary:         Rgba::hex(0x2d2f3d),
    bg_elevated:          Rgba::hex(0x3c3f4e),
    bg_hover:             Rgba::hex(0x4d5066),
    border:               Rgba::hex(0x44475a),
    border_focus:         Rgba::hex(0xbd93f9),
    text_primary:         Rgba::hex(0xf8f8f2),
    text_secondary:       Rgba::hex(0xd8d8d0),
    text_muted:           Rgba::hex(0xb0b2c0),
    accent_primary:       Rgba::hex(0xffb86c),
    accent_secondary:     Rgba::hex(0x50fa7b),
    accent_info:          Rgba::hex(0x8be9fd),
    tab_active:           Rgba::hex(0x3c3f4e),
    tab_inactive:         Rgba::hex(0x282a36),
    tab_hover:            Rgba::hex(0x333544),
    editor_bg:            Rgba::hex(0x21222c),
    editor_gutter:        Rgba::hex(0x282a36),
    editor_selection:     Rgba::new(68, 71, 90, 120),
    graph_bg:             Rgba::hex(0x1c1d26),
    graph_grid:           Rgba::new(60, 63, 78, 80),
    graph_axis:           Rgba::hex(0xb0b2c0),
    toolbar_bg:           Rgba::hex(0x282a36),
    toolbar_button:       Rgba::hex(0x3c3f4e),
    toolbar_button_hover: Rgba::hex(0x4d5066),
    toolbar_button_active: Rgba::rgb(75, 165, 95),
    split_handle:         Rgba::hex(0x383a48),
    split_handle_hover:   Rgba::hex(0x4d5066),
    close_button_hover:   Rgba::hex(0xff5555),
    dropdown_bg:          Rgba::hex(0x2d2f3d),
    dropdown_hover:       Rgba::hex(0x44475a),
    dropdown_separator:   Rgba::hex(0x3c3f4e),
    menu_item_hover:      Rgba::hex(0x44475a),
    scrollbar_track:      Rgba::new(45, 47, 61, 120),
    scrollbar_thumb:      Rgba::new(98, 100, 120, 190),
    scrollbar_thumb_hover: Rgba::new(130, 133, 158, 230),
    cursor:               Rgba::hex(0xf8f8f2),
    play_button:          Rgba::rgb(75, 165, 95),
    play_button_hover:    Rgba::rgb(95, 195, 115),
    stop_button:          Rgba::hex(0xff5555),
    stop_button_hover:    Rgba::hex(0xff6e6e),
    tooltip_bg:           Rgba::hex(0x3c3f4e),
    tooltip_border:       Rgba::hex(0x44475a),
    // Syntax
    keyword:       Rgba::hex(0xff79c6),
    identifier:    Rgba::hex(0x50fa7b),
    math_variable: Rgba::hex(0xf8f8f2),
    number:        Rgba::hex(0xbd93f9),
    operator:      Rgba::hex(0xff79c6),
    string:        Rgba::hex(0xf1fa8c),
    comment:       Rgba::hex(0x6272a4),
    punctuation:   Rgba::hex(0xf8f8f2),
    whitespace:    Rgba::hex(0xf8f8f2),
    builtin:       Rgba::hex(0x8be9fd),
    axis:          Rgba::hex(0xffb86c),
    type_name:     Rgba::hex(0x8be9fd),
    unknown:       Rgba::hex(0xff5555),
};

pub const THEME_GRUVBOX: Theme = Theme {
    // UI chrome — warm brown
    bg_primary:           Rgba::hex(0x282828),
    bg_secondary:         Rgba::hex(0x2e2e2a),
    bg_elevated:          Rgba::hex(0x3c3836),
    bg_hover:             Rgba::hex(0x504945),
    border:               Rgba::hex(0x504945),
    border_focus:         Rgba::hex(0xfe8019),
    text_primary:         Rgba::hex(0xebdbb2),
    text_secondary:       Rgba::hex(0xd5c4a1),
    text_muted:           Rgba::hex(0xbdae93),
    accent_primary:       Rgba::hex(0xfe8019),
    accent_secondary:     Rgba::hex(0xb8bb26),
    accent_info:          Rgba::hex(0x83a598),
    tab_active:           Rgba::hex(0x3c3836),
    tab_inactive:         Rgba::hex(0x282828),
    tab_hover:            Rgba::hex(0x33302e),
    editor_bg:            Rgba::hex(0x1d2021),
    editor_gutter:        Rgba::hex(0x282828),
    editor_selection:     Rgba::new(80, 73, 69, 120),
    graph_bg:             Rgba::hex(0x171819),
    graph_grid:           Rgba::new(60, 56, 54, 80),
    graph_axis:           Rgba::hex(0xbdae93),
    toolbar_bg:           Rgba::hex(0x282828),
    toolbar_button:       Rgba::hex(0x3c3836),
    toolbar_button_hover: Rgba::hex(0x504945),
    toolbar_button_active: Rgba::rgb(75, 165, 95),
    split_handle:         Rgba::hex(0x343130),
    split_handle_hover:   Rgba::hex(0x504945),
    close_button_hover:   Rgba::hex(0xcc241d),
    dropdown_bg:          Rgba::hex(0x2e2e2a),
    dropdown_hover:       Rgba::hex(0x504945),
    dropdown_separator:   Rgba::hex(0x3c3836),
    menu_item_hover:      Rgba::hex(0x504945),
    scrollbar_track:      Rgba::new(46, 46, 42, 120),
    scrollbar_thumb:      Rgba::new(102, 92, 84, 190),
    scrollbar_thumb_hover: Rgba::new(140, 128, 118, 230),
    cursor:               Rgba::hex(0xebdbb2),
    play_button:          Rgba::rgb(75, 165, 95),
    play_button_hover:    Rgba::rgb(95, 195, 115),
    stop_button:          Rgba::hex(0xcc241d),
    stop_button_hover:    Rgba::hex(0xfb4934),
    tooltip_bg:           Rgba::hex(0x3c3836),
    tooltip_border:       Rgba::hex(0x504945),
    // Syntax
    keyword:       Rgba::hex(0xfb4934),
    identifier:    Rgba::hex(0x83a598),
    math_variable: Rgba::hex(0xfabd2f),
    number:        Rgba::hex(0xd3869b),
    operator:      Rgba::hex(0xfe8019),
    string:        Rgba::hex(0xb8bb26),
    comment:       Rgba::hex(0x928374),
    punctuation:   Rgba::hex(0xa89984),
    whitespace:    Rgba::hex(0xa89984),
    builtin:       Rgba::hex(0x8ec07c),
    axis:          Rgba::hex(0xfabd2f),
    type_name:     Rgba::hex(0x83a598),
    unknown:       Rgba::hex(0xfb4934),
};

pub const THEME_NORD: Theme = Theme {
    // UI chrome — polar night
    bg_primary:           Rgba::hex(0x2e3440),
    bg_secondary:         Rgba::hex(0x333a47),
    bg_elevated:          Rgba::hex(0x3b4252),
    bg_hover:             Rgba::hex(0x4c566a),
    border:               Rgba::hex(0x434c5e),
    border_focus:         Rgba::hex(0x88c0d0),
    text_primary:         Rgba::hex(0xeceff4),
    text_secondary:       Rgba::hex(0xd8dee9),
    text_muted:           Rgba::hex(0xb0b8c8),
    accent_primary:       Rgba::hex(0xebcb8b),
    accent_secondary:     Rgba::hex(0xa3be8c),
    accent_info:          Rgba::hex(0x88c0d0),
    tab_active:           Rgba::hex(0x3b4252),
    tab_inactive:         Rgba::hex(0x2e3440),
    tab_hover:            Rgba::hex(0x353c4a),
    editor_bg:            Rgba::hex(0x272d38),
    editor_gutter:        Rgba::hex(0x2e3440),
    editor_selection:     Rgba::new(76, 86, 106, 110),
    graph_bg:             Rgba::hex(0x222730),
    graph_grid:           Rgba::new(59, 66, 82, 80),
    graph_axis:           Rgba::hex(0xb0b8c8),
    toolbar_bg:           Rgba::hex(0x2e3440),
    toolbar_button:       Rgba::hex(0x3b4252),
    toolbar_button_hover: Rgba::hex(0x4c566a),
    toolbar_button_active: Rgba::rgb(75, 165, 95),
    split_handle:         Rgba::hex(0x373e4d),
    split_handle_hover:   Rgba::hex(0x4c566a),
    close_button_hover:   Rgba::hex(0xbf616a),
    dropdown_bg:          Rgba::hex(0x333a47),
    dropdown_hover:       Rgba::hex(0x434c5e),
    dropdown_separator:   Rgba::hex(0x3b4252),
    menu_item_hover:      Rgba::hex(0x434c5e),
    scrollbar_track:      Rgba::new(51, 58, 71, 120),
    scrollbar_thumb:      Rgba::new(100, 110, 130, 190),
    scrollbar_thumb_hover: Rgba::new(135, 145, 168, 230),
    cursor:               Rgba::hex(0xeceff4),
    play_button:          Rgba::rgb(75, 165, 95),
    play_button_hover:    Rgba::rgb(95, 195, 115),
    stop_button:          Rgba::hex(0xbf616a),
    stop_button_hover:    Rgba::hex(0xd08770),
    tooltip_bg:           Rgba::hex(0x3b4252),
    tooltip_border:       Rgba::hex(0x434c5e),
    // Syntax
    keyword:       Rgba::hex(0x81a1c1),
    identifier:    Rgba::hex(0x88c0d0),
    math_variable: Rgba::hex(0xd8dee9),
    number:        Rgba::hex(0xb48ead),
    operator:      Rgba::hex(0x81a1c1),
    string:        Rgba::hex(0xa3be8c),
    comment:       Rgba::hex(0x616e88),
    punctuation:   Rgba::hex(0xd8dee9),
    whitespace:    Rgba::hex(0xd8dee9),
    builtin:       Rgba::hex(0x88c0d0),
    axis:          Rgba::hex(0xebcb8b),
    type_name:     Rgba::hex(0x8fbcbb),
    unknown:       Rgba::hex(0xbf616a),
};

pub const THEME_SOLARIZED: Theme = Theme {
    // UI chrome — dark teal
    bg_primary:           Rgba::hex(0x002b36),
    bg_secondary:         Rgba::hex(0x003340),
    bg_elevated:          Rgba::hex(0x073642),
    bg_hover:             Rgba::hex(0x0a4555),
    border:               Rgba::hex(0x0a4555),
    border_focus:         Rgba::hex(0x268bd2),
    text_primary:         Rgba::hex(0xfdf6e3),
    text_secondary:       Rgba::hex(0xeee8d5),
    text_muted:           Rgba::hex(0x93a1a1),
    accent_primary:       Rgba::hex(0xcb4b16),
    accent_secondary:     Rgba::hex(0x859900),
    accent_info:          Rgba::hex(0x268bd2),
    tab_active:           Rgba::hex(0x073642),
    tab_inactive:         Rgba::hex(0x002b36),
    tab_hover:            Rgba::hex(0x04303d),
    editor_bg:            Rgba::hex(0x00212b),
    editor_gutter:        Rgba::hex(0x002b36),
    editor_selection:     Rgba::new(7, 54, 66, 130),
    graph_bg:             Rgba::hex(0x001a22),
    graph_grid:           Rgba::new(7, 54, 66, 80),
    graph_axis:           Rgba::hex(0x93a1a1),
    toolbar_bg:           Rgba::hex(0x002b36),
    toolbar_button:       Rgba::hex(0x073642),
    toolbar_button_hover: Rgba::hex(0x0a4555),
    toolbar_button_active: Rgba::rgb(75, 165, 95),
    split_handle:         Rgba::hex(0x053340),
    split_handle_hover:   Rgba::hex(0x0a4555),
    close_button_hover:   Rgba::hex(0xdc322f),
    dropdown_bg:          Rgba::hex(0x003340),
    dropdown_hover:       Rgba::hex(0x0a4555),
    dropdown_separator:   Rgba::hex(0x073642),
    menu_item_hover:      Rgba::hex(0x0a4555),
    scrollbar_track:      Rgba::new(0, 51, 64, 120),
    scrollbar_thumb:      Rgba::new(88, 110, 117, 190),
    scrollbar_thumb_hover: Rgba::new(120, 140, 148, 230),
    cursor:               Rgba::hex(0xfdf6e3),
    play_button:          Rgba::rgb(75, 165, 95),
    play_button_hover:    Rgba::rgb(95, 195, 115),
    stop_button:          Rgba::hex(0xdc322f),
    stop_button_hover:    Rgba::hex(0xef4f4c),
    tooltip_bg:           Rgba::hex(0x073642),
    tooltip_border:       Rgba::hex(0x0a4555),
    // Syntax
    keyword:       Rgba::hex(0x859900),
    identifier:    Rgba::hex(0x268bd2),
    math_variable: Rgba::hex(0xcb4b16),
    number:        Rgba::hex(0xd33682),
    operator:      Rgba::hex(0x93a1a1),
    string:        Rgba::hex(0x2aa198),
    comment:       Rgba::hex(0x586e75),
    punctuation:   Rgba::hex(0x839496),
    whitespace:    Rgba::hex(0x839496),
    builtin:       Rgba::hex(0x268bd2),
    axis:          Rgba::hex(0xb58900),
    type_name:     Rgba::hex(0xcb4b16),
    unknown:       Rgba::hex(0xdc322f),
};

pub const BUILTIN_THEMES: &[(&str, &Theme)] = &[
    ("Catppuccin", &THEME_CATPPUCCIN),
    ("One Dark",   &THEME_ONE_DARK),
    ("Monokai",    &THEME_MONOKAI),
    ("Dracula",    &THEME_DRACULA),
    ("Gruvbox",    &THEME_GRUVBOX),
    ("Nord",       &THEME_NORD),
    ("Solarized",  &THEME_SOLARIZED),
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

/// Get the currently active theme (UI chrome + syntax).
pub fn theme() -> &'static Theme {
    let idx = theme_state::index().min(BUILTIN_THEMES.len() - 1);
    BUILTIN_THEMES[idx].1
}

/// Get the name of the currently active theme.
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
