//! Type Checker for the Logos Language
//!
//! Performs semantic analysis including:
//! - Type inference
//! - Type validation
//! - Symbol resolution
//! - Return path analysis
//!
//! The type checker walks the AST and annotates nodes with their resolved types,
//! producing typed AST nodes suitable for code generation.

const std = @import("std");
const ast_node = @import("ast_node.zig");
const types = @import("types.zig");

const AstNode = ast_node.AstNode;
const Builtin = ast_node.Builtin;
const getBuiltin = ast_node.getBuiltin;
const SourceSpan = ast_node.SourceSpan;
const Type = types.Type;
const TypeEnv = types.TypeEnv;
const Builtins = types.Builtins;
const Symbol = types.Symbol;

/// Type checking error
pub const TypeError = struct {
    message: []const u8,
    span: SourceSpan,
    kind: Kind,

    pub const Kind = enum {
        undefined_variable,
        type_mismatch,
        invalid_operation,
        arity_mismatch,
        duplicate_definition,
        not_callable,
        invalid_assignment,
        incompatible_branches,
        invalid_property,
        not_indexable,
    };

    pub fn format(
        self: TypeError,
        comptime _: []const u8,
        _: std.fmt.FormatOptions,
        writer: anytype,
    ) !void {
        try writer.print("[{}-{}] {s}", .{ self.span.start, self.span.end, self.message });
    }
};

