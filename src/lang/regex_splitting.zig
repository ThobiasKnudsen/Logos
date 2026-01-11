const std = @import("std");
const pcrez = @import("pcrez");

pub const Segment = struct {
    str: []const u8,
    is_lit: bool = false,
    allocator: ?std.mem.Allocator = null, // For owned strings

    pub fn deinit(self: *Segment) void {
        if (self.allocator) |alloc| {
            if (self.is_lit or self.str.len > 0) {
                alloc.free(self.str);
            }
        }
    }

    pub fn initLiteral(allocator: std.mem.Allocator, str: []const u8) !Segment {
        const owned = try allocator.dupe(u8, str);
        return Segment{
            .str = owned,
            .is_lit = true,
            .allocator = allocator,
        };
    }

    pub fn initRegex(allocator: std.mem.Allocator, str: []const u8) !Segment {
        const owned = try allocator.dupe(u8, str);
        return Segment{
            .str = owned,
            .is_lit = false,
            .allocator = allocator,
        };
    }

    // Deep copy a segment, duplicating the string if it's owned
    pub fn duplicate(self: Segment, allocator: std.mem.Allocator) !Segment {
        if (self.allocator) |_| {
            // Segment owns the string, so duplicate it
            const owned = try allocator.dupe(u8, self.str);
            return Segment{
                .str = owned,
                .is_lit = self.is_lit,
                .allocator = allocator,
            };
        } else {
            // Segment doesn't own the string, so just copy the struct
            return self;
        }
    }
};

const MAX_PATHS = 1000;

fn getQuantN(_: std.mem.Allocator, quant_str: []const u8) struct { min: usize, max: usize } {
    const INF = std.math.maxInt(usize);
    if (std.mem.eql(u8, quant_str, "*")) return .{ .min = 0, .max = INF };
    if (std.mem.eql(u8, quant_str, "+")) return .{ .min = 1, .max = INF };
    if (std.mem.eql(u8, quant_str, "?")) return .{ .min = 0, .max = 1 };
    if (quant_str.len > 1 and quant_str[0] == '{' and quant_str[quant_str.len - 1] == '}') {
        const inner = quant_str[1 .. quant_str.len - 1];
        if (std.mem.indexOf(u8, inner, ",")) |comma_pos| {
            const min_s = inner[0..comma_pos];
            const max_s = inner[comma_pos + 1 ..];
            const min_n = if (min_s.len > 0) std.fmt.parseInt(usize, min_s, 10) catch 0 else 0;
            const max_n = if (max_s.len > 0) std.fmt.parseInt(usize, max_s, 10) catch INF else INF;
            return .{ .min = min_n, .max = max_n };
        } else {
            const n = std.fmt.parseInt(usize, inner, 10) catch 0;
            return .{ .min = n, .max = n };
        }
    }
    return .{ .min = 0, .max = 0 };
}

fn mergeAdjacentSegments(allocator: std.mem.Allocator, path: *std.ArrayList(Segment)) !void {
    if (path.items.len == 0) return;
    var write_idx: usize = 0;
    var read_idx: usize = 1;
    while (read_idx < path.items.len) {
        if (path.items[write_idx].is_lit == path.items[read_idx].is_lit) {
            // Merge strings efficiently using ArrayList instead of allocPrint
            const total_len = path.items[write_idx].str.len + path.items[read_idx].str.len;
            const new_str = try allocator.alloc(u8, total_len);
            @memcpy(new_str[0..path.items[write_idx].str.len], path.items[write_idx].str);
            @memcpy(new_str[path.items[write_idx].str.len..], path.items[read_idx].str);
            // Free old strings - but first clear allocators to mark them as no longer owning the memory
            // This prevents double free if segments are shared (via appendSlice)
            if (path.items[write_idx].allocator) |a| {
                a.free(path.items[write_idx].str);
                path.items[write_idx].allocator = null; // Clear after freeing to prevent double free
            }
            if (path.items[read_idx].allocator) |a| {
                a.free(path.items[read_idx].str);
                path.items[read_idx].allocator = null; // Clear after freeing to prevent double free
            }
            path.items[write_idx].str = new_str;
            path.items[write_idx].allocator = allocator;
            path.items[read_idx].str = "";
        } else {
            write_idx += 1;
            if (write_idx != read_idx) {
                path.items[write_idx] = path.items[read_idx];
                path.items[read_idx].str = "";
                path.items[read_idx].allocator = null; // Clear allocator to prevent double free
            }
        }
        read_idx += 1;
    }
    path.shrinkRetainingCapacity(write_idx + 1);
}

