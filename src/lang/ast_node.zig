//! AST Node definitions for the Logos language
//!
//! Logos is a mathematical expression language that compiles to GLSL
//! fragment shaders for GPU rendering. This module defines all AST
//! node types used by the parser and code generators.
//!
//! Design: All operations (binary ops, unary ops, function calls) are unified
//! into a single "apply" node. The operation name determines semantics:
//! - Builtins like "add", "sin" are looked up in a table for arity/emit style
//! - User-defined functions are resolved during semantic analysis

const std = @import("std");

/// How an operation should be emitted in generated code
pub const EmitStyle = enum {
    /// Infix: `left op right` (e.g., `a + b`)
    infix,
    /// Prefix: `op operand` (e.g., `-x`, `!x`)
    prefix,
    /// Postfix: special handling (e.g., `x²` → `x * x`)
    postfix,
    /// Function call: `name(args...)` (e.g., `sin(x)`)
    call,
};

/// Built-in operation definition
pub const Builtin = struct {
    arity: u8,
    emit_style: EmitStyle,
    /// GLSL representation (null means special handling required)
    glsl: ?[]const u8,
    /// Category for type checking
    category: Category,

    pub const Category = enum {
        /// Arithmetic: numeric -> numeric (e.g., add, mul)
        arithmetic,
        /// Comparison: numeric -> bool (e.g., eq, lt)
        comparison,
        /// Logical: bool -> bool (e.g., and, or, not)
        logical,
        /// Math function: numeric -> numeric (e.g., sin, cos)
        math,
        /// Special: custom type rules
        special,
    };

    /// Check if this is a comparison operation (returns bool)
    pub fn isComparison(self: Builtin) bool {
        return self.category == .comparison;
    }

    /// Check if this is a logical operation
    pub fn isLogical(self: Builtin) bool {
        return self.category == .logical;
    }
};

