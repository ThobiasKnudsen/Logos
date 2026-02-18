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
const TabSession = @import("../../session/tab_session.zig").TabSession;

// Re-export theme for convenience
pub const ui_theme = theme;

/// Auto-save state
var last_auto_save_time: i64 = 0;
const auto_save_interval_ms: i64 = 2000; // Save every 2 seconds if modified

/// Persistent state for the split view
var split_view: components.SplitView = .{};

/// Non-blocking file dialog using zenity as a child process.
/// Spawns zenity via std.process.Child, polls with waitpid(WNOHANG) each frame,
/// and reads the result from stdout when the process exits.
const FileDialog = struct {
    pid: std.posix.pid_t,
    stdout_fd: std.posix.fd_t,
    kind: Kind,
    folder_tab_idx: usize,
    result_buf: [4096]u8 = undefined,

    const Kind = enum { open, save, folder_select };

    fn spawn(allocator: std.mem.Allocator, kind: Kind, path: ?[]const u8, folder_idx: usize) !FileDialog {
        // Build --filename= argument
        var filename_buf: [512]u8 = undefined;
        var filename_arg: ?[]const u8 = null;
        if (path) |p| {
            // Ensure path ends with / so zenity opens the directory
            if (p.len > 0 and p[p.len - 1] == '/') {
                filename_arg = std.fmt.bufPrint(&filename_buf, "--filename={s}", .{p}) catch null;
            } else {
                filename_arg = std.fmt.bufPrint(&filename_buf, "--filename={s}/", .{p}) catch null;
            }
        }

        // Build argv on the stack — all slices are string literals or point
        // into filename_buf which lives until after child.spawn().
        var argv_buf: [10][]const u8 = undefined;
        var argc: usize = 0;
        argv_buf[argc] = "zenity";
        argc += 1;
        argv_buf[argc] = "--file-selection";
        argc += 1;

        switch (kind) {
            .open => {
                argv_buf[argc] = "--title=Open File";
                argc += 1;
            },
            .save => {
                argv_buf[argc] = "--save";
                argc += 1;
                argv_buf[argc] = "--confirm-overwrite";
                argc += 1;
                argv_buf[argc] = "--title=Save File";
                argc += 1;
            },
            .folder_select => {
                argv_buf[argc] = "--directory";
                argc += 1;
                argv_buf[argc] = "--title=Select Folder";
                argc += 1;
            },
        }

        if (filename_arg) |fa| {
            argv_buf[argc] = fa;
            argc += 1;
        }

        if (kind == .open or kind == .save) {
            argv_buf[argc] = "--file-filter=Logos files | *.logos";
            argc += 1;
            argv_buf[argc] = "--file-filter=All files | *";
            argc += 1;
        }

        var child = std.process.Child.init(argv_buf[0..argc], allocator);
        child.stdout_behavior = .Pipe;
        child.stderr_behavior = .Ignore;
        try child.spawn();
        try child.waitForSpawn(); // consume err_pipe (non-blocking)

        const pid = child.id;
        const stdout_fd = child.stdout.?.handle;

        return .{
            .pid = pid,
            .stdout_fd = stdout_fd,
            .kind = kind,
            .folder_tab_idx = folder_idx,
        };
    }

    /// Non-blocking poll. Returns null if still running, empty string if
    /// cancelled/error, or the selected path (trimmed of trailing newline).
    fn poll(self: *FileDialog) ?[]const u8 {
        const w = std.posix.waitpid(self.pid, std.os.linux.W.NOHANG);
        if (w.pid == 0) return null; // still running

        // Child exited — read stdout for the selected path
        const file = std.fs.File{ .handle = self.stdout_fd };
        const n = file.read(&self.result_buf) catch 0;
        std.posix.close(self.stdout_fd);

        if (n == 0) return ""; // user cancelled (zenity exits 1 with no output)

        // Trim trailing newline
        const end = if (n > 0 and self.result_buf[n - 1] == '\n') n - 1 else n;
        return self.result_buf[0..end];
    }

    fn cleanup(self: *FileDialog) void {
        // Kill the child if still running and close the pipe
        _ = std.posix.kill(self.pid, std.posix.SIG.TERM) catch {};
        _ = std.posix.waitpid(self.pid, 0); // reap
        std.posix.close(self.stdout_fd);
    }
};

var pending_dialog: ?FileDialog = null;

/// Returns true if a dialog child process is running (for continuous redraw).
pub fn isDialogPending() bool {
    return pending_dialog != null;
}

/// Clean up module-level state (must be called on app shutdown)
pub fn deinit() void {
    if (pending_dialog) |*pd| {
        pd.cleanup();
        pending_dialog = null;
    }
    split_view.deinit();
}

