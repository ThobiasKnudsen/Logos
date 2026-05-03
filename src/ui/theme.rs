// Centralized visual theme — colors, spacing, fonts, syntax themes.
//
// Two built-in themes (Dark/Light) using the Catppuccin palette.
// All UI colors are derived from a compact base set (~20 fields per theme).
#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};

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

    /// Format as `#rrggbb` (or `#rrggbbaa` when alpha < 255).
    pub fn to_hex_string(self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!(
                "#{:02x}{:02x}{:02x}{:02x}",
                self.r, self.g, self.b, self.a
            )
        }
    }

    /// Parse a `#rrggbb` or `#rrggbbaa` hex string. Leading `#` optional.
    pub fn from_hex_string(s: &str) -> Result<Self, String> {
        let h = s.strip_prefix('#').unwrap_or(s);
        let parse = |i: usize| -> Result<u8, String> {
            u8::from_str_radix(&h[i..i + 2], 16)
                .map_err(|_| format!("invalid hex color: {:?}", s))
        };
        match h.len() {
            6 => Ok(Self {
                r: parse(0)?,
                g: parse(2)?,
                b: parse(4)?,
                a: 255,
            }),
            8 => Ok(Self {
                r: parse(0)?,
                g: parse(2)?,
                b: parse(4)?,
                a: parse(6)?,
            }),
            _ => Err(format!("invalid hex color: {:?}", s)),
        }
    }
}

impl serde::Serialize for Rgba {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&self.to_hex_string())
    }
}