fn validateSegment(allocator: std.mem.Allocator, seg_str: []const u8, is_lit: bool) bool {
    _ = is_lit;
    const pat = std.fmt.allocPrint(allocator, "^({s})", .{seg_str}) catch return false;
    defer allocator.free(pat);

    // Use PCREz to validate - TODO: fix PCREz API usage
    // var regex = pcrez.compile(pat, .{ .utf = true, .ucp = true }) catch return false;
    // defer regex.deinit();
    return true;
}

fn parseAtom(allocator: std.mem.Allocator, s: []const u8, pos: *usize) !std.ArrayList(std.ArrayList(Segment)) {
    var result = try std.ArrayList(std.ArrayList(Segment)).initCapacity(allocator, 0);
    errdefer {
        for (result.items) |*path| {
            for (path.items) |*seg| seg.deinit();
            path.deinit(allocator);
        }
        result.deinit(allocator);
    }

    if (pos.* >= s.len) return result;

    const c = s[pos.*];
    if (c == '\\') {
        pos.* += 1;
        if (pos.* >= s.len) {
            var path = try std.ArrayList(Segment).initCapacity(allocator, 0);
            try path.append(allocator, try Segment.initLiteral(allocator, "\\"));
            try result.append(allocator, path);
            return result;
        }
        const esc = s[pos.*];
        pos.* += 1;
        if (esc == 'd' or esc == 'D' or esc == 'w' or esc == 'W' or esc == 's' or esc == 'S' or
            esc == 'b' or esc == 'B' or esc == 'A' or esc == 'Z' or esc == 'z' or esc == 'R')
        {
            // Allocate 2 bytes for "\X" efficiently instead of using allocPrint
            const esc_str = try allocator.alloc(u8, 2);
            esc_str[0] = '\\';
            esc_str[1] = esc;
            var path = try std.ArrayList(Segment).initCapacity(allocator, 0);
            try path.append(allocator, Segment{ .str = esc_str, .is_lit = false, .allocator = allocator });
            try result.append(allocator, path);
            return result;
        } else if (esc == 'p' or esc == 'P') {
            if (pos.* < 2) {
                var path = try std.ArrayList(Segment).initCapacity(allocator, 0);
                // Allocate single character efficiently instead of using allocPrint
                const single = try allocator.alloc(u8, 1);
                errdefer allocator.free(single);
                single[0] = esc;
                try path.append(allocator, try Segment.initLiteral(allocator, single));
                allocator.free(single); // Free after initLiteral duplicates it
                try result.append(allocator, path);
                return result;
            }
            const start = pos.* - 2;
            while (pos.* < s.len and s[pos.*] != '}') pos.* += 1;
            if (pos.* < s.len) pos.* += 1;
            const prop_esc = s[start..pos.*];
            var path = try std.ArrayList(Segment).initCapacity(allocator, 0);
            try path.append(allocator, Segment{ .str = try allocator.dupe(u8, prop_esc), .is_lit = false, .allocator = allocator });
            try result.append(allocator, path);
            return result;
        } else if (esc == 'Q') {
            const q_start = pos.*;
            while (pos.* + 1 < s.len) {
                if (s[pos.*] == '\\' and s[pos.* + 1] == 'E') {
                    const quoted = s[q_start..pos.*];
                    var path = try std.ArrayList(Segment).initCapacity(allocator, 0);
                    try path.append(allocator, try Segment.initLiteral(allocator, quoted));
                    pos.* += 2;
                    try result.append(allocator, path);
                    return result;
                }
                pos.* += 1;
            }
            const quoted = s[q_start..];
            var path = try std.ArrayList(Segment).initCapacity(allocator, 0);
            try path.append(allocator, try Segment.initLiteral(allocator, quoted));
            pos.* = s.len;
            try result.append(allocator, path);
            return result;
        } else {
            var path = try std.ArrayList(Segment).initCapacity(allocator, 0);
            // Allocate single character efficiently instead of using allocPrint
            const single = try allocator.alloc(u8, 1);
            errdefer allocator.free(single);
            single[0] = esc;
            try path.append(allocator, try Segment.initLiteral(allocator, single));
            allocator.free(single); // Free after initLiteral duplicates it
            try result.append(allocator, path);
            return result;
        }
    } else if (c == '.') {
        pos.* += 1;
        var path = try std.ArrayList(Segment).initCapacity(allocator, 0);
        try path.append(allocator, Segment{ .str = ".", .is_lit = false, .allocator = null });
        try result.append(allocator, path);
        return result;
    } else if (c == '[') {
        const start = pos.*;
        pos.* += 1;
        var empty_class = true;
        while (pos.* < s.len and s[pos.*] != ']') {
            empty_class = false;
            if (s[pos.*] == '\\' and pos.* + 1 < s.len) pos.* += 1;
            if (pos.* < s.len) pos.* += 1;
        }
        if (pos.* < s.len and s[pos.*] == ']') pos.* += 1;
        var class_str = s[start..pos.*];
        if (empty_class) class_str = "[]";
        var path = try std.ArrayList(Segment).initCapacity(allocator, 0);
        try path.append(allocator, Segment{ .str = try allocator.dupe(u8, class_str), .is_lit = false, .allocator = allocator });
        try result.append(allocator, path);
        return result;
    } else if (c == '(') {
        pos.* += 1;
        var is_look = false;
        const look_pos = pos.*;
        if (pos.* + 1 < s.len and s[pos.*] == '?') {
            pos.* += 1;
            if (pos.* >= s.len) {
                if (pos.* > 0) pos.* -= 1;
            } else {
                const next = s[pos.*];
                if (next == '=' or next == '!' or next == ':') {
                    is_look = true;
                } else {
                    if (pos.* > 0) pos.* -= 1;
                }
            }
        }
        if (is_look) {
            if (pos.* < 2) {
                pos.* = look_pos;
            } else {
                const group_start = pos.* - 2;
                var paren_level: i32 = 1;
                while (pos.* < s.len) {
                    if (s[pos.*] == '(') {
                        paren_level += 1;
                    } else if (s[pos.*] == ')') {
                        paren_level -= 1;
                        if (paren_level == 0) break;
                    }
                    pos.* += 1;
                }
                var length = pos.* - group_start;
                if (pos.* < s.len and s[pos.*] == ')') {
                    length += 1;
                    pos.* += 1;
                } else {
                    pos.* = s.len;
                }
                const look_str = s[group_start .. group_start + length];
                var path = try std.ArrayList(Segment).initCapacity(allocator, 0);
                try path.append(allocator, Segment{ .str = try allocator.dupe(u8, look_str), .is_lit = false, .allocator = allocator });
                try result.append(allocator, path);
                return result;
            }
        }
        // Standard group
        const paths = try parseRE(allocator, s, pos);
        if (pos.* < s.len and s[pos.*] == ')') {
            pos.* += 1;
        } else {
            const tail = s[pos.*..];
            for (paths.items) |*path| {
                try path.append(allocator, Segment{ .str = try allocator.dupe(u8, tail), .is_lit = false, .allocator = allocator });
            }
        }
        return paths;
    } else if (c == '^' or c == '$') {
        pos.* += 1;
        var path = try std.ArrayList(Segment).initCapacity(allocator, 0);
        // Allocate single character efficiently instead of using allocPrint
        const single = try allocator.alloc(u8, 1);
        single[0] = c;
        try path.append(allocator, Segment{ .str = single, .is_lit = false, .allocator = allocator });
        try result.append(allocator, path);
        return result;
    } else {
        pos.* += 1;
        var path = try std.ArrayList(Segment).initCapacity(allocator, 0);
        // Allocate single character efficiently instead of using allocPrint
        const single = try allocator.alloc(u8, 1);
        errdefer allocator.free(single);
        single[0] = c;
        try path.append(allocator, try Segment.initLiteral(allocator, single));
        allocator.free(single); // Free after initLiteral duplicates it
        try result.append(allocator, path);
        return result;
    }
}

