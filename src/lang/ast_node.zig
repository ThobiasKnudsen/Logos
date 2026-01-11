//! AST Node definitions for the Logos language
//!
//! Logos is a mathematical expression language that compiles to GLSL
//! fragment shaders for GPU rendering. This module defines all AST
//! node types used by the parser and code generators.

const std = @import("std");

/// Binary operators
pub const BinaryOp = enum {
    // Arithmetic
    add, // +
    sub, // -
    mul, // *
    div, // /
    pow, // ^

    // Comparison
    eq, // =
    neq, // !=
    lt, // <
    gt, // >
    lte, // <=
    gte, // >=

    // Logical
    @"and", // and
    @"or", // or

    /// Convert to GLSL operator string
    pub fn toGlsl(self: BinaryOp) []const u8 {
        return switch (self) {
            .add => "+",
            .sub => "-",
            .mul => "*",
            .div => "/",
            .pow => "pow", // GLSL uses pow(a, b) function
            .eq => "==",
            .neq => "!=",
            .lt => "<",
            .gt => ">",
            .lte => "<=",
            .gte => ">=",
            .@"and" => "&&",
            .@"or" => "||",
        };
    }

    /// Check if this is a comparison operator (returns bool)
    pub fn isComparison(self: BinaryOp) bool {
        return switch (self) {
            .eq, .neq, .lt, .gt, .lte, .gte => true,
            else => false,
        };
    }

    /// Check if this is a logical operator
    pub fn isLogical(self: BinaryOp) bool {
        return switch (self) {
            .@"and", .@"or" => true,
            else => false,
        };
    }
};

/// Unary operators
pub const UnaryOp = enum {
    neg, // -x (negation)
    not, // !x (logical not)
    square, // x² (parsed from ² suffix)

    /// Convert to GLSL
    pub fn toGlsl(self: UnaryOp) []const u8 {
        return switch (self) {
            .neg => "-",
            .not => "!",
            .square => "", // Handled specially: emits (x * x)
        };
    }
};

/// Primitive types in the Logos type system
pub const PrimitiveType = enum {
    f32,
    f64,
    i32,
    i64,
    bool,
    vec2,
    vec3,
    vec4,

    /// Convert to GLSL type name
    pub fn toGlsl(self: PrimitiveType) []const u8 {
        return switch (self) {
            .f32 => "float",
            .f64 => "double",
            .i32 => "int",
            .i64 => "int64_t", // GLSL extension
            .bool => "bool",
            .vec2 => "vec2",
            .vec3 => "vec3",
            .vec4 => "vec4",
        };
    }
};

/// Binding pattern for variable declarations
pub const BindingPattern = union(enum) {
    /// Single variable: `x: expr`
    single: []const u8,

    /// Tuple destructuring: `(a, b, c): expr`
    tuple: []const []const u8,

    pub fn format(
        self: BindingPattern,
        comptime _: []const u8,
        _: std.fmt.FormatOptions,
        writer: anytype,
    ) !void {
        switch (self) {
            .single => |name| try writer.writeAll(name),
            .tuple => |names| {
                try writer.writeByte('(');
                for (names, 0..) |name, i| {
                    if (i > 0) try writer.writeAll(", ");
                    try writer.writeAll(name);
                }
                try writer.writeByte(')');
            },
        }
    }
};

/// Source location for error reporting
pub const SourceSpan = struct {
    start: usize, // Byte offset
    end: usize, // Byte offset (exclusive)

    pub fn merge(self: SourceSpan, other: SourceSpan) SourceSpan {
        return .{
            .start = @min(self.start, other.start),
            .end = @max(self.end, other.end),
        };
    }
};