impl<'de> serde::Deserialize<'de> for Rgba {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = <String as serde::Deserialize>::deserialize(de)?;
        Rgba::from_hex_string(&s).map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// Color helpers
// ---------------------------------------------------------------------------

const fn lighten(c: Rgba, n: u8) -> Rgba {
    Rgba::rgb(
        c.r.saturating_add(n),
        c.g.saturating_add(n),
        c.b.saturating_add(n),
    )
}
const fn darken(c: Rgba, n: u8) -> Rgba {
    Rgba::rgb(
        c.r.saturating_sub(n),
        c.g.saturating_sub(n),
        c.b.saturating_sub(n),
    )
}
const fn mid(a: Rgba, b: Rgba) -> Rgba {
    Rgba::rgb(
        ((a.r as u16 + b.r as u16) / 2) as u8,
        ((a.g as u16 + b.g as u16) / 2) as u8,
        ((a.b as u16 + b.b as u16) / 2) as u8,
    )
}
const fn alpha(c: Rgba, a: u8) -> Rgba {
    Rgba::new(c.r, c.g, c.b, a)
}

// ---------------------------------------------------------------------------
// Full Theme — used internally by the renderer (55 fields, not serialized)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub bg_primary: Rgba,
    pub bg_secondary: Rgba,
    /// Canvas-style tone (notebook surface, active tab, render area).
    /// Same value as the JSON theme's `tertiary_bg`. Exposed so chrome
    /// accents (e.g. the logo plate) can match the canvas explicitly.
    pub bg_tertiary: Rgba,
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
// JsonTheme — compact base representation that derives all Theme colors
// ---------------------------------------------------------------------------

pub struct JsonTheme {
    // 3 background tones — chrome, elevated (cells/dropdowns/tooltips), and
    // canvas (notebook + render area, formerly `render_bg`).
    pub primary_bg: Rgba,
    pub secondary_bg: Rgba,
    pub tertiary_bg: Rgba,
    // Background used by hovered surfaces (button bg, tab close, plus
    // button, etc.) — explicit color so themes can pick it directly
    // instead of having it derived from `secondary_bg`.
    pub hover_bg: Rgba,
    // 2 text tones — strong (active text + accent + cursor) and weak
    // (muted text + axis labels + output text).
    pub primary_text: Rgba,
    pub secondary_text: Rgba,
    // 2 line tones — strong (the selected cell outline) and weak (all
    // other borders, separators, dividers, grid lines).
    pub primary_line: Rgba,
    pub secondary_line: Rgba,
    // Functional (2 colors)
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

/// In dark themes, "hover" lightens; in light themes it darkens.
const fn hover(c: Rgba, n: u8, is_dark: bool) -> Rgba {
    if is_dark {
        lighten(c, n)
    } else {
        darken(c, n)
    }
}
/// In dark themes, "muted" darkens; in light themes it lightens.
const fn muted(c: Rgba, n: u8, is_dark: bool) -> Rgba {
    if is_dark {
        darken(c, n)
    } else {
        lighten(c, n)
    }
}

impl JsonTheme {
    pub const fn to_theme(&self) -> Theme {
        // Backgrounds: 3 tones — chrome / elevated / canvas.
        // Foregrounds: 2 tones — strong (`pt`) for active/important
        // text+borders+accent+cursor, weak (`st`) for muted text +
        // subtle/inactive borders + grid lines.
        let pb = self.primary_bg;
        let sb = self.secondary_bg;
        let tb = self.tertiary_bg;
        let pt = self.primary_text;
        let st = self.secondary_text;
        let pl = self.primary_line;
        let sl = self.secondary_line;
        let rd = self.red;
        let gn = self.green;
        // Canvas (notebook + render area + active tab) — replaces the old
        // `render_bg` slot with `tertiary_bg`.
        let canvas = tb;

        // Auto-detect dark vs light from background brightness
        let is_dark = (pb.r as u16 + pb.g as u16 + pb.b as u16) < 384;

        Theme {
            bg_primary: pb,
            bg_secondary: sb,
            bg_tertiary: tb,
            bg_elevated: sb,
            bg_hover: self.hover_bg,
            border: sl,
            border_focus: pl,
            text_primary: pt,
            text_secondary: st,
            text_muted: st,
            accent_primary: pt,
            accent_secondary: gn,
            accent_info: pt,
            tab_active: canvas,
            tab_inactive: pb,
            tab_hover: mid(pb, sb),
            editor_bg: canvas,
            editor_gutter: sb,
            editor_selection: alpha(pt, 60),
            graph_bg: canvas,
            graph_grid: alpha(sl, 130),
            graph_axis: st,
            axis_zone_bg: alpha(pb, 210),
            axis_label: st,
            toolbar_bg: sb,
            toolbar_button: sb,
            toolbar_button_hover: hover(sb, 12, is_dark),
            toolbar_button_active: gn,
            split_handle: sl,
            split_handle_hover: sl,
            close_button_hover: rd,
            dropdown_bg: sb,
            dropdown_hover: hover(sb, 12, is_dark),
            dropdown_separator: sl,
            menu_item_hover: sb,
            scrollbar_track: alpha(sb, 120),
            scrollbar_thumb: alpha(st, 190),
            scrollbar_thumb_hover: alpha(hover(st, 20, is_dark), 230),
            cursor: pt,
            play_button: gn,
            play_button_hover: hover(gn, 25, is_dark),
            stop_button: rd,
            stop_button_hover: hover(rd, 25, is_dark),
            tooltip_bg: sb,
            tooltip_border: sl,
            keyword: self.keyword,
            identifier: self.identifier,
            math_variable: self.math_variable,
            number: self.number,
            operator: self.operator,
            string: self.string,
            comment: self.comment,
            punctuation: st,
            whitespace: st,
            builtin: self.builtin,
            axis: self.axis,
            type_name: self.type_name,
            unknown: rd,
        }
    }
}

// ---------------------------------------------------------------------------
// Built-in themes (original UI + Catppuccin syntax)
// ---------------------------------------------------------------------------

const THEMES: [Theme; 2] = [
    // Dark — original UI, Catppuccin Mocha syntax
    JsonTheme {
        primary_bg: Rgba::hex(0x303030),
        secondary_bg: Rgba::hex(0x202020),
        tertiary_bg: Rgba::hex(0x181818),
        hover_bg: Rgba::hex(0x4040404),
        primary_text: Rgba::hex(0xffffff),
        secondary_text: Rgba::hex(0xd0d0d0),
        primary_line: Rgba::hex(0xffffff),
        secondary_line: Rgba::hex(0x404040),
        red: Rgba::hex(0xc43c32),
        green: Rgba::hex(0x4ba55f),
        keyword: Rgba::hex(0xcba6f7),
        identifier: Rgba::hex(0xcdd6f4),
        math_variable: Rgba::hex(0xf9e2af),
        number: Rgba::hex(0xfab387),
        operator: Rgba::hex(0x89dceb),
        string: Rgba::hex(0xa6e3a1),
        comment: Rgba::hex(0x6c7086),
        builtin: Rgba::hex(0x89b4fa),
        type_name: Rgba::hex(0xf9e2af),
        axis: Rgba::hex(0xfab387),
    }
    .to_theme(),
    // Light — original UI, Catppuccin Latte syntax
    JsonTheme {
        primary_bg: Rgba::hex(0xffffff),
        secondary_bg: Rgba::hex(0xf3f4f6),
        tertiary_bg: Rgba::hex(0xe5e7eb),
        hover_bg: Rgba::hex(0xcbccce),
        primary_text: Rgba::hex(0x1f2937),
        secondary_text: Rgba::hex(0x6b7280),
        primary_line: Rgba::hex(0x1f2937),
        secondary_line: Rgba::hex(0xd1d5db),
        red: Rgba::hex(0xdc2626),
        green: Rgba::hex(0x16a34a),
        keyword: Rgba::hex(0x8839ef),
        identifier: Rgba::hex(0xdd7878),
        math_variable: Rgba::hex(0xdf8e1d),
        number: Rgba::hex(0xfe640b),
        operator: Rgba::hex(0x04a5e5),
        string: Rgba::hex(0x40a02b),
        comment: Rgba::hex(0x9ca0b0),
        builtin: Rgba::hex(0x1e66f5),
        type_name: Rgba::hex(0xdf8e1d),
        axis: Rgba::hex(0xfe640b),
    }
    .to_theme(),
];

const THEME_NAMES: [&str; 2] = ["Dark", "Light"];

// ---------------------------------------------------------------------------
// Active theme selection + public API
// ---------------------------------------------------------------------------

static CURRENT_THEME: AtomicUsize = AtomicUsize::new(0);

mod theme_state {
    use super::*;
    pub fn index() -> usize {
        CURRENT_THEME.load(Ordering::Relaxed)
    }
    pub fn set(i: usize) {
        CURRENT_THEME.store(i, Ordering::Relaxed);
    }
}

pub fn theme() -> &'static Theme {
    &THEMES[theme_state::index()]
}

pub fn active_theme_name() -> String {
    THEME_NAMES[theme_state::index()].to_string()
}

pub fn theme_names() -> Vec<String> {
    THEME_NAMES.iter().map(|s| s.to_string()).collect()
}

pub fn cycle_theme() -> String {
    let next = (theme_state::index() + 1) % THEMES.len();
    theme_state::set(next);
    THEME_NAMES[next].to_string()
}

pub fn set_theme(idx: usize) {
    if idx < THEMES.len() {
        theme_state::set(idx);
    }
}

pub fn theme_count() -> usize {
    THEMES.len()
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
    /// Standard thickness for all UI lines (borders, separators, split handle visual).
    pub const LINE_THICKNESS: f32 = 1.0;
    pub const MENU_HEIGHT: f32 = 28.0;
    pub const TAB_HEIGHT: f32 = 36.0;
    pub const STATUS_HEIGHT: f32 = 24.0;
    /// Hit/drag area for the split-handle. Visual line thickness is `LINE_THICKNESS`.
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

    pub fn scale() -> f32 {
        super::fonts::scale()
    }
    pub fn xs() -> f32 {
        XS * scale()
    }
    pub fn sm() -> f32 {
        SM * scale()
    }
    pub fn md() -> f32 {
        MD * scale()
    }
    pub fn lg() -> f32 {
        LG * scale()
    }
    pub fn xl() -> f32 {
        XL * scale()
    }
    pub fn line_thickness() -> f32 {
        LINE_THICKNESS * scale()
    }
    pub fn menu_height() -> f32 {
        MENU_HEIGHT * scale()
    }
    pub fn tab_height() -> f32 {
        TAB_HEIGHT * scale()
    }
    pub fn status_height() -> f32 {
        STATUS_HEIGHT * scale()
    }
    pub fn split_handle_width() -> f32 {
        SPLIT_HANDLE_WIDTH * scale()
    }
    pub fn window_control_width() -> f32 {
        WINDOW_CONTROL_WIDTH * scale()
    }
    pub fn dropdown_item_height() -> f32 {
        DROPDOWN_ITEM_HEIGHT * scale()
    }
    pub fn dropdown_padding() -> f32 {
        DROPDOWN_PADDING * scale()
    }
    pub fn dropdown_min_width() -> f32 {
        DROPDOWN_MIN_WIDTH * scale()
    }
    pub fn scrollbar_height() -> f32 {
        SCROLLBAR_HEIGHT * scale()
    }
    pub fn scrollbar_width() -> f32 {
        SCROLLBAR_WIDTH * scale()
    }
    pub fn scrollbar_thumb_min_w() -> f32 {
        SCROLLBAR_THUMB_MIN_W * scale()
    }
    pub fn scrollbar_thumb_min_h() -> f32 {
        SCROLLBAR_THUMB_MIN_H * scale()
    }
    pub fn gutter_width() -> f32 {
        GUTTER_WIDTH * scale()
    }
    pub fn axis_zone_size() -> f32 {
        AXIS_ZONE_SIZE * scale()
    }
    pub fn cell_padding() -> f32 {
        CELL_PADDING * scale()
    }
    pub fn cell_spacing() -> f32 {
        CELL_SPACING * scale()
    }
    pub fn button_size() -> f32 {
        BUTTON_SIZE * scale()
    }
    pub fn header_height() -> f32 {
        HEADER_HEIGHT * scale()
    }
    pub fn text_padding() -> f32 {
        TEXT_PADDING * scale()
    }
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
    pub fn scale() -> f32 {
        f32::from_bits(CURRENT_SCALE.load(Ordering::Relaxed))
    }
    pub fn zoom_in() {
        let n = (scale() + SCALE_STEP).min(MAX_SCALE);
        CURRENT_SCALE.store(n.to_bits(), Ordering::Relaxed);
    }
    pub fn zoom_out() {
        let n = (scale() - SCALE_STEP).max(MIN_SCALE);
        CURRENT_SCALE.store(n.to_bits(), Ordering::Relaxed);
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
    /// Glyph size for the chrome "Λ" logo. Slightly larger than the
    /// menu text so the logo reads as a logo rather than another menu
    /// word.
    pub fn logo_size() -> f32 {
        BASE_MENU * scale() * 1.5
    }
}

// ---------------------------------------------------------------------------
// Monospace font families — registry of embedded TTFs the editor can switch
// between at runtime via the Fonts menu. To add a new font, drop the .ttf into
// `assets/fonts/`, append a `FontFamily` entry below, and rebuild.
// ---------------------------------------------------------------------------

pub mod font_family {
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// One selectable monospace family. Multiple weights of the same family
    /// (e.g. Regular + Bold) live in `font_data` and are loaded together.
    pub struct FontFamily {
        /// Display name in the Fonts menu.
        pub name: &'static str,
        /// Family name embedded in the TTFs — must match the OS/2 `name` table
        /// so cosmic-text can resolve `Family::Name(family)`.
        pub family: &'static str,
        /// Raw bytes of every weight to load into the FontSystem at startup.
        pub font_data: &'static [&'static [u8]],
    }

    /// "Λ" logo font: GFS Didot Regular. Greek-first Didot revival, what
    /// the user picked when they first saw the lambda render. Loaded as a
    /// chrome-only family — never selected through the Fonts menu.
    pub const LOGO_FONT_FAMILY: &str = "GFS Didot";
    pub const LOGO_FONT_DATA: &[u8] = include_bytes!("../../assets/fonts/GFSDidot-Regular.ttf");

    /// Italic font for chrome accents (untitled tab labels, etc.). Bundled
    /// because the user-selectable code fonts (Geist Mono, JuliaMono via
    /// our shipped weights) do not include an italic file, and `fontdb`
    /// does not synthesize italics — without a real italic face glyphon
    /// silently falls back to upright.
    pub const ITALIC_CHROME_FONT_FAMILY: &str = "Source Code Pro";
    pub const ITALIC_CHROME_FONT_DATA: &[u8] =
        include_bytes!("../../assets/fonts/SourceCodePro-Italic.ttf");

    pub const FAMILIES: &[FontFamily] = &[
        FontFamily {
            name: "Geist Mono",
            family: "Geist Mono",
            font_data: &[
                include_bytes!("../../assets/fonts/GeistMono-Regular.ttf"),
                include_bytes!("../../assets/fonts/GeistMono-Bold.ttf"),
            ],
        },
        FontFamily {
            name: "JuliaMono",
            family: "JuliaMono",
            font_data: &[
                include_bytes!("../../assets/fonts/JuliaMono-Regular.ttf"),
                include_bytes!("../../assets/fonts/JuliaMono-Bold.ttf"),
            ],
        },
    ];

    static CURRENT: AtomicUsize = AtomicUsize::new(0);

    pub fn count() -> usize {
        FAMILIES.len()
    }

    pub fn active_index() -> usize {
        CURRENT.load(Ordering::Relaxed).min(FAMILIES.len().saturating_sub(1))
    }

    pub fn active_family() -> &'static str {
        FAMILIES[active_index()].family
    }

    pub fn active_name() -> &'static str {
        FAMILIES[active_index()].name
    }

    pub fn name(idx: usize) -> &'static str {
        FAMILIES.get(idx).map(|f| f.name).unwrap_or("")
    }

    pub fn set(idx: usize) {
        if idx < FAMILIES.len() {
            CURRENT.store(idx, Ordering::Relaxed);
        }
    }
}

// ---------------------------------------------------------------------------
// Split defaults
// ---------------------------------------------------------------------------

pub mod split {
    pub const DEFAULT_LEFT_WIDTH: f32 = 540.0;
    pub const MIN_PANE_SIZE: f32 = 200.0;
}
