//! Visual theme constants - colors, fonts, spacing
//!
//! Inspired by modern code editors with a dark, focused aesthetic.

const dvui = @import("dvui");
const parse_state = @import("../session/parse_state.zig");

/// Color palette - deep blue-gray base with warm accents
pub const colors = struct {
    // Backgrounds
    pub const bg_primary = dvui.Color{ .r = 16, .g = 16, .b = 16, .a = 255 };
    pub const bg_secondary = dvui.Color{ .r = 24, .g = 24, .b = 24, .a = 255 };
    pub const bg_elevated = dvui.Color{ .r = 40, .g = 40, .b = 40, .a = 255 };
    pub const bg_hover = dvui.Color{ .r = 96, .g = 96, .b = 96, .a = 255 };

    // Borders & dividers
    pub const border = dvui.Color{ .r = 86, .g = 86, .b = 86, .a = 255 };
    pub const border_focus = dvui.Color{ .r = 180, .g = 180, .b = 180, .a = 255 };

    // Text
    pub const text_primary = dvui.Color{ .r = 242, .g = 242, .b = 247, .a = 255 };
    pub const text_secondary = dvui.Color{ .r = 192, .g = 192, .b = 192, .a = 255 };
    pub const text_muted = dvui.Color{ .r = 128, .g = 128, .b = 128, .a = 255 };

    // Accents
    pub const accent_primary = dvui.Color{ .r = 236, .g = 167, .b = 107, .a = 255 }; // #eca76b warm orange
    pub const accent_secondary = dvui.Color{ .r = 129, .g = 199, .b = 132, .a = 255 }; // #81c784 soft green
    pub const accent_info = dvui.Color{ .r = 100, .g = 181, .b = 246, .a = 255 }; // #64b5f6 blue

    // Tab states
    pub const tab_active = dvui.Color{ .r = 40, .g = 49, .b = 61, .a = 255 };
    pub const tab_inactive = dvui.Color{ .r = 22, .g = 27, .b = 34, .a = 255 };
    pub const tab_hover = dvui.Color{ .r = 35, .g = 43, .b = 54, .a = 255 };

    // Editor
    pub const editor_bg = dvui.Color{ .r = 18, .g = 22, .b = 28, .a = 255 };
    pub const editor_gutter = dvui.Color{ .r = 30, .g = 37, .b = 46, .a = 255 };
    pub const editor_selection = dvui.Color{ .r = 68, .g = 85, .b = 108, .a = 128 };

    // Graph area
    pub const graph_bg = dvui.Color{ .r = 15, .g = 18, .b = 23, .a = 255 }; // #0f1217
    pub const graph_grid = dvui.Color{ .r = 40, .g = 49, .b = 61, .a = 80 };
    pub const graph_axis = dvui.Color{ .r = 113, .g = 128, .b = 150, .a = 255 };

    // Toolbar
    pub const toolbar_bg = dvui.Color{ .r = 28, .g = 34, .b = 42, .a = 255 }; // #1c222a
    pub const toolbar_button = dvui.Color{ .r = 45, .g = 55, .b = 68, .a = 255 };
    pub const toolbar_button_hover = dvui.Color{ .r = 60, .g = 72, .b = 88, .a = 255 };
    pub const toolbar_button_active = dvui.Color{ .r = 75, .g = 165, .b = 95, .a = 255 }; // green for play
};