/// AST Node - represents all syntactic constructs in Logos
pub const AstNode = struct {
    /// The kind of node and its associated data
    data: Data,

    /// Source location for error reporting
    span: SourceSpan,

    /// Type information (populated during type checking)
    /// null before type checking pass
    resolved_type: ?*const Type = null,

    pub const Data = union(enum) {
        // ============ Literals ============

        /// Numeric literal: `42`, `3.14`, `1e-5`
        number: f64,

        /// Identifier reference: `x`, `foo`, `axis1`
        identifier: []const u8,

        /// Boolean literal: `true`, `false`
        bool_lit: bool,

        // ============ Expressions ============

        /// Binary operation: `a + b`, `x and y`
        binary_op: struct {
            op: BinaryOp,
            left: *AstNode,
            right: *AstNode,
        },

        /// Unary operation: `-x`, `!cond`, `x²`
        unary_op: struct {
            op: UnaryOp,
            operand: *AstNode,
        },

        /// Function call: `sin(x)`, `mandelbrot(x, y)`
        function_call: struct {
            /// Function name or expression
            callee: *AstNode,
            /// Arguments
            args: []*AstNode,
        },

        /// Property access: `x.max`, `axis1.res`
        property_access: struct {
            base: *AstNode,
            property: []const u8,
        },

        /// Tuple literal: `(a, b, c)`
        tuple: []*AstNode,

        /// Type cast: `f32(x)`, `i32(y)`
        cast: struct {
            target_type: PrimitiveType,
            operand: *AstNode,
        },

        /// Array/tuple indexing: `A[i]`
        index: struct {
            base: *AstNode,
            index_expr: *AstNode,
        },

        // ============ Statements ============

        /// Variable binding: `x: expr` or `(a, b): expr`
        binding: struct {
            pattern: BindingPattern,
            value: *AstNode,
        },

        /// If expression: `if (cond) (then) else (else)`
        /// Both branches must have same type
        if_expr: struct {
            condition: *AstNode,
            then_branch: *AstNode,
            else_branch: ?*AstNode, // null if no else
        },

        /// While loop: `while (cond) (body)`
        while_loop: struct {
            condition: *AstNode,
            body: *AstNode,
        },

        /// For loop: `for (init, cond, update) (body)`
        for_loop: struct {
            init: *AstNode,
            condition: *AstNode,
            update: *AstNode,
            body: *AstNode,
        },

        /// Function definition: `name(params): (body)`
        /// Supports closures - captures populated during semantic analysis
        function_def: struct {
            name: []const u8,
            params: []const []const u8,
            body: *AstNode,
            /// Variables captured from outer scope (populated during analysis)
            captures: []const []const u8,
        },

        /// Block: sequence of statements, last is return value
        /// `(stmt1, stmt2, ..., result)`
        block: []*AstNode,
    };

    // ============ Node Construction Helpers ============

    /// Create a number literal node
    pub fn number(allocator: std.mem.Allocator, value: f64, span: SourceSpan) !*AstNode {
        const node = try allocator.create(AstNode);
        node.* = .{
            .data = .{ .number = value },
            .span = span,
        };
        return node;
    }

    /// Create an identifier node
    pub fn identifier(allocator: std.mem.Allocator, name: []const u8, span: SourceSpan) !*AstNode {
        const node = try allocator.create(AstNode);
        node.* = .{
            .data = .{ .identifier = name },
            .span = span,
        };
        return node;
    }

    /// Create a boolean literal node
    pub fn boolLit(allocator: std.mem.Allocator, value: bool, span: SourceSpan) !*AstNode {
        const node = try allocator.create(AstNode);
        node.* = .{
            .data = .{ .bool_lit = value },
            .span = span,
        };
        return node;
    }

    /// Create a binary operation node
    pub fn binaryOp(allocator: std.mem.Allocator, op: BinaryOp, left: *AstNode, right: *AstNode) !*AstNode {
        const node = try allocator.create(AstNode);
        node.* = .{
            .data = .{ .binary_op = .{ .op = op, .left = left, .right = right } },
            .span = left.span.merge(right.span),
        };
        return node;
    }

    /// Create a unary operation node
    pub fn unaryOp(allocator: std.mem.Allocator, op: UnaryOp, operand: *AstNode, span: SourceSpan) !*AstNode {
        const node = try allocator.create(AstNode);
        node.* = .{
            .data = .{ .unary_op = .{ .op = op, .operand = operand } },
            .span = span.merge(operand.span),
        };
        return node;
    }

    /// Create a function call node
    pub fn functionCall(allocator: std.mem.Allocator, callee: *AstNode, args: []*AstNode, span: SourceSpan) !*AstNode {
        const node = try allocator.create(AstNode);
        node.* = .{
            .data = .{ .function_call = .{ .callee = callee, .args = args } },
            .span = span,
        };
        return node;
    }

    /// Create a property access node
    pub fn propertyAccess(allocator: std.mem.Allocator, base: *AstNode, property: []const u8, span: SourceSpan) !*AstNode {
        const node = try allocator.create(AstNode);
        node.* = .{
            .data = .{ .property_access = .{ .base = base, .property = property } },
            .span = span,
        };
        return node;
    }

    /// Create a tuple node
    pub fn tupleLit(allocator: std.mem.Allocator, elements: []*AstNode, span: SourceSpan) !*AstNode {
        const node = try allocator.create(AstNode);
        node.* = .{
            .data = .{ .tuple = elements },
            .span = span,
        };
        return node;
    }

    /// Create a cast node
    pub fn castExpr(allocator: std.mem.Allocator, target_type: PrimitiveType, operand: *AstNode, span: SourceSpan) !*AstNode {
        const node = try allocator.create(AstNode);
        node.* = .{
            .data = .{ .cast = .{ .target_type = target_type, .operand = operand } },
            .span = span,
        };
        return node;
    }

    /// Create a binding node
    pub fn bindingNode(allocator: std.mem.Allocator, pattern: BindingPattern, value: *AstNode, span: SourceSpan) !*AstNode {
        const node = try allocator.create(AstNode);
        node.* = .{
            .data = .{ .binding = .{ .pattern = pattern, .value = value } },
            .span = span,
        };
        return node;
    }

    /// Create an if expression node
    pub fn ifExpr(allocator: std.mem.Allocator, condition: *AstNode, then_branch: *AstNode, else_branch: ?*AstNode, span: SourceSpan) !*AstNode {
        const node = try allocator.create(AstNode);
        node.* = .{
            .data = .{ .if_expr = .{
                .condition = condition,
                .then_branch = then_branch,
                .else_branch = else_branch,
            } },
            .span = span,
        };
        return node;
    }

    /// Create a while loop node
    pub fn whileLoop(allocator: std.mem.Allocator, condition: *AstNode, body: *AstNode, span: SourceSpan) !*AstNode {
        const node = try allocator.create(AstNode);
        node.* = .{
            .data = .{ .while_loop = .{ .condition = condition, .body = body } },
            .span = span,
        };
        return node;
    }

    /// Create a for loop node
    pub fn forLoop(allocator: std.mem.Allocator, init: *AstNode, condition: *AstNode, update: *AstNode, body: *AstNode, span: SourceSpan) !*AstNode {
        const node = try allocator.create(AstNode);
        node.* = .{
            .data = .{ .for_loop = .{
                .init = init,
                .condition = condition,
                .update = update,
                .body = body,
            } },
            .span = span,
        };
        return node;
    }

    /// Create a function definition node
    pub fn functionDef(allocator: std.mem.Allocator, name: []const u8, params: []const []const u8, body: *AstNode, span: SourceSpan) !*AstNode {
        const node = try allocator.create(AstNode);
        node.* = .{
            .data = .{ .function_def = .{
                .name = name,
                .params = params,
                .body = body,
                .captures = &.{}, // Populated during semantic analysis
            } },
            .span = span,
        };
        return node;
    }

    /// Create a block node
    pub fn blockNode(allocator: std.mem.Allocator, statements: []*AstNode, span: SourceSpan) !*AstNode {
        const node = try allocator.create(AstNode);
        node.* = .{
            .data = .{ .block = statements },
            .span = span,
        };
        return node;
    }

    // ============ Utility Methods ============

    /// Check if this node is an axis-dependent expression
    /// (Used for determining if curve expansion is needed)
    pub fn isAxisDependent(self: *const AstNode) bool {
        return switch (self.data) {
            .identifier => |name| {
                // Check for axis variables
                return std.mem.startsWith(u8, name, "axis") or
                    std.mem.eql(u8, name, "x") or
                    std.mem.eql(u8, name, "y") or
                    std.mem.eql(u8, name, "z");
            },
            .binary_op => |op| op.left.isAxisDependent() or op.right.isAxisDependent(),
            .unary_op => |op| op.operand.isAxisDependent(),
            .function_call => |call| {
                for (call.args) |arg| {
                    if (arg.isAxisDependent()) return true;
                }
                return false;
            },
            .property_access => |acc| acc.base.isAxisDependent(),
            .tuple => |elements| {
                for (elements) |elem| {
                    if (elem.isAxisDependent()) return true;
                }
                return false;
            },
            .cast => |c| c.operand.isAxisDependent(),
            .index => |idx| idx.base.isAxisDependent() or idx.index_expr.isAxisDependent(),
            .if_expr => |ie| ie.condition.isAxisDependent() or
                ie.then_branch.isAxisDependent() or
                (if (ie.else_branch) |eb| eb.isAxisDependent() else false),
            .block => |stmts| {
                for (stmts) |stmt| {
                    if (stmt.isAxisDependent()) return true;
                }
                return false;
            },
            else => false,
        };
    }

    /// Check if this is a boolean expression (comparison or logical)
    pub fn isBooleanExpr(self: *const AstNode) bool {
        return switch (self.data) {
            .bool_lit => true,
            .binary_op => |op| op.op.isComparison() or op.op.isLogical(),
            .unary_op => |op| op.op == .not,
            else => false,
        };
    }
};

