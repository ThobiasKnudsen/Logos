//! Main application view - orchestrates all UI components
//!
//! Layout:
//! ┌─────────────────────────────────────────────────────┐
//! │ Menu Bar (File, Edit, View, Help)                   │
//! ├─────────────────────────────────────────────────────┤
//! │ Tab Bar [Session 1] [Session 2] [+]                 │
//! ├─────────────────────────────────────────────────────┤
//! │                       │                             │
//! │   Editor Panel        │   Graph/Plot Renderer       │
//! │   (Text input)        │   (SDL3 texture + shaders)  │
//! │                       │                             │
//! └───────────────────────┴─────────────────────────────┘

const std = @import("std");
const dvui = @import("dvui");
const theme = @import("../theme.zig");
const components = @import("../components/components.zig");

const App = @import("../../app.zig").App;

/// Auto-save state
var last_auto_save_time: i64 = 0;
const auto_save_interval_ms: i64 = 2000; // Save every 2 seconds if modified

/// Persistent state for the split view
var split_view: components.SplitView = .{};

/// Separator line color
const separator_color = dvui.Color{ .r = 50, .g = 58, .b = 70, .a = 255 };

/// Returns false if user wants to quit
pub fn mainView(app: *App) bool {
    // Root container - fills entire window
    var root = dvui.box(@src(), .{ .dir = .vertical }, .{
        .expand = .both,
        .color_fill = dvui.Color{ .r = 28, .g = 33, .b = 42, .a = 255 },
        .background = true,
    });
    defer root.deinit();

    // Menu bar at top
    const menu_action = components.MenuBar.render();
    if (!handleMenuAction(app, menu_action)) return false;

    // Tab bar below menu (includes its own top/bottom border lines)
    const tab_action = components.TabBar.render(&app.session_manager);
    handleTabAction(app, tab_action);

    // Main content area - split view (editor | graph)
    if (app.session_manager.activeSession()) |active_session| {
        split_view.render(active_session, &app.graph_renderer);

        // Auto-save logic: save non-Untitled tabs when modified
        if (active_session.file_path != null and active_session.is_modified) {
            const now = std.time.milliTimestamp();
            if (now - last_auto_save_time > auto_save_interval_ms) {
                active_session.saveToFile() catch {};
                last_auto_save_time = now;
            }
        }
    } else {
        // No sessions - show empty state
        renderEmptyState(app);
    }

    // Check for keyboard shortcuts and window events
    var keyboard_action: components.MenuBar.Action = .none;
    for (dvui.events()) |*e| {
        if (e.evt == .key and e.evt.key.action == .down) {
            const ctrl_pressed = e.evt.key.mod.has(.lcontrol) or e.evt.key.mod.has(.rcontrol);
            if (ctrl_pressed) {
                switch (e.evt.key.code) {
                    .n => {
                        keyboard_action = .new_session;
                        e.handled = true;
                    },
                    .o => {
                        keyboard_action = .open_file;
                        e.handled = true;
                    },
                    .s => {
                        keyboard_action = .save;
                        e.handled = true;
                    },
                    .w => {
                        keyboard_action = .close_tab;
                        e.handled = true;
                    },
                    .q => {
                        keyboard_action = .quit;
                        e.handled = true;
                    },
                    else => {},
                }
            }
        }
        if (e.evt == .window and e.evt.window.action == .close) return false;
        if (e.evt == .app and e.evt.app.action == .quit) return false;
    }

    // Handle keyboard shortcuts
    if (!handleMenuAction(app, keyboard_action)) return false;

    return true;
}

/// Draw a horizontal separator line
fn horizontalSeparator(id_extra: usize) void {
    var sep = dvui.box(@src(), .{}, .{
        .id_extra = id_extra,
        .min_size_content = .{ .h = 1 },
        .expand = .horizontal,
        .color_fill = separator_color,
        .background = true,
    });
    sep.deinit();
}

