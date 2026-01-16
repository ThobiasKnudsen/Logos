//! Type System for the Logos Language
//!
//! Defines the type representation, type utilities, and built-in types
//! for the Logos mathematical expression language.

const std = @import("std");
const ast_node = @import("ast_node.zig");

pub const PrimitiveType = ast_node.PrimitiveType;

/// Type representation for the Logos type system
pub const Type = union(enum) {
    /// Primitive types: f32, f64, i32, i64, bool, vec2, vec3, vec4
    primitive: PrimitiveType,

    /// Tuple type: (T1, T2, ..., Tn)
    tuple: []const *const Type,

    /// Function type: (T1, T2, ...) -> R
    function: FunctionType,

    /// Axis type with optional properties (.min, .max, .res)
    axis: AxisType,

    /// Array type: [n]T
    array: ArrayType,

    /// Unknown type (before inference)
    unknown,

    /// Type error (propagates through expressions)
    err,

    /// Void type (for statements with no value)
    void,

    pub const FunctionType = struct {
        params: []const *const Type,
        return_type: *const Type,
    };

    pub const AxisType = struct {
        has_min: bool = true,
        has_max: bool = true,
        has_res: bool = true,
    };

    pub const ArrayType = struct {
        element_type: *const Type,
        length: ?usize, // null for dynamic arrays
    };

    /// Check type equality
    pub fn eql(self: *const Type, other: *const Type) bool {
        return switch (self.*) {
            .primitive => |p1| switch (other.*) {
                .primitive => |p2| p1 == p2,
                else => false,
            },
            .tuple => |t1| switch (other.*) {
                .tuple => |t2| {
                    if (t1.len != t2.len) return false;
                    for (t1, t2) |e1, e2| {
                        if (!e1.eql(e2)) return false;
                    }
                    return true;
                },
                else => false,
            },
            .function => |f1| switch (other.*) {
                .function => |f2| {
                    if (f1.params.len != f2.params.len) return false;
                    for (f1.params, f2.params) |p1, p2| {
                        if (!p1.eql(p2)) return false;
                    }
                    return f1.return_type.eql(f2.return_type);
                },
                else => false,
            },
            .axis => |a1| switch (other.*) {
                .axis => |a2| a1.has_min == a2.has_min and
                    a1.has_max == a2.has_max and
                    a1.has_res == a2.has_res,
                else => false,
            },
            .array => |arr1| switch (other.*) {
                .array => |arr2| arr1.element_type.eql(arr2.element_type) and
                    arr1.length == arr2.length,
                else => false,
            },
            .unknown => other.* == .unknown,
            .err => other.* == .err,
            .void => other.* == .void,
        };
    }

    /// Check if types are compatible (same type or can be implicitly converted)
    pub fn isCompatible(self: *const Type, other: *const Type) bool {
        if (self.eql(other)) return true;

        // Allow numeric type promotions
        return switch (self.*) {
            .primitive => |p1| switch (other.*) {
                .primitive => |p2| isNumericPromotion(p1, p2),
                else => false,
            },
            else => false,
        };
    }

    /// Check if this type is numeric (can be used in arithmetic)
    pub fn isNumeric(self: *const Type) bool {
        return switch (self.*) {
            .primitive => |p| switch (p) {
                .f32, .f64, .i32, .i64 => true,
                .vec2, .vec3, .vec4 => true, // Vector types support arithmetic
                else => false,
            },
            else => false,
        };
    }

    /// Check if this type is a scalar (not a vector or tuple)
    pub fn isScalar(self: *const Type) bool {
        return switch (self.*) {
            .primitive => |p| switch (p) {
                .f32, .f64, .i32, .i64, .bool => true,
                else => false,
            },
            else => false,
        };
    }

    /// Check if this type is a vector type
    pub fn isVector(self: *const Type) bool {
        return switch (self.*) {
            .primitive => |p| switch (p) {
                .vec2, .vec3, .vec4 => true,
                else => false,
            },
            else => false,
        };
    }

    /// Get vector dimension (0 for non-vectors)
    pub fn vectorDimension(self: *const Type) usize {
        return switch (self.*) {
            .primitive => |p| switch (p) {
                .vec2 => 2,
                .vec3 => 3,
                .vec4 => 4,
                else => 0,
            },
            else => 0,
        };
    }

    /// Format type for error messages
    pub fn format(
        self: *const Type,
        comptime _: []const u8,
        _: std.fmt.FormatOptions,
        writer: anytype,
    ) !void {
        switch (self.*) {
            .primitive => |p| try writer.writeAll(p.toGlsl()),
            .tuple => |elements| {
                try writer.writeByte('(');
                for (elements, 0..) |elem, i| {
                    if (i > 0) try writer.writeAll(", ");
                    try elem.format("", .{}, writer);
                }
                try writer.writeByte(')');
            },
            .function => |f| {
                try writer.writeAll("fn(");
                for (f.params, 0..) |param, i| {
                    if (i > 0) try writer.writeAll(", ");
                    try param.format("", .{}, writer);
                }
                try writer.writeAll(") -> ");
                try f.return_type.format("", .{}, writer);
            },
            .axis => try writer.writeAll("axis"),
            .array => |arr| {
                try writer.writeByte('[');
                if (arr.length) |len| {
                    try writer.print("{}", .{len});
                }
                try writer.writeByte(']');
                try arr.element_type.format("", .{}, writer);
            },
            .unknown => try writer.writeAll("?"),
            .err => try writer.writeAll("<error>"),
            .void => try writer.writeAll("void"),
        }
    }
};

