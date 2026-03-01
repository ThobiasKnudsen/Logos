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
    pub axis_zone_bg: Rgba,
    pub axis_label: Rgba,

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
    // UI chrome — deep purple-navy (Catppuccin Mocha, darkened)
    bg_primary:           Rgba::rgb(14, 14, 22),
    bg_secondary:         Rgba::rgb(19, 19, 30),
    bg_elevated:          Rgba::rgb(28, 28, 42),
    bg_hover:             Rgba::rgb(42, 42, 58),
    border:               Rgba::rgb(38, 38, 54),
    border_focus:         Rgba::hex(0x89b4fa),
    text_primary:         Rgba::hex(0xcdd6f4),
    text_secondary:       Rgba::hex(0xbac2de),
    text_muted:           Rgba::hex(0xa6adc8),
    accent_primary:       Rgba::hex(0xfab387),
    accent_secondary:     Rgba::hex(0xa6e3a1),
    accent_info:          Rgba::hex(0x89b4fa),
    tab_active:           Rgba::rgb(28, 28, 42),
    tab_inactive:         Rgba::rgb(14, 14, 22),
    tab_hover:            Rgba::rgb(22, 22, 34),
    editor_bg:            Rgba::rgb(11, 11, 18),
    editor_gutter:        Rgba::rgb(16, 16, 26),
    editor_selection:     Rgba::new(50, 50, 72, 110),
    graph_bg:             Rgba::rgb(8, 8, 14),
    graph_grid:           Rgba::new(30, 30, 46, 80),
    graph_axis:           Rgba::hex(0xa6adc8),
    axis_zone_bg:         Rgba::new(14, 14, 22, 210),
    axis_label:           Rgba::hex(0x7f849c),
    toolbar_bg:           Rgba::rgb(16, 16, 26),
    toolbar_button:       Rgba::rgb(28, 28, 42),
    toolbar_button_hover: Rgba::rgb(42, 42, 58),
    toolbar_button_active: Rgba::rgb(75, 165, 95),
    split_handle:         Rgba::rgb(24, 24, 36),
    split_handle_hover:   Rgba::rgb(42, 42, 58),
    close_button_hover:   Rgba::rgb(196, 43, 28),
    dropdown_bg:          Rgba::rgb(19, 19, 30),
    dropdown_hover:       Rgba::rgb(32, 32, 48),
    dropdown_separator:   Rgba::rgb(28, 28, 42),
    menu_item_hover:      Rgba::rgb(32, 32, 48),
    scrollbar_track:      Rgba::new(19, 19, 30, 120),
    scrollbar_thumb:      Rgba::new(70, 72, 90, 190),
    scrollbar_thumb_hover: Rgba::new(100, 102, 125, 230),
    cursor:               Rgba::hex(0xcdd6f4),
    play_button:          Rgba::rgb(75, 165, 95),
    play_button_hover:    Rgba::rgb(95, 195, 115),
    stop_button:          Rgba::rgb(196, 60, 50),
    stop_button_hover:    Rgba::rgb(220, 80, 70),
    tooltip_bg:           Rgba::rgb(28, 28, 42),
    tooltip_border:       Rgba::rgb(42, 42, 58),
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
    // UI chrome — blue-gray, darkened
    bg_primary:           Rgba::rgb(15, 17, 22),
    bg_secondary:         Rgba::rgb(20, 23, 30),
    bg_elevated:          Rgba::rgb(30, 33, 40),
    bg_hover:             Rgba::rgb(44, 48, 58),
    border:               Rgba::rgb(36, 40, 50),
    border_focus:         Rgba::hex(0x61afef),
    text_primary:         Rgba::hex(0xabb2bf),
    text_secondary:       Rgba::hex(0x9da5b4),
    text_muted:           Rgba::hex(0x8891a5),
    accent_primary:       Rgba::hex(0xd19a66),
    accent_secondary:     Rgba::hex(0x98c379),
    accent_info:          Rgba::hex(0x61afef),
    tab_active:           Rgba::rgb(30, 33, 40),
    tab_inactive:         Rgba::rgb(15, 17, 22),
    tab_hover:            Rgba::rgb(24, 27, 34),
    editor_bg:            Rgba::rgb(12, 14, 18),
    editor_gutter:        Rgba::rgb(17, 19, 25),
    editor_selection:     Rgba::new(40, 55, 90, 110),
    graph_bg:             Rgba::rgb(9, 11, 15),
    graph_grid:           Rgba::new(35, 40, 52, 80),
    graph_axis:           Rgba::hex(0x8891a5),
    axis_zone_bg:         Rgba::new(15, 17, 22, 210),
    axis_label:           Rgba::hex(0x6b7280),
    toolbar_bg:           Rgba::rgb(17, 19, 25),
    toolbar_button:       Rgba::rgb(30, 33, 40),
    toolbar_button_hover: Rgba::rgb(44, 48, 58),
    toolbar_button_active: Rgba::rgb(75, 165, 95),
    split_handle:         Rgba::rgb(25, 28, 36),
    split_handle_hover:   Rgba::rgb(44, 48, 58),
    close_button_hover:   Rgba::rgb(196, 43, 28),
    dropdown_bg:          Rgba::rgb(20, 23, 30),
    dropdown_hover:       Rgba::rgb(34, 38, 48),
    dropdown_separator:   Rgba::rgb(30, 33, 40),
    menu_item_hover:      Rgba::rgb(34, 38, 48),
    scrollbar_track:      Rgba::new(20, 23, 30, 120),
    scrollbar_thumb:      Rgba::new(60, 66, 78, 190),
    scrollbar_thumb_hover: Rgba::new(88, 95, 110, 230),
    cursor:               Rgba::hex(0xabb2bf),
    play_button:          Rgba::rgb(75, 165, 95),
    play_button_hover:    Rgba::rgb(95, 195, 115),
    stop_button:          Rgba::rgb(196, 60, 50),
    stop_button_hover:    Rgba::rgb(220, 80, 70),
    tooltip_bg:           Rgba::rgb(30, 33, 40),
    tooltip_border:       Rgba::rgb(44, 48, 58),
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
    // UI chrome — warm charcoal, darkened
    bg_primary:           Rgba::rgb(16, 17, 12),
    bg_secondary:         Rgba::rgb(22, 22, 16),
    bg_elevated:          Rgba::rgb(32, 32, 24),
    bg_hover:             Rgba::rgb(46, 46, 36),
    border:               Rgba::rgb(38, 39, 30),
    border_focus:         Rgba::hex(0xa6e22e),
    text_primary:         Rgba::hex(0xf8f8f2),
    text_secondary:       Rgba::hex(0xd6d6ca),
    text_muted:           Rgba::hex(0xb0ae9e),
    accent_primary:       Rgba::hex(0xfd971f),
    accent_secondary:     Rgba::hex(0xa6e22e),
    accent_info:          Rgba::hex(0x66d9ef),
    tab_active:           Rgba::rgb(32, 32, 24),
    tab_inactive:         Rgba::rgb(16, 17, 12),
    tab_hover:            Rgba::rgb(25, 25, 18),
    editor_bg:            Rgba::rgb(13, 13, 9),
    editor_gutter:        Rgba::rgb(18, 18, 13),
    editor_selection:     Rgba::new(50, 50, 40, 120),
    graph_bg:             Rgba::rgb(9, 10, 6),
    graph_grid:           Rgba::new(36, 36, 28, 80),
    graph_axis:           Rgba::hex(0xb0ae9e),
    axis_zone_bg:         Rgba::new(16, 17, 12, 210),
    axis_label:           Rgba::hex(0x75736a),
    toolbar_bg:           Rgba::rgb(18, 18, 13),
    toolbar_button:       Rgba::rgb(32, 32, 24),
    toolbar_button_hover: Rgba::rgb(46, 46, 36),
    toolbar_button_active: Rgba::rgb(75, 165, 95),
    split_handle:         Rgba::rgb(26, 27, 20),
    split_handle_hover:   Rgba::rgb(46, 46, 36),
    close_button_hover:   Rgba::rgb(196, 43, 28),
    dropdown_bg:          Rgba::rgb(22, 22, 16),
    dropdown_hover:       Rgba::rgb(36, 36, 28),
    dropdown_separator:   Rgba::rgb(32, 32, 24),
    menu_item_hover:      Rgba::rgb(36, 36, 28),
    scrollbar_track:      Rgba::new(22, 22, 16, 120),
    scrollbar_thumb:      Rgba::new(68, 67, 58, 190),
    scrollbar_thumb_hover: Rgba::new(95, 94, 84, 230),
    cursor:               Rgba::hex(0xf8f8f2),
    play_button:          Rgba::rgb(75, 165, 95),
    play_button_hover:    Rgba::rgb(95, 195, 115),
    stop_button:          Rgba::rgb(196, 60, 50),
    stop_button_hover:    Rgba::rgb(220, 80, 70),
    tooltip_bg:           Rgba::rgb(32, 32, 24),
    tooltip_border:       Rgba::rgb(46, 46, 36),
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
    // UI chrome — purple-dark, darkened
    bg_primary:           Rgba::rgb(14, 15, 22),
    bg_secondary:         Rgba::rgb(20, 21, 30),
    bg_elevated:          Rgba::rgb(30, 31, 42),
    bg_hover:             Rgba::rgb(44, 46, 60),
    border:               Rgba::rgb(36, 38, 50),
    border_focus:         Rgba::hex(0xbd93f9),
    text_primary:         Rgba::hex(0xf8f8f2),
    text_secondary:       Rgba::hex(0xd8d8d0),
    text_muted:           Rgba::hex(0xb0b2c0),
    accent_primary:       Rgba::hex(0xffb86c),
    accent_secondary:     Rgba::hex(0x50fa7b),
    accent_info:          Rgba::hex(0x8be9fd),
    tab_active:           Rgba::rgb(30, 31, 42),
    tab_inactive:         Rgba::rgb(14, 15, 22),
    tab_hover:            Rgba::rgb(23, 24, 34),
    editor_bg:            Rgba::rgb(11, 12, 18),
    editor_gutter:        Rgba::rgb(16, 17, 24),
    editor_selection:     Rgba::new(48, 50, 68, 120),
    graph_bg:             Rgba::rgb(8, 9, 14),
    graph_grid:           Rgba::new(34, 36, 50, 80),
    graph_axis:           Rgba::hex(0xb0b2c0),
    axis_zone_bg:         Rgba::new(14, 15, 22, 210),
    axis_label:           Rgba::hex(0x6272a4),
    toolbar_bg:           Rgba::rgb(16, 17, 24),
    toolbar_button:       Rgba::rgb(30, 31, 42),
    toolbar_button_hover: Rgba::rgb(44, 46, 60),
    toolbar_button_active: Rgba::rgb(75, 165, 95),
    split_handle:         Rgba::rgb(24, 25, 36),
    split_handle_hover:   Rgba::rgb(44, 46, 60),
    close_button_hover:   Rgba::hex(0xff5555),
    dropdown_bg:          Rgba::rgb(20, 21, 30),
    dropdown_hover:       Rgba::rgb(34, 36, 48),
    dropdown_separator:   Rgba::rgb(30, 31, 42),
    menu_item_hover:      Rgba::rgb(34, 36, 48),
    scrollbar_track:      Rgba::new(20, 21, 30, 120),
    scrollbar_thumb:      Rgba::new(64, 66, 82, 190),
    scrollbar_thumb_hover: Rgba::new(92, 95, 115, 230),
    cursor:               Rgba::hex(0xf8f8f2),
    play_button:          Rgba::rgb(75, 165, 95),
    play_button_hover:    Rgba::rgb(95, 195, 115),
    stop_button:          Rgba::hex(0xff5555),
    stop_button_hover:    Rgba::hex(0xff6e6e),
    tooltip_bg:           Rgba::rgb(30, 31, 42),
    tooltip_border:       Rgba::rgb(44, 46, 60),
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
    // UI chrome — warm brown, darkened
    bg_primary:           Rgba::rgb(16, 15, 14),
    bg_secondary:         Rgba::rgb(22, 20, 18),
    bg_elevated:          Rgba::rgb(32, 29, 26),
    bg_hover:             Rgba::rgb(46, 42, 38),
    border:               Rgba::rgb(38, 35, 32),
    border_focus:         Rgba::hex(0xfe8019),
    text_primary:         Rgba::hex(0xebdbb2),
    text_secondary:       Rgba::hex(0xd5c4a1),
    text_muted:           Rgba::hex(0xbdae93),
    accent_primary:       Rgba::hex(0xfe8019),
    accent_secondary:     Rgba::hex(0xb8bb26),
    accent_info:          Rgba::hex(0x83a598),
    tab_active:           Rgba::rgb(32, 29, 26),
    tab_inactive:         Rgba::rgb(16, 15, 14),
    tab_hover:            Rgba::rgb(25, 23, 20),
    editor_bg:            Rgba::rgb(12, 12, 10),
    editor_gutter:        Rgba::rgb(18, 17, 15),
    editor_selection:     Rgba::new(55, 50, 45, 120),
    graph_bg:             Rgba::rgb(9, 9, 7),
    graph_grid:           Rgba::new(36, 33, 30, 80),
    graph_axis:           Rgba::hex(0xbdae93),
    axis_zone_bg:         Rgba::new(16, 15, 14, 210),
    axis_label:           Rgba::hex(0x7c6f64),
    toolbar_bg:           Rgba::rgb(18, 17, 15),
    toolbar_button:       Rgba::rgb(32, 29, 26),
    toolbar_button_hover: Rgba::rgb(46, 42, 38),
    toolbar_button_active: Rgba::rgb(75, 165, 95),
    split_handle:         Rgba::rgb(26, 24, 22),
    split_handle_hover:   Rgba::rgb(46, 42, 38),
    close_button_hover:   Rgba::hex(0xcc241d),
    dropdown_bg:          Rgba::rgb(22, 20, 18),
    dropdown_hover:       Rgba::rgb(36, 33, 30),
    dropdown_separator:   Rgba::rgb(32, 29, 26),
    menu_item_hover:      Rgba::rgb(36, 33, 30),
    scrollbar_track:      Rgba::new(22, 20, 18, 120),
    scrollbar_thumb:      Rgba::new(66, 60, 54, 190),
    scrollbar_thumb_hover: Rgba::new(95, 86, 78, 230),
    cursor:               Rgba::hex(0xebdbb2),
    play_button:          Rgba::rgb(75, 165, 95),
    play_button_hover:    Rgba::rgb(95, 195, 115),
    stop_button:          Rgba::hex(0xcc241d),
    stop_button_hover:    Rgba::hex(0xfb4934),
    tooltip_bg:           Rgba::rgb(32, 29, 26),
    tooltip_border:       Rgba::rgb(46, 42, 38),
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
    // UI chrome — polar night, darkened
    bg_primary:           Rgba::rgb(14, 18, 24),
    bg_secondary:         Rgba::rgb(19, 24, 32),
    bg_elevated:          Rgba::rgb(28, 34, 44),
    bg_hover:             Rgba::rgb(42, 50, 62),
    border:               Rgba::rgb(35, 42, 54),
    border_focus:         Rgba::hex(0x88c0d0),
    text_primary:         Rgba::hex(0xeceff4),
    text_secondary:       Rgba::hex(0xd8dee9),
    text_muted:           Rgba::hex(0xb0b8c8),
    accent_primary:       Rgba::hex(0xebcb8b),
    accent_secondary:     Rgba::hex(0xa3be8c),
    accent_info:          Rgba::hex(0x88c0d0),
    tab_active:           Rgba::rgb(28, 34, 44),
    tab_inactive:         Rgba::rgb(14, 18, 24),
    tab_hover:            Rgba::rgb(22, 28, 36),
    editor_bg:            Rgba::rgb(11, 14, 20),
    editor_gutter:        Rgba::rgb(16, 20, 28),
    editor_selection:     Rgba::new(50, 58, 76, 110),
    graph_bg:             Rgba::rgb(8, 11, 16),
    graph_grid:           Rgba::new(32, 38, 50, 80),
    graph_axis:           Rgba::hex(0xb0b8c8),
    axis_zone_bg:         Rgba::new(14, 18, 24, 210),
    axis_label:           Rgba::hex(0x616e88),
    toolbar_bg:           Rgba::rgb(16, 20, 28),
    toolbar_button:       Rgba::rgb(28, 34, 44),
    toolbar_button_hover: Rgba::rgb(42, 50, 62),
    toolbar_button_active: Rgba::rgb(75, 165, 95),
    split_handle:         Rgba::rgb(23, 28, 38),
    split_handle_hover:   Rgba::rgb(42, 50, 62),
    close_button_hover:   Rgba::hex(0xbf616a),
    dropdown_bg:          Rgba::rgb(19, 24, 32),
    dropdown_hover:       Rgba::rgb(32, 38, 50),
    dropdown_separator:   Rgba::rgb(28, 34, 44),
    menu_item_hover:      Rgba::rgb(32, 38, 50),
    scrollbar_track:      Rgba::new(19, 24, 32, 120),
    scrollbar_thumb:      Rgba::new(62, 72, 90, 190),
    scrollbar_thumb_hover: Rgba::new(90, 100, 122, 230),
    cursor:               Rgba::hex(0xeceff4),
    play_button:          Rgba::rgb(75, 165, 95),
    play_button_hover:    Rgba::rgb(95, 195, 115),
    stop_button:          Rgba::hex(0xbf616a),
    stop_button_hover:    Rgba::hex(0xd08770),
    tooltip_bg:           Rgba::rgb(28, 34, 44),
    tooltip_border:       Rgba::rgb(42, 50, 62),
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
    // UI chrome — dark teal, darkened
    bg_primary:           Rgba::rgb(0, 16, 22),
    bg_secondary:         Rgba::rgb(0, 22, 30),
    bg_elevated:          Rgba::rgb(3, 30, 38),
    bg_hover:             Rgba::rgb(6, 42, 52),
    border:               Rgba::rgb(5, 36, 46),
    border_focus:         Rgba::hex(0x268bd2),
    text_primary:         Rgba::hex(0xfdf6e3),
    text_secondary:       Rgba::hex(0xeee8d5),
    text_muted:           Rgba::hex(0x93a1a1),
    accent_primary:       Rgba::hex(0xcb4b16),
    accent_secondary:     Rgba::hex(0x859900),
    accent_info:          Rgba::hex(0x268bd2),
    tab_active:           Rgba::rgb(3, 30, 38),
    tab_inactive:         Rgba::rgb(0, 16, 22),
    tab_hover:            Rgba::rgb(1, 24, 32),
    editor_bg:            Rgba::rgb(0, 12, 17),
    editor_gutter:        Rgba::rgb(0, 18, 24),
    editor_selection:     Rgba::new(4, 38, 48, 130),
    graph_bg:             Rgba::rgb(0, 9, 13),
    graph_grid:           Rgba::new(4, 32, 42, 80),
    graph_axis:           Rgba::hex(0x93a1a1),
    axis_zone_bg:         Rgba::new(0, 16, 22, 210),
    axis_label:           Rgba::hex(0x586e75),
    toolbar_bg:           Rgba::rgb(0, 18, 24),
    toolbar_button:       Rgba::rgb(3, 30, 38),
    toolbar_button_hover: Rgba::rgb(6, 42, 52),
    toolbar_button_active: Rgba::rgb(75, 165, 95),
    split_handle:         Rgba::rgb(2, 26, 34),
    split_handle_hover:   Rgba::rgb(6, 42, 52),
    close_button_hover:   Rgba::hex(0xdc322f),
    dropdown_bg:          Rgba::rgb(0, 22, 30),
    dropdown_hover:       Rgba::rgb(4, 34, 44),
    dropdown_separator:   Rgba::rgb(3, 30, 38),
    menu_item_hover:      Rgba::rgb(4, 34, 44),
    scrollbar_track:      Rgba::new(0, 22, 30, 120),
    scrollbar_thumb:      Rgba::new(50, 72, 80, 190),
    scrollbar_thumb_hover: Rgba::new(78, 100, 108, 230),
    cursor:               Rgba::hex(0xfdf6e3),
    play_button:          Rgba::rgb(75, 165, 95),
    play_button_hover:    Rgba::rgb(95, 195, 115),
    stop_button:          Rgba::hex(0xdc322f),
    stop_button_hover:    Rgba::hex(0xef4f4c),
    tooltip_bg:           Rgba::rgb(3, 30, 38),
    tooltip_border:       Rgba::rgb(6, 42, 52),
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
    pub const AXIS_ZONE_SIZE: f32 = 40.0;
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
    pub fn gutter_width() -> f32 { GUTTER_WIDTH * super::fonts::scale() }
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
    pub const DEFAULT_LEFT_WIDTH: f32 = 540.0; // initial left pane width in pixels
    pub const MIN_PANE_SIZE: f32 = 200.0;
}