/// Returns false if should quit
fn handleMenuAction(app: *App, action: components.MenuBar.Action) bool {
    switch (action) {
        .none => {},
        .new_session => {
            // Create new untitled session and start editing its name
            if (app.session_manager.createUntitledSession()) |new_idx| {
                if (app.session_manager.sessions.items[new_idx].name.len > 0) {
                    components.TabBar.startEditing(new_idx, app.session_manager.sessions.items[new_idx].name);
                }
            } else |_| {}
        },
        .open_file => {
            // Open native file picker dialog
            const default_dir = app.session_manager.getDefaultDocsDirectory();
            if (dvui.dialogNativeFileOpen(app.allocator, .{
                .title = "Open File",
                .path = default_dir,
                .filters = &[_][]const u8{"*.logos"},
            }) catch null) |selected_file| {
                defer app.allocator.free(selected_file);

                // Create new session from file
                const TabSession = @import("../../session/tab_session.zig").TabSession;
                if (TabSession.initFromFile(app.allocator, selected_file)) |sess| {
                    app.session_manager.sessions.append(app.allocator, sess) catch {};
                    app.session_manager.active_index = app.session_manager.sessions.items.len - 1;
                } else |_| {}
            }
        },
        .save => {
            if (app.session_manager.activeSession()) |active_session| {
                if (active_session.file_path != null) {
                    // Save to existing file
                    active_session.saveToFile() catch {};
                } else {
                    // No file path - show save dialog
                    const default_dir = app.session_manager.getDefaultDocsDirectory();
                    if (dvui.dialogNativeFileSave(app.allocator, .{
                        .title = "Save File",
                        .path = default_dir,
                        .filters = &[_][]const u8{"*.logos"},
                    }) catch null) |selected_path| {
                        defer app.allocator.free(selected_path);

                        // Set file path and save
                        if (app.allocator.dupe(u8, selected_path)) |owned_path| {
                            if (active_session.file_path) |old_path| {
                                app.allocator.free(old_path);
                            }
                            active_session.file_path = owned_path;

                            // Update tab name to match filename
                            const new_name = std.fs.path.basename(owned_path);
                            app.session_manager.renameSession(app.session_manager.active_index, new_name) catch {};

                            active_session.saveToFile() catch {};
                        } else |_| {}
                    }
                }
            }
        },
        .close_tab => {
            if (app.session_manager.activeSession() != null) {
                app.session_manager.closeSession(app.session_manager.active_index);
            }
        },
        .quit => {
            return false;
        },
        else => {},
    }
    return true;
}

fn handleTabAction(app: *App, action: components.TabBar.Action) void {
    switch (action) {
        .none => {},
        .add_tab => {
            // Create new untitled session and start editing its name
            if (app.session_manager.createUntitledSession()) |new_idx| {
                if (app.session_manager.sessions.items[new_idx].name.len > 0) {
                    components.TabBar.startEditing(new_idx, app.session_manager.sessions.items[new_idx].name);
                }
            } else |_| {}
        },
        .select_tab => |idx| {
            app.session_manager.setActive(idx);
        },
        .close_tab => |idx| {
            // Cancel any editing if we're closing the tab being edited
            if (components.TabBar.isEditing(idx)) {
                components.TabBar.cancelEditing();
            }
            // Auto-save before closing if it has a file path
            app.session_manager.autoSaveSession(idx) catch {};
            app.session_manager.closeSession(idx);
        },
        .start_edit => |idx| {
            // Start editing the tab name
            if (idx < app.session_manager.sessions.items.len) {
                const current_name = app.session_manager.sessions.items[idx].name;
                components.TabBar.startEditing(idx, current_name);
            }
        },
        .finish_edit => |edit_info| {
            // Finish editing - rename the tab
            if (edit_info.new_name.len > 0) {
                app.session_manager.renameSession(edit_info.index, edit_info.new_name) catch {};
            }
            components.TabBar.cancelEditing();
        },
        .cancel_edit => {
            components.TabBar.cancelEditing();
        },
        .change_folder => |idx| {
            // Open native folder picker dialog
            if (idx < app.session_manager.sessions.items.len) {
                const current_dir = if (app.session_manager.sessions.items[idx].file_path) |path|
                    std.fs.path.dirname(path)
                else
                    app.session_manager.getDefaultDocsDirectory();

                if (dvui.dialogNativeFolderSelect(app.allocator, .{
                    .title = "Select Folder for File",
                    .path = current_dir,
                }) catch null) |selected_dir| {
                    defer app.allocator.free(selected_dir);
                    app.session_manager.setSessionDirectory(idx, selected_dir) catch {};
                }
            }
        },
    }
}

fn renderEmptyState(app: *App) void {
    var center = dvui.box(@src(), .{ .dir = .vertical }, .{
        .expand = .both,
    });
    defer center.deinit();

    dvui.labelNoFmt(@src(), "No sessions open", .{}, .{
        .color_text = dvui.Color{ .r = 130, .g = 140, .b = 160, .a = 255 },
    });

    // Spacing
    {
        var spacer = dvui.box(@src(), .{}, .{ .min_size_content = .{ .h = 16 } });
        spacer.deinit();
    }

    if (dvui.button(@src(), "Create New Session", .{}, .{
        .padding = .{ .x = 16, .y = 8, .w = 16, .h = 8 },
        .corner_radius = dvui.Rect{ .x = 4, .y = 4, .w = 4, .h = 4 },
    })) {
        // Create new untitled session and start editing its name
        if (app.session_manager.createUntitledSession()) |new_idx| {
            if (app.session_manager.sessions.items[new_idx].name.len > 0) {
                components.TabBar.startEditing(new_idx, app.session_manager.sessions.items[new_idx].name);
            }
        } else |_| {}
    }
}