/// Type representation for the Logos type system
pub const Type = union(enum) {
    /// Primitive type
    primitive: PrimitiveType,

    /// Tuple type: (T1, T2, ..., Tn)
    tuple: []const *const Type,

    /// Function type: (T1, T2, ...) -> R
    function: struct {
        params: []const *const Type,
        return_type: *const Type,
    },

    /// Axis type with optional properties
    axis: struct {
        has_min: bool,
        has_max: bool,
        has_res: bool,
    },

    /// Unknown type (before inference)
    unknown,

    /// Type error (propagates through expressions)
    err,

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
            .unknown => other.* == .unknown,
            .err => other.* == .err,
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
            .unknown => try writer.writeAll("?"),
            .err => try writer.writeAll("<error>"),
        }
    }
};

/// Common built-in types (for convenience)
pub const BuiltinTypes = struct {
    pub const f32_type: Type = .{ .primitive = .f32 };
    pub const f64_type: Type = .{ .primitive = .f64 };
    pub const i32_type: Type = .{ .primitive = .i32 };
    pub const bool_type: Type = .{ .primitive = .bool };
    pub const vec2_type: Type = .{ .primitive = .vec2 };
    pub const vec3_type: Type = .{ .primitive = .vec3 };
    pub const vec4_type: Type = .{ .primitive = .vec4 };
    pub const unknown_type: Type = .unknown;
    pub const error_type: Type = .err;
};