/// Syntax highlighting colors - inspired by One Dark Pro
pub const syntax = struct {
    pub const keyword = dvui.Color{ .r = 198, .g = 120, .b = 221, .a = 255 }; // #c678dd purple
    pub const identifier = dvui.Color{ .r = 224, .g = 108, .b = 117, .a = 255 }; // #e06c75 red
    pub const number = dvui.Color{ .r = 209, .g = 154, .b = 102, .a = 255 }; // #d19a66 orange
    pub const operator = dvui.Color{ .r = 86, .g = 182, .b = 194, .a = 255 }; // #56b6c2 cyan
    pub const string = dvui.Color{ .r = 152, .g = 195, .b = 121, .a = 255 }; // #98c379 green
    pub const comment = dvui.Color{ .r = 92, .g = 99, .b = 112, .a = 255 }; // #5c6370 gray
    pub const punctuation = dvui.Color{ .r = 171, .g = 178, .b = 191, .a = 255 }; // #abb2bf light gray
    pub const whitespace = dvui.Color{ .r = 171, .g = 178, .b = 191, .a = 255 }; // same as punctuation
    pub const builtin = dvui.Color{ .r = 97, .g = 175, .b = 239, .a = 255 }; // #61afef blue
    pub const axis = dvui.Color{ .r = 229, .g = 192, .b = 123, .a = 255 }; // #e5c07b yellow
    pub const type_name = dvui.Color{ .r = 86, .g = 182, .b = 194, .a = 255 }; // #56b6c2 cyan
    pub const unknown = dvui.Color{ .r = 224, .g = 108, .b = 117, .a = 255 }; // #e06c75 red (error-ish)

    /// Get color for a token type
    pub fn colorForTokenType(token_type: parse_state.TokenType) dvui.Color {
        return switch (token_type) {
            .keyword => keyword,
            .identifier => identifier,
            .number => number,
            .operator => operator,
            .string => string,
            .comment => comment,
            .punctuation => punctuation,
            .whitespace => whitespace,
            .builtin => builtin,
            .axis => axis,
            .type_name => type_name,
            .unknown => unknown,
        };
    }
};

/// Spacing and sizing constants
pub const spacing = struct {
    pub const xs: f32 = 4;
    pub const sm: f32 = 8;
    pub const md: f32 = 12;
    pub const lg: f32 = 16;
    pub const xl: f32 = 24;

    pub const menu_height: f32 = 28;
    pub const tab_height: f32 = 36;
    pub const split_handle_width: f32 = 6;
};

/// Border radius
pub const radius = struct {
    pub const none: f32 = 0;
    pub const sm: f32 = 2;
    pub const md: f32 = 4;
    pub const lg: f32 = 6;
    pub const xl: f32 = 8;
};

/// Font size configuration with global zoom control
pub const fonts = struct {
    // Base font sizes (before zoom)
    pub const base_editor: f32 = 20.0;
    pub const base_ui: f32 = 14.0;
    pub const base_small: f32 = 12.0;
    pub const base_tooltip: f32 = 13.0;
    pub const base_status: f32 = 13.0;
    pub const base_menu: f32 = 14.0;

    // Zoom constraints
    pub const min_scale: f32 = 0.6;
    pub const max_scale: f32 = 2.0;
    pub const scale_step: f32 = 0.1;

    // Current zoom scale (1.0 = 100%)
    var current_scale: f32 = 1.0;

    /// Get current scale factor
    pub fn getScale() f32 {
        return current_scale;
    }

    /// Increase zoom
    pub fn zoomIn() void {
        current_scale = @min(current_scale + scale_step, max_scale);
    }

    /// Decrease zoom
    pub fn zoomOut() void {
        current_scale = @max(current_scale - scale_step, min_scale);
    }

    /// Reset zoom to 100%
    pub fn resetZoom() void {
        current_scale = 1.0;
    }

    /// Get scaled editor font size
    pub fn editorSize() f32 {
        return base_editor * current_scale;
    }

    /// Get scaled UI font size
    pub fn uiSize() f32 {
        return base_ui * current_scale;
    }

    /// Get scaled small font size
    pub fn smallSize() f32 {
        return base_small * current_scale;
    }

    /// Get scaled tooltip font size
    pub fn tooltipSize() f32 {
        return base_tooltip * current_scale;
    }

    /// Get scaled status bar font size
    pub fn statusSize() f32 {
        return base_status * current_scale;
    }

    /// Get scaled menu font size
    pub fn menuSize() f32 {
        return base_menu * current_scale;
    }

    /// Get a scaled mono font for the editor
    pub fn editorFont() dvui.Font {
        return dvui.Font.theme(.mono).withSize(editorSize());
    }

    /// Get a scaled body font for UI
    pub fn uiFont() dvui.Font {
        return dvui.Font.theme(.body).withSize(uiSize());
    }

    /// Get a scaled small font
    pub fn smallFont() dvui.Font {
        return dvui.Font.theme(.body).withSize(smallSize());
    }
};
