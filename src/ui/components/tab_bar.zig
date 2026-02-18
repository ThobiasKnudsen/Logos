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
        start_edit: usize, // Start editing tab name at index
        finish_edit: struct { index: usize, new_name: []const u8 }, // Finish editing with new name
        cancel_edit, // Cancel editing
        change_folder: usize, // Open folder picker for tab at index
    };

    // Padding relative to font size (em units) - smaller values for thinner tabs
    const tab_padding_x: f32 = 10.0; // horizontal padding in em
    const tab_padding_y: f32 = 1.0; // vertical padding in em

    // Double-click detection
    const double_click_time_ms: i64 = 400;
    var last_click_tab: ?usize = null;
    var last_click_time: i64 = 0;

    // Persistent state for tab editing
    var editing_tab_index: ?usize = null;
    var edit_buffer: [256]u8 = undefined;
    var edit_len: usize = 0;
    var edit_had_focus: bool = false; // Track if text entry has been focused at least once

    // Right-click context menu state
    var context_menu_tab: ?usize = null;
    var context_menu_point: dvui.Point.Physical = .{};

    /// Start editing a tab name
    pub fn startEditing(index: usize, current_name: []const u8) void {
        editing_tab_index = index;
        edit_len = @min(current_name.len, edit_buffer.len - 1); // Leave room for null terminator
        @memcpy(edit_buffer[0..edit_len], current_name[0..edit_len]);
        edit_buffer[edit_len] = 0; // Null terminate
        edit_had_focus = false; // Reset focus tracking for new edit session
    }

    /// Stop editing without saving
    pub fn cancelEditing() void {
        editing_tab_index = null;
        edit_len = 0;
    }

    /// Check if currently editing a specific tab
    pub fn isEditing(index: usize) bool {
        return editing_tab_index != null and editing_tab_index.? == index;
    }

    /// Check if any tab is being edited
    pub fn isEditingAny() bool {
        return editing_tab_index != null;
    }

    pub fn render(session_manager: *session.SessionManager) Action {
        var action: Action = .none;

        // Tab bar container with top and bottom border lines
        var tabs_container = dvui.box(@src(), .{ .dir = .vertical }, .{
            .expand = .horizontal,
            .color_fill = theme.colors.bg_secondary,
            .background = true,
        });
        defer tabs_container.deinit();

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

        // Main tab row
        {
            var tabs_row = dvui.box(@src(), .{ .dir = .horizontal }, .{
                .expand = .horizontal,
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
                        .color_fill = theme.colors.border,
                        .background = true,
                    });
                    sep.deinit();
                }

                // Render the tab as a unified clickable area
                if (renderTab(sess, idx, is_active)) |tab_action| {
                    action = tab_action;
                }
            }

            // Separator before add button
            {
                var sep = dvui.box(@src(), .{}, .{
                    .min_size_content = .{ .w = 1 },
                    .expand = .vertical,
                    .color_fill = theme.colors.border,
                    .background = true,
                });
                sep.deinit();
            }

            // Add tab button - using transparent button style
            const tab_font = dvui.Font.theme(.body).withSize(theme.fonts.uiSize());
            if (dvui.button(@src(), "+", .{ .draw_focus = false }, .{
                .border = .{},
                .corner_radius = .{},
                .margin = .{},
                .padding = .{ .x = tab_padding_x, .y = tab_padding_y, .w = tab_padding_x, .h = tab_padding_y },
                .background = false, // Transparent background
                .color_text = theme.colors.text_primary,
                .color_fill = dvui.Color{ .r = 0, .g = 0, .b = 0, .a = 0 },
                .gravity_y = 0.5,
                .font = tab_font,
            })) {
                action = .add_tab;
            }

            // Separator after add button
            {
                var sep = dvui.box(@src(), .{}, .{
                    .min_size_content = .{ .w = 1 },
                    .expand = .vertical,
                    .color_fill = theme.colors.border,
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
                .color_fill = theme.colors.border,
                .background = true,
            });
            bottom_line.deinit();
        }

        // Render context menu if active
        if (renderContextMenu()) |ctx_action| {
            action = ctx_action;
        }

        return action;
    }

    /// Render a single tab as a unified clickable element
    fn renderTab(sess: *session.TabSession, idx: usize, is_active: bool) ?Action {
        var action: ?Action = null;
        const is_editing_this = isEditing(idx);
        const tab_font = dvui.Font.theme(.body).withSize(theme.fonts.uiSize());

        // Outer tab container with background - this creates the unified appearance
        var tab_box = dvui.box(@src(), .{ .dir = .horizontal }, .{
            .id_extra = idx,
            .color_fill = if (is_active) theme.colors.bg_elevated else theme.colors.bg_primary,
            .background = true,
        });
        defer tab_box.deinit();

        if (is_editing_this) {
            // Null-terminate the buffer at current length for textEntry
            if (edit_len < edit_buffer.len) {
                edit_buffer[edit_len] = 0;
            }

            // Show text entry for editing - pass full buffer so it can grow
            // Background enabled to show text selection for copy/paste support
            var text_entry = dvui.textEntry(@src(), .{
                .text = .{ .buffer = &edit_buffer },
            }, .{
                .id_extra = idx,
                .margin = .{ .x = tab_padding_x, .y = tab_padding_y, .w = 0.2, .h = tab_padding_y },
                .padding = .{ .x = 4, .y = 2, .w = 4, .h = 2 },
                .border = .{},
                .corner_radius = dvui.Rect{ .x = 2, .y = 2, .w = 2, .h = 2 },
                .background = true,
                .color_fill = theme.colors.bg_hover, // Use theme color
                .min_size_content = .{ .w = 60, .h = 16 },
                .color_text = theme.colors.text_primary,
                .font = tab_font,
                .gravity_y = 0.5,
            });
            defer text_entry.deinit();

            // Update edit length if changed - find null terminator
            if (text_entry.text_changed) {
                edit_len = std.mem.indexOfScalar(u8, &edit_buffer, 0) orelse edit_buffer.len;
            }

            // Check for Enter (confirm) or Escape (cancel)
            for (dvui.events()) |*e| {
                if (e.evt == .key and e.evt.key.action == .down) {
                    if (e.evt.key.code == .enter or e.evt.key.code == .kp_enter) {
                        // Finish editing with new name
                        const new_name = edit_buffer[0..edit_len];
                        if (new_name.len > 0) {
                            action = .{ .finish_edit = .{ .index = idx, .new_name = new_name } };
                        } else {
                            action = .cancel_edit;
                        }
                        e.handled = true;
                    } else if (e.evt.key.code == .escape) {
                        action = .cancel_edit;
                        e.handled = true;
                    }
                }
            }

            // Track focus state and detect focus loss
            const has_focus = text_entry.data().id == dvui.focusedWidgetId();
            if (has_focus) {
                edit_had_focus = true;
            } else if (!edit_had_focus) {
                // First frame of editing - auto-focus the text entry
                dvui.focusWidget(text_entry.data().id, null, null);
            } else if (action == null) {
                // Lost focus after having it - finish editing
                const new_name = edit_buffer[0..edit_len];
                if (new_name.len > 0) {
                    action = .{ .finish_edit = .{ .index = idx, .new_name = new_name } };
                } else {
                    action = .cancel_edit;
                }
            }
        } else {
            // Tab name label - strip .logos extension for display
            const display_name = if (std.mem.endsWith(u8, sess.name, ".logos"))
                sess.name[0 .. sess.name.len - 6]
            else
                sess.name;
            dvui.label(@src(), "{s}", .{display_name}, .{
                .id_extra = idx,
                .border = .{},
                .corner_radius = .{},
                .margin = .{},
                .padding = .{ .x = tab_padding_x, .y = tab_padding_y, .w = 0.2, .h = tab_padding_y },
                .background = false,
                .color_text = if (is_active) theme.colors.text_primary else theme.colors.text_secondary,
                .font = tab_font,
                .gravity_y = 0.5,
            });
        }

        // Close button - render first to get its rect for click exclusion
        var close_btn: dvui.ButtonWidget = undefined;
        close_btn.init(@src(), .{ .draw_focus = false }, .{
            .id_extra = idx,
            .border = .{},
            .corner_radius = .{},
            .margin = .{},
            .padding = .{ .x = 0.2, .y = tab_padding_y, .w = 0.4, .h = tab_padding_y },
            .background = false,
            .color_text = theme.colors.text_secondary,
            .color_fill = dvui.Color{ .r = 0, .g = 0, .b = 0, .a = 0 },
            .font = tab_font,
            .gravity_y = 0.5,
        });
        defer close_btn.deinit();

        close_btn.processEvents();
        close_btn.drawBackground();
        dvui.label(@src(), "×", .{}, .{});

        if (close_btn.clicked()) {
            action = .{ .close_tab = idx };
            dvui.refresh(null, @src(), null);
        }

        // Handle mouse clicks on tab area (but NOT on close button) - only when not editing
        if (!is_editing_this) {
            const close_rect = close_btn.data().borderRectScale().r;
            for (dvui.events()) |*e| {
                if (e.evt == .mouse and e.evt.mouse.action == .release) {
                    const tab_rect = tab_box.data().borderRectScale().r;
                    // Check click is in tab area but NOT in close button
                    if (tab_rect.contains(e.evt.mouse.p) and !close_rect.contains(e.evt.mouse.p)) {
                        if (e.evt.mouse.button == .left) {
                            // Left click - check for double-click
                            const now = std.time.milliTimestamp();
                            if (last_click_tab != null and last_click_tab.? == idx and
                                (now - last_click_time) < double_click_time_ms)
                            {
                                // Double-click detected - start editing
                                action = .{ .start_edit = idx };
                                last_click_tab = null;
                                last_click_time = 0;
                            } else {
                                // Single click - select tab and record for potential double-click
                                action = .{ .select_tab = idx };
                                last_click_tab = idx;
                                last_click_time = now;
                            }
                            e.handled = true;
                            dvui.refresh(null, @src(), null); // Request immediate redraw
                        } else if (e.evt.mouse.button == .right) {
                            // Right-click - open context menu
                            context_menu_tab = idx;
                            context_menu_point = e.evt.mouse.p;
                            e.handled = true;
                            dvui.refresh(null, @src(), null); // Request immediate redraw
                        }
                    }
                }
            }
        }

        // Show tooltip with file path when hovering over the tab (below the tab)
        const tooltip_font = dvui.Font.theme(.body).withSize(theme.fonts.tooltipSize());
        if (sess.file_path) |path| {
            var tt: dvui.FloatingTooltipWidget = undefined;
            tt.init(@src(), .{
                .active_rect = tab_box.data().borderRectScale().r,
                .position = .vertical,
            }, .{
                .id_extra = idx,
                .color_border = theme.colors.border,
                .color_fill = theme.colors.bg_secondary,
                .border = .all(1),
            });
            if (tt.shown()) {
                var tl = dvui.textLayout(@src(), .{}, .{
                    .background = false,
                    .color_text = theme.colors.text_primary,
                    .font = tooltip_font,
                });
                tl.format("{s}", .{path}, .{});
                tl.deinit();
            }
            tt.deinit();
        } else {
            var tt: dvui.FloatingTooltipWidget = undefined;
            tt.init(@src(), .{
                .active_rect = tab_box.data().borderRectScale().r,
                .position = .vertical,
            }, .{
                .id_extra = idx,
                .color_border = theme.colors.border,
                .color_fill = theme.colors.bg_secondary,
                .border = .all(1),
            });
            if (tt.shown()) {
                var tl = dvui.textLayout(@src(), .{}, .{
                    .background = false,
                    .color_text = theme.colors.text_primary,
                    .font = tooltip_font,
                });
                tl.format("NOT SAVED", .{}, .{});
                tl.deinit();
            }
            tt.deinit();
        }

        return action;
    }

    /// Render the context menu if active
    fn renderContextMenu() ?Action {
        const idx = context_menu_tab orelse return null;

        // Create floating menu at the click position (convert physical to natural coords)
        var fw = dvui.floatingMenu(@src(), .{ .from = dvui.Rect.Natural.fromPoint(context_menu_point.toNatural()) }, .{
            .id_extra = idx,
        });
        defer fw.deinit();

        // Check for mouse clicks outside the menu to close it
        const menu_rect = fw.data().borderRectScale().r;
        for (dvui.events()) |*e| {
            if (e.evt == .mouse and e.evt.mouse.action == .press) {
                if (!menu_rect.contains(e.evt.mouse.p)) {
                    // Clicked outside menu - close it
                    fw.close();
                    context_menu_tab = null;
                    return null;
                }
            }
        }

        if (dvui.menuItemLabel(@src(), "Change Folder Path...", .{}, .{ .expand = .horizontal }) != null) {
            fw.close();
            context_menu_tab = null;
            return .{ .change_folder = idx };
        }

        if (dvui.menuItemLabel(@src(), "Rename", .{}, .{ .expand = .horizontal }) != null) {
            fw.close();
            context_menu_tab = null;
            return .{ .start_edit = idx };
        }

        _ = dvui.separator(@src(), .{});

        if (dvui.menuItemLabel(@src(), "Close Tab", .{}, .{ .expand = .horizontal }) != null) {
            fw.close();
            context_menu_tab = null;
            return .{ .close_tab = idx };
        }

        return null;
    }

    /// Close context menu
    pub fn closeContextMenu() void {
        context_menu_tab = null;
    }
};
