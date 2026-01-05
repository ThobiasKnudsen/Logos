//! Text editor panel - left side of the split view
//!
//! Multi-line text area where users write their expressions/code
//! that gets parsed and visualized in the graph panel.

const std = @import("std");
const dvui = @import("dvui");
const theme = @import("../theme.zig");
const session = @import("../../session/session.zig");

pub const EditorPanel = struct {
    pub fn render(active_session: *session.TabSession) void {
        // Editor container - no background/border, just layout
        var editor_box = dvui.box(@src(), .{ .dir = .horizontal }, .{
            .expand = .both,
        });
        defer editor_box.deinit();

        // Line number gutter - same background as editor
        // Padding must match the text entry padding exactly for alignment
        {
            var gutter = dvui.box(@src(), .{ .dir = .vertical }, .{
                .min_size_content = .{ .w = 40 },
                .padding = .{ .x = 8, .y = 4, .w = 4, .h = 4 },
                .expand = .vertical,
            });
            defer gutter.deinit();

            const line_count = countLines(active_session.content.items);
            for (1..line_count + 1) |line_num| {
                var buf: [16]u8 = undefined;
                const line_str = std.fmt.bufPrint(&buf, "{d: >3}", .{line_num}) catch "???";
                // Use mono font, muted color - padding matches text entry line height
                dvui.labelNoFmt(@src(), line_str, .{}, .{
                    .id_extra = line_num,
                    .font = .theme(.mono),
                    .color_text = dvui.Color{ .r = 100, .g = 110, .b = 130, .a = 255 },
                    .padding = .{}, // No extra padding - let natural line height work
                });
            }
        }

        // Vertical separator line between gutter and text
        {
            var sep = dvui.box(@src(), .{}, .{
                .min_size_content = .{ .w = 1 },
                .expand = .vertical,
                .color_fill = dvui.Color{ .r = 60, .g = 70, .b = 85, .a = 255 },
                .background = true,
            });
            sep.deinit();
        }

        // Main text area - no visible border or focus highlight
        {
            // Text entry using ArrayList backing for dynamic content
            var text_entry = dvui.textEntry(@src(), .{
                .text = .{
                    .array_list = .{
                        .backing = &active_session.content,
                        .allocator = active_session.allocator,
                    },
                },
                .multiline = true,
                .scroll_vertical = true,
            }, .{
                .expand = .both,
                .padding = .{ .x = 8, .y = 4, .w = 8, .h = 4 },
                .border = .{}, // No border
                .corner_radius = .{}, // No corner radius
                .color_border = dvui.Color{ .r = 0, .g = 0, .b = 0, .a = 0 }, // Transparent border (removes focus highlight)
            });
            defer text_entry.deinit();

            // Mark as modified if text changed
            if (text_entry.text_changed) {
                active_session.is_modified = true;
                active_session.render_state.needs_update = true;
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