/// Check if promotion from t1 to t2 is valid
fn isNumericPromotion(t1: PrimitiveType, t2: PrimitiveType) bool {
    // i32 -> i64, f32 -> f64, i32 -> f32, i32 -> f64, i64 -> f64
    return switch (t1) {
        .i32 => t2 == .i64 or t2 == .f32 or t2 == .f64,
        .i64 => t2 == .f64,
        .f32 => t2 == .f64,
        else => false,
    };
}

/// Common built-in types (singleton instances)
pub const Builtins = struct {
    // Primitives
    pub const f32_type: Type = .{ .primitive = .f32 };
    pub const f64_type: Type = .{ .primitive = .f64 };
    pub const i32_type: Type = .{ .primitive = .i32 };
    pub const i64_type: Type = .{ .primitive = .i64 };
    pub const bool_type: Type = .{ .primitive = .bool };

    // Vectors
    pub const vec2_type: Type = .{ .primitive = .vec2 };
    pub const vec3_type: Type = .{ .primitive = .vec3 };
    pub const vec4_type: Type = .{ .primitive = .vec4 };

    // Special types
    pub const unknown_type: Type = .unknown;
    pub const error_type: Type = .err;
    pub const void_type: Type = .void;

    // Default axis type (has all properties)
    pub const axis_type: Type = .{ .axis = .{ .has_min = true, .has_max = true, .has_res = true } };
};

/// Symbol information in the type environment
pub const Symbol = struct {
    name: []const u8,
    sym_type: *const Type,
    is_mutable: bool,
    is_defined: bool, // false for forward references
    definition_span: ?ast_node.SourceSpan,

    pub fn format(
        self: Symbol,
        comptime _: []const u8,
        _: std.fmt.FormatOptions,
        writer: anytype,
    ) !void {
        try writer.print("{s}: ", .{self.name});
        try self.sym_type.format("", .{}, writer);
        if (!self.is_defined) try writer.writeAll(" (undefined)");
    }
};

/// Type environment / scope for type checking
pub const TypeEnv = struct {
    symbols: std.StringHashMap(Symbol),
    parent: ?*TypeEnv,
    allocator: std.mem.Allocator,

    /// Arena for allocating types during type checking
    type_arena: std.mem.Allocator,

    pub fn init(allocator: std.mem.Allocator, type_arena: std.mem.Allocator) TypeEnv {
        return .{
            .symbols = std.StringHashMap(Symbol).init(allocator),
            .parent = null,
            .allocator = allocator,
            .type_arena = type_arena,
        };
    }

    pub fn initChild(parent: *TypeEnv) TypeEnv {
        return .{
            .symbols = std.StringHashMap(Symbol).init(parent.allocator),
            .parent = parent,
            .allocator = parent.allocator,
            .type_arena = parent.type_arena,
        };
    }

    pub fn deinit(self: *TypeEnv) void {
        self.symbols.deinit();
    }

    /// Look up a symbol, checking parent scopes
    pub fn lookup(self: *const TypeEnv, name: []const u8) ?Symbol {
        if (self.symbols.get(name)) |sym| {
            return sym;
        }
        if (self.parent) |parent| {
            return parent.lookup(name);
        }
        return null;
    }

    /// Define a new symbol in this scope
    pub fn define(self: *TypeEnv, name: []const u8, sym_type: *const Type, span: ?ast_node.SourceSpan) !void {
        try self.symbols.put(name, .{
            .name = name,
            .sym_type = sym_type,
            .is_mutable = false,
            .is_defined = true,
            .definition_span = span,
        });
    }

    /// Check if a name is defined in this scope (not parent scopes)
    pub fn isDefinedLocally(self: *const TypeEnv, name: []const u8) bool {
        return self.symbols.contains(name);
    }

    /// Allocate a new type in the type arena
    pub fn allocType(self: *TypeEnv, t: Type) !*const Type {
        const ptr = try self.type_arena.create(Type);
        ptr.* = t;
        return ptr;
    }

    /// Create a tuple type
    pub fn makeTupleType(self: *TypeEnv, elements: []const *const Type) !*const Type {
        const elems_copy = try self.type_arena.dupe(*const Type, elements);
        return self.allocType(.{ .tuple = elems_copy });
    }

    /// Create a function type
    pub fn makeFunctionType(self: *TypeEnv, params: []const *const Type, return_type: *const Type) !*const Type {
        const params_copy = try self.type_arena.dupe(*const Type, params);
        return self.allocType(.{ .function = .{ .params = params_copy, .return_type = return_type } });
    }
};

