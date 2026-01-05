//! Visual theme constants - colors, fonts, spacing
//!
//! Inspired by modern code editors with a dark, focused aesthetic.

const dvui = @import("dvui");

/// Color palette - deep blue-gray base with warm accents
pub const colors = struct {
    // Backgrounds
    pub const bg_primary = dvui.Color{ .r = 22, .g = 27, .b = 34, .a = 255 }; // #161b22
    pub const bg_secondary = dvui.Color{ .r = 30, .g = 37, .b = 46, .a = 255 }; // #1e252e
    pub const bg_elevated = dvui.Color{ .r = 40, .g = 49, .b = 61, .a = 255 }; // #28313d
    pub const bg_hover = dvui.Color{ .r = 52, .g = 63, .b = 78, .a = 255 }; // #343f4e

    // Borders & dividers
    pub const border = dvui.Color{ .r = 55, .g = 65, .b = 81, .a = 255 }; // #374151
    pub const border_focus = dvui.Color{ .r = 99, .g = 179, .b = 237, .a = 255 }; // #63b3ed

    // Text
    pub const text_primary = dvui.Color{ .r = 237, .g = 242, .b = 247, .a = 255 }; // #edf2f7
    pub const text_secondary = dvui.Color{ .r = 160, .g = 174, .b = 192, .a = 255 }; // #a0aec0
    pub const text_muted = dvui.Color{ .r = 113, .g = 128, .b = 150, .a = 255 }; // #718096

    // Accents
    pub const accent_primary = dvui.Color{ .r = 236, .g = 167, .b = 107, .a = 255 }; // #eca76b warm orange
    pub const accent_secondary = dvui.Color{ .r = 129, .g = 199, .b = 132, .a = 255 }; // #81c784 soft green
    pub const accent_info = dvui.Color{ .r = 100, .g = 181, .b = 246, .a = 255 }; // #64b5f6 blue

    // Tab states
    pub const tab_active = dvui.Color{ .r = 40, .g = 49, .b = 61, .a = 255 };
    pub const tab_inactive = dvui.Color{ .r = 22, .g = 27, .b = 34, .a = 255 };
    pub const tab_hover = dvui.Color{ .r = 35, .g = 43, .b = 54, .a = 255 };

    // Editor
    pub const editor_bg = dvui.Color{ .r = 18, .g = 22, .b = 28, .a = 255 }; // #12161c
    pub const editor_gutter = dvui.Color{ .r = 30, .g = 37, .b = 46, .a = 255 };
    pub const editor_selection = dvui.Color{ .r = 68, .g = 85, .b = 108, .a = 128 };

    // Graph area
    pub const graph_bg = dvui.Color{ .r = 15, .g = 18, .b = 23, .a = 255 }; // #0f1217
    pub const graph_grid = dvui.Color{ .r = 40, .g = 49, .b = 61, .a = 80 };
    pub const graph_axis = dvui.Color{ .r = 113, .g = 128, .b = 150, .a = 255 };
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
