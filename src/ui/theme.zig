//! Visual theme constants - colors, fonts, spacing
//!
//! Syntax highlighting themes are loaded from .logos.theme.json in the
//! working directory. The file is auto-created with defaults if missing.

const std = @import("std");
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

// ============================================================================
// Syntax Highlighting — loaded from .logos.theme.json
// ============================================================================

pub const SyntaxTheme = struct {
    keyword: dvui.Color,
    identifier: dvui.Color,
    math_variable: dvui.Color,
    number: dvui.Color,
    operator: dvui.Color,
    string: dvui.Color,
    comment: dvui.Color,
    punctuation: dvui.Color,
    whitespace: dvui.Color,
    builtin: dvui.Color,
    axis: dvui.Color,
    type_name: dvui.Color,
    unknown: dvui.Color,
};

fn hex(comptime rgb: u24) dvui.Color {
    return .{
        .r = @truncate(rgb >> 16),
        .g = @truncate(rgb >> 8),
        .b = @truncate(rgb),
        .a = 255,
    };
}

// Field names in the order they appear in JSON / SyntaxTheme struct
const field_names = [_][]const u8{
    "keyword",
    "identifier",
    "math_variable",
    "number",
    "operator",
    "string",
    "comment",
    "punctuation",
    "whitespace",
    "builtin",
    "axis",
    "type_name",
    "unknown",
};

fn getField(t: *const SyntaxTheme, comptime name: []const u8) dvui.Color {
    return @field(t, name);
}

fn setField(t: *SyntaxTheme, comptime name: []const u8, c: dvui.Color) void {
    @field(t, name) = c;
}

// ============================================================================
// Built-in default themes (used to generate .logos.theme.json on first run)
// ============================================================================

const theme_names = [_][]const u8{
    "one_dark",
    "monokai",
    "dracula",
    "catppuccin",
    "gruvbox",
    "nord",
    "solarized",
};

const default_themes = [_]SyntaxTheme{
    // One Dark Pro
    .{
        .keyword = hex(0xc678dd),
        .identifier = hex(0xe06c75),
        .math_variable = hex(0xe5c07b),
        .number = hex(0xd19a66),
        .operator = hex(0x56b6c2),
        .string = hex(0x98c379),
        .comment = hex(0x5c6370),
        .punctuation = hex(0xabb2bf),
        .whitespace = hex(0xabb2bf),
        .builtin = hex(0x61afef),
        .axis = hex(0xd19a66),
        .type_name = hex(0xe5c07b),
        .unknown = hex(0xe06c75),
    },
    // Monokai Pro
    .{
        .keyword = hex(0xff6188),
        .identifier = hex(0xa9dc76),
        .math_variable = hex(0xffd866),
        .number = hex(0xab9df2),
        .operator = hex(0xff6188),
        .string = hex(0xffd866),
        .comment = hex(0x727072),
        .punctuation = hex(0x939293),
        .whitespace = hex(0x939293),
        .builtin = hex(0x78dce8),
        .axis = hex(0xfc9867),
        .type_name = hex(0x78dce8),
        .unknown = hex(0xfc9867),
    },
    // Dracula
    .{
        .keyword = hex(0xff79c6),
        .identifier = hex(0x50fa7b),
        .math_variable = hex(0xf8f8f2),
        .number = hex(0xbd93f9),
        .operator = hex(0xff79c6),
        .string = hex(0xf1fa8c),
        .comment = hex(0x6272a4),
        .punctuation = hex(0xf8f8f2),
        .whitespace = hex(0xf8f8f2),
        .builtin = hex(0x8be9fd),
        .axis = hex(0xffb86c),
        .type_name = hex(0x8be9fd),
        .unknown = hex(0xff5555),
    },
    // Catppuccin Mocha
    .{
        .keyword = hex(0xcba6f7),
        .identifier = hex(0xa6e3a1),
        .math_variable = hex(0xf38ba8),
        .number = hex(0xfab387),
        .operator = hex(0x89dceb),
        .string = hex(0xa6e3a1),
        .comment = hex(0x6c7086),
        .punctuation = hex(0xbac2de),
        .whitespace = hex(0xbac2de),
        .builtin = hex(0x89b4fa),
        .axis = hex(0xf9e2af),
        .type_name = hex(0x94e2d5),
        .unknown = hex(0xf38ba8),
    },
    // Gruvbox Dark
    .{
        .keyword = hex(0xfb4934),
        .identifier = hex(0x83a598),
        .math_variable = hex(0xfabd2f),
        .number = hex(0xd3869b),
        .operator = hex(0xfe8019),
        .string = hex(0xb8bb26),
        .comment = hex(0x928374),
        .punctuation = hex(0xa89984),
        .whitespace = hex(0xa89984),
        .builtin = hex(0x8ec07c),
        .axis = hex(0xfabd2f),
        .type_name = hex(0x83a598),
        .unknown = hex(0xfb4934),
    },
    // Nord
    .{
        .keyword = hex(0x81a1c1),
        .identifier = hex(0x88c0d0),
        .math_variable = hex(0xd8dee9),
        .number = hex(0xb48ead),
        .operator = hex(0x81a1c1),
        .string = hex(0xa3be8c),
        .comment = hex(0x616e88),
        .punctuation = hex(0xd8dee9),
        .whitespace = hex(0xd8dee9),
        .builtin = hex(0x88c0d0),
        .axis = hex(0xebcb8b),
        .type_name = hex(0x8fbcbb),
        .unknown = hex(0xbf616a),
    },
    // Solarized Dark
    .{
        .keyword = hex(0x859900),
        .identifier = hex(0x268bd2),
        .math_variable = hex(0xcb4b16),
        .number = hex(0xd33682),
        .operator = hex(0x93a1a1),
        .string = hex(0x2aa198),
        .comment = hex(0x586e75),
        .punctuation = hex(0x839496),
        .whitespace = hex(0x839496),
        .builtin = hex(0x268bd2),
        .axis = hex(0xb58900),
        .type_name = hex(0xcb4b16),
        .unknown = hex(0xdc322f),
    },
};