/// Built-in function signatures
pub const BuiltinFunctions = struct {
    /// Get the type signature of a built-in function
    pub fn getSignature(name: []const u8) ?BuiltinSignature {
        // Math functions (f32 -> f32)
        const unary_math = [_][]const u8{
            "sin",  "cos",   "tan",   "asin", "acos",  "atan",
            "sinh", "cosh",  "tanh",  "log",  "log2",  "log10",
            "exp",  "exp2",  "sqrt",  "abs",  "sign",  "floor",
            "ceil", "round", "fract",
        };
        for (unary_math) |fn_name| {
            if (std.mem.eql(u8, name, fn_name)) {
                return .{ .kind = .unary_math };
            }
        }

        // Binary math functions (f32, f32 -> f32)
        const binary_math = [_][]const u8{ "pow", "mod", "min", "max", "step" };
        for (binary_math) |fn_name| {
            if (std.mem.eql(u8, name, fn_name)) {
                return .{ .kind = .binary_math };
            }
        }

        // Special functions
        if (std.mem.eql(u8, name, "clamp")) return .{ .kind = .clamp };
        if (std.mem.eql(u8, name, "mix")) return .{ .kind = .mix };
        if (std.mem.eql(u8, name, "smoothstep")) return .{ .kind = .smoothstep };

        // Vector functions
        if (std.mem.eql(u8, name, "len") or std.mem.eql(u8, name, "length")) return .{ .kind = .length };
        if (std.mem.eql(u8, name, "normalize")) return .{ .kind = .normalize };
        if (std.mem.eql(u8, name, "dot")) return .{ .kind = .dot };
        if (std.mem.eql(u8, name, "cross")) return .{ .kind = .cross };

        return null;
    }

    pub const BuiltinSignature = struct {
        kind: Kind,

        pub const Kind = enum {
            unary_math, // f32 -> f32 (or matching vector type)
            binary_math, // (f32, f32) -> f32
            clamp, // (f32, f32, f32) -> f32
            mix, // (f32, f32, f32) -> f32
            smoothstep, // (f32, f32, f32) -> f32
            length, // vec -> f32
            normalize, // vec -> vec
            dot, // (vec, vec) -> f32
            cross, // (vec3, vec3) -> vec3
        };
    };
};

/// Reserved identifiers in the Logos language
pub const ReservedNames = struct {
    /// Check if a name is a reserved axis variable
    pub fn isAxisVariable(name: []const u8) bool {
        const axis_names = [_][]const u8{
            "axis1", "axis2", "axis3",
            "x",     "y",     "z", // Common aliases
        };
        for (axis_names) |axis| {
            if (std.mem.eql(u8, name, axis)) return true;
        }
        return false;
    }

    /// Check if a name is a reserved output variable
    pub fn isOutputVariable(name: []const u8) bool {
        const output_names = [_][]const u8{ "red", "green", "blue", "alpha" };
        for (output_names) |output| {
            if (std.mem.eql(u8, name, output)) return true;
        }
        return false;
    }

    /// Check if a name is a reserved time variable
    pub fn isTimeVariable(name: []const u8) bool {
        return std.mem.eql(u8, name, "time");
    }

    /// Check if a name is any reserved identifier
    pub fn isReserved(name: []const u8) bool {
        return isAxisVariable(name) or isOutputVariable(name) or isTimeVariable(name);
    }

    /// Get the type for a reserved name
    pub fn getType(name: []const u8) ?*const Type {
        if (isAxisVariable(name)) return &Builtins.axis_type;
        if (isOutputVariable(name)) return &Builtins.f32_type;
        if (isTimeVariable(name)) return &Builtins.f32_type;
        return null;
    }
};
