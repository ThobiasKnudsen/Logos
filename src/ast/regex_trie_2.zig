const std = @import("std");
const pcrez = @import("pcrez");
const regex_splitting = @import("regex_splitting");
const verztable = @import("verztable");

// PCRE2 C bindings for JIT compilation
const pcre2_c = @cImport({
    @cDefine("PCRE2_CODE_UNIT_WIDTH", "8");
    @cInclude("pcre2.h");
});

// Error types
pub const RegexTrieError_2 = error{
    AllocationFailure,
    NullArgument,
    NodeNotFound,
    NodeFound,
    DestroyFailed,
    DuplicateLeafValue,
};

/// Internal value storage - not exposed to users
const RegexTrieValue_2 = struct {
    regex_key: []const u8,
    /// Generic value pointer - cast to your type when retrieving
    /// Caller is responsible for freeing this when removing patterns
    data: ?*anyopaque,
    allocator: std.mem.Allocator,
    /// Flag to prevent double-free when multiple nodes share same value
    freed: bool,

    fn deinit(self: *RegexTrieValue_2) void {
        if (!self.freed) {
            self.allocator.free(self.regex_key);
            self.freed = true;
        }
    }

    fn destroy(self: *RegexTrieValue_2) void {
        if (!self.freed) {
            self.deinit();
            self.allocator.destroy(self);
        }
    }

    fn create(allocator: std.mem.Allocator, regex_key: []const u8, data: ?*anyopaque) !*RegexTrieValue_2 {
        const value = try allocator.create(RegexTrieValue_2);
        value.regex_key = try allocator.dupe(u8, regex_key);
        value.data = data;
        value.allocator = allocator;
        value.freed = false;
        return value;
    }
};

/// Result from get() and getAllMatches()
pub const MatchResult_2 = struct {
    /// Number of bytes matched
    matched: usize,
    /// The regex pattern key that matched
    regex_key: []const u8,
    /// User data pointer associated with the pattern (cast to your type)
    data: ?*anyopaque,
};

const RegexEntry_2 = struct {
    node: *RegexTrie_2,
    pattern: []const u8,
    allocator: std.mem.Allocator,

    pub fn deinit(self: *RegexEntry_2) void {
        self.allocator.free(self.pattern);
    }
};

// Use verztable HashMap for children - much more efficient than [256]u8 + ArrayList
const ChildMap = verztable.HashMap(u8, *RegexTrie_2);