const default_selected = "catppuccin";
const theme_file = ".logos.theme.json";

// ============================================================================
// Runtime state
// ============================================================================

pub const syntax = struct {
    var active: SyntaxTheme = default_themes[3]; // catppuccin
    var current_name: [64]u8 = nameBuffer(default_selected);
    var current_name_len: usize = default_selected.len;

    // Loaded theme data (names + colors) from JSON
    var loaded_themes: [max_themes]SyntaxTheme = undefined;
    var loaded_names: [max_themes][64]u8 = undefined;
    var loaded_name_lens: [max_themes]usize = undefined;
    var loaded_count: usize = 0;

    const max_themes = 16;

    fn nameBuffer(comptime name: []const u8) [64]u8 {
        var buf: [64]u8 = .{0} ** 64;
        @memcpy(buf[0..name.len], name);
        return buf;
    }

    pub fn init() void {
        loadFromFile() catch {
            // File doesn't exist or is invalid — use defaults and create it
            loadDefaults();
            saveToFile() catch |err| {
                std.log.warn("Could not create {s}: {}", .{ theme_file, err });
            };
        };
    }

    fn loadDefaults() void {
        loaded_count = theme_names.len;
        for (theme_names, 0..) |name, i| {
            loaded_themes[i] = default_themes[i];
            loaded_name_lens[i] = name.len;
            loaded_names[i] = .{0} ** 64;
            @memcpy(loaded_names[i][0..name.len], name);
        }
        // Set default selection
        current_name = nameBuffer(default_selected);
        current_name_len = default_selected.len;
        active = default_themes[3]; // catppuccin
    }

    fn loadFromFile() !void {
        const file = try std.fs.cwd().openFile(theme_file, .{});
        defer file.close();

        const content = try file.readToEndAlloc(std.heap.page_allocator, 1024 * 64);
        defer std.heap.page_allocator.free(content);

        const parsed = try std.json.parseFromSlice(std.json.Value, std.heap.page_allocator, content, .{});
        defer parsed.deinit();

        const root = parsed.value;
        if (root != .object) return error.InvalidFormat;

        // Read "selected"
        if (root.object.get("selected")) |sel| {
            if (sel == .string) {
                const name = sel.string;
                if (name.len <= 64) {
                    current_name_len = name.len;
                    current_name = .{0} ** 64;
                    @memcpy(current_name[0..name.len], name);
                }
            }
        }

        // Read "themes"
        const themes_val = root.object.get("themes") orelse return error.InvalidFormat;
        if (themes_val != .object) return error.InvalidFormat;

        loaded_count = 0;
        var it = themes_val.object.iterator();
        while (it.next()) |entry| {
            if (loaded_count >= max_themes) break;
            const name = entry.key_ptr.*;
            const theme_obj = entry.value_ptr.*;
            if (theme_obj != .object) continue;
            if (name.len > 64) continue;

            loaded_name_lens[loaded_count] = name.len;
            loaded_names[loaded_count] = .{0} ** 64;
            @memcpy(loaded_names[loaded_count][0..name.len], name);

            // Start from catppuccin defaults for any missing fields
            var t = default_themes[3];
            inline for (field_names) |fname| {
                if (theme_obj.object.get(fname)) |color_val| {
                    if (color_val == .string) {
                        if (parseHexColor(color_val.string)) |c| {
                            setField(&t, fname, c);
                        }
                    }
                }
            }
            loaded_themes[loaded_count] = t;
            loaded_count += 1;
        }

        // Apply the selected theme
        applyByName(current_name[0..current_name_len]);
    }

    fn applyByName(name: []const u8) void {
        for (0..loaded_count) |i| {
            if (std.mem.eql(u8, loaded_names[i][0..loaded_name_lens[i]], name)) {
                active = loaded_themes[i];
                return;
            }
        }
        // Not found — use first loaded theme
        if (loaded_count > 0) {
            active = loaded_themes[0];
            current_name_len = loaded_name_lens[0];
            current_name = loaded_names[0];
        }
    }

    pub fn setThemeByName(name: []const u8) void {
        if (name.len > 64) return;
        current_name = .{0} ** 64;
        @memcpy(current_name[0..name.len], name);
        current_name_len = name.len;
        applyByName(name);
        saveToFile() catch |err| {
            std.log.warn("Could not save theme selection: {}", .{err});
        };
    }

    pub fn currentThemeName() []const u8 {
        return current_name[0..current_name_len];
    }

    pub fn themeCount() usize {
        return loaded_count;
    }

    pub fn themeName(idx: usize) []const u8 {
        return loaded_names[idx][0..loaded_name_lens[idx]];
    }

    /// Get the color for a token. Single-letter identifiers (math variables
    /// like x, y, n) get the math_variable color; everything else uses
    /// the standard color for its token type.
    pub fn colorForToken(token_type: parse_state.TokenType, text: []const u8) dvui.Color {
        if (token_type == .identifier and text.len == 1) {
            return active.math_variable;
        }
        return switch (token_type) {
            .keyword => active.keyword,
            .identifier => active.identifier,
            .number => active.number,
            .operator => active.operator,
            .string => active.string,
            .comment => active.comment,
            .punctuation => active.punctuation,
            .whitespace => active.whitespace,
            .builtin => active.builtin,
            .axis => active.axis,
            .type_name => active.type_name,
            .unknown => active.unknown,
        };
    }

    // ---- JSON I/O helpers ----

    fn parseHexColor(s: []const u8) ?dvui.Color {
        // Accept "#RRGGBB" format
        if (s.len != 7 or s[0] != '#') return null;
        const r = std.fmt.parseUnsigned(u8, s[1..3], 16) catch return null;
        const g = std.fmt.parseUnsigned(u8, s[3..5], 16) catch return null;
        const b = std.fmt.parseUnsigned(u8, s[5..7], 16) catch return null;
        return .{ .r = r, .g = g, .b = b, .a = 255 };
    }

    fn colorToHex(buf: *[7]u8, c: dvui.Color) void {
        const hex_chars = "0123456789abcdef";
        buf[0] = '#';
        buf[1] = hex_chars[c.r >> 4];
        buf[2] = hex_chars[c.r & 0xf];
        buf[3] = hex_chars[c.g >> 4];
        buf[4] = hex_chars[c.g & 0xf];
        buf[5] = hex_chars[c.b >> 4];
        buf[6] = hex_chars[c.b & 0xf];
    }

    fn saveToFile() !void {
        var file = try std.fs.cwd().createFile(theme_file, .{});
        defer file.close();

        try file.writeAll("{\n");

        // "selected"
        try file.writeAll("  \"selected\": \"");
        try file.writeAll(current_name[0..current_name_len]);
        try file.writeAll("\",\n");

        // "themes"
        try file.writeAll("  \"themes\": {\n");
        for (0..loaded_count) |i| {
            try file.writeAll("    \"");
            try file.writeAll(loaded_names[i][0..loaded_name_lens[i]]);
            try file.writeAll("\": {\n");

            const t = &loaded_themes[i];
            inline for (field_names, 0..) |fname, fi| {
                var hbuf: [7]u8 = undefined;
                colorToHex(&hbuf, getField(t, fname));
                try file.writeAll("      \"");
                try file.writeAll(fname);
                try file.writeAll("\": \"");
                try file.writeAll(&hbuf);
                if (fi < field_names.len - 1) {
                    try file.writeAll("\",\n");
                } else {
                    try file.writeAll("\"\n");
                }
            }

            if (i < loaded_count - 1) {
                try file.writeAll("    },\n");
            } else {
                try file.writeAll("    }\n");
            }
        }
        try file.writeAll("  }\n");
        try file.writeAll("}\n");
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