/// Table of all built-in operations
/// Operations are identified by string name and looked up here for metadata
pub const builtins = std.StaticStringMap(Builtin).initComptime(&[_]struct { []const u8, Builtin }{
    // ============ Arithmetic (infix, 2 args) ============
    .{ "add", Builtin{ .arity = 2, .emit_style = .infix, .glsl = "+", .category = .arithmetic } },
    .{ "sub", Builtin{ .arity = 2, .emit_style = .infix, .glsl = "-", .category = .arithmetic } },
    .{ "mul", Builtin{ .arity = 2, .emit_style = .infix, .glsl = "*", .category = .arithmetic } },
    .{ "div", Builtin{ .arity = 2, .emit_style = .infix, .glsl = "/", .category = .arithmetic } },
    .{ "pow", Builtin{ .arity = 2, .emit_style = .call, .glsl = "pow", .category = .arithmetic } },

    // ============ Comparison (infix, 2 args, returns bool) ============
    .{ "eq", Builtin{ .arity = 2, .emit_style = .infix, .glsl = "==", .category = .comparison } },
    .{ "neq", Builtin{ .arity = 2, .emit_style = .infix, .glsl = "!=", .category = .comparison } },
    .{ "lt", Builtin{ .arity = 2, .emit_style = .infix, .glsl = "<", .category = .comparison } },
    .{ "gt", Builtin{ .arity = 2, .emit_style = .infix, .glsl = ">", .category = .comparison } },
    .{ "lte", Builtin{ .arity = 2, .emit_style = .infix, .glsl = "<=", .category = .comparison } },
    .{ "gte", Builtin{ .arity = 2, .emit_style = .infix, .glsl = ">=", .category = .comparison } },

    // ============ Logical (2 args for binary, 1 for not) ============
    .{ "and", Builtin{ .arity = 2, .emit_style = .infix, .glsl = "&&", .category = .logical } },
    .{ "or", Builtin{ .arity = 2, .emit_style = .infix, .glsl = "||", .category = .logical } },
    .{ "not", Builtin{ .arity = 1, .emit_style = .prefix, .glsl = "!", .category = .logical } },

    // ============ Unary arithmetic (prefix, 1 arg) ============
    .{ "neg", Builtin{ .arity = 1, .emit_style = .prefix, .glsl = "-", .category = .arithmetic } },
    .{ "square", Builtin{ .arity = 1, .emit_style = .postfix, .glsl = null, .category = .arithmetic } }, // x² → (x * x)

    // ============ Math functions (call style) ============
    .{ "sin", Builtin{ .arity = 1, .emit_style = .call, .glsl = "sin", .category = .math } },
    .{ "cos", Builtin{ .arity = 1, .emit_style = .call, .glsl = "cos", .category = .math } },
    .{ "tan", Builtin{ .arity = 1, .emit_style = .call, .glsl = "tan", .category = .math } },
    .{ "asin", Builtin{ .arity = 1, .emit_style = .call, .glsl = "asin", .category = .math } },
    .{ "acos", Builtin{ .arity = 1, .emit_style = .call, .glsl = "acos", .category = .math } },
    .{ "atan", Builtin{ .arity = 1, .emit_style = .call, .glsl = "atan", .category = .math } },
    .{ "sinh", Builtin{ .arity = 1, .emit_style = .call, .glsl = "sinh", .category = .math } },
    .{ "cosh", Builtin{ .arity = 1, .emit_style = .call, .glsl = "cosh", .category = .math } },
    .{ "tanh", Builtin{ .arity = 1, .emit_style = .call, .glsl = "tanh", .category = .math } },
    .{ "log", Builtin{ .arity = 1, .emit_style = .call, .glsl = "log", .category = .math } },
    .{ "log2", Builtin{ .arity = 1, .emit_style = .call, .glsl = "log2", .category = .math } },
    .{ "log10", Builtin{ .arity = 1, .emit_style = .call, .glsl = "log10", .category = .math } },
    .{ "exp", Builtin{ .arity = 1, .emit_style = .call, .glsl = "exp", .category = .math } },
    .{ "exp2", Builtin{ .arity = 1, .emit_style = .call, .glsl = "exp2", .category = .math } },
    .{ "sqrt", Builtin{ .arity = 1, .emit_style = .call, .glsl = "sqrt", .category = .math } },
    .{ "abs", Builtin{ .arity = 1, .emit_style = .call, .glsl = "abs", .category = .math } },
    .{ "sign", Builtin{ .arity = 1, .emit_style = .call, .glsl = "sign", .category = .math } },
    .{ "floor", Builtin{ .arity = 1, .emit_style = .call, .glsl = "floor", .category = .math } },
    .{ "ceil", Builtin{ .arity = 1, .emit_style = .call, .glsl = "ceil", .category = .math } },
    .{ "round", Builtin{ .arity = 1, .emit_style = .call, .glsl = "round", .category = .math } },
    .{ "fract", Builtin{ .arity = 1, .emit_style = .call, .glsl = "fract", .category = .math } },
    .{ "mod", Builtin{ .arity = 2, .emit_style = .call, .glsl = "mod", .category = .math } },
    .{ "min", Builtin{ .arity = 2, .emit_style = .call, .glsl = "min", .category = .math } },
    .{ "max", Builtin{ .arity = 2, .emit_style = .call, .glsl = "max", .category = .math } },
    .{ "step", Builtin{ .arity = 2, .emit_style = .call, .glsl = "step", .category = .math } },
    .{ "clamp", Builtin{ .arity = 3, .emit_style = .call, .glsl = "clamp", .category = .math } },
    .{ "mix", Builtin{ .arity = 3, .emit_style = .call, .glsl = "mix", .category = .math } },
    .{ "smoothstep", Builtin{ .arity = 3, .emit_style = .call, .glsl = "smoothstep", .category = .math } },

    // ============ Vector functions ============
    .{ "len", Builtin{ .arity = 1, .emit_style = .call, .glsl = "length", .category = .special } },
    .{ "length", Builtin{ .arity = 1, .emit_style = .call, .glsl = "length", .category = .special } },
    .{ "normalize", Builtin{ .arity = 1, .emit_style = .call, .glsl = "normalize", .category = .special } },
    .{ "dot", Builtin{ .arity = 2, .emit_style = .call, .glsl = "dot", .category = .special } },
    .{ "cross", Builtin{ .arity = 2, .emit_style = .call, .glsl = "cross", .category = .special } },
});

