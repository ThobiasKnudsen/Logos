const std = @import("std");
const pcrez = @import("pcrez");
const regex_splitting = @import("regex_splitting.zig");

// PCRE2 C bindings for JIT compilation
const pcre2_c = @cImport({
    @cDefine("PCRE2_CODE_UNIT_WIDTH", "8");
    @cInclude("pcre2.h");
});

// Error types
pub const RegexTrieError = error{
    AllocationFailure,
    NullArgument,
    NodeNotFound,
    NodeFound,
    DestroyFailed,
    DuplicateLeafValue,
};

/// Internal value storage - not exposed to users
const RegexTrieValue = struct {
    regex_key: []const u8,
    /// Generic value pointer - cast to your type when retrieving
    /// Caller is responsible for freeing this when removing patterns
    data: ?*anyopaque,
    allocator: std.mem.Allocator,
    /// Flag to prevent double-free when multiple nodes share same value
    freed: bool,

    fn deinit(self: *RegexTrieValue) void {
        if (!self.freed) {
            self.allocator.free(self.regex_key);
            self.freed = true;
        }
    }

    fn destroy(self: *RegexTrieValue) void {
        if (!self.freed) {
            self.deinit();
            self.allocator.destroy(self);
        }
    }

    fn create(allocator: std.mem.Allocator, regex_key: []const u8, data: ?*anyopaque) !*RegexTrieValue {
        const value = try allocator.create(RegexTrieValue);
        value.regex_key = try allocator.dupe(u8, regex_key);
        value.data = data;
        value.allocator = allocator;
        value.freed = false;
        return value;
    }
};

/// Result from get() and getAllMatches()
pub const MatchResult = struct {
    /// Number of bytes matched
    matched: usize,
    /// The regex pattern key that matched
    regex_key: []const u8,
    /// User data pointer associated with the pattern (cast to your type)
    data: ?*anyopaque,
};

const RegexEntry = struct {
    node: *RegexTrie,
    pattern: []const u8,
    allocator: std.mem.Allocator,

    pub fn deinit(self: *RegexEntry) void {
        self.allocator.free(self.pattern);
    }
};