fn parseTerm(allocator: std.mem.Allocator, s: []const u8, pos: *usize) !std.ArrayList(std.ArrayList(Segment)) {
    var paths = try parseAtom(allocator, s, pos);
    errdefer {
        for (paths.items) |*path| {
            for (path.items) |*seg| seg.deinit();
            path.deinit(allocator);
        }
        paths.deinit(allocator);
    }

    if (paths.items.len == 0) return paths;

    var quant_str: []const u8 = "";
    var has_quant = false;
    if (pos.* < s.len) {
        const qc = s[pos.*];
        if (qc == '*' or qc == '+' or qc == '?') {
            quant_str = s[pos.* .. pos.* + 1];
            pos.* += 1;
            has_quant = true;
        } else if (qc == '{') {
            const brace_start = pos.*;
            pos.* += 1;
            while (pos.* < s.len and std.ascii.isDigit(s[pos.*])) pos.* += 1;
            if (pos.* < s.len and s[pos.*] == ',') pos.* += 1;
            while (pos.* < s.len and std.ascii.isDigit(s[pos.*])) pos.* += 1;
            if (pos.* < s.len and s[pos.*] == '}') {
                pos.* += 1;
                quant_str = s[brace_start..pos.*];
                has_quant = true;
            } else {
                pos.* = brace_start;
            }
        }
    }
    var full_quant = quant_str;
    if (has_quant and pos.* < s.len) {
        if (s[pos.*] == '?') {
            full_quant = s[quant_str.ptr - s.ptr .. pos.* + 1];
            pos.* += 1;
        } else if (s[pos.*] == '+') {
            full_quant = s[quant_str.ptr - s.ptr .. pos.* + 1];
            pos.* += 1;
        }
    }
    if (has_quant) {
        const quant_n = getQuantN(allocator, quant_str);
        if (quant_n.min == 0 and quant_n.max == 0) {
            const tail_start = if (full_quant.len > quant_str.len) pos.* - full_quant.len else pos.* - quant_str.len;
            const tail = s[tail_start..];
            for (paths.items) |*path| {
                try path.append(allocator, Segment{ .str = try allocator.dupe(u8, tail), .is_lit = false, .allocator = allocator });
            }
            return paths;
        }
        // Fixed rep: expand (capped)
        if (quant_n.max != std.math.maxInt(usize) and quant_n.max == quant_n.min and quant_n.min > 0 and quant_n.min < 10) {
            const n = quant_n.min;
            var repeated = try std.ArrayList(std.ArrayList(Segment)).initCapacity(allocator, 0);
            try repeated.append(allocator, try std.ArrayList(Segment).initCapacity(allocator, 0));
            var k: usize = 0;
            while (k < n) : (k += 1) {
                var new_rep = try std.ArrayList(std.ArrayList(Segment)).initCapacity(allocator, 0);
                for (repeated.items) |pre| {
                    for (paths.items) |p| {
                        // Pre-allocate capacity to reduce reallocations
                        const estimated_capacity = pre.items.len + p.items.len;
                        var np = try std.ArrayList(Segment).initCapacity(allocator, estimated_capacity);
                        // Duplicate segments to avoid sharing string memory
                        for (pre.items) |seg| {
                            try np.append(allocator, try seg.duplicate(allocator));
                        }
                        for (p.items) |seg| {
                            try np.append(allocator, try seg.duplicate(allocator));
                        }
                        try mergeAdjacentSegments(allocator, &np);
                        try new_rep.append(allocator, np);
                    }
                }
                for (repeated.items) |*path| {
                    for (path.items) |*seg| seg.deinit();
                    path.deinit(allocator);
                }
                repeated.deinit(allocator);
                repeated = new_rep;
            }
            for (paths.items) |*path| {
                for (path.items) |*seg| seg.deinit();
                path.deinit(allocator);
            }
            paths.deinit(allocator);
            return repeated;
        }
        // Variable: append to last
        var new_paths = try std.ArrayList(std.ArrayList(Segment)).initCapacity(allocator, 0);
        for (paths.items) |p| {
            var new_p = try std.ArrayList(Segment).initCapacity(allocator, 0);
            // Duplicate segments to avoid sharing string memory
            for (p.items) |seg| {
                try new_p.append(allocator, try seg.duplicate(allocator));
            }
            if (new_p.items.len > 0) {
                const old_str = new_p.items[new_p.items.len - 1].str;
                // Concatenate efficiently without allocPrint
                const total_len = old_str.len + full_quant.len;
                const combined = try allocator.alloc(u8, total_len);
                @memcpy(combined[0..old_str.len], old_str);
                @memcpy(combined[old_str.len..], full_quant);
                if (new_p.items[new_p.items.len - 1].allocator) |a| a.free(old_str);
                new_p.items[new_p.items.len - 1].str = combined;
                new_p.items[new_p.items.len - 1].is_lit = false;
                new_p.items[new_p.items.len - 1].allocator = allocator;
            } else {
                try new_p.append(allocator, Segment{ .str = try allocator.dupe(u8, full_quant), .is_lit = false, .allocator = allocator });
            }
            try mergeAdjacentSegments(allocator, &new_p);
            try new_paths.append(allocator, new_p);
        }
        if (quant_n.min == 0) {
            // Append empty path at the END to ensure longer patterns match first
            // in PCRE2 alternation (first alternative wins)
            const empty_path = try std.ArrayList(Segment).initCapacity(allocator, 0);
            try new_paths.append(allocator, empty_path);
        }
        for (paths.items) |*path| {
            for (path.items) |*seg| seg.deinit();
            path.deinit(allocator);
        }
        paths.deinit(allocator);
        return new_paths;
    }
    return paths;
}