/// Look up a builtin by name
pub fn getBuiltin(name: []const u8) ?Builtin {
    return builtins.get(name);
}

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

        /// Unified operation application: covers all operations
        /// - Binary ops: `a + b` → apply("add", [a, b])
        /// - Unary ops: `-x` → apply("neg", [x])
        /// - Function calls: `sin(x)` → apply("sin", [x])
        /// - User functions: `foo(a, b)` → apply("foo", [a, b])
        apply: struct {
            /// Operation/function name (e.g., "add", "sin", "my_func")
            name: []const u8,
            /// Arguments to the operation
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

    /// Create an apply node (unified operation)
    pub fn apply(allocator: std.mem.Allocator, name: []const u8, args: []*AstNode, span: SourceSpan) !*AstNode {
        const node = try allocator.create(AstNode);
        node.* = .{
            .data = .{ .apply = .{ .name = name, .args = args } },
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
            .apply => |op| {
                for (op.args) |arg| {
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
            .apply => |op| {
                if (getBuiltin(op.name)) |builtin| {
                    return builtin.isComparison() or builtin.isLogical();
                }
                return false;
            },
            else => false,
        };
    }

    // ============ Debug Printing ============

    /// Print AST node with indentation for debugging
    pub fn debugPrint(self: *const AstNode, depth: usize) void {
        const max_depth = 64;
        const indent = "  " ** max_depth;
        const clamped_depth = @min(depth, max_depth);
        const prefix = indent[0 .. clamped_depth * 2];

        switch (self.data) {
            .number => |n| {
                std.debug.print("{s}Number: {d}\n", .{ prefix, n });
            },
            .identifier => |name| {
                std.debug.print("{s}Identifier: {s}\n", .{ prefix, name });
            },
            .bool_lit => |b| {
                std.debug.print("{s}Bool: {}\n", .{ prefix, b });
            },
            .apply => |op| {
                const builtin_info = if (getBuiltin(op.name)) |b|
                    @tagName(b.emit_style)
                else
                    "user";
                std.debug.print("{s}Apply: {s} ({s}, {d} args)\n", .{ prefix, op.name, builtin_info, op.args.len });
                for (op.args, 0..) |arg, i| {
                    std.debug.print("{s}  arg[{d}]:\n", .{ prefix, i });
                    arg.debugPrint(depth + 2);
                }
            },
            .property_access => |acc| {
                std.debug.print("{s}PropertyAccess: .{s}\n", .{ prefix, acc.property });
                std.debug.print("{s}  base:\n", .{prefix});
                acc.base.debugPrint(depth + 2);
            },
            .tuple => |elements| {
                std.debug.print("{s}Tuple ({d} elements):\n", .{ prefix, elements.len });
                for (elements, 0..) |elem, i| {
                    std.debug.print("{s}  [{d}]:\n", .{ prefix, i });
                    elem.debugPrint(depth + 2);
                }
            },
            .cast => |c| {
                std.debug.print("{s}Cast: {s}\n", .{ prefix, @tagName(c.target_type) });
                std.debug.print("{s}  operand:\n", .{prefix});
                c.operand.debugPrint(depth + 2);
            },
            .index => |idx| {
                std.debug.print("{s}Index:\n", .{prefix});
                std.debug.print("{s}  base:\n", .{prefix});
                idx.base.debugPrint(depth + 2);
                std.debug.print("{s}  index:\n", .{prefix});
                idx.index_expr.debugPrint(depth + 2);
            },
            .binding => |b| {
                switch (b.pattern) {
                    .single => |name| std.debug.print("{s}Binding: {s}\n", .{ prefix, name }),
                    .tuple => |names| {
                        std.debug.print("{s}Binding: (", .{prefix});
                        for (names, 0..) |name, i| {
                            if (i > 0) std.debug.print(", ", .{});
                            std.debug.print("{s}", .{name});
                        }
                        std.debug.print(")\n", .{});
                    },
                }
                std.debug.print("{s}  value:\n", .{prefix});
                b.value.debugPrint(depth + 2);
            },
            .if_expr => |ie| {
                std.debug.print("{s}If:\n", .{prefix});
                std.debug.print("{s}  condition:\n", .{prefix});
                ie.condition.debugPrint(depth + 2);
                std.debug.print("{s}  then:\n", .{prefix});
                ie.then_branch.debugPrint(depth + 2);
                if (ie.else_branch) |eb| {
                    std.debug.print("{s}  else:\n", .{prefix});
                    eb.debugPrint(depth + 2);
                }
            },
            .while_loop => |wl| {
                std.debug.print("{s}While:\n", .{prefix});
                std.debug.print("{s}  condition:\n", .{prefix});
                wl.condition.debugPrint(depth + 2);
                std.debug.print("{s}  body:\n", .{prefix});
                wl.body.debugPrint(depth + 2);
            },
            .for_loop => |fl| {
                std.debug.print("{s}For:\n", .{prefix});
                std.debug.print("{s}  init:\n", .{prefix});
                fl.init.debugPrint(depth + 2);
                std.debug.print("{s}  condition:\n", .{prefix});
                fl.condition.debugPrint(depth + 2);
                std.debug.print("{s}  update:\n", .{prefix});
                fl.update.debugPrint(depth + 2);
                std.debug.print("{s}  body:\n", .{prefix});
                fl.body.debugPrint(depth + 2);
            },
            .function_def => |fd| {
                std.debug.print("{s}FunctionDef: {s}\n", .{ prefix, fd.name });
                std.debug.print("{s}  params: ", .{prefix});
                for (fd.params, 0..) |p, i| {
                    if (i > 0) std.debug.print(", ", .{});
                    std.debug.print("{s}", .{p});
                }
                std.debug.print("\n", .{});
                std.debug.print("{s}  body:\n", .{prefix});
                fd.body.debugPrint(depth + 2);
            },
            .block => |stmts| {
                std.debug.print("{s}Block ({d} statements):\n", .{ prefix, stmts.len });
                for (stmts, 0..) |stmt, i| {
                    std.debug.print("{s}  [{d}]:\n", .{ prefix, i });
                    stmt.debugPrint(depth + 2);
                }
            },
        }
    }

    /// Print the entire AST tree with a header
    pub fn debugPrintTree(self: *const AstNode) void {
        std.debug.print("\n========== AST DEBUG OUTPUT ==========\n", .{});
        self.debugPrint(0);
        std.debug.print("=======================================\n\n", .{});
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