/// Returns false if user wants to quit
pub fn mainView(app: *App) bool {
    // Poll for completed file dialog (non-blocking)
    pollDialogResult(app);

    // Root container - fills entire window
    var root = dvui.box(@src(), .{ .dir = .vertical }, .{
        .expand = .both,
        .color_fill = theme.colors.bg_primary,
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
                active_session.saveToFile() catch |err| {
                    std.log.err("Auto-save failed: {}", .{err});
                };
                last_auto_save_time = now;
            }
        }

        // Status bar at bottom
        components.StatusBar.render(active_session, &app.graph_renderer);
    } else {
        // No sessions - show empty state
        renderEmptyState(app);

        // Status bar at bottom (no active session)
        components.StatusBar.render(null, null);
    }

    // Check for keyboard shortcuts and window events
    var keyboard_action: components.MenuBar.Action = .none;
    for (dvui.events()) |*e| {
        if (e.evt == .key and e.evt.key.action == .down) {
            const ctrl_pressed = e.evt.key.mod.has(.lcontrol) or e.evt.key.mod.has(.rcontrol);
            const shift_pressed = e.evt.key.mod.has(.lshift) or e.evt.key.mod.has(.rshift);
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
                    // Zoom controls: Ctrl+= (US layout) and Ctrl+-
                    .equal => {
                        // Ctrl+= zooms in (same physical key as + on US keyboards)
                        keyboard_action = .zoom_in;
                        e.handled = true;
                    },
                    .kp_add => {
                        // Keypad + also zooms in
                        keyboard_action = .zoom_in;
                        e.handled = true;
                    },
                    .unknown => {
                        // Handle Norwegian/non-US keyboard + key (SDL keysym 43)
                        // This catches the + key on keyboards where it's separate from =
                        keyboard_action = .zoom_in;
                        e.handled = true;
                    },
                    .minus => {
                        // Only zoom out if Shift is NOT pressed (Shift+- would be '_')
                        if (!shift_pressed) {
                            keyboard_action = .zoom_out;
                            e.handled = true;
                        }
                    },
                    .kp_subtract => {
                        keyboard_action = .zoom_out;
                        e.handled = true;
                    },
                    .zero, .kp_0 => {
                        keyboard_action = .reset_zoom;
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
            if (pending_dialog == null) {
                const default_dir = app.session_manager.getDefaultDocsDirectory();
                pending_dialog = FileDialog.spawn(app.allocator, .open, default_dir, 0) catch |err| blk: {
                    std.log.err("Failed to spawn file dialog: {}", .{err});
                    break :blk null;
                };
            }
        },
        .save => {
            if (app.session_manager.activeSession()) |active_session| {
                if (active_session.file_path != null) {
                    active_session.saveToFile() catch |err| {
                        std.log.err("Failed to save file: {}", .{err});
                    };
                } else if (pending_dialog == null) {
                    const default_dir = app.session_manager.getDefaultDocsDirectory();
                    pending_dialog = FileDialog.spawn(app.allocator, .save, default_dir, 0) catch |err| blk: {
                        std.log.err("Failed to spawn save dialog: {}", .{err});
                        break :blk null;
                    };
                }
            }
        },
        .save_as => {
            if (pending_dialog == null) {
                const default_dir = app.session_manager.getDefaultDocsDirectory();
                pending_dialog = FileDialog.spawn(app.allocator, .save, default_dir, 0) catch |err| blk: {
                    std.log.err("Failed to spawn save dialog: {}", .{err});
                    break :blk null;
                };
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
        .zoom_in => {
            theme.fonts.zoomIn();
        },
        .zoom_out => {
            theme.fonts.zoomOut();
        },
        .reset_zoom => {
            theme.fonts.resetZoom();
        },
        .set_theme => |buf| {
            const len = std.mem.indexOfScalar(u8, &buf, 0) orelse buf.len;
            if (len > 0) {
                theme.syntax.setThemeByName(buf[0..len]);
            }
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
            } else |err| {
                std.log.err("Failed to create new session: {}", .{err});
            }
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
            app.session_manager.autoSaveSession(idx) catch |err| {
                std.log.err("Failed to auto-save before closing session: {}", .{err});
            };
            app.session_manager.closeSession(idx);
        },
        .start_edit => |idx| {
            // Start editing the tab name (strip .logos for editing)
            if (idx < app.session_manager.sessions.items.len) {
                const full_name = app.session_manager.sessions.items[idx].name;
                const display_name = if (std.mem.endsWith(u8, full_name, ".logos"))
                    full_name[0 .. full_name.len - 6]
                else
                    full_name;
                components.TabBar.startEditing(idx, display_name);
            }
        },
        .finish_edit => |edit_info| {
            // Finish editing - rename the tab
            if (edit_info.new_name.len > 0) {
                app.session_manager.renameSession(edit_info.index, edit_info.new_name) catch |err| {
                    std.log.err("Failed to rename session: {}", .{err});
                };
            }
            components.TabBar.cancelEditing();
        },
        .cancel_edit => {
            components.TabBar.cancelEditing();
        },
        .change_folder => |idx| {
            if (pending_dialog == null and idx < app.session_manager.sessions.items.len) {
                const current_dir = if (app.session_manager.sessions.items[idx].file_path) |path|
                    std.fs.path.dirname(path)
                else
                    app.session_manager.getDefaultDocsDirectory();
                pending_dialog = FileDialog.spawn(app.allocator, .folder_select, current_dir, idx) catch |err| {
                    std.log.err("Failed to spawn folder dialog: {}", .{err});
                    return;
                };
            }
        },
    }
}

/// Non-blocking poll: check if the zenity child process has exited.
/// If so, process its result (open file, save, or folder select).
fn pollDialogResult(app: *App) void {
    var pd = &(pending_dialog orelse return);
    const selected = pd.poll() orelse return; // still running

    const kind = pd.kind;
    const folder_idx = pd.folder_tab_idx;

    // Copy result before nullifying pending_dialog, because `selected`
    // points into pd.result_buf which lives inside the optional.
    var local_buf: [4096]u8 = undefined;
    const len = @min(selected.len, local_buf.len);
    @memcpy(local_buf[0..len], selected[0..len]);
    const result = local_buf[0..len];

    pending_dialog = null;

    if (result.len == 0) return; // user cancelled

    switch (kind) {
        .open => {
            if (TabSession.initFromFile(app.allocator, result)) |sess| {
                app.session_manager.sessions.append(app.allocator, sess) catch |err| {
                    std.log.err("Failed to add session: {}", .{err});
                    return;
                };
                app.session_manager.active_index = app.session_manager.sessions.items.len - 1;

                // Auto-interpret all cells in the opened file
                if (app.session_manager.activeSession()) |active| {
                    for (0..active.cells.items.len) |i| {
                        active.validateAndParseCell(i);
                        if (!active.cells.items[i].has_validation_error) {
                            active.cells.items[i].is_playing = true;
                        }
                    }
                }
            } else |err| {
                std.log.err("Failed to open file '{s}': {}", .{ result, err });
            }
        },
        .save => {
            const active_session = app.session_manager.activeSession() orelse return;

            // Ensure the path ends with .logos
            const has_ext = std.mem.endsWith(u8, result, ".logos");
            const owned_path = if (has_ext)
                app.allocator.dupe(u8, result) catch |err| {
                    std.log.err("Failed to duplicate path: {}", .{err});
                    return;
                }
            else
                std.fmt.allocPrint(app.allocator, "{s}.logos", .{result}) catch |err| {
                    std.log.err("Failed to allocate path: {}", .{err});
                    return;
                };

            if (active_session.file_path) |old_path| {
                app.allocator.free(old_path);
            }
            active_session.file_path = owned_path;

            const new_name = std.fs.path.basename(owned_path);
            app.session_manager.renameSession(app.session_manager.active_index, new_name) catch |err| {
                std.log.err("Failed to rename session: {}", .{err});
            };

            active_session.saveToFile() catch |err| {
                std.log.err("Failed to save file: {}", .{err});
            };
        },
        .folder_select => {
            if (folder_idx < app.session_manager.sessions.items.len) {
                app.session_manager.setSessionDirectory(folder_idx, result) catch |err| {
                    std.log.err("Failed to set session directory: {}", .{err});
                };
            }
        },
    }
}

fn renderEmptyState(app: *App) void {
    var center = dvui.box(@src(), .{ .dir = .vertical }, .{
        .expand = .both,
    });
    defer center.deinit();

    const ui_font = dvui.Font.theme(.body).withSize(theme.fonts.uiSize());

    dvui.labelNoFmt(@src(), "No sessions open", .{}, .{
        .color_text = theme.colors.text_muted,
        .font = ui_font,
    });

    // Spacing
    {
        var spacer = dvui.box(@src(), .{}, .{ .min_size_content = .{ .h = 16 } });
        spacer.deinit();
    }

    if (dvui.button(@src(), "Create New Session", .{}, .{
        .padding = .{ .x = 16, .y = 8, .w = 16, .h = 8 },
        .corner_radius = dvui.Rect{ .x = 4, .y = 4, .w = 4, .h = 4 },
        .font = ui_font,
    })) {
        // Create new untitled session and start editing its name
        if (app.session_manager.createUntitledSession()) |new_idx| {
            if (app.session_manager.sessions.items[new_idx].name.len > 0) {
                components.TabBar.startEditing(new_idx, app.session_manager.sessions.items[new_idx].name);
            }
        } else |_| {}
    }
}