fn parseConcat(allocator: std.mem.Allocator, s: []const u8, pos: *usize) !std.ArrayList(std.ArrayList(Segment)) {
    var sub_groups = try std.ArrayList(std.ArrayList(std.ArrayList(Segment))).initCapacity(allocator, 0);
    errdefer {
        for (sub_groups.items) |*group| {
            for (group.items) |*path| {
                for (path.items) |*seg| seg.deinit();
                path.deinit(allocator);
            }
            group.deinit(allocator);
        }
        sub_groups.deinit(allocator);
    }

    while (pos.* < s.len and s[pos.*] != '|' and s[pos.*] != ')') {
        const sub = try parseTerm(allocator, s, pos);
        if (sub.items.len > 0) {
            try sub_groups.append(allocator, sub);
        } else break;
    }

    var current = try std.ArrayList(std.ArrayList(Segment)).initCapacity(allocator, 0);
    try current.append(allocator, try std.ArrayList(Segment).initCapacity(allocator, 0));
    errdefer {
        for (current.items) |*path| {
            for (path.items) |*seg| seg.deinit();
            path.deinit(allocator);
        }
        current.deinit(allocator);
    }

    for (sub_groups.items) |group| {
        var new_current = try std.ArrayList(std.ArrayList(Segment)).initCapacity(allocator, 0);
        try new_current.ensureTotalCapacity(allocator, current.items.len * group.items.len);
        for (current.items) |prefix| {
            for (group.items) |suffix_group| {
                if (new_current.items.len >= MAX_PATHS) break;
                // Pre-allocate capacity for the new path to reduce reallocations
                const estimated_capacity = prefix.items.len + suffix_group.items.len;
                var new_path = try std.ArrayList(Segment).initCapacity(allocator, estimated_capacity);
                // Duplicate segments to avoid sharing string memory
                for (prefix.items) |seg| {
                    try new_path.append(allocator, try seg.duplicate(allocator));
                }
                for (suffix_group.items) |seg| {
                    try new_path.append(allocator, try seg.duplicate(allocator));
                }
                try mergeAdjacentSegments(allocator, &new_path);
                try new_current.append(allocator, new_path);
            }
            if (new_current.items.len >= MAX_PATHS) break;
        }
        for (current.items) |*path| {
            for (path.items) |*seg| seg.deinit();
            path.deinit(allocator);
        }
        current.deinit(allocator);
        current = new_current;
        if (current.items.len == 0) {
            for (sub_groups.items) |*group2| {
                for (group2.items) |*path| {
                    for (path.items) |*seg| seg.deinit();
                    path.deinit(allocator);
                }
                group2.deinit(allocator);
            }
            sub_groups.deinit(allocator);
            return current;
        }
    }
    for (sub_groups.items) |*group| {
        for (group.items) |*path| {
            for (path.items) |*seg| seg.deinit();
            path.deinit(allocator);
        }
        group.deinit(allocator);
    }
    sub_groups.deinit(allocator);
    return current;
}