/// Type checker state and operations
pub const TypeChecker = struct {
    env: TypeEnv,
    errors: std.ArrayList(TypeError),
    allocator: std.mem.Allocator,

    /// Arena allocator for type objects (freed when checker is deinitialized)
    type_arena: std.heap.ArenaAllocator,

    pub fn init(allocator: std.mem.Allocator) TypeChecker {
        var arena = std.heap.ArenaAllocator.init(allocator);
        return .{
            .env = TypeEnv.init(allocator, arena.allocator()),
            .errors = std.ArrayList(TypeError).init(allocator),
            .allocator = allocator,
            .type_arena = arena,
        };
    }

    pub fn deinit(self: *TypeChecker) void {
        self.env.deinit();
        self.errors.deinit();
        self.type_arena.deinit();
    }

    /// Reset for a new check (clears errors and symbol table, keeps arena)
    pub fn reset(self: *TypeChecker) void {
        self.errors.clearRetainingCapacity();
        self.env.symbols.clearRetainingCapacity();
        _ = self.type_arena.reset(.retain_capacity);
        self.env.type_arena = self.type_arena.allocator();
    }

    /// Type check an AST, returning whether there were any errors
    pub fn check(self: *TypeChecker, node: *AstNode) bool {
        _ = self.inferType(node);
        return self.errors.items.len == 0;
    }

    /// Get all type errors
    pub fn getErrors(self: *const TypeChecker) []const TypeError {
        return self.errors.items;
    }

    /// Add an error
    fn addError(self: *TypeChecker, kind: TypeError.Kind, span: SourceSpan, comptime fmt: []const u8, args: anytype) void {
        const message = std.fmt.allocPrint(self.allocator, fmt, args) catch return;
        self.errors.append(.{
            .message = message,
            .span = span,
            .kind = kind,
        }) catch {};
    }

    /// Main type inference function - infers and annotates the type of a node
    pub fn inferType(self: *TypeChecker, node: *AstNode) *const Type {
        const inferred = self.inferTypeImpl(node);
        // Annotate the node with its resolved type
        node.resolved_type = inferred;
        return inferred;
    }

    fn inferTypeImpl(self: *TypeChecker, node: *AstNode) *const Type {
        return switch (node.data) {
            .number => &Builtins.f64_type, // All numeric literals are f64 by default
            .bool_lit => &Builtins.bool_type,
            .identifier => |name| self.inferIdentifier(name, node.span),
            .apply => |op| self.inferApply(op.name, op.args, node.span),
            .property_access => |acc| self.inferPropertyAccess(acc.base, acc.property, node.span),
            .tuple => |elements| self.inferTuple(elements, node.span),
            .cast => |c| self.inferCast(c.target_type, c.operand, node.span),
            .index => |idx| self.inferIndex(idx.base, idx.index_expr, node.span),
            .binding => |b| self.inferBinding(b.pattern, b.value, node.span),
            .if_expr => |ie| self.inferIfExpr(ie.condition, ie.then_branch, ie.else_branch, node.span),
            .while_loop => |wl| self.inferWhileLoop(wl.condition, wl.body, node.span),
            .for_loop => |fl| self.inferForLoop(fl.init, fl.condition, fl.update, fl.body, node.span),
            .function_def => |fd| self.inferFunctionDef(fd.name, fd.params, fd.body, node.span),
            .block => |stmts| self.inferBlock(stmts, node.span),
        };
    }

    fn inferIdentifier(self: *TypeChecker, name: []const u8, span: SourceSpan) *const Type {
        // Check reserved names first
        if (types.ReservedNames.getType(name)) |t| {
            return t;
        }

        // Look up in symbol table
        if (self.env.lookup(name)) |sym| {
            return sym.sym_type;
        }

        // Undefined variable
        self.addError(.undefined_variable, span, "undefined variable '{s}'", .{name});
        return &Builtins.error_type;
    }

    /// Unified type inference for all operations (binary ops, unary ops, function calls)
    fn inferApply(self: *TypeChecker, name: []const u8, args: []*AstNode, span: SourceSpan) *const Type {
        // First, check if this is a built-in operation
        if (getBuiltin(name)) |builtin| {
            return self.inferBuiltinApply(name, builtin, args, span);
        }

        // Check for type cast (f32, i32, etc.)
        if (self.isPrimitiveTypeName(name)) |prim_type| {
            if (args.len != 1) {
                self.addError(.arity_mismatch, span, "type cast expects 1 argument, got {}", .{args.len});
                return &Builtins.error_type;
            }
            _ = self.inferType(args[0]); // Check the argument
            return switch (prim_type) {
                .f32 => &Builtins.f32_type,
                .f64 => &Builtins.f64_type,
                .i32 => &Builtins.i32_type,
                .i64 => &Builtins.i64_type,
                .bool => &Builtins.bool_type,
                .vec2 => &Builtins.vec2_type,
                .vec3 => &Builtins.vec3_type,
                .vec4 => &Builtins.vec4_type,
            };
        }

        // User-defined function call - look up in symbol table
        if (self.env.lookup(name)) |sym| {
            if (sym.sym_type.* != .function) {
                self.addError(.not_callable, span, "'{s}' is not callable (type: {})", .{ name, sym.sym_type });
                return &Builtins.error_type;
            }

            const fn_type = sym.sym_type.function;

            // Check arity
            if (args.len != fn_type.params.len) {
                self.addError(.arity_mismatch, span, "'{s}' expects {} arguments, got {}", .{ name, fn_type.params.len, args.len });
                return &Builtins.error_type;
            }

            // Check argument types
            for (args, fn_type.params) |arg, expected| {
                const arg_type = self.inferType(arg);
                if (!arg_type.isCompatible(expected)) {
                    self.addError(.type_mismatch, arg.span, "expected {}, got {}", .{ expected, arg_type });
                }
            }

            return fn_type.return_type;
        }

        // Unknown function
        self.addError(.undefined_variable, span, "undefined function '{s}'", .{name});
        return &Builtins.error_type;
    }

    /// Type check a builtin operation using metadata from the builtin table
    fn inferBuiltinApply(self: *TypeChecker, name: []const u8, builtin: Builtin, args: []*AstNode, span: SourceSpan) *const Type {
        // Check arity
        if (args.len != builtin.arity) {
            self.addError(.arity_mismatch, span, "'{s}' expects {} argument(s), got {}", .{ name, builtin.arity, args.len });
            return &Builtins.error_type;
        }

        // Infer types of all arguments
        var arg_types: [8]*const Type = undefined; // Max 8 args should be plenty
        for (args, 0..) |arg, i| {
            arg_types[i] = self.inferType(arg);
            if (arg_types[i].* == .err) return &Builtins.error_type;
        }

        // Type check based on category
        return switch (builtin.category) {
            .arithmetic => self.checkArithmeticOp(name, args, arg_types[0..args.len], span),
            .comparison => self.checkComparisonOp(name, args, arg_types[0..args.len], span),
            .logical => self.checkLogicalOp(name, args, arg_types[0..args.len], span),
            .math => self.checkMathOp(name, builtin.arity, args, arg_types[0..args.len], span),
            .special => self.checkSpecialOp(name, args, arg_types[0..args.len], span),
        };
    }

    fn checkArithmeticOp(self: *TypeChecker, name: []const u8, args: []*AstNode, arg_types: []const *const Type, span: SourceSpan) *const Type {
        _ = name;
        // All args must be numeric
        for (arg_types, args) |t, arg| {
            if (!t.isNumeric()) {
                self.addError(.type_mismatch, arg.span, "arithmetic requires numeric type, got {}", .{t});
                return &Builtins.error_type;
            }
        }

        // Return promoted type for binary, same type for unary
        if (args.len == 2) {
            return self.promoteNumericTypes(arg_types[0], arg_types[1]);
        }
        return arg_types[0];
    }

    fn checkComparisonOp(self: *TypeChecker, name: []const u8, args: []*AstNode, arg_types: []const *const Type, span: SourceSpan) *const Type {
        _ = name;
        _ = span;
        // Both args must be numeric
        for (arg_types, args) |t, arg| {
            if (!t.isNumeric()) {
                self.addError(.type_mismatch, arg.span, "comparison requires numeric types, got {}", .{t});
                return &Builtins.error_type;
            }
        }
        return &Builtins.bool_type;
    }

    fn checkLogicalOp(self: *TypeChecker, name: []const u8, args: []*AstNode, arg_types: []const *const Type, span: SourceSpan) *const Type {
        _ = name;
        _ = span;
        // All args must be bool
        for (arg_types, args) |t, arg| {
            if (t.* != .primitive or t.primitive != .bool) {
                self.addError(.type_mismatch, arg.span, "logical operator requires bool, got {}", .{t});
                return &Builtins.error_type;
            }
        }
        return &Builtins.bool_type;
    }

    fn checkMathOp(self: *TypeChecker, name: []const u8, arity: u8, args: []*AstNode, arg_types: []const *const Type, span: SourceSpan) *const Type {
        _ = span;
        _ = name;
        // All args must be numeric
        for (arg_types, args) |t, arg| {
            if (!t.isNumeric()) {
                self.addError(.type_mismatch, arg.span, "math function requires numeric type, got {}", .{t});
                return &Builtins.error_type;
            }
        }

        // Return type based on arity
        if (arity == 1) {
            return arg_types[0]; // Unary: same type as input
        } else if (arity == 2) {
            return self.promoteNumericTypes(arg_types[0], arg_types[1]);
        } else {
            return arg_types[0]; // 3+ args: return first arg type
        }
    }

    fn checkSpecialOp(self: *TypeChecker, name: []const u8, args: []*AstNode, arg_types: []const *const Type, span: SourceSpan) *const Type {
        // Handle special operations with custom type rules
        if (std.mem.eql(u8, name, "len") or std.mem.eql(u8, name, "length")) {
            if (!arg_types[0].isVector() and !arg_types[0].isScalar()) {
                self.addError(.type_mismatch, span, "length expects vector or scalar, got {}", .{arg_types[0]});
                return &Builtins.error_type;
            }
            return &Builtins.f32_type;
        }

        if (std.mem.eql(u8, name, "normalize")) {
            if (!arg_types[0].isVector()) {
                self.addError(.type_mismatch, span, "normalize expects vector, got {}", .{arg_types[0]});
                return &Builtins.error_type;
            }
            return arg_types[0];
        }

        if (std.mem.eql(u8, name, "dot")) {
            if (!arg_types[0].isVector() or !arg_types[1].isVector()) {
                self.addError(.type_mismatch, span, "dot expects vectors, got {} and {}", .{ arg_types[0], arg_types[1] });
                return &Builtins.error_type;
            }
            return &Builtins.f32_type;
        }

        if (std.mem.eql(u8, name, "cross")) {
            if (arg_types[0].vectorDimension() != 3 or arg_types[1].vectorDimension() != 3) {
                self.addError(.type_mismatch, span, "cross requires vec3, got {} and {}", .{ arg_types[0], arg_types[1] });
                return &Builtins.error_type;
            }
            return &Builtins.vec3_type;
        }

        // Fallback for unknown special ops
        _ = args;
        return &Builtins.error_type;
    }

    fn inferPropertyAccess(self: *TypeChecker, base: *AstNode, property: []const u8, span: SourceSpan) *const Type {
        const base_type = self.inferType(base);

        if (base_type.* == .err) return &Builtins.error_type;

        // Axis property access (axis1.min, axis1.max, axis1.res)
        if (base_type.* == .axis) {
            if (std.mem.eql(u8, property, "min") or
                std.mem.eql(u8, property, "max") or
                std.mem.eql(u8, property, "res"))
            {
                return &Builtins.f32_type;
            }
            self.addError(.invalid_property, span, "axis has no property '{s}'", .{property});
            return &Builtins.error_type;
        }

        // Vector swizzling (v.x, v.xy, v.xyz, etc.)
        if (base_type.isVector()) {
            const dim = base_type.vectorDimension();
            // Simple single-component access
            if (property.len == 1) {
                const valid = switch (property[0]) {
                    'x', 'r' => true,
                    'y', 'g' => dim >= 2,
                    'z', 'b' => dim >= 3,
                    'w', 'a' => dim >= 4,
                    else => false,
                };
                if (valid) return &Builtins.f32_type;
            }
            // Multi-component swizzle - for now, just return error
            self.addError(.invalid_property, span, "invalid swizzle '{s}'", .{property});
            return &Builtins.error_type;
        }

        self.addError(.invalid_property, span, "type {} has no property '{s}'", .{ base_type, property });
        return &Builtins.error_type;
    }

    fn inferTuple(self: *TypeChecker, elements: []*AstNode, span: SourceSpan) *const Type {
        _ = span;

        var elem_types = std.ArrayList(*const Type).init(self.allocator);
        defer elem_types.deinit();

        for (elements) |elem| {
            const t = self.inferType(elem);
            elem_types.append(t) catch return &Builtins.error_type;
        }

        return self.env.makeTupleType(elem_types.items) catch &Builtins.error_type;
    }

    fn inferCast(self: *TypeChecker, target_type: ast_node.PrimitiveType, operand: *AstNode, span: SourceSpan) *const Type {
        const operand_type = self.inferType(operand);
        _ = span;

        if (operand_type.* == .err) return &Builtins.error_type;

        // Return the target type
        return switch (target_type) {
            .f32 => &Builtins.f32_type,
            .f64 => &Builtins.f64_type,
            .i32 => &Builtins.i32_type,
            .i64 => &Builtins.i64_type,
            .bool => &Builtins.bool_type,
            .vec2 => &Builtins.vec2_type,
            .vec3 => &Builtins.vec3_type,
            .vec4 => &Builtins.vec4_type,
        };
    }

    fn inferIndex(self: *TypeChecker, base: *AstNode, index_expr: *AstNode, span: SourceSpan) *const Type {
        const base_type = self.inferType(base);
        const index_type = self.inferType(index_expr);

        if (base_type.* == .err or index_type.* == .err) return &Builtins.error_type;

        // Check index is integer
        if (index_type.* != .primitive or (index_type.primitive != .i32 and index_type.primitive != .i64)) {
            self.addError(.type_mismatch, span, "index must be integer, got {}", .{index_type});
            return &Builtins.error_type;
        }

        // Vector indexing
        if (base_type.isVector()) {
            return &Builtins.f32_type;
        }

        // Array indexing
        if (base_type.* == .array) {
            return base_type.array.element_type;
        }

        // Tuple indexing
        if (base_type.* == .tuple) {
            // For dynamic index, we can't know the type at compile time
            // For constant index, we could extract the specific type
            // For now, return error_type to indicate we need more analysis
            return &Builtins.error_type;
        }

        self.addError(.not_indexable, span, "type {} is not indexable", .{base_type});
        return &Builtins.error_type;
    }

    fn inferBinding(self: *TypeChecker, pattern: ast_node.BindingPattern, value: *AstNode, span: SourceSpan) *const Type {
        const value_type = self.inferType(value);

        if (value_type.* == .err) return &Builtins.error_type;

        switch (pattern) {
            .single => |name| {
                // Check for redefinition in current scope
                if (self.env.isDefinedLocally(name)) {
                    self.addError(.duplicate_definition, span, "'{s}' is already defined in this scope", .{name});
                    return &Builtins.error_type;
                }

                // Check for reserved name
                if (types.ReservedNames.isReserved(name)) {
                    self.addError(.invalid_assignment, span, "cannot bind to reserved name '{s}'", .{name});
                    return &Builtins.error_type;
                }

                // Add to symbol table
                self.env.define(name, value_type, span) catch return &Builtins.error_type;
            },
            .tuple => |names| {
                // Value must be a tuple
                if (value_type.* != .tuple) {
                    self.addError(.type_mismatch, span, "expected tuple for destructuring, got {}", .{value_type});
                    return &Builtins.error_type;
                }

                if (names.len != value_type.tuple.len) {
                    self.addError(.type_mismatch, span, "tuple pattern has {} elements, but value has {}", .{ names.len, value_type.tuple.len });
                    return &Builtins.error_type;
                }

                // Bind each element
                for (names, value_type.tuple) |name, elem_type| {
                    if (self.env.isDefinedLocally(name)) {
                        self.addError(.duplicate_definition, span, "'{s}' is already defined", .{name});
                        continue;
                    }
                    if (types.ReservedNames.isReserved(name)) {
                        self.addError(.invalid_assignment, span, "cannot bind to reserved name '{s}'", .{name});
                        continue;
                    }
                    self.env.define(name, elem_type, span) catch {};
                }
            },
        }

        return value_type;
    }

    fn inferIfExpr(self: *TypeChecker, condition: *AstNode, then_branch: *AstNode, else_branch: ?*AstNode, span: SourceSpan) *const Type {
        const cond_type = self.inferType(condition);

        // Condition must be bool
        if (cond_type.* != .primitive or cond_type.primitive != .bool) {
            if (cond_type.* != .err) {
                self.addError(.type_mismatch, span, "if condition must be bool, got {}", .{cond_type});
            }
        }

        const then_type = self.inferType(then_branch);

        if (else_branch) |eb| {
            const else_type = self.inferType(eb);

            // Both branches must have compatible types
            if (!then_type.eql(else_type)) {
                self.addError(.incompatible_branches, span, "if branches have incompatible types: {} vs {}", .{ then_type, else_type });
                return &Builtins.error_type;
            }

            return then_type;
        } else {
            // No else branch - expression returns void
            return &Builtins.void_type;
        }
    }

    fn inferWhileLoop(self: *TypeChecker, condition: *AstNode, body: *AstNode, span: SourceSpan) *const Type {
        const cond_type = self.inferType(condition);

        if (cond_type.* != .primitive or cond_type.primitive != .bool) {
            if (cond_type.* != .err) {
                self.addError(.type_mismatch, span, "while condition must be bool, got {}", .{cond_type});
            }
        }

        // Create child scope for loop body
        var body_env = TypeEnv.initChild(&self.env);
        defer body_env.deinit();
        const old_env = self.env;
        self.env = body_env;
        defer self.env = old_env;

        _ = self.inferType(body);

        return &Builtins.void_type;
    }

    fn inferForLoop(self: *TypeChecker, init: *AstNode, condition: *AstNode, update: *AstNode, body: *AstNode, span: SourceSpan) *const Type {
        // Create child scope for loop
        var loop_env = TypeEnv.initChild(&self.env);
        defer loop_env.deinit();
        const old_env = self.env;
        self.env = loop_env;
        defer self.env = old_env;

        // Check init
        _ = self.inferType(init);

        // Check condition
        const cond_type = self.inferType(condition);
        if (cond_type.* != .primitive or cond_type.primitive != .bool) {
            if (cond_type.* != .err) {
                self.addError(.type_mismatch, span, "for condition must be bool, got {}", .{cond_type});
            }
        }

        // Check update
        _ = self.inferType(update);

        // Check body
        _ = self.inferType(body);

        return &Builtins.void_type;
    }

    fn inferFunctionDef(self: *TypeChecker, name: []const u8, params: []const []const u8, body: *AstNode, span: SourceSpan) *const Type {
        // Check for redefinition
        if (self.env.isDefinedLocally(name)) {
            self.addError(.duplicate_definition, span, "function '{s}' is already defined", .{name});
            return &Builtins.error_type;
        }

        // Create child scope for function body
        var fn_env = TypeEnv.initChild(&self.env);
        defer fn_env.deinit();

        // Add parameters to function scope (as unknown type for now)
        var param_types = std.ArrayList(*const Type).init(self.allocator);
        defer param_types.deinit();

        for (params) |param| {
            // Parameters have unknown type until we implement type annotations
            const param_type = &Builtins.unknown_type;
            fn_env.define(param, param_type, null) catch return &Builtins.error_type;
            param_types.append(param_type) catch return &Builtins.error_type;
        }

        // Type check body in function scope
        const old_env = self.env;
        self.env = fn_env;
        const return_type = self.inferType(body);
        self.env = old_env;

        // Create function type
        const fn_type = self.env.makeFunctionType(param_types.items, return_type) catch return &Builtins.error_type;

        // Add function to outer scope
        self.env.define(name, fn_type, span) catch return &Builtins.error_type;

        return fn_type;
    }

    fn inferBlock(self: *TypeChecker, stmts: []*AstNode, span: SourceSpan) *const Type {
        _ = span;

        if (stmts.len == 0) {
            return &Builtins.void_type;
        }

        // Create child scope for block
        var block_env = TypeEnv.initChild(&self.env);
        defer block_env.deinit();
        const old_env = self.env;
        self.env = block_env;
        defer self.env = old_env;

        // Type check all statements
        var last_type: *const Type = &Builtins.void_type;
        for (stmts) |stmt| {
            last_type = self.inferType(stmt);
        }

        // Block returns the type of the last expression
        return last_type;
    }

    // Helper functions

    fn promoteNumericTypes(self: *TypeChecker, t1: *const Type, t2: *const Type) *const Type {
        _ = self;

        // If either is f64, result is f64
        if ((t1.* == .primitive and t1.primitive == .f64) or
            (t2.* == .primitive and t2.primitive == .f64))
        {
            return &Builtins.f64_type;
        }

        // If either is f32, result is f32
        if ((t1.* == .primitive and t1.primitive == .f32) or
            (t2.* == .primitive and t2.primitive == .f32))
        {
            return &Builtins.f32_type;
        }

        // If either is i64, result is i64
        if ((t1.* == .primitive and t1.primitive == .i64) or
            (t2.* == .primitive and t2.primitive == .i64))
        {
            return &Builtins.i64_type;
        }

        // Default to i32
        return &Builtins.i32_type;
    }

    fn isPrimitiveTypeName(self: *TypeChecker, name: []const u8) ?ast_node.PrimitiveType {
        _ = self;
        if (std.mem.eql(u8, name, "f32")) return .f32;
        if (std.mem.eql(u8, name, "f64")) return .f64;
        if (std.mem.eql(u8, name, "i32")) return .i32;
        if (std.mem.eql(u8, name, "i64")) return .i64;
        if (std.mem.eql(u8, name, "bool")) return .bool;
        if (std.mem.eql(u8, name, "vec2")) return .vec2;
        if (std.mem.eql(u8, name, "vec3")) return .vec3;
        if (std.mem.eql(u8, name, "vec4")) return .vec4;
        return null;
    }
};

// ============ Tests ============

test "type checker basic" {
    const allocator = std.testing.allocator;
    var checker = TypeChecker.init(allocator);
    defer checker.deinit();

    // Test that the checker initializes correctly
    try std.testing.expect(checker.errors.items.len == 0);
}
