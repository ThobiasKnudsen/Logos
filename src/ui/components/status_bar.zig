//! Status bar - bottom of window showing file info and cursor position
//!
//! Layout:
//! ┌─────────────────────────────────────────────────────────────────┐
//! │ path/to/file.logos                          142 lines │ Ln 42  │
//! └─────────────────────────────────────────────────────────────────┘

const std = @import("std");
const dvui = @import("dvui");
const theme = @import("../theme.zig");
const session = @import("../../session/session.zig");
const GraphRenderer = @import("../../renderer/graph_renderer.zig").GraphRenderer;

pub const StatusBar = struct {
    const status_padding_x: f32 = 12.0;
    const status_padding_y: f32 = 4.0;

    pub fn render(active_session: ?*session.TabSession, graph_renderer: ?*GraphRenderer) void {
        // Get scaled font for status bar
        const status_font = dvui.Font.theme(.body).withSize(theme.fonts.statusSize());

        // Top border line
        {
            var top_line = dvui.box(@src(), .{}, .{
                .min_size_content = .{ .h = 1 },
                .expand = .horizontal,
                .color_fill = theme.colors.border,
                .background = true,
            });
            top_line.deinit();
        }

        // Main status bar container
        var status_container = dvui.box(@src(), .{ .dir = .horizontal }, .{
            .expand = .horizontal,
            .color_fill = theme.colors.bg_primary,
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
                        .font = status_font,
                        .color_text = theme.colors.text_secondary,
                    });

                    // Show modified indicator
                    if (sess.is_modified) {
                        dvui.labelNoFmt(@src(), " •", .{}, .{
                            .font = status_font,
                            .color_text = theme.colors.text_primary,
                        });
                    }
                } else {
                    // Unsaved file
                    dvui.labelNoFmt(@src(), "Untitled", .{}, .{
                        .font = status_font,
                        .color_text = theme.colors.text_secondary,
                    });
                    if (sess.is_modified) {
                        dvui.labelNoFmt(@src(), " (unsaved)", .{}, .{
                            .font = status_font,
                            .color_text = theme.colors.text_primary,
                        });
                    }
                }
            } else {
                dvui.labelNoFmt(@src(), "No file open", .{}, .{
                    .font = status_font,
                    .color_text = theme.colors.text_secondary,
                });
            }
        }

        // Right side: parse status, shader status, line count, cursor position
        {
            var right = dvui.box(@src(), .{ .dir = .horizontal }, .{
                .gravity_y = 0.5,
            });
            defer right.deinit();

            if (active_session) |sess| {
                // Check for shader errors first (highest priority)
                const has_shader_error = if (graph_renderer) |gr| gr.last_error != null else false;

                if (has_shader_error) {
                    // Show shader error prominently
                    if (graph_renderer) |gr| {
                        if (gr.last_error) |err_msg| {
                            dvui.labelNoFmt(@src(), "⚠ Shader Error: ", .{}, .{
                                .font = status_font,
                                .color_text = dvui.Color{ .r = 255, .g = 100, .b = 100, .a = 255 },
                            });

                            // Truncate error message if too long
                            const max_len = 80;
                            if (err_msg.len > max_len) {
                                var truncated_buf: [max_len + 3]u8 = undefined;
                                @memcpy(truncated_buf[0..max_len], err_msg[0..max_len]);
                                truncated_buf[max_len] = '.';
                                truncated_buf[max_len + 1] = '.';
                                truncated_buf[max_len + 2] = '.';
                                dvui.labelNoFmt(@src(), truncated_buf[0 .. max_len + 3], .{}, .{
                                    .font = status_font,
                                    .color_text = dvui.Color{ .r = 255, .g = 120, .b = 120, .a = 255 },
                                });
                            } else {
                                dvui.labelNoFmt(@src(), err_msg, .{}, .{
                                    .font = status_font,
                                    .color_text = dvui.Color{ .r = 255, .g = 120, .b = 120, .a = 255 },
                                });
                            }
                        }
                    }
                } else {
                    // No shader error - show parse status
                    const parse_status = sess.parse_state.status;
                    const parse_status_text = switch (parse_status) {
                        .idle => "Ready",
                        .lexing => "Lexing...",
                        .parsing => "Parsing...",
                        .ready => "OK",
                        .err => "Error",
                    };

                    // Status colors (green for OK, red for Error, muted otherwise)
                    const parse_status_color = switch (parse_status) {
                        .ready => theme.colors.accent_secondary, // green
                        .err => dvui.Color{ .r = 220, .g = 80, .b = 80, .a = 255 }, // red
                        else => theme.colors.text_secondary,
                    };

                    dvui.labelNoFmt(@src(), parse_status_text, .{}, .{
                        .font = status_font,
                        .color_text = parse_status_color,
                    });

                    // Separator
                    dvui.labelNoFmt(@src(), "│", .{}, .{
                        .font = status_font,
                        .color_text = theme.colors.border,
                        .padding = .{ .x = 8, .y = 0, .w = 8, .h = 0 },
                    });

                    const all_content = sess.getAllCellsContent() catch "";
                    defer if (all_content.len > 0) sess.allocator.free(all_content);
                    const line_count = countLines(all_content);

                    // Line count
                    if (line_count == 1) {
                        dvui.label(@src(), "{d} line", .{line_count}, .{
                            .font = status_font,
                            .color_text = theme.colors.text_secondary,
                        });
                    } else {
                        dvui.label(@src(), "{d} lines", .{line_count}, .{
                            .font = status_font,
                            .color_text = theme.colors.text_secondary,
                        });
                    }

                    // Separator
                    dvui.labelNoFmt(@src(), "│", .{}, .{
                        .font = status_font,
                        .color_text = theme.colors.border,
                        .padding = .{ .x = 8, .y = 0, .w = 8, .h = 0 },
                    });

                    // Cursor position: Ln X, Col Y
                    dvui.label(@src(), "Ln {d}, Col {d}", .{ sess.cursor_line, sess.cursor_col }, .{
                        .font = status_font,
                        .color_text = theme.colors.text_secondary,
                    });
                }
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

