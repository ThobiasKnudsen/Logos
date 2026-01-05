//! Single user session - represents one tab
//!
//! Contains the text content, parsed data, and render state
//! for one "document" the user is working on.

const std = @import("std");

pub const TabSession = struct {
    allocator: std.mem.Allocator,

    /// Display name for the tab
    name: []const u8,

    /// File path if saved, null for unsaved sessions
    file_path: ?[]const u8,

    /// The raw text content the user has written
    content: std.ArrayList(u8),

    /// Has content been modified since last save?
    is_modified: bool,

    /// Parsed/analyzed representation of the content
    /// (placeholder for your AST/expression data)
    parsed_data: ?ParsedData,

    /// Render state for the graph visualization
    render_state: RenderState,

    /// Placeholder for parsed data - replace with your actual AST type
    pub const ParsedData = struct {
        // TODO: Replace with actual parsed representation
        // e.g., expressions, functions, data series, etc.
        is_valid: bool = false,
        error_message: ?[]const u8 = null,
    };

    /// State for the graph renderer
    pub const RenderState = struct {
        /// Does the graph need to be re-rendered?
        needs_update: bool = true,

        /// Zoom level
        zoom: f32 = 1.0,

        /// Pan offset
        pan_x: f32 = 0.0,
        pan_y: f32 = 0.0,

        /// Selected elements in the graph
        selection: ?usize = null,
    };

    pub fn init(allocator: std.mem.Allocator, name: []const u8) !TabSession {
        const owned_name = try allocator.dupe(u8, name);
        errdefer allocator.free(owned_name);

        return .{
            .allocator = allocator,
            .name = owned_name,
            .file_path = null,
            .content = .{ .items = &.{}, .capacity = 0 },
            .is_modified = false,
            .parsed_data = null,
            .render_state = .{},
        };
    }

    pub fn deinit(self: *TabSession) void {
        self.allocator.free(self.name);
        if (self.file_path) |path| {
            self.allocator.free(path);
        }
        self.content.deinit(self.allocator);
        if (self.parsed_data) |*data| {
            if (data.error_message) |msg| {
                self.allocator.free(msg);
            }
        }
    }

    /// Get content as a slice
    pub fn getContentSlice(self: *TabSession) []const u8 {
        return self.content.items;
    }

    /// Set content from a string
    pub fn setContent(self: *TabSession, new_content: []const u8) !void {
        self.content.clearRetainingCapacity();
        try self.content.appendSlice(self.allocator, new_content);
        self.is_modified = true;
        self.render_state.needs_update = true;

        // Re-parse content
        self.parseContent();
    }

    /// Append to content
    pub fn appendContent(self: *TabSession, text: []const u8) !void {
        try self.content.appendSlice(self.allocator, text);
        self.is_modified = true;
        self.render_state.needs_update = true;
        self.parseContent();
    }

    /// Parse the content into the structured representation
    fn parseContent(self: *TabSession) void {
        // TODO: Implement actual parsing using your AST module
        // For now, just mark as valid if non-empty
        self.parsed_data = .{
            .is_valid = self.content.items.len > 0,
            .error_message = null,
        };
    }

    /// Mark session as saved
    pub fn markSaved(self: *TabSession, path: ?[]const u8) !void {
        if (path) |p| {
            if (self.file_path) |old| {
                self.allocator.free(old);
            }
            self.file_path = try self.allocator.dupe(u8, p);
        }
        self.is_modified = false;
    }
};
