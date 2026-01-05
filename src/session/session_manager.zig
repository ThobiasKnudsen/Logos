//! Manages all open user sessions (tabs)

const std = @import("std");
const TabSession = @import("tab_session.zig").TabSession;

pub const SessionManager = struct {
    allocator: std.mem.Allocator,

    /// All open sessions
    sessions: std.ArrayList(TabSession),

    /// Index of the currently active session
    active_index: usize,

    pub fn init(allocator: std.mem.Allocator) SessionManager {
        return .{
            .allocator = allocator,
            .sessions = .{ .items = &.{}, .capacity = 0 },
            .active_index = 0,
        };
    }

    pub fn deinit(self: *SessionManager) void {
        for (self.sessions.items) |*sess| {
            sess.deinit();
        }
        self.sessions.deinit(self.allocator);
    }

    /// Create a new session with the given name
    pub fn createSession(self: *SessionManager, name: []const u8) !void {
        var session = try TabSession.init(self.allocator, name);
        errdefer session.deinit();

        try self.sessions.append(self.allocator, session);
        self.active_index = self.sessions.items.len - 1;
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

        var session = self.sessions.orderedRemove(index);
        session.deinit();

        // Adjust active index if needed
        if (self.sessions.items.len == 0) {
            self.active_index = 0;
        } else if (self.active_index >= self.sessions.items.len) {
            self.active_index = self.sessions.items.len - 1;
        }
    }

    /// Get session count
    pub fn count(self: *const SessionManager) usize {
        return self.sessions.items.len;
    }
};
