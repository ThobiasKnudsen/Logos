//! Single user session - represents one tab
//!
//! Contains the text content, parsed data, and render state
//! for one "document" the user is working on.

const std = @import("std");
const parse_state = @import("parse_state.zig");
const code_cell = @import("code_cell.zig");
const lexer_mod = @import("../lang/lexer.zig");
const parser_mod = @import("../lang/parser.zig");
const glsl_gen = @import("../lang/glsl_gen.zig");

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

            const text = if (cell_json.object.get("code")) |t| blk: {
                if (t == .string) break :blk t.string;
                break :blk "";
            } else if (cell_json.object.get("text")) |t| blk: {
                // Backward compat with old format
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

            // Code field (escaped)
            try json_array.appendSlice(self.allocator, "    \"code\": ");
            try appendJsonString(&json_array, self.allocator, cell.content.items);

            // Color field
            try json_array.appendSlice(self.allocator, ",\n    \"color\": ");
            const color_hex = try cell.getColorHex(self.allocator);
            defer self.allocator.free(color_hex);
            try appendJsonString(&json_array, self.allocator, color_hex);

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

    /// Shared lexer instance (initialized on first use)
    var shared_lexer: ?lexer_mod.Lexer = null;

    /// Ensure lexer is initialized and return a pointer to it
    fn ensureLexer(allocator: std.mem.Allocator) !*lexer_mod.Lexer {
        if (shared_lexer == null) {
            shared_lexer = try lexer_mod.Lexer.init(allocator, lexer_mod.LexerConfig.logosPatterns());
        }
        return &shared_lexer.?;
    }

    /// Deinitialize the shared lexer to free all associated memory.
    /// Should be called during application shutdown (e.g., from SessionManager.deinit).
    pub fn deinitSharedLexer() void {
        if (shared_lexer) |*lex| {
            lex.deinit();
            shared_lexer = null;
        }
    }

    /// Get content of cells 0..cell_index concatenated (context for parsing cell N)
    pub fn getAllCellsContentUpTo(self: *TabSession, cell_index: usize) ![]const u8 {
        var buffer: std.ArrayList(u8) = .{ .items = &.{}, .capacity = 0 };
        errdefer buffer.deinit(self.allocator);

        const end = @min(cell_index, self.cells.items.len);
        for (self.cells.items[0..end], 0..) |*cell, i| {
            if (i > 0) {
                try buffer.append(self.allocator, '\n');
            }
            try buffer.appendSlice(self.allocator, cell.content.items);
        }

        return buffer.toOwnedSlice(self.allocator);
    }

    /// Get content of cells 0..cell_index+1 concatenated (context + cell itself)
    pub fn getAllCellsContentIncluding(self: *TabSession, cell_index: usize) ![]const u8 {
        var buffer: std.ArrayList(u8) = .{ .items = &.{}, .capacity = 0 };
        errdefer buffer.deinit(self.allocator);

        const end = @min(cell_index + 1, self.cells.items.len);
        for (self.cells.items[0..end], 0..) |*cell, i| {
            if (i > 0) {
                try buffer.append(self.allocator, '\n');
            }
            try buffer.appendSlice(self.allocator, cell.content.items);
        }

        return buffer.toOwnedSlice(self.allocator);
    }

    /// Validate and parse a single cell with context from all previous cells.
    /// Sets the cell's validation_error and has_validation_error fields.
    /// On success, generates a shader if the cell has root-scope output.
    pub fn validateAndParseCell(self: *TabSession, cell_index: usize) void {
        if (cell_index >= self.cells.items.len) return;

        var cell = &self.cells.items[cell_index];

        // Clear previous validation state
        cell.has_validation_error = false;
        if (cell.validation_error) |err| {
            self.allocator.free(err);
            cell.validation_error = null;
        }

        // Clear previous output
        if (cell.output) |*out| {
            out.deinit(self.allocator);
            cell.output = null;
        }

        const cell_content = cell.content.items;

        // Skip empty cells
        if (std.mem.trim(u8, cell_content, " \t\n\r").len == 0) {
            return;
        }

        // Get all content up to and including this cell for parsing
        const full_content = self.getAllCellsContentIncluding(cell_index) catch |err| {
            cell.has_validation_error = true;
            cell.validation_error = std.fmt.allocPrint(self.allocator, "Failed to get cell content: {}", .{err}) catch null;
            return;
        };
        defer self.allocator.free(full_content);

        // Get lexer
        const lex = ensureLexer(self.allocator) catch |err| {
            cell.has_validation_error = true;
            cell.validation_error = std.fmt.allocPrint(self.allocator, "Lexer init failed: {}", .{err}) catch null;
            return;
        };

        // Tokenize
        const tokens = lex.tokenize(full_content) catch |err| {
            cell.has_validation_error = true;
            cell.validation_error = std.fmt.allocPrint(self.allocator, "Lexer error: {}", .{err}) catch null;
            return;
        };
        defer lex.allocator.free(tokens);

        if (tokens.len == 0) {
            return;
        }

        // Parse
        var parser = parser_mod.Parser.init(self.allocator, tokens);
        defer parser.deinit();

        const ast = parser.parse() catch |err| {
            cell.has_validation_error = true;

            if (parser.errors.items.len > 0) {
                const first_err = parser.errors.items[0];
                const loc = parse_state.byteOffsetToLocation(full_content, first_err.byte_start);
                cell.validation_error = std.fmt.allocPrint(
                    self.allocator,
                    "{s} (Ln {d}, Col {d})",
                    .{ first_err.message, loc.line, loc.column },
                ) catch std.fmt.allocPrint(self.allocator, "Parse error: {}", .{err}) catch null;
            } else {
                cell.validation_error = std.fmt.allocPrint(self.allocator, "Parse error: {}", .{err}) catch null;
            }
            return;
        };
        defer ast.deinit(self.allocator);

        // Generate GLSL shaders
        var generator = glsl_gen.GlslGenerator.init(self.allocator);
        defer generator.deinit();

        const result = generator.generate(ast) catch |err| {
            cell.has_validation_error = true;
            cell.validation_error = std.fmt.allocPrint(self.allocator, "GLSL generation failed: {}", .{err}) catch null;
            return;
        };
        defer {
            // Free the errors array
            self.allocator.free(result.errors);
        }

        if (result.shaders.len == 0) {
            // No root outputs in this cell - that's OK, it just defines functions/variables
            // Free the empty shaders slice
            self.allocator.free(result.shaders);
            return;
        }

        // Calculate the byte offset where this cell's content starts in full_content.
        // getAllCellsContentIncluding concatenates cells with '\n' separators.
        var current_cell_byte_start: usize = 0;
        for (self.cells.items[0..cell_index]) |c| {
            current_cell_byte_start += c.content.items.len + 1; // +1 for '\n' separator
        }

        // Find the last shader whose source expression falls within this cell's byte range.
        // Shaders from previous cells have source_start < current_cell_byte_start.
        var this_cell_shader_idx: ?usize = null;
        for (result.shaders, 0..) |shader, i| {
            if (shader.source_start >= current_cell_byte_start) {
                this_cell_shader_idx = i;
            }
        }

        if (this_cell_shader_idx == null) {
            // This cell only defines functions/bindings, no own output
            for (result.shaders) |shader| {
                self.allocator.free(shader.source);
            }
            self.allocator.free(result.shaders);
            return;
        }

        const last_shader = result.shaders[this_cell_shader_idx.?];

        // Store output in cell
        cell.output = .{
            .text = null,
            .shader = .{
                .source = self.allocator.dupe(u8, last_shader.source) catch {
                    cell.has_validation_error = true;
                    cell.validation_error = std.fmt.allocPrint(self.allocator, "Failed to copy shader source", .{}) catch null;
                    // Free all shader sources
                    for (result.shaders) |shader| {
                        self.allocator.free(shader.source);
                    }
                    self.allocator.free(result.shaders);
                    return;
                },
                .output_type = last_shader.output_type,
                .color_seed = last_shader.color_seed,
                .index = last_shader.index,
                .source_start = last_shader.source_start,
            },
            .output_type = .plot_only,
            .error_msg = null,
        };

        // Free all shader sources from the result (we duped the one we need)
        for (result.shaders) |shader| {
            self.allocator.free(shader.source);
        }
        self.allocator.free(result.shaders);

        std.log.info("Cell {d} validated and parsed successfully, shader generated", .{cell_index});
    }

    /// Find cells that depend on the given cell.
    /// A cell B depends on cell A if B comes after A and B's content references
    /// variables/functions that could be defined in A.
    /// For simplicity, we consider all playing cells after cell_index as potentially dependent.
    pub fn findDependentCells(self: *TabSession, cell_index: usize) []usize {
        // Simple dependency: all cells after cell_index that are playing
        var dependents: std.ArrayList(usize) = .{ .items = &.{}, .capacity = 0 };

        for (cell_index + 1..self.cells.items.len) |i| {
            if (self.cells.items[i].is_playing) {
                dependents.append(self.allocator, i) catch continue;
            }
        }

        return dependents.toOwnedSlice(self.allocator) catch &.{};
    }

    /// Replay all dependent cells that are currently playing.
    /// Called when a cell is updated that other cells might depend on.
    pub fn replayDependentCells(self: *TabSession, cell_index: usize) void {
        const dependents = self.findDependentCells(cell_index);
        defer self.allocator.free(dependents);

        for (dependents) |dep_idx| {
            // Bounds check: cell might have been removed or index is stale
            if (dep_idx >= self.cells.items.len) {
                std.log.warn("Skipping replay of out-of-bounds cell {d} (len={d})", .{ dep_idx, self.cells.items.len });
                continue;
            }
            std.log.info("Replaying dependent cell {d}", .{dep_idx});
            self.validateAndParseCell(dep_idx);
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
