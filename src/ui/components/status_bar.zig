//! Status bar - bottom of window showing file info and cursor position
//!
//! Layout:
//! ┌─────────────────────────────────────────────────────────────────┐
//! │ path/to/file.logos                          142 lines │ Ln 42  │
//! └─────────────────────────────────────────────────────────────────┘

const std = @import("std");
const dvui = @import("dvui");
const session = @import("../../session/session.zig");

pub const StatusBar = struct {
    // Colors matching the app theme
    const bg_color = dvui.Color{ .r = 24, .g = 28, .b = 36, .a = 255 }; // Slightly darker than main bg
    const border_color = dvui.Color{ .r = 50, .g = 58, .b = 70, .a = 255 };
    const text_color = dvui.Color{ .r = 140, .g = 150, .b = 170, .a = 255 }; // Muted text
    const text_accent = dvui.Color{ .r = 180, .g = 190, .b = 210, .a = 255 }; // Slightly brighter for emphasis

    const status_padding_x: f32 = 12.0;
    const status_padding_y: f32 = 4.0;

    pub fn render(active_session: ?*session.TabSession) void {
        // Top border line
        {
            var top_line = dvui.box(@src(), .{}, .{
                .min_size_content = .{ .h = 1 },
                .expand = .horizontal,
                .color_fill = border_color,
                .background = true,
            });
            top_line.deinit();
        }

        // Main status bar container
        var status_container = dvui.box(@src(), .{ .dir = .horizontal }, .{
            .expand = .horizontal,
            .color_fill = bg_color,
            .background = true,
            .padding = .{ .x = status_padding_x, .y = status_padding_y, .w = status_padding_x, .h = status_padding_y },
        });
        defer status_container.deinit();

        // Left side: file path or status
        {
            var left = dvui.box(@src(), .{ .dir = .horizontal }, .{
                .expand = .horizontal,
                .gravity_y = 0.5,
            });
            defer left.deinit();

            if (active_session) |sess| {
                if (sess.file_path) |path| {
                    // Show file path
                    dvui.label(@src(), "{s}", .{path}, .{
                        .font = dvui.Font.theme(.body).withSize(15),
                        .color_text = text_color,
                    });

                    // Show modified indicator
                    if (sess.is_modified) {
                        dvui.labelNoFmt(@src(), " •", .{}, .{
                            .font = dvui.Font.theme(.body).withSize(15),
                            .color_text = text_accent,
                        });
                    }
                } else {
                    // Unsaved file
                    dvui.labelNoFmt(@src(), "Untitled", .{}, .{
                        .font = dvui.Font.theme(.body).withSize(15),
                        .color_text = text_color,
                    });
                    if (sess.is_modified) {
                        dvui.labelNoFmt(@src(), " (unsaved)", .{}, .{
                            .font = dvui.Font.theme(.body).withSize(15),
                            .color_text = text_accent,
                        });
                    }
                }
            } else {
                dvui.labelNoFmt(@src(), "No file open", .{}, .{
                    .font = dvui.Font.theme(.body).withSize(15),
                    .color_text = text_color,
                });
            }
        }

        // Right side: line count and cursor position
        {
            var right = dvui.box(@src(), .{ .dir = .horizontal }, .{
                .gravity_y = 0.5,
            });
            defer right.deinit();

            if (active_session) |sess| {
                const line_count = countLines(sess.content.items);

                // Separator
                dvui.labelNoFmt(@src(), "│", .{}, .{
                    .font = dvui.Font.theme(.body).withSize(15),
                    .color_text = border_color,
                    .padding = .{ .x = 8, .y = 0, .w = 8, .h = 0 },
                });

                // Line count
                if (line_count == 1) {
                    dvui.label(@src(), "{d} line", .{line_count}, .{
                        .font = dvui.Font.theme(.body).withSize(15),
                        .color_text = text_color,
                    });
                } else {
                    dvui.label(@src(), "{d} lines", .{line_count}, .{
                        .font = dvui.Font.theme(.body).withSize(15),
                        .color_text = text_color,
                    });
                }

                // Separator
                dvui.labelNoFmt(@src(), "│", .{}, .{
                    .font = dvui.Font.theme(.body).withSize(15),
                    .color_text = border_color,
                    .padding = .{ .x = 8, .y = 0, .w = 8, .h = 0 },
                });

                // Cursor position: Ln X, Col Y
                dvui.label(@src(), "Ln {d}, Col {d}", .{ sess.cursor_line, sess.cursor_col }, .{
                    .font = dvui.Font.theme(.body).withSize(15),
                    .color_text = text_color,
                });
            }
        }
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

