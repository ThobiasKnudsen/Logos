//! Text editor panel - left side of the split view
//!
//! Multi-line text area where users write their expressions/code
//! that gets parsed and visualized in the graph panel.

const std = @import("std");
const dvui = @import("dvui");
const theme = @import("../theme.zig");
const session = @import("../../session/session.zig");

pub const EditorPanel = struct {
    // Shared styling constants - use theme where possible
    const gutter_width: f32 = 48;
    const content_padding_x: f32 = 8;
    const content_padding_y: f32 = 4;

    // Custom theme for text entry with transparent focus (to hide focus border)
    const no_focus_theme = blk: {
        var t = dvui.Theme.builtin.adwaita_dark;
        t.focus = dvui.Color{ .r = 0, .g = 0, .b = 0, .a = 0 }; // Transparent focus
        break :blk t;
    };

    pub fn render(active_session: *session.TabSession) void {
        // Outer scroll area - handles scrolling for both line numbers and text
        var scroll = dvui.scrollArea(@src(), .{}, .{
            .expand = .both,
            .background = false,
        });
        defer scroll.deinit();

        // Content row containing line numbers and text entry
        var content_row = dvui.box(@src(), .{ .dir = .horizontal }, .{
            .expand = .both,
        });
        defer content_row.deinit();

        // Line number gutter
        renderLineNumbers(active_session);

        // Vertical separator line between gutter and text
        renderSeparator();

        // Main text area
        renderTextArea(active_session);
    }

    fn renderLineNumbers(active_session: *session.TabSession) void {
        // Get the mono font with current size from centralized theme
        const scaled_font = theme.fonts.editorFont();
        const line_height = scaled_font.lineHeight();

        var gutter = dvui.box(@src(), .{ .dir = .vertical }, .{
            .min_size_content = .{ .w = gutter_width },
            .padding = .{ .x = content_padding_x, .y = content_padding_y, .w = 4, .h = content_padding_y },
        });
        defer gutter.deinit();

        const line_count = countLines(active_session.content.items);
        for (1..line_count + 1) |line_num| {
            var buf: [16]u8 = undefined;
            const line_str = std.fmt.bufPrint(&buf, "{d: >3}", .{line_num}) catch "???";
            // Use mono font with scaled size and explicit min height matching TextLayoutWidget's line height
            dvui.labelNoFmt(@src(), line_str, .{}, .{
                .id_extra = line_num,
                .font = scaled_font,
                .color_text = theme.colors.text_muted, // Use theme color
                .padding = .{},
                .margin = .{},
                .min_size_content = .{ .h = line_height }, // Match TextLayoutWidget line height
            });
        }
    }

    fn renderSeparator() void {
        var sep = dvui.box(@src(), .{}, .{
            .min_size_content = .{ .w = 1 },
            .expand = .vertical,
            .color_fill = theme.colors.border, // Use theme color
            .background = true,
        });
        sep.deinit();
    }

    fn renderTextArea(active_session: *session.TabSession) void {
        // Get scaled font from centralized theme
        const scaled_font = theme.fonts.editorFont();

        // Text entry with internal scroll disabled - parent scrollArea handles scrolling
        // Using custom theme with transparent focus to hide the focus border
        var text_entry = dvui.textEntry(@src(), .{
            .text = .{
                .array_list = .{
                    .backing = &active_session.content,
                    .allocator = active_session.allocator,
                },
            },
            .multiline = true,
            .scroll_vertical = false, // Let parent scrollArea handle scrolling
            .scroll_horizontal = false,
        }, .{
            .expand = .both,
            .margin = .{}, // Remove default 4px margin
            .padding = .{ .x = content_padding_x, .y = content_padding_y, .w = content_padding_x, .h = content_padding_y },
            .border = .{}, // No border
            .corner_radius = .{}, // No corner radius
            .background = false, // No background drawing
            .color_border = dvui.Color{ .r = 0, .g = 0, .b = 0, .a = 0 }, // Transparent border
            .font = scaled_font, // Use scaled mono font
            .theme = &no_focus_theme, // Use theme with transparent focus
        });
        defer text_entry.deinit();

        // Mark as modified if text changed
        if (text_entry.text_changed) {
            active_session.is_modified = true;
            active_session.render_state.needs_update = true;
        }

        // Update cursor position in session for status bar display
        active_session.updateCursorPosition(text_entry.textLayout.selection.cursor);
    }

    fn countLines(text: []const u8) usize {
        if (text.len == 0) return 1;

        var count: usize = 1;
        for (text) |c| {
            if (c == '\n') count += 1;
        }
        return count;
    }
};