pub const RegexTrie_2 = struct {
    // Use verztable HashMap instead of [256]u8 index array + ArrayList
    // This reduces memory from ~360 bytes to ~80 bytes per node
    children: ChildMap,
    has_eow: bool, // End-of-word marker (replaces null sentinel in children)
    leaf_value: ?*RegexTrieValue_2,
    compiled_regex: ?pcrez.Regex,
    match_data: ?*pcre2_c.pcre2_match_data_8, // Reusable match data
    regexes: std.ArrayList(RegexEntry_2),
    matcher_updated: bool,
    allocator: std.mem.Allocator,

    pub fn init(allocator: std.mem.Allocator) !*RegexTrie_2 {
        const self = try allocator.create(RegexTrie_2);
        self.* = .{
            .children = ChildMap.init(allocator),
            .has_eow = false,
            .leaf_value = null,
            .compiled_regex = null,
            .match_data = null,
            .regexes = try std.ArrayList(RegexEntry_2).initCapacity(allocator, 0),
            .matcher_updated = true,
            .allocator = allocator,
        };
        return self;
    }

    pub fn deinit(self: *RegexTrie_2) void {
        const allocator = self.allocator;
        deinitInternal(self);
        allocator.destroy(self);
    }

    fn deinitInternal(self: *RegexTrie_2) void {
        // Clean up children using verztable iterator
        var iter = self.children.iterator();
        while (iter.next()) |entry| {
            const child = entry.val;
            child.deinitInternal();
            self.allocator.destroy(child);
        }
        self.children.deinit();

        // Clean up regex children
        for (self.regexes.items) |*entry| {
            entry.node.deinitInternal();
            self.allocator.destroy(entry.node);
            entry.deinit();
        }
        self.regexes.deinit(self.allocator);

        // Clean up PCRE2 resources
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
    }

    fn checkEow(self: *const RegexTrie_2) bool {
        return self.has_eow;
    }

    fn hasChildren(self: *const RegexTrie_2) bool {
        return self.children.count() > 0 or self.regexes.items.len > 0;
    }

    const ChainInfo_2 = struct {
        label: []const u8,
        target_node: *RegexTrie_2,
        allocator: std.mem.Allocator,

        pub fn deinit(self: *ChainInfo_2) void {
            self.allocator.free(self.label);
        }
    };

    fn getLiteralChain(allocator: std.mem.Allocator, node: *RegexTrie_2) !ChainInfo_2 {
        var label = try std.ArrayList(u8).initCapacity(allocator, 0);
        errdefer label.deinit(allocator);
        var curr = node;
        while (true) {
            // Count children using verztable
            const child_count = curr.children.count();
            if (child_count != 1) break;
            if (curr.regexes.items.len > 0) break;

            // Get the single child key
            var iter = curr.children.iterator();
            const entry = iter.next() orelse break;
            const c = entry.key;

            try label.append(allocator, c);
            curr = entry.val;
        }
        return ChainInfo_2{
            .label = try label.toOwnedSlice(allocator),
            .target_node = curr,
            .allocator = allocator,
        };
    }

    fn updateMatcher(self: *RegexTrie_2) RegexTrieError_2!void {
        std.debug.assert(!self.matcher_updated);

        // Free existing
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
                return RegexTrieError_2.AllocationFailure;
            }
            // Enable JIT compilation for speed
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
            // Create reusable match data
            if (regex.code) |code_ptr| {
                self.match_data = pcre2_c.pcre2_match_data_create_from_pattern_8(
                    @as(*pcre2_c.pcre2_code_8, @ptrCast(code_ptr)),
                    null,
                );
            }
        } else {
            // Build combined pattern
            var pattern_buf = std.ArrayList(u8).initCapacity(self.allocator, 0) catch return RegexTrieError_2.AllocationFailure;
            defer pattern_buf.deinit(self.allocator);
            pattern_buf.append(self.allocator, '^') catch return RegexTrieError_2.AllocationFailure;
            for (self.regexes.items, 0..) |entry, i| {
                if (i > 0) {
                    pattern_buf.append(self.allocator, '|') catch return RegexTrieError_2.AllocationFailure;
                }
                pattern_buf.append(self.allocator, '(') catch return RegexTrieError_2.AllocationFailure;
                pattern_buf.appendSlice(self.allocator, entry.pattern) catch return RegexTrieError_2.AllocationFailure;
                pattern_buf.append(self.allocator, ')') catch return RegexTrieError_2.AllocationFailure;
            }
            const pattern = pattern_buf.toOwnedSlice(self.allocator) catch return RegexTrieError_2.AllocationFailure;
            defer self.allocator.free(pattern);

            var regex = pcrez.Regex.init(self.allocator);
            const compile_result = regex.compile(pattern, true);
            if (compile_result != pcrez.Regex.CompileError.Compiled) {
                std.log.err("PCRE2 compile fail\n", .{});
                return RegexTrieError_2.AllocationFailure;
            }
            // Enable JIT compilation for speed
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
            // Create reusable match data
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
    fn insertLiteralFast(self: *RegexTrie_2, str: []const u8, key_value: *RegexTrieValue_2) RegexTrieError_2!void {
        var current = self;

        // Traverse/create path for each character
        for (str) |c| {
            if (current.children.get(c)) |child| {
                current = child;
            } else {
                // Create new child
                const new_trie = init(self.allocator) catch return RegexTrieError_2.AllocationFailure;
                errdefer new_trie.deinit();

                current.children.put(c, new_trie) catch return RegexTrieError_2.AllocationFailure;
                current = new_trie;
            }
        }

        // Mark end of word
        if (!current.has_eow) {
            current.has_eow = true;
            if (current.leaf_value != null) {
                return RegexTrieError_2.DuplicateLeafValue;
            }
            current.leaf_value = key_value;
        } else {
            // EOW marker already exists, check if leaf value already exists
            if (current.leaf_value != null) {
                return RegexTrieError_2.DuplicateLeafValue;
            }
            current.leaf_value = key_value;
        }
    }

    /// Insert a pattern into the trie with associated data
    /// allocator: Used to allocate internal storage for the key
    /// key: The regex pattern or literal string to insert
    /// data: Optional user data pointer associated with the pattern
    pub fn insert(self: *RegexTrie_2, allocator: std.mem.Allocator, key: []const u8, data: ?*anyopaque) RegexTrieError_2!void {
        std.debug.assert(key.len > 0);

        // Create the internal value
        const key_value = RegexTrieValue_2.create(allocator, key, data) catch return RegexTrieError_2.AllocationFailure;
        errdefer {
            key_value.deinit();
            allocator.destroy(key_value);
        }

        const str = key;

        // Fast path: if it's a pure literal, completely bypass regex splitting
        if (regex_splitting.isPureLiteral(str)) {
            return self.insertLiteralFast(str, key_value);
        }

        var paths = regex_splitting.regexSplitting(self.allocator, str) catch return RegexTrieError_2.AllocationFailure;
        defer {
            for (paths.items) |*path| {
                for (path.items) |*seg| seg.deinit();
                path.deinit(self.allocator);
            }
            paths.deinit(self.allocator);
        }

        // Phase 1: Navigate all paths, create nodes, check for duplicates BEFORE any assignment
        var target_nodes = std.ArrayList(*RegexTrie_2).initCapacity(self.allocator, paths.items.len) catch return RegexTrieError_2.AllocationFailure;
        defer target_nodes.deinit(self.allocator);

        for (paths.items) |path| {
            var current = self;
            for (path.items) |seg| {
                if (seg.is_lit) {
                    for (seg.str) |c| {
                        if (current.children.get(c)) |child| {
                            current = child;
                        } else {
                            // Create new child
                            const new_trie = init(self.allocator) catch return RegexTrieError_2.AllocationFailure;
                            errdefer new_trie.deinit();

                            current.children.put(c, new_trie) catch return RegexTrieError_2.AllocationFailure;
                            current = new_trie;
                        }
                    }
                } else {
                    // Non-literal: find or create regex branch
                    var found_existing = false;
                    for (self.regexes.items) |*entry| {
                        if (std.mem.eql(u8, entry.pattern, seg.str)) {
                            current = entry.node;
                            found_existing = true;
                            break;
                        }
                    }
                    if (!found_existing) {
                        const new_trie = init(self.allocator) catch return RegexTrieError_2.AllocationFailure;
                        errdefer new_trie.deinit();
                        const pattern_copy = self.allocator.dupe(u8, seg.str) catch return RegexTrieError_2.AllocationFailure;
                        errdefer self.allocator.free(pattern_copy);
                        current.matcher_updated = false;
                        current.regexes.append(self.allocator, RegexEntry_2{
                            .node = new_trie,
                            .pattern = pattern_copy,
                            .allocator = self.allocator,
                        }) catch return RegexTrieError_2.AllocationFailure;
                        current = new_trie;
                    }
                }
            }
            // Mark end of word
            const is_new_eow = !current.has_eow;
            if (is_new_eow) {
                current.has_eow = true;
            }

            // Check if this node is already in our target list
            var already_targeted = false;
            for (target_nodes.items) |target_node| {
                if (target_node == current) {
                    already_targeted = true;
                    break;
                }
            }

            if (!already_targeted) {
                // Check for duplicate BEFORE adding to targets
                if (!is_new_eow and current.leaf_value != null) {
                    return RegexTrieError_2.DuplicateLeafValue;
                }
                target_nodes.append(self.allocator, current) catch return RegexTrieError_2.AllocationFailure;
            }
        }

        // Phase 2: All checks passed, now assign the value to all target nodes
        for (target_nodes.items) |target_node| {
            target_node.leaf_value = key_value;
        }
    }

    const PathStep_2 = struct {
        node: *RegexTrie_2,
        is_literal: bool,
        lit_key: u8,
        regex_index: usize,
    };

    /// Remove a pattern from the trie and return the associated data pointer
    pub fn remove(self: *RegexTrie_2, regex_key: []const u8) RegexTrieError_2!?*anyopaque {
        std.debug.assert(regex_key.len > 0);

        var paths = regex_splitting.regexSplitting(self.allocator, regex_key) catch {
            return RegexTrieError_2.AllocationFailure;
        };
        defer {
            for (paths.items) |*path| {
                for (path.items) |*seg| seg.deinit();
                path.deinit(self.allocator);
            }
            paths.deinit(self.allocator);
        }

        var all_removed = true;
        var removed_internal: ?*RegexTrieValue_2 = null;

        for (paths.items) |path| {
            var stack = std.ArrayList(PathStep_2).initCapacity(self.allocator, 0) catch return RegexTrieError_2.AllocationFailure;
            defer stack.deinit(self.allocator);
            var current = self;
            var path_found = true;

            for (path.items) |seg| {
                if (!path_found) break;
                if (seg.is_lit) {
                    for (seg.str) |c| {
                        if (!path_found) break;
                        if (current.children.get(c)) |child| {
                            stack.append(self.allocator, PathStep_2{
                                .node = current,
                                .is_literal = true,
                                .lit_key = c,
                                .regex_index = 0,
                            }) catch return RegexTrieError_2.AllocationFailure;
                            current = child;
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
                        stack.append(self.allocator, PathStep_2{
                            .node = current,
                            .is_literal = false,
                            .lit_key = 0,
                            .regex_index = idx,
                        }) catch return RegexTrieError_2.AllocationFailure;
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
            current.has_eow = false;

            // Prune upwards
            while (stack.items.len > 0) {
                if (current.hasChildren() or current.leaf_value != null or current.checkEow()) {
                    break;
                }
                const ps = stack.pop() orelse break;
                const parent = ps.node;
                if (ps.is_literal) {
                    _ = parent.children.remove(ps.lit_key);
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
            return RegexTrieError_2.NodeNotFound;
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

    pub fn get(self: *RegexTrie_2, string: []const u8) RegexTrieError_2!MatchResult_2 {
        std.debug.assert(string.len > 0);

        var current = self;
        var pos: usize = 0;
        var max_matched: usize = 0;
        var max_value: ?*RegexTrieValue_2 = null;

        while (pos < string.len) {
            const c: u8 = string[pos];
            var advanced = false;
            var advance_len: usize = 0;

            // Try literal child first using verztable O(1) lookup
            if (current.children.get(c)) |child| {
                current = child;
                advance_len = 1;
                advanced = true;
            }

            if (!advanced) {
                // No literal: Try regex branches
                if (current.regexes.items.len == 0) {
                    break;
                }
                if (!current.matcher_updated) {
                    try current.updateMatcher();
                }
                std.debug.assert(current.compiled_regex != null);
                std.debug.assert(current.match_data != null);

                if (current.compiled_regex) |*regex| {
                    if (regex.code) |code_ptr| {
                        const remaining = string[pos..];
                        // Use stored match_data for efficiency (like C++ version)
                        const rc = pcre2_c.pcre2_match_8(
                            @as(*pcre2_c.pcre2_code_8, @ptrCast(code_ptr)),
                            remaining.ptr,
                            remaining.len,
                            0,
                            0,
                            current.match_data.?,
                            null,
                        );
                        if (rc >= 0) {
                            const ovector = pcre2_c.pcre2_get_ovector_pointer_8(current.match_data.?);
                            const start = ovector[0];
                            const end = ovector[1];
                            if (start == 0 and end > 0) {
                                const regex_len = end;
                                // Identify which alternative matched
                                var which: ?usize = null;
                                for (current.regexes.items, 1..) |_, g| {
                                    const group_start = ovector[2 * g];
                                    if (group_start != std.math.maxInt(usize)) {
                                        which = g - 1;
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

            if (!advanced) {
                break;
            }

            pos += advance_len;

            // Check EOW
            if (current.has_eow) {
                max_matched = pos;
                max_value = current.leaf_value;
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
        return RegexTrieError_2.NodeNotFound;
    }

    // Find all possible matches at the start of the string
    pub fn getAllMatches(self: *RegexTrie_2, string: []const u8, allocator: std.mem.Allocator) RegexTrieError_2!std.ArrayList(MatchResult_2) {
        var matches = std.ArrayList(MatchResult_2).initCapacity(allocator, 0) catch return RegexTrieError_2.AllocationFailure;
        errdefer matches.deinit(allocator);

        const State = struct {
            node: *RegexTrie_2,
            pos: usize,
        };

        var stack = std.ArrayList(State).initCapacity(allocator, 0) catch return RegexTrieError_2.AllocationFailure;
        defer stack.deinit(allocator);

        stack.append(allocator, State{ .node = self, .pos = 0 }) catch return RegexTrieError_2.AllocationFailure;

        while (stack.items.len > 0) {
            const state = stack.pop() orelse break;
            const current = state.node;
            const pos = state.pos;

            // Check if we're at end of word
            if (current.has_eow) {
                if (current.leaf_value) |value| {
                    matches.append(allocator, .{
                        .matched = pos,
                        .regex_key = value.regex_key,
                        .data = value.data,
                    }) catch return RegexTrieError_2.AllocationFailure;
                }
            }

            if (pos >= string.len) continue;

            const c: u8 = string[pos];

            // Try literal child
            if (current.children.get(c)) |child| {
                stack.append(allocator, State{ .node = child, .pos = pos + 1 }) catch return RegexTrieError_2.AllocationFailure;
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
                        const rc = pcre2_c.pcre2_match_8(
                            @as(*pcre2_c.pcre2_code_8, @ptrCast(code_ptr)),
                            remaining.ptr,
                            remaining.len,
                            0,
                            0,
                            current.match_data.?,
                            null,
                        );
                        if (rc >= 0) {
                            const ovector = pcre2_c.pcre2_get_ovector_pointer_8(current.match_data.?);
                            const start = ovector[0];
                            const end = ovector[1];
                            if (start == 0 and end > 0) {
                                const regex_len = end;
                                for (current.regexes.items, 1..) |entry, g| {
                                    const group_start = ovector[2 * g];
                                    if (group_start != std.math.maxInt(usize)) {
                                        stack.append(allocator, State{ .node = entry.node, .pos = pos + regex_len }) catch return RegexTrieError_2.AllocationFailure;
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

    pub fn print(self: *const RegexTrie_2) void {
        printTrieRecursive(self, 0);
    }

    fn printIndent(indent: usize) void {
        var i: usize = 0;
        while (i < indent * 4) : (i += 1) {
            std.debug.print(" ", .{});
        }
    }

    fn printTrieRecursive(trie: *const RegexTrie_2, indent: usize) void {
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

    fn printBranches(trie: *const RegexTrie_2, indent: usize) void {
        // Regex branches first, sorted
        if (trie.regexes.items.len > 0) {
            var regex_list = std.ArrayList(struct { []const u8, *const RegexTrie_2 }).initCapacity(trie.allocator, 0) catch return;
            defer regex_list.deinit(trie.allocator);
            for (trie.regexes.items) |entry| {
                regex_list.append(trie.allocator, .{ entry.pattern, entry.node }) catch return;
            }
            std.mem.sort(struct { []const u8, *const RegexTrie_2 }, regex_list.items, {}, struct {
                fn lessThan(_: void, a: struct { []const u8, *const RegexTrie_2 }, b: struct { []const u8, *const RegexTrie_2 }) bool {
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

        // Literal branches, sorted - use verztable iterator
        var lit_keys = std.ArrayList(u8).initCapacity(trie.allocator, 0) catch return;
        defer lit_keys.deinit(trie.allocator);

        var iter = @constCast(trie).children.iterator();
        while (iter.next()) |entry| {
            lit_keys.append(trie.allocator, entry.key) catch return;
        }
        std.mem.sort(u8, lit_keys.items, {}, struct {
            fn lessThan(_: void, a: u8, b: u8) bool {
                return a < b;
            }
        }.lessThan);

        for (lit_keys.items) |c| {
            if (@constCast(trie).children.get(c)) |child| {
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
