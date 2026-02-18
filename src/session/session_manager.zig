//! Manages all open user sessions (tabs)

const std = @import("std");
const TabSession = @import("tab_session.zig").TabSession;

pub const SessionManager = struct {
    allocator: std.mem.Allocator,

    /// All open sessions
    sessions: std.ArrayList(TabSession),

    /// Index of the currently active session
    active_index: usize,

    /// Counter for generating unique "Untitled N" names
    untitled_counter: usize,

    /// Default documents directory (near executable)
    default_docs_dir: ?[]const u8,

    pub fn init(allocator: std.mem.Allocator) SessionManager {
        // Get default documents directory near executable
        const docs_dir = getDefaultDocsDir(allocator) catch null;

        return .{
            .allocator = allocator,
            .sessions = .{ .items = &.{}, .capacity = 0 },
            .active_index = 0,
            .untitled_counter = 0,
            .default_docs_dir = docs_dir,
        };
    }

    /// Get the default documents directory near the executable
    fn getDefaultDocsDir(allocator: std.mem.Allocator) ![]const u8 {
        const exe_path = try std.fs.selfExePathAlloc(allocator);
        defer allocator.free(exe_path);

        const exe_dir = std.fs.path.dirname(exe_path) orelse return error.NoExeDir;

        // Join with "documents" subdirectory
        const docs_dir = try std.fs.path.join(allocator, &.{ exe_dir, "documents" });

        // Ensure the directory exists
        std.fs.makeDirAbsolute(docs_dir) catch |err| switch (err) {
            error.PathAlreadyExists => {},
            else => {
                allocator.free(docs_dir);
                return err;
            },
        };

        return docs_dir;
    }

    pub fn deinit(self: *SessionManager) void {
        for (self.sessions.items) |*sess| {
            sess.deinit();
        }
        self.sessions.deinit(self.allocator);
        if (self.default_docs_dir) |dir| {
            self.allocator.free(dir);
        }
        // Clean up the shared lexer to avoid memory leaks on shutdown
        TabSession.deinitSharedLexer();
    }

    /// Generate a unique "Untitled N" name
    pub fn generateUntitledName(self: *SessionManager, buf: []u8) []const u8 {
        while (true) {
            self.untitled_counter += 1;
            const name = std.fmt.bufPrint(buf, "Untitled {d}", .{self.untitled_counter}) catch "Untitled";

            // Check if this name already exists
            var exists = false;
            for (self.sessions.items) |*sess| {
                if (std.mem.eql(u8, sess.name, name)) {
                    exists = true;
                    break;
                }
            }

            if (!exists) {
                return name;
            }
            // Name exists, try next number
        }
    }

    /// Check if a name already exists among sessions
    pub fn nameExists(self: *SessionManager, name: []const u8) bool {
        for (self.sessions.items) |*sess| {
            if (std.mem.eql(u8, sess.name, name)) {
                return true;
            }
        }
        return false;
    }

    /// Create a new session with the given name
    pub fn createSession(self: *SessionManager, name: []const u8) !void {
        var sess = try TabSession.init(self.allocator, name);
        errdefer sess.deinit();

        try self.sessions.append(self.allocator, sess);
        self.active_index = self.sessions.items.len - 1;
    }

    /// Create a new session with a unique "Untitled N" name
    /// Returns the index of the new session
    pub fn createUntitledSession(self: *SessionManager) !usize {
        var name_buf: [32]u8 = undefined;
        const name = self.generateUntitledName(&name_buf);
        try self.createSession(name);
        return self.active_index;
    }

    /// Get the currently active session
    pub fn activeSession(self: *SessionManager) ?*TabSession {
        if (self.sessions.items.len == 0) return null;
        if (self.active_index >= self.sessions.items.len) {
            self.active_index = self.sessions.items.len - 1;
        }
        return &self.sessions.items[self.active_index];
    }

    /// Set the active session by index
    pub fn setActive(self: *SessionManager, index: usize) void {
        if (index < self.sessions.items.len) {
            self.active_index = index;
        }
    }

    /// Close a session by index
    pub fn closeSession(self: *SessionManager, index: usize) void {
        if (index >= self.sessions.items.len) return;

        var sess = self.sessions.orderedRemove(index);
        sess.deinit();

        // Adjust active index if needed
        if (self.sessions.items.len == 0) {
            self.active_index = 0;
        } else if (self.active_index >= self.sessions.items.len) {
            self.active_index = self.sessions.items.len - 1;
        }
    }

    /// Rename a session by index - also sets up file path for non-Untitled names
    pub fn renameSession(self: *SessionManager, index: usize, new_name: []const u8) !void {
        if (index >= self.sessions.items.len) return;

        var sess = &self.sessions.items[index];

        // Ensure non-Untitled names end with .logos
        if (!std.mem.startsWith(u8, new_name, "Untitled") and !std.mem.endsWith(u8, new_name, ".logos")) {
            const with_ext = try std.fmt.allocPrint(sess.allocator, "{s}.logos", .{new_name});
            defer sess.allocator.free(with_ext);
            try sess.setName(with_ext);
        } else {
            try sess.setName(new_name);
        }

        // If renamed from Untitled to a real name, set up file path in default docs dir
        if (!std.mem.startsWith(u8, new_name, "Untitled") and sess.file_path == null) {
            if (self.default_docs_dir) |docs_dir| {
                try sess.setFileDirectory(docs_dir);
            }
        }
    }

    /// Change the directory for a session's file path
    pub fn setSessionDirectory(self: *SessionManager, index: usize, new_dir: []const u8) !void {
        if (index >= self.sessions.items.len) return;
        try self.sessions.items[index].setFileDirectory(new_dir);
    }

    /// Auto-save the active session if it has a file path and is modified
    pub fn autoSaveActive(self: *SessionManager) !void {
        if (self.activeSession()) |sess| {
            if (sess.file_path != null and sess.is_modified) {
                try sess.saveToFile();
            }
        }
    }

    /// Auto-save a specific session by index if it has a file path and is modified
    pub fn autoSaveSession(self: *SessionManager, index: usize) !void {
        if (index >= self.sessions.items.len) return;
        var sess = &self.sessions.items[index];
        if (sess.file_path != null and sess.is_modified) {
            try sess.saveToFile();
        }
    }

    /// Get session count
    pub fn count(self: *const SessionManager) usize {
        return self.sessions.items.len;
    }

    /// Get the default documents directory
    pub fn getDefaultDocsDirectory(self: *const SessionManager) ?[]const u8 {
        return self.default_docs_dir;
    }
};
