//! Tab bar for switching between user sessions

const std = @import("std");
const dvui = @import("dvui");
const theme = @import("../theme.zig");
const session = @import("../../session/session.zig");

pub const TabBar = struct {
    pub const Action = union(enum) {
        none,
        add_tab,
        select_tab: usize,
        close_tab: usize,
    };

    const tab_height: f32 = 32;
    const border_color = dvui.Color{ .r = 60, .g = 70, .b = 85, .a = 255 };
    const bg_color = dvui.Color{ .r = 30, .g = 36, .b = 45, .a = 255 };
    const active_bg = dvui.Color{ .r = 40, .g = 48, .b = 60, .a = 255 };

    pub fn render(session_manager: *session.SessionManager) Action {
        var action: Action = .none;

        // Tab bar container with top and bottom border lines
        var tabs_container = dvui.box(@src(), .{ .dir = .vertical }, .{
            .expand = .horizontal,
            .color_fill = bg_color,
            .background = true,
        });
        defer tabs_container.deinit();

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

        // Main tab row
        {
            var tabs_row = dvui.box(@src(), .{ .dir = .horizontal }, .{
                .expand = .horizontal,
                .min_size_content = .{ .h = tab_height },
            });
            defer tabs_row.deinit();

            // Render each tab
            for (session_manager.sessions.items, 0..) |*sess, idx| {
                const is_active = idx == session_manager.active_index;

                // Add vertical separator before each tab (except first)
                if (idx > 0) {
                    var sep = dvui.box(@src(), .{}, .{
                        .id_extra = idx,
                        .min_size_content = .{ .w = 1 },
                        .expand = .vertical,
                        .color_fill = border_color,
                        .background = true,
                    });
                    sep.deinit();
                }

                // Tab container - no border, just background color difference for active
                var tab_box = dvui.box(@src(), .{ .dir = .horizontal }, .{
                    .id_extra = idx,
                    .color_fill = if (is_active) active_bg else bg_color,
                    .background = true,
                    .padding = .{ .x = 12, .y = 0, .w = 8, .h = 0 },
                    .min_size_content = .{ .h = tab_height },
                });
                defer tab_box.deinit();

                // Tab name - clickable label, vertically centered
                if (dvui.button(@src(), sess.name, .{}, .{
                    .id_extra = idx,
                    .border = .{}, // No border on button
                    .corner_radius = .{},
                    .padding = .{ .y = 8, .h = 8 },
                    .margin = .{},
                })) {
                    action = .{ .select_tab = idx };
                }

                // Small spacing between name and close button
                {
                    var spacer = dvui.box(@src(), .{}, .{
                        .id_extra = idx,
                        .min_size_content = .{ .w = 8 },
                    });
                    spacer.deinit();
                }

                // Close button - vertically centered
                if (dvui.button(@src(), "x", .{}, .{
                    .id_extra = idx,
                    .border = .{}, // No border
                    .corner_radius = .{},
                    .padding = .{ .x = 4, .y = 8, .w = 4, .h = 8 },
                    .margin = .{},
                    .color_text = dvui.Color{ .r = 140, .g = 150, .b = 170, .a = 255 },
                })) {
                    action = .{ .close_tab = idx };
                }
            }

            // Separator before add button
            {
                var sep = dvui.box(@src(), .{}, .{
                    .min_size_content = .{ .w = 1 },
                    .expand = .vertical,
                    .color_fill = border_color,
                    .background = true,
                });
                sep.deinit();
            }

            // Add tab button
            if (dvui.button(@src(), "+", .{}, .{
                .border = .{},
                .corner_radius = .{},
                .padding = .{ .x = 12, .y = 8, .w = 12, .h = 8 },
            })) {
                action = .add_tab;
            }

            // Separator after add button
            {
                var sep = dvui.box(@src(), .{}, .{
                    .min_size_content = .{ .w = 1 },
                    .expand = .vertical,
                    .color_fill = border_color,
                    .background = true,
                });
                sep.deinit();
            }

            // Fill remaining space
            {
                var fill = dvui.box(@src(), .{}, .{
                    .expand = .horizontal,
                });
                fill.deinit();
            }
        }

        // Bottom border line
        {
            var bottom_line = dvui.box(@src(), .{}, .{
                .min_size_content = .{ .h = 1 },
                .expand = .horizontal,
                .color_fill = border_color,
                .background = true,
            });
            bottom_line.deinit();
        }

        return action;
    }
};