pub const RegexTrie = struct {
    child_indices: [256]u8,
    children: std.ArrayList(?*RegexTrie),
    leaf_value: ?*RegexTrieValue,
    compiled_regex: ?pcrez.Regex,
    match_data: ?*pcre2_c.pcre2_match_data_8, // Reusable match data (like C++ version)
    regexes: std.ArrayList(RegexEntry),
    matcher_updated: bool,
    allocator: std.mem.Allocator,

    pub fn init(allocator: std.mem.Allocator) !*RegexTrie {
        const self = try allocator.create(RegexTrie);
        self.* = .{
            .child_indices = [_]u8{255} ** 256,
            .children = undefined,
            .leaf_value = null,
            .compiled_regex = null,
            .match_data = null,
            .regexes = undefined,
            .matcher_updated = true,
            .allocator = allocator,
        };
        // Pre-allocate capacity to reduce reallocations during insertion
        self.children = try std.ArrayList(?*RegexTrie).initCapacity(allocator, 8);
        self.regexes = try std.ArrayList(RegexEntry).initCapacity(allocator, 0);
        return self;
    }

    pub fn deinit(self: *RegexTrie) void {
        const allocator = self.allocator;
        deinitInternal(self);
        allocator.destroy(self);
    }

    fn deinitInternal(self: *RegexTrie) void {
        // Clean up children
        for (self.children.items) |maybe_child| {
            if (maybe_child) |child| {
                child.deinitInternal();
                self.allocator.destroy(child);
            }
        }

        // Clean up regex children
        for (self.regexes.items) |*entry| {
            entry.node.deinitInternal();
            self.allocator.destroy(entry.node);
            entry.deinit();
        }
        self.regexes.deinit(self.allocator);

        // Clean up PCRE2 resources - free match_data first, then code
        if (self.match_data) |md| {
            pcre2_c.pcre2_match_data_free_8(md);
        }
        if (self.compiled_regex) |*regex| {
            regex.deinit();
        }

        // Clean up leaf value (freed flag prevents double-free for shared values)
        if (self.leaf_value) |val| {
            val.destroy();
        }

        self.children.deinit(self.allocator);
    }

    fn checkEow(self: *const RegexTrie) bool {
        const idx = self.child_indices[0];
        if (idx != 255) {
            return self.children.items[idx] == null; // Sentinel has null value
        }
        return false;
    }

    fn hasChildren(self: *const RegexTrie) bool {
        for (self.child_indices, 0..) |idx, key| {
            if (key != 0 and idx != 255) {
                if (self.children.items[idx] != null) {
                    return true;
                }
            }
        }
        return self.regexes.items.len > 0;
    }

    const ChainInfo = struct {
        label: []const u8,
        target_node: *RegexTrie,
        allocator: std.mem.Allocator,

        pub fn deinit(self: *ChainInfo) void {
            self.allocator.free(self.label);
        }
    };

    fn getLiteralChain(allocator: std.mem.Allocator, node: *RegexTrie) !ChainInfo {
        var label = try std.ArrayList(u8).initCapacity(allocator, 0);
        errdefer label.deinit(allocator);
        var curr = node;
        while (true) {
            var keys = try std.ArrayList(u8).initCapacity(allocator, 0);
            defer keys.deinit(allocator);
            for (curr.child_indices, 0..) |idx, key| {
                if (key != 0 and idx != 255) {
                    if (curr.children.items[idx] != null) {
                        try keys.append(allocator, @as(u8, @intCast(key)));
                    }
                }
            }
            if (keys.items.len != 1) break;
            if (curr.regexes.items.len > 0) break;
            const c = keys.items[0];
            try label.append(allocator, c);
            const idx = curr.child_indices[c];
            if (idx == 255) break;
            if (curr.children.items[idx]) |child| {
                curr = child;
            } else break;
        }
        return ChainInfo{
            .label = try label.toOwnedSlice(allocator),
            .target_node = curr,
            .allocator = allocator,
        };
    }

    fn updateMatcher(self: *RegexTrie) RegexTrieError!void {
        std.debug.assert(!self.matcher_updated);

        // Free existing match_data and compiled_regex
        if (self.match_data) |md| {
            pcre2_c.pcre2_match_data_free_8(md);
            self.match_data = null;
        }
        if (self.compiled_regex) |*regex| {
            regex.deinit();
            self.compiled_regex = null;
        }

        if (self.regexes.items.len == 0) {
            // Compile never-matching pattern
            const pattern = "^(?!)";
            var regex = pcrez.Regex.init(self.allocator);
            const compile_result = regex.compile(pattern, true);
            if (compile_result != pcrez.Regex.CompileError.Compiled) {
                std.log.err("PCRE2 compile fail\n", .{});
                return RegexTrieError.AllocationFailure;
            }
            // Enable JIT compilation for speed (like C++ version)
            var jit_available: c_uint = undefined;
            _ = pcre2_c.pcre2_config_8(pcre2_c.PCRE2_CONFIG_JIT, &jit_available);
            if (jit_available != 0) {
                if (regex.code) |code_ptr| {
                    const jit_rc = pcre2_c.pcre2_jit_compile_8(@as(*pcre2_c.pcre2_code_8, @ptrCast(code_ptr)), 0);
                    if (jit_rc < 0 and jit_rc != pcre2_c.PCRE2_ERROR_JIT_UNSUPPORTED) {
                        std.log.warn("PCRE2 JIT compile failed: {d}, continuing without JIT\n", .{jit_rc});
                    }
                }
            }
            self.compiled_regex = regex;
            // Create reusable match data (like C++ version - key for performance)
            if (regex.code) |code_ptr| {
                self.match_data = pcre2_c.pcre2_match_data_create_from_pattern_8(
                    @as(*pcre2_c.pcre2_code_8, @ptrCast(code_ptr)),
                    null,
                );
            }
        } else {
            // Build combined pattern with proper anchoring
            // The pattern is: ^(?:(pattern1)|(pattern2)|...)
            // The outer non-capturing group ensures ^ anchors all alternatives
            var pattern_buf = std.ArrayList(u8).initCapacity(self.allocator, 0) catch return RegexTrieError.AllocationFailure;
            defer pattern_buf.deinit(self.allocator);
            pattern_buf.appendSlice(self.allocator, "^(?:") catch return RegexTrieError.AllocationFailure;
            for (self.regexes.items, 0..) |entry, i| {
                if (i > 0) {
                    pattern_buf.append(self.allocator, '|') catch return RegexTrieError.AllocationFailure;
                }
                pattern_buf.append(self.allocator, '(') catch return RegexTrieError.AllocationFailure;
                pattern_buf.appendSlice(self.allocator, entry.pattern) catch return RegexTrieError.AllocationFailure;
                pattern_buf.append(self.allocator, ')') catch return RegexTrieError.AllocationFailure;
            }
            pattern_buf.append(self.allocator, ')') catch return RegexTrieError.AllocationFailure;
            const pattern = pattern_buf.toOwnedSlice(self.allocator) catch return RegexTrieError.AllocationFailure;
            defer self.allocator.free(pattern);

            var regex = pcrez.Regex.init(self.allocator);
            const compile_result = regex.compile(pattern, true);
            if (compile_result != pcrez.Regex.CompileError.Compiled) {
                std.log.err("PCRE2 compile fail\n", .{});
                return RegexTrieError.AllocationFailure;
            }
            // Enable JIT compilation for speed (like C++ version)
            var jit_available: c_uint = undefined;
            _ = pcre2_c.pcre2_config_8(pcre2_c.PCRE2_CONFIG_JIT, &jit_available);
            if (jit_available != 0) {
                if (regex.code) |code_ptr| {
                    const jit_rc = pcre2_c.pcre2_jit_compile_8(@as(*pcre2_c.pcre2_code_8, @ptrCast(code_ptr)), 0);
                    if (jit_rc < 0 and jit_rc != pcre2_c.PCRE2_ERROR_JIT_UNSUPPORTED) {
                        std.log.warn("PCRE2 JIT compile failed: {d}, continuing without JIT\n", .{jit_rc});
                    }
                }
            }
            self.compiled_regex = regex;
            // Create reusable match data (like C++ version - key for performance)
            if (regex.code) |code_ptr| {
                self.match_data = pcre2_c.pcre2_match_data_create_from_pattern_8(
                    @as(*pcre2_c.pcre2_code_8, @ptrCast(code_ptr)),
                    null,
                );
            }
        }

        self.matcher_updated = true;
    }

    // Fast path for inserting pure literal strings (completely bypasses regex splitting)
    fn insertLiteralFast(self: *RegexTrie, str: []const u8, key_value: *RegexTrieValue) RegexTrieError!void {
        var current = self;

        // Traverse/create path for each character
        for (str) |c_byte| {
            const c: u8 = c_byte;
            const idx = current.child_indices[c];
            if (idx != 255) {
                if (current.children.items[idx]) |child| {
                    current = child;
                } else {
                    return RegexTrieError.NullArgument;
                }
            } else {
                // Create new child
                const new_trie = init(self.allocator) catch return RegexTrieError.AllocationFailure;
                errdefer new_trie.deinit();

                // Ensure capacity before appending to reduce reallocations
                current.children.ensureTotalCapacity(self.allocator, current.children.items.len + 1) catch return RegexTrieError.AllocationFailure;

                const child_idx = @as(u8, @intCast(current.children.items.len));
                current.children.appendAssumeCapacity(new_trie);
                current.child_indices[c] = child_idx;
                current = new_trie;
            }
        }

        // Mark end of word
        const eow_idx = current.child_indices[0];
        if (eow_idx == 255) {
            // Ensure capacity before appending
            current.children.ensureTotalCapacity(self.allocator, current.children.items.len + 1) catch return RegexTrieError.AllocationFailure;
            const child_idx = @as(u8, @intCast(current.children.items.len));
            current.children.appendAssumeCapacity(null);
            current.child_indices[0] = child_idx;
            // New EOW marker - check for safety (shouldn't happen, but be safe)
            if (current.leaf_value != null) {
                return RegexTrieError.DuplicateLeafValue;
            }
            current.leaf_value = key_value;
        } else {
            // EOW marker already exists, check if leaf value already exists
            if (current.leaf_value != null) {
                return RegexTrieError.DuplicateLeafValue;
            }
            // Assign leaf value
            current.leaf_value = key_value;
        }
    }

    /// Insert a pattern into the trie with associated data
    /// allocator: Used to allocate internal storage for the key
    /// key: The regex pattern or literal string to insert
    /// data: Optional user data pointer associated with the pattern
    pub fn insert(self: *RegexTrie, allocator: std.mem.Allocator, key: []const u8, data: ?*anyopaque) RegexTrieError!void {
        std.debug.assert(key.len > 0);

        // Create the internal value
        const key_value = RegexTrieValue.create(allocator, key, data) catch return RegexTrieError.AllocationFailure;
        errdefer {
            key_value.deinit();
            allocator.destroy(key_value);
        }

        const str = key;

        // Fast path: if it's a pure literal, completely bypass regex splitting
        // This avoids all Segment/path allocations and iterations
        if (regex_splitting.isPureLiteral(str)) {
            return self.insertLiteralFast(str, key_value);
        }

        var paths = regex_splitting.regexSplitting(self.allocator, str) catch return RegexTrieError.AllocationFailure;
        defer {
            for (paths.items) |*path| {
                for (path.items) |*seg| seg.deinit();
                path.deinit(self.allocator);
            }
            paths.deinit(self.allocator);
        }

        // Phase 1: Navigate all paths, create nodes, check for duplicates BEFORE any assignment
        // This prevents the bug where path 1 assigns the value but path 2 hits a duplicate
        var target_nodes = std.ArrayList(*RegexTrie).initCapacity(self.allocator, paths.items.len) catch return RegexTrieError.AllocationFailure;
        defer target_nodes.deinit(self.allocator);

        for (paths.items) |path| {
            var current = self;
            for (path.items) |seg| {
                if (seg.is_lit) {
                    for (seg.str) |c_byte| {
                        const c: u8 = c_byte;
                        const idx = current.child_indices[c];
                        if (idx != 255) {
                            if (current.children.items[idx]) |child| {
                                current = child;
                            } else {
                                return RegexTrieError.NullArgument;
                            }
                        } else {
                            // Create new child
                            const new_trie = init(self.allocator) catch return RegexTrieError.AllocationFailure;
                            errdefer new_trie.deinit();

                            // Ensure capacity before appending to reduce reallocations
                            current.children.ensureTotalCapacity(self.allocator, current.children.items.len + 1) catch return RegexTrieError.AllocationFailure;

                            const child_idx = @as(u8, @intCast(current.children.items.len));
                            current.children.appendAssumeCapacity(new_trie);
                            current.child_indices[c] = child_idx;
                            current = new_trie;
                        }
                    }
                } else {
                    // Non-literal: find or create regex branch
                    var found_existing = false;
                    for (current.regexes.items) |*entry| {
                        if (std.mem.eql(u8, entry.pattern, seg.str)) {
                            current = entry.node;
                            found_existing = true;
                            break;
                        }
                    }
                    if (!found_existing) {
                        const new_trie = init(self.allocator) catch return RegexTrieError.AllocationFailure;
                        errdefer new_trie.deinit();
                        const pattern_copy = self.allocator.dupe(u8, seg.str) catch return RegexTrieError.AllocationFailure;
                        errdefer self.allocator.free(pattern_copy);
                        current.matcher_updated = false;
                        current.regexes.append(self.allocator, RegexEntry{
                            .node = new_trie,
                            .pattern = pattern_copy,
                            .allocator = self.allocator,
                        }) catch return RegexTrieError.AllocationFailure;
                        current = new_trie;
                    }
                }
            }
            // Mark end of word
            const eow_idx = current.child_indices[0];
            const is_new_eow = eow_idx == 255;
            if (is_new_eow) {
                // Ensure capacity before appending
                current.children.ensureTotalCapacity(self.allocator, current.children.items.len + 1) catch return RegexTrieError.AllocationFailure;
                const child_idx = @as(u8, @intCast(current.children.items.len));
                current.children.appendAssumeCapacity(null);
                current.child_indices[0] = child_idx;
            }

            // Check if this node is already in our target list (same path visited twice)
            var already_targeted = false;
            for (target_nodes.items) |target_node| {
                if (target_node == current) {
                    already_targeted = true;
                    break;
                }
            }

            if (!already_targeted) {
                // Check for duplicate BEFORE adding to targets (no assignment yet)
                if (!is_new_eow and current.leaf_value != null) {
                    return RegexTrieError.DuplicateLeafValue;
                }
                target_nodes.append(self.allocator, current) catch return RegexTrieError.AllocationFailure;
            }
        }

        // Phase 2: All checks passed, now assign the value to all target nodes
        for (target_nodes.items) |target_node| {
            target_node.leaf_value = key_value;
        }
    }

    const PathStep = struct {
        node: *RegexTrie,
        is_literal: bool,
        lit_key: u8,
        regex_index: usize,
    };

    /// Remove a pattern from the trie and return the associated data pointer
    /// The trie handles cleanup of the internal key storage
    /// The caller is responsible for cleaning up the returned data pointer if needed
    pub fn remove(self: *RegexTrie, regex_key: []const u8) RegexTrieError!?*anyopaque {
        std.debug.assert(regex_key.len > 0);

        var paths = regex_splitting.regexSplitting(self.allocator, regex_key) catch {
            return RegexTrieError.AllocationFailure;
        };
        defer {
            for (paths.items) |*path| {
                for (path.items) |*seg| seg.deinit();
                path.deinit(self.allocator);
            }
            paths.deinit(self.allocator);
        }

        var all_removed = true;
        var removed_internal: ?*RegexTrieValue = null;

        for (paths.items) |path| {
            var stack = std.ArrayList(PathStep).initCapacity(self.allocator, 0) catch return RegexTrieError.AllocationFailure;
            defer stack.deinit(self.allocator);
            var current = self;
            var path_found = true;

            for (path.items) |seg| {
                if (!path_found) break;
                if (seg.is_lit) {
                    for (seg.str) |c_byte| {
                        if (!path_found) break;
                        const c: u8 = c_byte;
                        const idx = current.child_indices[c];
                        if (idx != 255) {
                            if (current.children.items[idx]) |child| {
                                stack.append(self.allocator, PathStep{
                                    .node = current,
                                    .is_literal = true,
                                    .lit_key = c,
                                    .regex_index = 0,
                                }) catch return RegexTrieError.AllocationFailure;
                                current = child;
                            } else {
                                path_found = false;
                            }
                        } else {
                            path_found = false;
                        }
                    }
                } else {
                    var found_idx: ?usize = null;
                    for (current.regexes.items, 0..) |entry, j| {
                        if (std.mem.eql(u8, entry.pattern, seg.str)) {
                            found_idx = j;
                            break;
                        }
                    }
                    if (found_idx) |idx| {
                        const child = current.regexes.items[idx].node;
                        stack.append(self.allocator, PathStep{
                            .node = current,
                            .is_literal = false,
                            .lit_key = 0,
                            .regex_index = idx,
                        }) catch return RegexTrieError.AllocationFailure;
                        current = child;
                    } else {
                        path_found = false;
                    }
                }
            }

            if (!path_found or !current.checkEow()) {
                all_removed = false;
                continue;
            }

            if (current.leaf_value == null or !std.mem.eql(u8, current.leaf_value.?.regex_key, regex_key)) {
                all_removed = false;
                continue;
            }

            if (removed_internal == null) {
                removed_internal = current.leaf_value;
            } else {
                if (removed_internal != current.leaf_value) {
                    all_removed = false;
                    continue;
                }
            }

            // Remove leaf marker
            current.leaf_value = null;
            const eow_idx = current.child_indices[0];
            if (eow_idx != 255) {
                current.child_indices[0] = 255;
                // Note: We don't remove from children array to avoid index shifting
                // The slot will be reused or ignored
            }

            // Prune upwards
            while (stack.items.len > 0) {
                if (current.hasChildren() or current.leaf_value != null or current.checkEow()) {
                    break;
                }
                const ps = stack.pop() orelse break;
                const parent = ps.node;
                if (ps.is_literal) {
                    const child_idx = parent.child_indices[ps.lit_key];
                    parent.child_indices[ps.lit_key] = 255;
                    // Set to null to avoid dangling pointer
                    if (child_idx != 255) {
                        parent.children.items[child_idx] = null;
                    }
                } else {
                    parent.matcher_updated = false;
                    var entry = parent.regexes.swapRemove(ps.regex_index);
                    entry.deinit();
                }
                current.deinit();
                current = parent;
            }
        }

        if (!all_removed) {
            return RegexTrieError.NodeNotFound;
        }

        // Extract data pointer and clean up internal value
        if (removed_internal) |internal| {
            const data = internal.data;
            internal.deinit();
            self.allocator.destroy(internal);
            return data;
        }
        return null;
    }

    pub fn get(self: *RegexTrie, string: []const u8) RegexTrieError!MatchResult {
        std.debug.assert(string.len > 0);

        var current = self;
        var pos: usize = 0;
        var max_matched: usize = 0;
        var max_value: ?*RegexTrieValue = null;

        while (pos < string.len) {
            const c: u8 = string[pos];
            var advanced = false;
            var advance_len: usize = 0;

            // Try literal child first (O(1) lookup via index array)
            const idx = current.child_indices[c];
            if (idx != 255) {
                if (current.children.items[idx]) |child| {
                    current = child;
                    advance_len = 1;
                    advanced = true;
                }
            }

            if (!advanced) {
                // No literal: Try regex branches
                if (current.regexes.items.len > 0) {
                    if (!current.matcher_updated) {
                        try current.updateMatcher();
                    }
                    std.debug.assert(current.compiled_regex != null);
                    std.debug.assert(current.match_data != null);

                    if (current.compiled_regex) |*regex| {
                        if (regex.code) |code_ptr| {
                            const remaining = string[pos..];
                            // Use direct PCRE2 call with reusable match_data (like C++ version)
                            const rc = pcre2_c.pcre2_match_8(
                                @as(*pcre2_c.pcre2_code_8, @ptrCast(code_ptr)),
                                remaining.ptr,
                                remaining.len,
                                0, // Start offset
                                0, // Options
                                current.match_data.?,
                                null, // Match context
                            );
                            if (rc >= 0) {
                                const ovector = pcre2_c.pcre2_get_ovector_pointer_8(current.match_data.?);
                                const start = ovector[0];
                                const end = ovector[1];
                                if (start == 0 and end > 0) {
                                    const regex_len = end;
                                    // Identify which alternative matched (check capture groups 1..n)
                                    var which: ?usize = null;
                                    for (current.regexes.items, 1..) |_, g| {
                                        const group_start = ovector[2 * g];
                                        if (group_start != std.math.maxInt(usize)) {
                                            which = g - 1; // Convert to 0-based index
                                            break;
                                        }
                                    }
                                    if (which) |w| {
                                        if (w < current.regexes.items.len) {
                                            current = current.regexes.items[w].node;
                                            advance_len = regex_len;
                                            advanced = true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // If still not advanced and we're not at root, try root's regex patterns
                // This handles cases like "test" where literal 't' (for "true") was matched
                // but then failed - we need to fall back to trying the identifier regex
                if (!advanced and current != self and pos == 0) {
                    // We went down a literal path but got stuck at position 0
                    // This shouldn't happen, but handle it anyway
                    break;
                }

                if (!advanced) {
                    break;
                }
            }

            pos += advance_len;

            // Check EOW sentinel
            const eow_idx = current.child_indices[0];
            if (eow_idx != 255) {
                if (current.children.items[eow_idx] == null) {
                    max_matched = pos;
                    max_value = current.leaf_value;
                }
            }
        }

        // If no match found via literal path, try regex patterns from root
        if (max_matched == 0 and self.regexes.items.len > 0) {
            if (!self.matcher_updated) {
                try self.updateMatcher();
            }
            if (self.compiled_regex) |*regex| {
                if (regex.code) |code_ptr| {
                    const rc = pcre2_c.pcre2_match_8(
                        @as(*pcre2_c.pcre2_code_8, @ptrCast(code_ptr)),
                        string.ptr,
                        string.len,
                        0,
                        0,
                        self.match_data.?,
                        null,
                    );
                    if (rc >= 0) {
                        const ovector = pcre2_c.pcre2_get_ovector_pointer_8(self.match_data.?);
                        const start = ovector[0];
                        const end = ovector[1];
                        if (start == 0 and end > 0) {
                            // Identify which alternative matched
                            var which: ?usize = null;
                            for (self.regexes.items, 1..) |_, g| {
                                const group_start = ovector[2 * g];
                                if (group_start != std.math.maxInt(usize)) {
                                    which = g - 1;
                                    break;
                                }
                            }
                            if (which) |w| {
                                if (w < self.regexes.items.len) {
                                    const target_node = self.regexes.items[w].node;
                                    // Check for EOW in target node
                                    const eow_idx = target_node.child_indices[0];
                                    if (eow_idx != 255 and target_node.children.items[eow_idx] == null) {
                                        if (target_node.leaf_value) |val| {
                                            return .{
                                                .matched = end,
                                                .regex_key = val.regex_key,
                                                .data = val.data,
                                            };
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if (max_matched > 0) {
            std.debug.assert(max_value != null);
            std.debug.assert(max_value.?.regex_key.len > 0);
            return .{
                .matched = max_matched,
                .regex_key = max_value.?.regex_key,
                .data = max_value.?.data,
            };
        }
        return RegexTrieError.NodeNotFound;
    }

    // Find all possible matches at the start of the string
    // This is much slower than get() because it explores all paths
    pub fn getAllMatches(self: *RegexTrie, string: []const u8, allocator: std.mem.Allocator) RegexTrieError!std.ArrayList(MatchResult) {
        var matches = std.ArrayList(MatchResult).initCapacity(allocator, 0) catch return RegexTrieError.AllocationFailure;
        errdefer matches.deinit(allocator);

        const State = struct {
            node: *RegexTrie,
            pos: usize,
        };

        var stack = std.ArrayList(State).initCapacity(allocator, 0) catch return RegexTrieError.AllocationFailure;
        defer stack.deinit(allocator);

        stack.append(allocator, State{ .node = self, .pos = 0 }) catch return RegexTrieError.AllocationFailure;

        while (stack.items.len > 0) {
            const state = stack.pop() orelse break;
            const current = state.node;
            const pos = state.pos;

            // Check if we're at end of word
            const eow_idx = current.child_indices[0];
            if (eow_idx != 255) {
                if (current.children.items[eow_idx] == null) {
                    if (current.leaf_value) |value| {
                        matches.append(allocator, .{
                            .matched = pos,
                            .regex_key = value.regex_key,
                            .data = value.data,
                        }) catch return RegexTrieError.AllocationFailure;
                    }
                }
            }

            if (pos >= string.len) continue;

            const c: u8 = string[pos];

            // Try literal child
            const idx = current.child_indices[c];
            if (idx != 255) {
                if (current.children.items[idx]) |child| {
                    stack.append(allocator, State{ .node = child, .pos = pos + 1 }) catch return RegexTrieError.AllocationFailure;
                }
            }

            // Try all regex branches
            if (current.regexes.items.len > 0) {
                if (!current.matcher_updated) {
                    try current.updateMatcher();
                }
                std.debug.assert(current.compiled_regex != null);
                std.debug.assert(current.match_data != null);

                if (current.compiled_regex) |*regex| {
                    if (regex.code) |code_ptr| {
                        const remaining = string[pos..];
                        // Use direct PCRE2 call with reusable match_data (like C++ version)
                        const rc = pcre2_c.pcre2_match_8(
                            @as(*pcre2_c.pcre2_code_8, @ptrCast(code_ptr)),
                            remaining.ptr,
                            remaining.len,
                            0, // Start offset
                            0, // Options
                            current.match_data.?,
                            null, // Match context
                        );
                        if (rc >= 0) {
                            const ovector = pcre2_c.pcre2_get_ovector_pointer_8(current.match_data.?);
                            const start = ovector[0];
                            const end = ovector[1];
                            if (start == 0 and end > 0) {
                                const regex_len = end;
                                // Check all capture groups to find which alternatives matched
                                for (current.regexes.items, 1..) |entry, g| {
                                    const group_start = ovector[2 * g];
                                    if (group_start != std.math.maxInt(usize)) {
                                        // This alternative matched
                                        stack.append(allocator, State{ .node = entry.node, .pos = pos + regex_len }) catch return RegexTrieError.AllocationFailure;
                                        // Note: PCRE2 typically only matches one alternative in an alternation
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        return matches;
    }

    pub fn print(self: *const RegexTrie) void {
        printTrieRecursive(self, 0);
    }

    fn printIndent(indent: usize) void {
        var i: usize = 0;
        while (i < indent * 4) : (i += 1) {
            std.debug.print(" ", .{});
        }
    }

    fn printTrieRecursive(trie: *const RegexTrie, indent: usize) void {
        var ci = getLiteralChain(trie.allocator, @constCast(trie)) catch return;
        defer ci.deinit();
        if (ci.label.len > 0) {
            printIndent(indent);
            std.debug.print("{s} (lit)", .{ci.label});
            const target_eow = ci.target_node.checkEow();
            const target_leaf = !ci.target_node.hasChildren();
            if (target_eow and target_leaf) {
                std.debug.print(" (EOW)\n", .{});
                return;
            }
            std.debug.print("\n", .{});
            printTrieRecursive(ci.target_node, indent + 1);
            return;
        }

        if (trie.checkEow()) {
            printIndent(indent);
            std.debug.print("(EOW)\n", .{});
        }
        printBranches(trie, indent);
    }

    fn printBranches(trie: *const RegexTrie, indent: usize) void {

        // Regex branches first, sorted
        if (trie.regexes.items.len > 0) {
            var regex_list = std.ArrayList(struct { []const u8, *const RegexTrie }).initCapacity(trie.allocator, 0) catch return;
            defer regex_list.deinit(trie.allocator);
            for (trie.regexes.items) |entry| {
                regex_list.append(trie.allocator, .{ entry.pattern, entry.node }) catch return;
            }
            std.mem.sort(struct { []const u8, *const RegexTrie }, regex_list.items, {}, struct {
                fn lessThan(_: void, a: struct { []const u8, *const RegexTrie }, b: struct { []const u8, *const RegexTrie }) bool {
                    return std.mem.order(u8, a[0], b[0]) == .lt;
                }
            }.lessThan);

            for (regex_list.items) |r| {
                const rstr = r[0];
                const rchild = r[1];
                printIndent(indent + 1);
                std.debug.print("{s} (regex)", .{rstr});
                const child_eow = rchild.checkEow();
                const child_leaf = !rchild.hasChildren();
                if (child_eow and child_leaf) {
                    std.debug.print(" (EOW)\n", .{});
                } else {
                    std.debug.print("\n", .{});
                    printTrieRecursive(rchild, indent + 2);
                }
            }
        }

        // Literal branches, sorted
        var lit_keys = std.ArrayList(u8).initCapacity(trie.allocator, 256) catch return;
        defer lit_keys.deinit(trie.allocator);

        // Collect all literal keys
        for (trie.child_indices, 0..) |idx, key| {
            if (key == 0) continue; // Skip EOW sentinel
            if (idx != 255) {
                if (trie.children.items[idx] != null) {
                    lit_keys.append(trie.allocator, @as(u8, @intCast(key))) catch return;
                }
            }
        }
        std.mem.sort(u8, lit_keys.items, {}, struct {
            fn lessThan(_: void, a: u8, b: u8) bool {
                return a < b;
            }
        }.lessThan);

        for (lit_keys.items) |c| {
            const idx = trie.child_indices[c];
            if (idx == 255) continue;
            if (trie.children.items[idx]) |child| {
                var ci = getLiteralChain(trie.allocator, @constCast(child)) catch continue;
                defer ci.deinit();
                const full_label = std.fmt.allocPrint(trie.allocator, "{c}{s}", .{ c, ci.label }) catch continue;
                defer trie.allocator.free(full_label);
                const target = ci.target_node;
                printIndent(indent + 1);
                std.debug.print("{s} (lit)", .{full_label});
                const t_eow = target.checkEow();
                const t_leaf = !target.hasChildren();
                if (t_eow and t_leaf) {
                    std.debug.print(" (EOW)\n", .{});
                } else {
                    std.debug.print("\n", .{});
                    printTrieRecursive(target, indent + 2);
                }
            }
        }
    }
};
