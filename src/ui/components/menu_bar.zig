//! Top menu bar - File, Edit, View, Help

const std = @import("std");
const dvui = @import("dvui");
const theme = @import("../theme.zig");

pub const MenuBar = struct {
    pub const Action = union(enum) {
        none,
        // File
        new_session,
        open_file,
        save,
        save_as,
        close_tab,
        quit,
        // Edit
        undo,
        redo,
        cut,
        copy,
        paste,
        // View
        zoom_in,
        zoom_out,
        reset_zoom,
        // Theme — carries the selected theme name
        set_theme: [64]u8,
    };

    pub fn render() Action {
        var action: Action = .none;

        // Get scaled font for menu items
        const menu_font = dvui.Font.theme(.body).withSize(theme.fonts.menuSize());

        // Menu bar container with subtle background
        var hbox = dvui.box(@src(), .{ .dir = .horizontal }, .{
            .color_fill = theme.colors.bg_primary,
            .background = true,
            .expand = .horizontal,
            .padding = .{ .x = 4, .y = 2, .w = 4, .h = 2 },
        });
        defer hbox.deinit();

        var m = dvui.menu(@src(), .horizontal, .{ .font = menu_font });
        defer m.deinit();

        // File menu
        if (dvui.menuItemLabel(@src(), "File", .{ .submenu = true }, .{})) |r| {
            var fw = dvui.floatingMenu(@src(), .{ .from = r }, .{});
            defer fw.deinit();

            if (dvui.menuItemLabel(@src(), "New Session   Ctrl+N", .{}, .{ .expand = .horizontal }) != null) {
                action = .new_session;
                m.close();
            }
            if (dvui.menuItemLabel(@src(), "Open...       Ctrl+O", .{}, .{ .expand = .horizontal }) != null) {
                action = .open_file;
                m.close();
            }
            _ = dvui.separator(@src(), .{});
            if (dvui.menuItemLabel(@src(), "Save          Ctrl+S", .{}, .{ .expand = .horizontal }) != null) {
                action = .save;
                m.close();
            }
            if (dvui.menuItemLabel(@src(), "Save As...    Ctrl+Shift+S", .{}, .{ .expand = .horizontal }) != null) {
                action = .save_as;
                m.close();
            }
            _ = dvui.separator(@src(), .{});
            if (dvui.menuItemLabel(@src(), "Close Tab     Ctrl+W", .{}, .{ .expand = .horizontal }) != null) {
                action = .close_tab;
                m.close();
            }
            if (dvui.menuItemLabel(@src(), "Quit          Ctrl+Q", .{}, .{ .expand = .horizontal }) != null) {
                action = .quit;
                m.close();
            }
        }

        // Edit menu
        if (dvui.menuItemLabel(@src(), "Edit", .{ .submenu = true }, .{})) |r| {
            var fw = dvui.floatingMenu(@src(), .{ .from = r }, .{});
            defer fw.deinit();

            if (dvui.menuItemLabel(@src(), "Undo    Ctrl+Z", .{}, .{ .expand = .horizontal }) != null) {
                action = .undo;
                m.close();
            }
            if (dvui.menuItemLabel(@src(), "Redo    Ctrl+Y", .{}, .{ .expand = .horizontal }) != null) {
                action = .redo;
                m.close();
            }
            _ = dvui.separator(@src(), .{});
            if (dvui.menuItemLabel(@src(), "Cut     Ctrl+X", .{}, .{ .expand = .horizontal }) != null) {
                action = .cut;
                m.close();
            }
            if (dvui.menuItemLabel(@src(), "Copy    Ctrl+C", .{}, .{ .expand = .horizontal }) != null) {
                action = .copy;
                m.close();
            }
            if (dvui.menuItemLabel(@src(), "Paste   Ctrl+V", .{}, .{ .expand = .horizontal }) != null) {
                action = .paste;
                m.close();
            }
        }

        // View menu
        if (dvui.menuItemLabel(@src(), "View", .{ .submenu = true }, .{})) |r| {
            var fw = dvui.floatingMenu(@src(), .{ .from = r }, .{});
            defer fw.deinit();

            if (dvui.menuItemLabel(@src(), "Zoom In   Ctrl+=", .{}, .{ .expand = .horizontal }) != null) {
                action = .zoom_in;
                m.close();
            }
            if (dvui.menuItemLabel(@src(), "Zoom Out  Ctrl+-", .{}, .{ .expand = .horizontal }) != null) {
                action = .zoom_out;
                m.close();
            }
            if (dvui.menuItemLabel(@src(), "Reset Zoom  Ctrl+0", .{}, .{ .expand = .horizontal }) != null) {
                action = .reset_zoom;
                m.close();
            }
            _ = dvui.separator(@src(), .{});

            // Theme submenu — entries loaded from .logos.theme.json
            if (dvui.menuItemLabel(@src(), "Theme", .{ .submenu = true }, .{ .expand = .horizontal })) |tr| {
                var tw = dvui.floatingMenu(@src(), .{ .from = tr }, .{});
                defer tw.deinit();

                const current = theme.syntax.currentThemeName();
                const count = theme.syntax.themeCount();

                for (0..count) |i| {
                    const name = theme.syntax.themeName(i);
                    const is_active = std.mem.eql(u8, name, current);

                    // Build display label: "checkmark name" or "  name"
                    var label_buf: [80]u8 = undefined;
                    const prefix: []const u8 = if (is_active) "\xE2\x9C\x93 " else "  ";
                    const label_len = prefix.len + name.len;
                    if (label_len <= label_buf.len) {
                        @memcpy(label_buf[0..prefix.len], prefix);
                        @memcpy(label_buf[prefix.len..][0..name.len], name);

                        if (dvui.menuItemLabel(@src(), label_buf[0..label_len], .{}, .{
                            .expand = .horizontal,
                            .id_extra = i,
                        }) != null) {
                            var name_buf: [64]u8 = .{0} ** 64;
                            @memcpy(name_buf[0..name.len], name);
                            action = .{ .set_theme = name_buf };
                            m.close();
                        }
                    }
                }
            }
        }

        // Help menu
        if (dvui.menuItemLabel(@src(), "Help", .{ .submenu = true }, .{})) |r| {
            var fw = dvui.floatingMenu(@src(), .{ .from = r }, .{});
            defer fw.deinit();

            _ = dvui.menuItemLabel(@src(), "Documentation", .{}, .{ .expand = .horizontal });
            _ = dvui.menuItemLabel(@src(), "About Logos", .{}, .{ .expand = .horizontal });
        }

        return action;
    }
};