fn parseAlt(allocator: std.mem.Allocator, s: []const u8, pos: *usize) !std.ArrayList(std.ArrayList(Segment)) {
    var paths = try parseConcat(allocator, s, pos);
    errdefer {
        for (paths.items) |*path| {
            for (path.items) |*seg| seg.deinit();
            path.deinit(allocator);
        }
        paths.deinit(allocator);
    }

    while (pos.* < s.len and s[pos.*] == '|') {
        pos.* += 1;
        var sub = try parseConcat(allocator, s, pos);
        // Duplicate paths to avoid sharing segment memory
        for (sub.items) |sub_path| {
            var new_path = try std.ArrayList(Segment).initCapacity(allocator, 0);
            for (sub_path.items) |seg| {
                try new_path.append(allocator, try seg.duplicate(allocator));
            }
            try paths.append(allocator, new_path);
        }
        // Now we can safely deinit sub since we've duplicated everything
        for (sub.items) |*path| {
            for (path.items) |*seg| seg.deinit();
            path.deinit(allocator);
        }
        sub.deinit(allocator);
    }
    if (pos.* < s.len and s[pos.*] != ')') {
        const tail = s[pos.*..];
        for (paths.items) |*path| {
            try path.append(allocator, Segment{ .str = try allocator.dupe(u8, tail), .is_lit = false, .allocator = allocator });
        }
    }
    return paths;
}

