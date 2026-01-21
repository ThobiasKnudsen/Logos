//! Single user session - represents one tab
//!
//! Contains the text content, parsed data, and render state
//! for one "document" the user is working on.

const std = @import("std");
const parse_state = @import("parse_state.zig");
const code_cell = @import("code_cell.zig");

pub const TabSession = struct {
    allocator: std.mem.Allocator,

    /// Display name for the tab
    name: []const u8,

    /// File path if saved, null for unsaved sessions
    file_path: ?[]const u8,

    /// The code cells in this notebook (replaces single content buffer)
    cells: std.ArrayList(code_cell.CodeCell),

    /// Index of the currently active/focused cell
    active_cell_index: usize,

    /// Counter for assigning unique IDs to new cells
    next_cell_id: usize,

    /// DEPRECATED: Legacy single content buffer for backward compatibility
    /// Used during migration from old single-text format to cell-based format
    legacy_content: ?std.ArrayList(u8),

    /// Has content been modified since last save?
    is_modified: bool,

    /// Parse state for lexing, parsing, and GLSL generation
    parse_state: parse_state.ParseState,

    /// Render state for the graph visualization
    render_state: RenderState,

    /// Editor cursor position (updated by EditorPanel)
    /// NOTE: This is now per-cell, but kept here for backward compatibility
    cursor_index: usize = 0,
    cursor_line: usize = 1,
    cursor_col: usize = 1,

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

        var cells: std.ArrayList(code_cell.CodeCell) = .{ .items = &.{}, .capacity = 0 };
        // Initialize with one empty cell
        const first_cell = code_cell.CodeCell.init(allocator, 0);
        try cells.append(allocator, first_cell);

        return .{
            .allocator = allocator,
            .name = owned_name,
            .file_path = null,
            .cells = cells,
            .active_cell_index = 0,
            .next_cell_id = 1,
            .legacy_content = null,
            .is_modified = false,
            .parse_state = parse_state.ParseState.init(allocator),
            .render_state = .{},
        };
    }

    /// Initialize a session from an existing file
    pub fn initFromFile(allocator: std.mem.Allocator, path: []const u8) !TabSession {
        const owned_path = try allocator.dupe(u8, path);
        errdefer allocator.free(owned_path);

        // Extract filename from path for tab name
        const name = std.fs.path.basename(path);
        const owned_name = try allocator.dupe(u8, name);
        errdefer allocator.free(owned_name);

        var cells: std.ArrayList(code_cell.CodeCell) = .{ .items = &.{}, .capacity = 0 };
        // Start with one empty cell (will be populated by loadFromFile)
        const first_cell = code_cell.CodeCell.init(allocator, 0);
        try cells.append(allocator, first_cell);

        var sess = TabSession{
            .allocator = allocator,
            .name = owned_name,
            .file_path = owned_path,
            .cells = cells,
            .active_cell_index = 0,
            .next_cell_id = 1,
            .legacy_content = null,
            .is_modified = false,
            .parse_state = parse_state.ParseState.init(allocator),
            .render_state = .{},
        };

        // Load file content
        try sess.loadFromFile();

        return sess;
    }

    /// Load content from the associated file path
    /// Detects JSON format (cells) vs plain text (legacy single cell)
    pub fn loadFromFile(self: *TabSession) !void {
        const path = self.file_path orelse return error.NoFilePath;

        const file = try std.fs.openFileAbsolute(path, .{});
        defer file.close();

        const stat = try file.stat();
        const size = stat.size;

        // Read file content into temp buffer
        const file_content = try self.allocator.alloc(u8, size);
        defer self.allocator.free(file_content);
        const bytes_read = try file.readAll(file_content);

        // Try to parse as JSON (new format)
        if (self.loadFromJson(file_content[0..bytes_read])) {
            // Success - loaded as JSON cells
            self.is_modified = false;
            self.render_state.needs_update = true;
            return;
        } else |_| {
            // Failed to parse as JSON - treat as legacy plain text
            // Clear cells and create single cell with content
            for (self.cells.items) |*cell| {
                cell.deinit(self.allocator);
            }
            self.cells.clearRetainingCapacity();

            var first_cell = code_cell.CodeCell.init(self.allocator, 0);
            try first_cell.content.appendSlice(self.allocator, file_content[0..bytes_read]);
            try self.cells.append(self.allocator, first_cell);
            self.next_cell_id = 1;
            self.active_cell_index = 0;

            self.is_modified = false;
            self.render_state.needs_update = true;
            self.parseContent();
        }
    }

    /// Load cells from JSON format
    fn loadFromJson(self: *TabSession, json_content: []const u8) !void {
        const parsed = try std.json.parseFromSlice(
            std.json.Value,
            self.allocator,
            json_content,
            .{},
        );
        defer parsed.deinit();

        const root = parsed.value;
        if (root != .array) {
            return error.InvalidJsonFormat;
        }

        // Clear existing cells
        for (self.cells.items) |*cell| {
            cell.deinit(self.allocator);
        }
        self.cells.clearRetainingCapacity();

        // Load each cell from JSON
        for (root.array.items, 0..) |cell_json, i| {
            if (cell_json != .object) continue;

            const text = if (cell_json.object.get("text")) |t| blk: {
                if (t == .string) break :blk t.string;
                break :blk "";
            } else "";

            var cell = try code_cell.CodeCell.initWithContent(self.allocator, i, text);

            // Load color if present
            if (cell_json.object.get("color")) |color_val| {
                if (color_val == .string) {
                    cell.setColorFromHex(color_val.string) catch {};
                }
            }

            try self.cells.append(self.allocator, cell);
        }

        // Ensure at least one cell exists
        if (self.cells.items.len == 0) {
            const first_cell = code_cell.CodeCell.init(self.allocator, 0);
            try self.cells.append(self.allocator, first_cell);
        }

        self.next_cell_id = self.cells.items.len;
        self.active_cell_index = 0;
    }

    /// Helper to append a JSON-escaped string
    fn appendJsonString(array: *std.ArrayList(u8), allocator: std.mem.Allocator, str: []const u8) !void {
        try array.append(allocator, '"');
        for (str) |c| {
            switch (c) {
                '"' => try array.appendSlice(allocator, "\\\""),
                '\\' => try array.appendSlice(allocator, "\\\\"),
                '\n' => try array.appendSlice(allocator, "\\n"),
                '\r' => try array.appendSlice(allocator, "\\r"),
                '\t' => try array.appendSlice(allocator, "\\t"),
                else => try array.append(allocator, c),
            }
        }
        try array.append(allocator, '"');
    }

    /// Save content to the associated file path
    /// Saves as JSON format with cell metadata
    pub fn saveToFile(self: *TabSession) !void {
        const path = self.file_path orelse return error.NoFilePath;

        // Ensure parent directory exists
        if (std.fs.path.dirname(path)) |dir| {
            std.fs.makeDirAbsolute(dir) catch |err| switch (err) {
                error.PathAlreadyExists => {},
                else => return err,
            };
        }

        const file = try std.fs.createFileAbsolute(path, .{});
        defer file.close();

        // Build JSON array of cells
        var json_array: std.ArrayList(u8) = .{ .items = &.{}, .capacity = 0 };
        defer json_array.deinit(self.allocator);

        try json_array.appendSlice(self.allocator, "[\n");

        for (self.cells.items, 0..) |*cell, i| {
            if (i > 0) try json_array.appendSlice(self.allocator, ",\n");

            try json_array.appendSlice(self.allocator, "  {\n");

            // Text field (escaped)
            try json_array.appendSlice(self.allocator, "    \"text\": ");
            try appendJsonString(&json_array, self.allocator, cell.content.items);

            // Color field
            try json_array.appendSlice(self.allocator, ",\n    \"color\": ");
            const color_hex = try cell.getColorHex(self.allocator);
            defer self.allocator.free(color_hex);
            try appendJsonString(&json_array, self.allocator, color_hex);

            // Last output (if any)
            if (cell.output) |output| {
                if (output.text) |text| {
                    try json_array.appendSlice(self.allocator, ",\n    \"last_output\": ");
                    try appendJsonString(&json_array, self.allocator, text);
                }
            }

            try json_array.appendSlice(self.allocator, "\n  }");
        }

        try json_array.appendSlice(self.allocator, "\n]\n");

        try file.writeAll(json_array.items);

        self.is_modified = false;
    }

    /// Check if this is an untitled session (no file path, name starts with "Untitled")
    pub fn isUntitled(self: *const TabSession) bool {
        return self.file_path == null and std.mem.startsWith(u8, self.name, "Untitled");
    }

    /// Update cursor position from byte index (for active cell)
    pub fn updateCursorPosition(self: *TabSession, cursor_idx: usize) void {
        self.cursor_index = cursor_idx;

        if (self.getActiveCell()) |cell| {
            cell.cursor_index = cursor_idx;

            // Calculate line and column from cursor index
            var line: usize = 1;
            var col: usize = 1;
            const text = cell.content.items;

            for (text[0..@min(cursor_idx, text.len)]) |c| {
                if (c == '\n') {
                    line += 1;
                    col = 1;
                } else {
                    col += 1;
                }
            }

            self.cursor_line = line;
            self.cursor_col = col;
        }
    }

    pub fn deinit(self: *TabSession) void {
        self.allocator.free(self.name);
        if (self.file_path) |path| {
            self.allocator.free(path);
        }
        // Clean up all cells
        for (self.cells.items) |*cell| {
            cell.deinit(self.allocator);
        }
        self.cells.deinit(self.allocator);
        if (self.legacy_content) |*content| {
            content.deinit(self.allocator);
        }
        self.parse_state.deinit();
    }

    /// Get the currently active cell
    pub fn getActiveCell(self: *TabSession) ?*code_cell.CodeCell {
        if (self.active_cell_index >= self.cells.items.len) {
            return null;
        }
        return &self.cells.items[self.active_cell_index];
    }

    /// Add a new empty cell at the end
    pub fn addCell(self: *TabSession) !*code_cell.CodeCell {
        const new_cell = code_cell.CodeCell.init(self.allocator, self.next_cell_id);
        self.next_cell_id += 1;
        try self.cells.append(self.allocator, new_cell);
        self.is_modified = true;
        return &self.cells.items[self.cells.items.len - 1];
    }

    /// Add a new cell after the specified index
    pub fn addCellAfter(self: *TabSession, index: usize) !*code_cell.CodeCell {
        const new_cell = code_cell.CodeCell.init(self.allocator, self.next_cell_id);
        self.next_cell_id += 1;
        try self.cells.insert(index + 1, new_cell);
        self.is_modified = true;
        return &self.cells.items[index + 1];
    }

    /// Remove a cell at the specified index (ensures at least one cell remains)
    pub fn removeCell(self: *TabSession, index: usize) !void {
        if (index >= self.cells.items.len) {
            return error.InvalidIndex;
        }

        var cell = self.cells.orderedRemove(index);
        cell.deinit(self.allocator);

        // Adjust active cell index if needed
        if (self.cells.items.len > 0 and self.active_cell_index >= self.cells.items.len) {
            self.active_cell_index = self.cells.items.len - 1;
        }

        self.is_modified = true;
    }

    /// Finalize a cell (mark as having output, read-only)
    pub fn finalizeCell(self: *TabSession, index: usize) void {
        if (index < self.cells.items.len) {
            self.cells.items[index].finalize();
            self.is_modified = true;
        }
    }

    /// Get content of all cells concatenated (for backward compatibility with parser)
    pub fn getAllCellsContent(self: *TabSession) ![]const u8 {
        var buffer: std.ArrayList(u8) = .{ .items = &.{}, .capacity = 0 };
        errdefer buffer.deinit(self.allocator);

        for (self.cells.items, 0..) |*cell, i| {
            if (i > 0) {
                try buffer.append(self.allocator, '\n'); // Separator between cells
            }
            try buffer.appendSlice(self.allocator, cell.content.items);
        }

        return buffer.toOwnedSlice(self.allocator);
    }

    /// Get content as a slice (DEPRECATED - for backward compatibility)
    /// Returns active cell content or all cells concatenated
    pub fn getContentSlice(self: *TabSession) []const u8 {
        if (self.getActiveCell()) |cell| {
            return cell.content.items;
        }
        return &.{};
    }

    /// Set content from a string (DEPRECATED - sets active cell content)
    pub fn setContent(self: *TabSession, new_content: []const u8) !void {
        if (self.getActiveCell()) |cell| {
            cell.content.clearRetainingCapacity();
            try cell.content.appendSlice(self.allocator, new_content);
            self.is_modified = true;
            self.render_state.needs_update = true;
            self.parseContent();
        }
    }

    /// Append to content (DEPRECATED - appends to active cell)
    pub fn appendContent(self: *TabSession, text: []const u8) !void {
        if (self.getActiveCell()) |cell| {
            try cell.content.appendSlice(self.allocator, text);
            self.is_modified = true;
            self.render_state.needs_update = true;
            self.parseContent();
        }
    }

    /// Parse the content into the structured representation
    /// Currently just updates the content hash - actual lexing/parsing is triggered separately
    fn parseContent(self: *TabSession) void {
        // For now, concatenate all cells to check if content changed
        // TODO: This will be replaced with per-cell parsing in Phase 2
        const content = self.getAllCellsContent() catch return;
        defer self.allocator.free(content);

        if (self.parse_state.shouldRelex(content)) {
            // Content changed - lexing will be needed on next debounce timeout
            // This is just a notification that content changed
            self.parse_state.status = .idle;
        }
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

    /// Set a new name for the tab
    pub fn setName(self: *TabSession, new_name: []const u8) !void {
        const owned_name = try self.allocator.dupe(u8, new_name);
        self.allocator.free(self.name);
        self.name = owned_name;
    }

    /// Set the file path (and optionally update name from filename)
    pub fn setFilePath(self: *TabSession, new_path: []const u8, update_name: bool) !void {
        const owned_path = try self.allocator.dupe(u8, new_path);

        if (self.file_path) |old| {
            self.allocator.free(old);
        }
        self.file_path = owned_path;

        if (update_name) {
            const filename = std.fs.path.basename(new_path);
            const owned_name = try self.allocator.dupe(u8, filename);
            self.allocator.free(self.name);
            self.name = owned_name;
        }
    }

    /// Set just the directory portion of the file path, keeping the filename
    pub fn setFileDirectory(self: *TabSession, new_dir: []const u8) !void {
        // Build new path: new_dir + "/" + current name
        const path_len = new_dir.len + 1 + self.name.len;
        const new_path = try self.allocator.alloc(u8, path_len);
        defer self.allocator.free(new_path);

        @memcpy(new_path[0..new_dir.len], new_dir);
        new_path[new_dir.len] = std.fs.path.sep;
        @memcpy(new_path[new_dir.len + 1 ..], self.name);

        // Now set the file path
        if (self.file_path) |old| {
            self.allocator.free(old);
        }
        self.file_path = try self.allocator.dupe(u8, new_path);
    }
};