fn parseRE(allocator: std.mem.Allocator, s: []const u8, pos: *usize) std.mem.Allocator.Error!std.ArrayList(std.ArrayList(Segment)) {
    return parseAlt(allocator, s, pos);
}

// Check if a string is a pure literal (no regex special characters)
pub fn isPureLiteral(str: []const u8) bool {
    for (str) |c| {
        switch (c) {
            '.', '*', '+', '?', '|', '(', ')', '[', ']', '{', '}', '^', '$', '\\' => return false,
            else => {},
        }
    }
    return true;
}

pub fn regexSplitting(allocator: std.mem.Allocator, pattern: []const u8) !std.ArrayList(std.ArrayList(Segment)) {
    // Fast path: if it's a pure literal, return a single path with one literal segment
    if (isPureLiteral(pattern)) {
        var paths = try std.ArrayList(std.ArrayList(Segment)).initCapacity(allocator, 1);
        var path = try std.ArrayList(Segment).initCapacity(allocator, 1);
        try path.append(allocator, Segment{ .str = try allocator.dupe(u8, pattern), .is_lit = true, .allocator = allocator });
        try paths.append(allocator, path);
        return paths;
    }

    var pos: usize = 0;
    var paths = try parseRE(allocator, pattern, &pos);
    errdefer {
        for (paths.items) |*path| {
            for (path.items) |*seg| seg.deinit();
            path.deinit(allocator);
        }
        paths.deinit(allocator);
    }

    if (pos > pattern.len) pos = pattern.len;
    if (pos < pattern.len) {
        const tail = pattern[pos..];
        if (paths.items.len == 0) {
            var path = try std.ArrayList(Segment).initCapacity(allocator, 0);
            try path.append(allocator, Segment{ .str = try allocator.dupe(u8, tail), .is_lit = false, .allocator = allocator });
            try paths.append(allocator, path);
        } else {
            for (paths.items) |*path| {
                try path.append(allocator, Segment{ .str = try allocator.dupe(u8, tail), .is_lit = false, .allocator = allocator });
            }
        }
    }
    return paths;
}

pub fn regexSplittingWithValidation(allocator: std.mem.Allocator, pattern: []const u8) !std.ArrayList(std.ArrayList(Segment)) {
    var paths = try regexSplitting(allocator, pattern);
    errdefer {
        for (paths.items) |*path| {
            for (path.items) |*seg| seg.deinit();
            path.deinit(allocator);
        }
        paths.deinit(allocator);
    }

    for (paths.items) |*path| {
        for (path.items) |*seg| {
            if (!seg.is_lit and !validateSegment(allocator, seg.str, false)) {
                seg.deinit();
                seg.str = "";
            }
        }
    }
    return paths;
}
