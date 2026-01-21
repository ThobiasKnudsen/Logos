//! GLSL Code Generator for the Logos Language
//!
//! Transpiles Logos AST to GLSL fragment shaders. Each non-void
//! root-level expression becomes a separate fragment shader.
//!
//! Output types:
//! - vec4: Rendered directly as RGBA color
//! - bool: Rendered with a random color where true, transparent where false
//! - float: Could be mapped to grayscale or a color gradient

const std = @import("std");
const ast = @import("ast_node.zig");
const AstNode = ast.AstNode;
const Builtin = ast.Builtin;
const getBuiltin = ast.getBuiltin;
const PrimitiveType = ast.PrimitiveType;
const code_cell = @import("../session/code_cell.zig");

/// Maximum magnitude for whole number float literals.
/// Numbers larger than this use scientific notation which includes a decimal point.
const MAX_WHOLE_NUMBER_MAGNITUDE = 1e15;

/// Type of output for a generated shader
pub const OutputType = enum {
    /// vec4 RGBA color output
    color,
    /// Boolean condition (region)
    boolean,
    /// Scalar float value
    scalar,
    /// Unknown/error
    unknown,
};

/// A generated GLSL fragment shader
pub const GeneratedShader = struct {
    /// The GLSL source code
    source: []const u8,
    /// What type of output this shader produces
    output_type: OutputType,
    /// Random color seed for boolean outputs
    color_seed: u32,
    /// Index in the output list (for debugging)
    index: usize,
};

/// Result of GLSL generation
pub const GenerationResult = struct {
    /// List of generated shaders (one per root output)
    shaders: []GeneratedShader,
    /// Any errors encountered during generation
    errors: []const []const u8,
};

/// Information about a nested function and its captured variables
const NestedFunctionInfo = struct {
    /// The function node
    node: *const AstNode,
    /// Name of parent function (null if top-level)
    parent_func: ?[]const u8,
    /// Variables captured from parent scope
    captured_vars: []const []const u8,
};

/// GLSL Code Generator
pub const GlslGenerator = struct {
    allocator: std.mem.Allocator,
    output: std.ArrayList(u8),
    indent_level: usize,
    /// Tracks defined variables in current scope
    defined_vars: std.StringHashMap(void),
    /// Tracks function definitions that need to be emitted
    functions: std.StringHashMap(*const AstNode),
    /// Set of functions already emitted
    emitted_functions: std.StringHashMap(void),
    /// Tracks which functions call which other functions (for ordering)
    function_deps: std.StringHashMap(std.ArrayList([]const u8)),
    /// Tracks nested function info (parent scope, captured variables)
    nested_func_info: std.StringHashMap(NestedFunctionInfo),
    /// Errors encountered during generation
    errors: std.ArrayList([]const u8),
    /// Current random seed for colors
    color_seed: u32,
    /// Temporary variable counter
    temp_counter: usize,

    const Self = @This();

    pub fn init(allocator: std.mem.Allocator) Self {
        return .{
            .allocator = allocator,
            .output = .{ .items = &.{}, .capacity = 0 },
            .indent_level = 0,
            .defined_vars = std.StringHashMap(void).init(allocator),
            .functions = std.StringHashMap(*const AstNode).init(allocator),
            .emitted_functions = std.StringHashMap(void).init(allocator),
            .function_deps = std.StringHashMap(std.ArrayList([]const u8)).init(allocator),
            .nested_func_info = std.StringHashMap(NestedFunctionInfo).init(allocator),
            .errors = .{ .items = &.{}, .capacity = 0 },
            .color_seed = 0x12345678,
            .temp_counter = 0,
        };
    }

    pub fn deinit(self: *Self) void {
        self.output.deinit(self.allocator);
        self.defined_vars.deinit();
        self.functions.deinit();
        self.emitted_functions.deinit();
        // Clean up function_deps ArrayLists
        var deps_iter = self.function_deps.iterator();
        while (deps_iter.next()) |entry| {
            entry.value_ptr.deinit(self.allocator);
        }
        self.function_deps.deinit();
        self.nested_func_info.deinit();
        self.errors.deinit(self.allocator);
    }

    /// Reset state for generating a new shader
    fn reset(self: *Self) void {
        self.output.clearRetainingCapacity();
        self.indent_level = 0;
        self.defined_vars.clearRetainingCapacity();
        self.emitted_functions.clearRetainingCapacity();
        self.temp_counter = 0;
    }

    /// Generate shaders from a root AST block
    pub fn generate(self: *Self, root: *const AstNode) !GenerationResult {
        var shaders: std.ArrayList(GeneratedShader) = .{ .items = &.{}, .capacity = 0 };
        errdefer shaders.deinit(self.allocator);

        // First pass: collect all function definitions and constants
        try self.collectDefinitions(root);

        // Second pass: analyze dependencies and captured variables (after all functions are known)
        try self.analyzeFunctionDependencies(root);

        // Third pass: find all root-level outputs (non-void expressions)
        const outputs = try self.findRootOutputs(root);
        defer self.allocator.free(outputs);

        // Generate a shader for each output
        for (outputs, 0..) |output, i| {
            self.reset();
            const timestamp: i128 = std.time.nanoTimestamp();
            self.color_seed = @truncate(@as(u64, @truncate(@as(u128, @bitCast(timestamp)))) +% i * 0x9E3779B9);

            const output_type = self.inferOutputType(output);
            try self.generateShader(output, output_type, root);

            const source = try self.output.toOwnedSlice(self.allocator);
            try shaders.append(self.allocator, .{
                .source = source,
                .output_type = output_type,
                .color_seed = self.color_seed,
                .index = i,
            });
        }

        return .{
            .shaders = try shaders.toOwnedSlice(self.allocator),
            .errors = try self.errors.toOwnedSlice(self.allocator),
        };
    }

    /// Collect function definitions and constants from the AST (including nested ones)
    /// This ONLY registers functions - dependency analysis happens in a separate pass
    fn collectDefinitions(self: *Self, node: *const AstNode) !void {
        try self.collectFunctionsOnly(node);
    }

    /// First pass: just collect all function definitions without analyzing dependencies
    fn collectFunctionsOnly(self: *Self, node: *const AstNode) !void {
        switch (node.data) {
            .block => |stmts| {
                for (stmts) |stmt| {
                    try self.collectFunctionsOnly(stmt);
                }
            },
            .function_def => |fd| {
                try self.functions.put(fd.name, node);
                // Recurse to find nested functions
                try self.collectFunctionsOnly(fd.body);
            },
            .binding => |b| {
                if (b.value.data == .function_def) {
                    const nested_fd = b.value.data.function_def;
                    try self.functions.put(nested_fd.name, b.value);
                    try self.collectFunctionsOnly(nested_fd.body);
                } else {
                    try self.collectFunctionsOnly(b.value);
                }
            },
            .if_expr => |ie| {
                try self.collectFunctionsOnly(ie.then_branch);
                if (ie.else_branch) |eb| {
                    try self.collectFunctionsOnly(eb);
                }
            },
            .while_loop => |wl| {
                try self.collectFunctionsOnly(wl.body);
            },
            .for_loop => |fl| {
                try self.collectFunctionsOnly(fl.body);
            },
            else => {},
        }
    }

    /// Second pass: analyze dependencies and captured variables (after all functions are registered)
    fn analyzeFunctionDependencies(self: *Self, root: *const AstNode) !void {
        try self.analyzeWithParent(root, null, &.{});
    }

    fn analyzeWithParent(
        self: *Self,
        node: *const AstNode,
        parent_func: ?[]const u8,
        parent_scope_vars: []const []const u8,
    ) !void {
        switch (node.data) {
            .block => |stmts| {
                // Build up scope variables as we go through bindings
                var scope_vars: std.ArrayList([]const u8) = .{ .items = &.{}, .capacity = 0 };
                defer scope_vars.deinit(self.allocator);
                try scope_vars.appendSlice(self.allocator, parent_scope_vars);

                for (stmts) |stmt| {
                    // Add binding names to scope before processing nested functions
                    if (stmt.data == .binding) {
                        const b = stmt.data.binding;
                        switch (b.pattern) {
                            .single => |name| try scope_vars.append(self.allocator, name),
                            .tuple => |names| {
                                for (names) |name| try scope_vars.append(self.allocator, name);
                            },
                        }
                    }
                    try self.analyzeWithParent(stmt, parent_func, scope_vars.items);
                }
            },
            .function_def => |fd| {
                // Track function dependencies (which functions this one calls)
                // Now all functions are registered, so findFunctionCalls will work correctly
                var deps: std.ArrayList([]const u8) = .{ .items = &.{}, .capacity = 0 };
                try self.findFunctionCalls(fd.body, &deps);
                try self.function_deps.put(fd.name, deps);

                // If this is a nested function, track captured variables
                if (parent_func != null) {
                    const captured = try self.findCapturedVars(fd.body, fd.params, parent_scope_vars);
                    try self.nested_func_info.put(fd.name, .{
                        .node = node,
                        .parent_func = parent_func,
                        .captured_vars = captured,
                    });
                }

                // Build param set for nested scope
                var nested_scope: std.ArrayList([]const u8) = .{ .items = &.{}, .capacity = 0 };
                defer nested_scope.deinit(self.allocator);
                for (fd.params) |p| try nested_scope.append(self.allocator, p);

                // Analyze nested functions
                try self.analyzeWithParent(fd.body, fd.name, nested_scope.items);
            },
            .binding => |b| {
                if (b.value.data == .function_def) {
                    const nested_fd = b.value.data.function_def;

                    // Track dependencies
                    var deps: std.ArrayList([]const u8) = .{ .items = &.{}, .capacity = 0 };
                    try self.findFunctionCalls(nested_fd.body, &deps);
                    try self.function_deps.put(nested_fd.name, deps);

                    // Track captured variables for nested function
                    if (parent_func != null) {
                        const captured = try self.findCapturedVars(nested_fd.body, nested_fd.params, parent_scope_vars);
                        try self.nested_func_info.put(nested_fd.name, .{
                            .node = b.value,
                            .parent_func = parent_func,
                            .captured_vars = captured,
                        });
                    }

                    // Build param set for nested scope
                    var nested_scope: std.ArrayList([]const u8) = .{ .items = &.{}, .capacity = 0 };
                    defer nested_scope.deinit(self.allocator);
                    for (nested_fd.params) |p| try nested_scope.append(self.allocator, p);

                    try self.analyzeWithParent(nested_fd.body, nested_fd.name, nested_scope.items);
                } else {
                    try self.analyzeWithParent(b.value, parent_func, parent_scope_vars);
                }
            },
            .if_expr => |ie| {
                try self.analyzeWithParent(ie.then_branch, parent_func, parent_scope_vars);
                if (ie.else_branch) |eb| {
                    try self.analyzeWithParent(eb, parent_func, parent_scope_vars);
                }
            },
            .while_loop => |wl| {
                try self.analyzeWithParent(wl.body, parent_func, parent_scope_vars);
            },
            .for_loop => |fl| {
                try self.analyzeWithParent(fl.body, parent_func, parent_scope_vars);
            },
            else => {},
        }
    }

    /// Find all function calls within an expression
    fn findFunctionCalls(self: *Self, node: *const AstNode, deps: *std.ArrayList([]const u8)) !void {
        switch (node.data) {
            .apply => |op| {
                // If it's not a builtin, it might be a user function
                if (getBuiltin(op.name) == null) {
                    // Check if it's in our functions map
                    if (self.functions.contains(op.name)) {
                        // Avoid duplicates
                        var found = false;
                        for (deps.items) |d| {
                            if (std.mem.eql(u8, d, op.name)) {
                                found = true;
                                break;
                            }
                        }
                        if (!found) {
                            try deps.append(self.allocator, op.name);
                        }
                    }
                }
                // Recurse into arguments
                for (op.args) |arg| {
                    try self.findFunctionCalls(arg, deps);
                }
            },
            .block => |stmts| {
                for (stmts) |stmt| try self.findFunctionCalls(stmt, deps);
            },
            .binding => |b| {
                try self.findFunctionCalls(b.value, deps);
            },
            .if_expr => |ie| {
                try self.findFunctionCalls(ie.condition, deps);
                try self.findFunctionCalls(ie.then_branch, deps);
                if (ie.else_branch) |eb| try self.findFunctionCalls(eb, deps);
            },
            .while_loop => |wl| {
                try self.findFunctionCalls(wl.condition, deps);
                try self.findFunctionCalls(wl.body, deps);
            },
            .for_loop => |fl| {
                try self.findFunctionCalls(fl.init, deps);
                try self.findFunctionCalls(fl.condition, deps);
                try self.findFunctionCalls(fl.update, deps);
                try self.findFunctionCalls(fl.body, deps);
            },
            .tuple => |elements| {
                for (elements) |elem| try self.findFunctionCalls(elem, deps);
            },
            .cast => |c| {
                try self.findFunctionCalls(c.operand, deps);
            },
            .property_access => |pa| {
                try self.findFunctionCalls(pa.base, deps);
            },
            .index => |idx| {
                try self.findFunctionCalls(idx.base, deps);
                try self.findFunctionCalls(idx.index_expr, deps);
            },
            else => {},
        }
    }

    /// Find variables captured from parent scope
    fn findCapturedVars(
        self: *Self,
        body: *const AstNode,
        params: []const []const u8,
        parent_scope_vars: []const []const u8,
    ) ![]const []const u8 {
        var referenced = std.StringHashMap(void).init(self.allocator);
        defer referenced.deinit();

        var local_vars = std.StringHashMap(void).init(self.allocator);
        defer local_vars.deinit();

        // Params are local
        for (params) |p| {
            try local_vars.put(p, {});
        }

        // Find all referenced identifiers
        try self.findReferencedVars(body, &referenced, &local_vars);

        // Captured = referenced AND in parent_scope AND not in params
        var captured: std.ArrayList([]const u8) = .{ .items = &.{}, .capacity = 0 };
        var ref_iter = referenced.iterator();
        while (ref_iter.next()) |entry| {
            const name = entry.key_ptr.*;
            // Check if it's in parent scope
            for (parent_scope_vars) |parent_var| {
                if (std.mem.eql(u8, name, parent_var)) {
                    // Check not a param
                    var is_param = false;
                    for (params) |p| {
                        if (std.mem.eql(u8, name, p)) {
                            is_param = true;
                            break;
                        }
                    }
                    if (!is_param) {
                        try captured.append(self.allocator, name);
                    }
                    break;
                }
            }
        }

        return captured.toOwnedSlice(self.allocator);
    }

    /// Find all referenced variable names in an expression
    fn findReferencedVars(
        self: *Self,
        node: *const AstNode,
        referenced: *std.StringHashMap(void),
        local_vars: *std.StringHashMap(void),
    ) !void {
        switch (node.data) {
            .identifier => |name| {
                if (!local_vars.contains(name)) {
                    try referenced.put(name, {});
                }
            },
            .apply => |op| {
                for (op.args) |arg| {
                    try self.findReferencedVars(arg, referenced, local_vars);
                }
            },
            .block => |stmts| {
                for (stmts) |stmt| {
                    // Add bindings to local vars before processing subsequent statements
                    if (stmt.data == .binding) {
                        const b = stmt.data.binding;
                        try self.findReferencedVars(b.value, referenced, local_vars);
                        switch (b.pattern) {
                            .single => |name| try local_vars.put(name, {}),
                            .tuple => |names| {
                                for (names) |name| try local_vars.put(name, {});
                            },
                        }
                    } else {
                        try self.findReferencedVars(stmt, referenced, local_vars);
                    }
                }
            },
            .binding => |b| {
                try self.findReferencedVars(b.value, referenced, local_vars);
            },
            .if_expr => |ie| {
                try self.findReferencedVars(ie.condition, referenced, local_vars);
                try self.findReferencedVars(ie.then_branch, referenced, local_vars);
                if (ie.else_branch) |eb| try self.findReferencedVars(eb, referenced, local_vars);
            },
            .while_loop => |wl| {
                try self.findReferencedVars(wl.condition, referenced, local_vars);
                try self.findReferencedVars(wl.body, referenced, local_vars);
            },
            .for_loop => |fl| {
                try self.findReferencedVars(fl.init, referenced, local_vars);
                try self.findReferencedVars(fl.condition, referenced, local_vars);
                try self.findReferencedVars(fl.update, referenced, local_vars);
                try self.findReferencedVars(fl.body, referenced, local_vars);
            },
            .tuple => |elements| {
                for (elements) |elem| try self.findReferencedVars(elem, referenced, local_vars);
            },
            .cast => |c| {
                try self.findReferencedVars(c.operand, referenced, local_vars);
            },
            .property_access => |pa| {
                try self.findReferencedVars(pa.base, referenced, local_vars);
            },
            .index => |idx| {
                try self.findReferencedVars(idx.base, referenced, local_vars);
                try self.findReferencedVars(idx.index_expr, referenced, local_vars);
            },
            else => {},
        }
    }

    /// Find all root-level expressions that produce output values
    fn findRootOutputs(self: *Self, root: *const AstNode) ![]*const AstNode {
        var outputs: std.ArrayList(*const AstNode) = .{ .items = &.{}, .capacity = 0 };
        errdefer outputs.deinit(self.allocator);

        switch (root.data) {
            .block => |stmts| {
                for (stmts) |stmt| {
                    if (self.isOutputExpression(stmt)) {
                        try outputs.append(self.allocator, stmt);
                    }
                }
            },
            else => {
                // Single expression at root
                if (self.isOutputExpression(root)) {
                    try outputs.append(self.allocator, root);
                }
            },
        }

        return outputs.toOwnedSlice(self.allocator);
    }

    /// Check if a node is an output expression (non-void, non-definition)
    fn isOutputExpression(self: *Self, node: *const AstNode) bool {
        _ = self;
        return switch (node.data) {
            // These are definitions, not outputs
            .function_def => false,
            .binding => false,
            // These produce values
            .number => true,
            .bool_lit => true,
            .identifier => true,
            .apply => true,
            .tuple => true,
            .cast => true,
            .property_access => true,
            .index => true,
            .if_expr => true,
            // Control flow typically doesn't produce root output
            .while_loop => false,
            .for_loop => false,
            .block => false,
        };
    }

    /// Infer the output type of an expression
    fn inferOutputType(self: *Self, node: *const AstNode) OutputType {
        return switch (node.data) {
            .bool_lit => .boolean,
            .number => .scalar,
            .apply => |op| blk: {
                // Check if it's a comparison or logical operation
                if (getBuiltin(op.name)) |builtin| {
                    std.log.info("[GlslGen] inferOutputType: op='{s}', category={s}, returning boolean", .{ op.name, @tagName(builtin.category) });
                    if (builtin.category == .comparison or builtin.category == .logical) {
                        break :blk .boolean;
                    }
                }
                // Check for tuple-returning functions (like mandelbrot returning vec4)
                // For now, assume user-defined functions might return color
                if (self.functions.get(op.name)) |_| {
                    // Could analyze return type, for now assume color if 4-tuple
                    break :blk .color;
                }
                break :blk .scalar;
            },
            .tuple => |elements| {
                // 4-tuple is assumed to be color
                if (elements.len == 4) return .color;
                if (elements.len == 3) return .color; // RGB
                return .unknown;
            },
            .identifier => .scalar, // Could be axis or other
            .if_expr => |ie| {
                // Infer from then branch
                return self.inferOutputType(ie.then_branch);
            },
            else => .unknown,
        };
    }

    /// Determine output type for a cell based on its AST node
    /// Checks if expression references axis variables to determine if it should be plotted
    pub fn determineOutputType(node: *const AstNode) code_cell.OutputType {
        // First check if it's a scalar (number literal)
        if (node.data == .number) {
            return .text_only;
        }

        // Check if expression contains axis1/axis2 references
        const has_axis_ref = containsAxisReference(node);

        // Check the expression type
        const is_boolean = isNodeBoolean(node);
        const is_tuple = node.data == .tuple;

        if (has_axis_ref) {
            // Contains axis variables - should be plotted
            if (is_boolean or is_tuple) {
                return .plot_only; // Plot boolean regions or tuples
            } else {
                return .both; // Plot and show value for scalar expressions
            }
        } else {
            // No axis variables - just show as text
            return .text_only;
        }
    }

    /// Check if a node contains references to axis1, axis2, x, or y
    fn containsAxisReference(node: *const AstNode) bool {
        return switch (node.data) {
            .identifier => |name| {
                return std.mem.eql(u8, name, "x") or
                    std.mem.eql(u8, name, "y") or
                    std.mem.eql(u8, name, "axis1") or
                    std.mem.eql(u8, name, "axis2");
            },
            .apply => |op| {
                for (op.args) |arg| {
                    if (containsAxisReference(arg)) return true;
                }
                return false;
            },
            .tuple => |elements| {
                for (elements) |elem| {
                    if (containsAxisReference(elem)) return true;
                }
                return false;
            },
            .property_access => |pa| {
                return containsAxisReference(pa.base);
            },
            .index => |idx| {
                return containsAxisReference(idx.base) or containsAxisReference(idx.index_expr);
            },
            .if_expr => |ie| {
                return containsAxisReference(ie.condition) or
                    containsAxisReference(ie.then_branch) or
                    (ie.else_branch != null and containsAxisReference(ie.else_branch.?));
            },
            .cast => |c| {
                return containsAxisReference(c.operand);
            },
            .block => |stmts| {
                for (stmts) |stmt| {
                    if (containsAxisReference(stmt)) return true;
                }
                return false;
            },
            .binding => |b| {
                return containsAxisReference(b.value);
            },
            else => false,
        };
    }

    /// Check if a node represents a boolean expression
    fn isNodeBoolean(node: *const AstNode) bool {
        return switch (node.data) {
            .bool_lit => true,
            .apply => |op| {
                if (getBuiltin(op.name)) |builtin| {
                    return builtin.category == .comparison or builtin.category == .logical;
                }
                return false;
            },
            else => false,
        };
    }

    /// Generate a complete fragment shader for one output
    fn generateShader(self: *Self, output: *const AstNode, output_type: OutputType, root: *const AstNode) !void {
        // Emit shader header
        try self.emitHeader();

        // Emit uniform block
        try self.emitUniforms();

        // Emit helper functions (from root definitions)
        // Pass the output expression so we only emit functions actually used
        try self.emitHelperFunctions(root, output);

        // Emit main function
        try self.emitMainFunction(output, output_type);
    }

    fn emitHeader(self: *Self) !void {
        try self.writeLine("#version 450");
        try self.writeLine("");
    }

    fn emitUniforms(self: *Self) !void {
        // Input/output declarations first (matching default shader order)
        try self.writeLine("layout(location = 0) in vec2 in_texcoord;");
        try self.writeLine("layout(location = 0) out vec4 out_color;");
        try self.writeLine("");
        // Uniform buffer for fragment shader.
        // SDL3 GPU API uses descriptor sets: set 3, binding 0 for fragment shader uniforms
        // This is different from Vulkan push constants - SDL3 requires proper set/binding numbers
        // Must match shaders.Uniforms struct exactly (std140 layout)
        try self.writeLine("layout(set = 3, binding = 0, std140) uniform PushConstants {");
        self.indent_level += 1;
        try self.writeLine("float time;");
        try self.writeLine("float _pad0;");
        try self.writeLine("vec2 resolution;");
        try self.writeLine("vec2 mouse;");
        try self.writeLine("float zoom;");
        try self.writeLine("float _pad1;");
        try self.writeLine("vec2 pan;");
        try self.writeLine("vec2 axis_min;"); // (min_x, min_y)
        try self.writeLine("vec2 axis_max;"); // (max_x, max_y)
        try self.writeLine("vec2 _pad2;"); // std140 alignment for vec4
        try self.writeLine("vec4 primary_color;");
        try self.writeLine("vec4 secondary_color;");
        try self.writeLine("vec4 background_color;");
        self.indent_level -= 1;
        try self.writeLine("};");
        try self.writeLine("");
    }

    fn emitHelperFunctions(self: *Self, root: *const AstNode, output: *const AstNode) !void {
        // First, collect all constants/bindings that need to be emitted as defines or globals
        switch (root.data) {
            .block => |stmts| {
                for (stmts) |stmt| {
                    switch (stmt.data) {
                        .binding => |b| {
                            // Check if it's a simple constant (not a nested function)
                            if (b.value.data != .function_def and self.isConstantBinding(b.value)) {
                                try self.emitConstantDefine(b.pattern, b.value);
                            }
                        },
                        else => {},
                    }
                }
            },
            else => {},
        }

        // Topologically sort functions (dependencies first)
        // Only include functions that are used (transitively) from the output expression
        const sorted_funcs = try self.topologicalSortFunctions(output);
        defer self.allocator.free(sorted_funcs);

        // Emit functions in sorted order
        for (sorted_funcs) |func_name| {
            if (!self.emitted_functions.contains(func_name)) {
                if (self.functions.get(func_name)) |func_node| {
                    if (func_node.data == .function_def) {
                        const fd = func_node.data.function_def;
                        try self.emitFunctionWithCaptures(fd.name, fd.params, fd.body);
                        try self.emitted_functions.put(fd.name, {});
                    }
                }
            }
        }
        try self.writeLine("");
    }

    /// Topologically sort functions so dependencies are emitted first
    /// Only includes functions transitively used from the output expression
    fn topologicalSortFunctions(self: *Self, output: *const AstNode) ![]const []const u8 {
        var result: std.ArrayList([]const u8) = .{ .items = &.{}, .capacity = 0 };
        errdefer result.deinit(self.allocator);

        var visited = std.StringHashMap(void).init(self.allocator);
        defer visited.deinit();

        var in_stack = std.StringHashMap(void).init(self.allocator);
        defer in_stack.deinit();

        // Find all functions called from output expression
        var used_funcs: std.ArrayList([]const u8) = .{ .items = &.{}, .capacity = 0 };
        defer used_funcs.deinit(self.allocator);
        try self.findFunctionCalls(output, &used_funcs);

        // Visit only the used functions (and their dependencies)
        for (used_funcs.items) |func_name| {
            if (!visited.contains(func_name)) {
                try self.topoVisit(func_name, &visited, &in_stack, &result);
            }
        }

        return result.toOwnedSlice(self.allocator);
    }

    fn topoVisit(
        self: *Self,
        func_name: []const u8,
        visited: *std.StringHashMap(void),
        in_stack: *std.StringHashMap(void),
        result: *std.ArrayList([]const u8),
    ) !void {
        if (in_stack.contains(func_name)) {
            // Cycle detected - just skip (will cause GLSL error but better than infinite loop)
            return;
        }
        if (visited.contains(func_name)) {
            return;
        }

        try in_stack.put(func_name, {});

        // Visit dependencies first
        if (self.function_deps.get(func_name)) |deps| {
            for (deps.items) |dep| {
                try self.topoVisit(dep, visited, in_stack, result);
            }
        }

        _ = in_stack.remove(func_name);
        try visited.put(func_name, {});
        try result.append(self.allocator, func_name);
    }

    /// Emit a function, adding captured variables as extra parameters for nested functions
    fn emitFunctionWithCaptures(self: *Self, name: []const u8, params: []const []const u8, body: *const AstNode) !void {
        // Infer return type from body
        const return_type = self.inferGlslType(body);

        try self.write(return_type);
        try self.write(" ");
        try self.write(name);
        try self.write("(");

        // Emit original parameters
        for (params, 0..) |param, i| {
            if (i > 0) try self.write(", ");
            try self.write("float ");
            try self.write(param);
        }

        // If this is a nested function, add captured variables as extra parameters
        if (self.nested_func_info.get(name)) |info| {
            for (info.captured_vars) |cap_var| {
                if (params.len > 0 or info.captured_vars.len > 0) {
                    try self.write(", ");
                }
                try self.write("float ");
                try self.write(cap_var);
            }
        }

        try self.write(") {\n");
        self.indent_level += 1;

        // Emit function body
        try self.emitFunctionBody(body);

        self.indent_level -= 1;
        try self.writeLine("}");
        try self.writeLine("");
    }

    fn isConstantBinding(self: *Self, value: *const AstNode) bool {
        _ = self;
        return switch (value.data) {
            .number => true,
            .bool_lit => true,
            else => false,
        };
    }

    fn emitConstantDefine(self: *Self, pattern: ast.BindingPattern, value: *const AstNode) !void {
        switch (pattern) {
            .single => |name| {
                try self.write("#define ");
                try self.write(name);
                try self.write(" ");
                try self.emitExpr(value);
                try self.write("\n");
            },
            .tuple => {}, // Can't emit tuple as define
        }
    }

    fn emitFunctionBody(self: *Self, body: *const AstNode) std.mem.Allocator.Error!void {
        switch (body.data) {
            .block => |stmts| {
                // Find the last non-function-def statement as return value
                var last_value_idx: ?usize = null;
                for (stmts, 0..) |stmt, i| {
                    // Skip function definitions (hoisted) and bindings
                    if (stmt.data != .function_def) {
                        if (stmt.data != .binding) {
                            last_value_idx = i;
                        } else {
                            // Binding - could still be the "last thing" if no expression follows
                            // but we don't return bindings
                        }
                    }
                }

                // Emit all statements except the last value expression
                for (stmts, 0..) |stmt, i| {
                    if (last_value_idx != null and i == last_value_idx.?) {
                        // This is the return expression
                        try self.writeIndent();
                        try self.write("return ");
                        try self.emitExpr(stmt);
                        try self.write(";\n");
                    } else {
                        try self.emitStatement(stmt);
                    }
                }

                // If no return expression found, emit void return
                if (last_value_idx == null) {
                    try self.writeLine("return;");
                }
            },
            else => {
                // Single expression - return it
                try self.writeIndent();
                try self.write("return ");
                try self.emitExpr(body);
                try self.write(";\n");
            },
        }
    }

    fn emitStatement(self: *Self, stmt: *const AstNode) std.mem.Allocator.Error!void {
        switch (stmt.data) {
            .binding => |b| {
                // Check for nested function definition - skip it (hoisted separately)
                if (b.value.data == .function_def) {
                    return;
                }

                try self.writeIndent();
                switch (b.pattern) {
                    .single => |name| {
                        // Check if variable already defined (reassignment vs declaration)
                        if (self.defined_vars.contains(name)) {
                            // Reassignment - no type declaration
                            try self.write(name);
                            try self.write(" = ");
                            try self.emitExpr(b.value);
                            try self.write(";\n");
                        } else {
                            // New variable declaration
                            const var_type = self.inferGlslType(b.value);
                            try self.write(var_type);
                            try self.write(" ");
                            try self.write(name);
                            try self.write(" = ");
                            try self.emitExpr(b.value);
                            try self.write(";\n");
                            try self.defined_vars.put(name, {});
                        }
                    },
                    .tuple => |names| {
                        // Tuple destructuring - emit temp and extract
                        const temp_name = try self.genTempName();
                        const var_type = self.inferGlslType(b.value);
                        try self.write(var_type);
                        try self.write(" ");
                        try self.write(temp_name);
                        try self.write(" = ");
                        try self.emitExpr(b.value);
                        try self.write(";\n");

                        for (names, 0..) |name, i| {
                            try self.writeIndent();
                            // Check if already defined
                            if (!self.defined_vars.contains(name)) {
                                try self.write("float ");
                            }
                            try self.write(name);
                            try self.write(" = ");
                            try self.write(temp_name);
                            try self.write(switch (i) {
                                0 => ".x",
                                1 => ".y",
                                2 => ".z",
                                3 => ".w",
                                else => ".x",
                            });
                            try self.write(";\n");
                            try self.defined_vars.put(name, {});
                        }
                    },
                }
            },
            .if_expr => |ie| {
                try self.writeIndent();
                try self.write("if (");
                try self.emitExpr(ie.condition);
                try self.write(") {\n");
                self.indent_level += 1;
                try self.emitStatement(ie.then_branch);
                self.indent_level -= 1;
                if (ie.else_branch) |eb| {
                    try self.writeIndent();
                    try self.write("} else {\n");
                    self.indent_level += 1;
                    try self.emitStatement(eb);
                    self.indent_level -= 1;
                }
                try self.writeIndent();
                try self.write("}\n");
            },
            .while_loop => |wl| {
                try self.writeIndent();
                try self.write("while (");
                try self.emitExpr(wl.condition);
                try self.write(") {\n");
                self.indent_level += 1;
                try self.emitStatement(wl.body);
                self.indent_level -= 1;
                try self.writeIndent();
                try self.write("}\n");
            },
            .for_loop => |fl| {
                try self.writeIndent();
                // Emit init outside loop
                try self.emitStatement(fl.init);
                try self.writeIndent();
                try self.write("while (");
                try self.emitExpr(fl.condition);
                try self.write(") {\n");
                self.indent_level += 1;
                try self.emitStatement(fl.body);
                try self.emitStatement(fl.update);
                self.indent_level -= 1;
                try self.writeIndent();
                try self.write("}\n");
            },
            .block => |stmts| {
                for (stmts) |s| {
                    try self.emitStatement(s);
                }
            },
            .function_def => {
                // Skip - already hoisted to top level
            },
            else => {
                // Expression statement
                try self.writeIndent();
                try self.emitExpr(stmt);
                try self.write(";\n");
            },
        }
    }

    fn emitMainFunction(self: *Self, output: *const AstNode, output_type: OutputType) !void {
        try self.writeLine("void main() {");
        self.indent_level += 1;

        // Set up UV coordinate variables (x and y in [0,1] range)
        // User code should map these to world coordinates using axis properties
        try self.writeLine("// UV coordinates (normalized screen space [0,1])");
        try self.writeLine("vec2 uv = in_texcoord;");
        try self.writeLine("float x = uv.x;");
        try self.writeLine("float y = uv.y;");
        try self.writeLine("");

        // Emit the output expression
        try self.writeIndent();

        std.log.info("[GlslGen] emitMainFunction: output_type={s}", .{@tagName(output_type)});

        switch (output_type) {
            .color => {
                try self.write("vec4 result = ");
                try self.emitExpr(output);
                try self.write(";\n");
                try self.writeLine("out_color = result;");
            },
            .boolean => {
                // For boolean expressions, override x/y to be world coordinates for proper evaluation
                try self.writeLine("// Map UV to world coordinates for boolean expressions");
                try self.writeLine("vec2 world = mix(axis_min, axis_max, uv);");
                try self.writeLine("x = world.x;  // Override x to world coordinate");
                try self.writeLine("y = world.y;  // Override y to world coordinate");
                try self.writeLine("");
                try self.writeLine("// Boolean expression evaluation with corner checking");
                try self.writeLine("// Calculate pixel size in world coordinates for anti-aliasing");
                try self.writeLine("vec2 pixel_size = (axis_max - axis_min) / resolution;");
                try self.writeLine("float half_px = pixel_size.x * 1.0;");
                try self.writeLine("float half_py = pixel_size.y * 1.0;");
                try self.writeLine("");
                try self.writeLine("// Pixel corner coordinates");
                try self.writeLine("float x_m = x - half_px;");
                try self.writeLine("float x_p = x + half_px;");
                try self.writeLine("float y_m = y - half_py;");
                try self.writeLine("float y_p = y + half_py;");
                try self.writeLine("");
                try self.writeLine("// Evaluate expression at corners");
                try self.writeLine("bool result = ");
                try self.emitBooleanWithCornerCheck(output);
                try self.writeLine(";");
                try self.writeLine("");
                try self.writeLine("// Output color based on result");
                try self.writeLine("if (result) {");
                try self.writeLine("    out_color = vec4(primary_color.rgb, 1.0);");
                try self.writeLine("} else {");
                try self.writeLine("    // Transparent for multi-pass rendering");
                try self.writeLine("    out_color = vec4(0.0, 0.0, 0.0, 0.0);");
                try self.writeLine("}");
            },
            .scalar => {
                try self.write("float val = ");
                try self.emitExpr(output);
                try self.write(";\n");
                // Map scalar to grayscale
                try self.writeLine("out_color = vec4(vec3(val), 1.0);");
            },
            .unknown => {
                try self.write("out_color = vec4(1.0, 0.0, 1.0, 1.0); // Unknown type\n");
            },
        }

        self.indent_level -= 1;
        try self.writeLine("}");
    }

    /// Emit a boolean expression with corner checking for proper curve/region rendering
    /// For equations (=): Check if corners straddle the curve (not all on same side)
    /// For inequalities (>, <, >=, <=): Check if all corners agree
    /// For and/or: Recursively apply corner checking
    fn emitBooleanWithCornerCheck(self: *Self, node: *const AstNode) std.mem.Allocator.Error!void {
        switch (node.data) {
            .apply => |op| {
                if (getBuiltin(op.name)) |builtin| {
                    if (builtin.category == .logical) {
                        // and/or - recursively apply to both sides
                        if (std.mem.eql(u8, op.name, "and") and op.args.len >= 2) {
                            try self.write("(");
                            try self.emitBooleanWithCornerCheck(op.args[0]);
                            try self.write(" && ");
                            try self.emitBooleanWithCornerCheck(op.args[1]);
                            try self.write(")");
                            return;
                        } else if (std.mem.eql(u8, op.name, "or") and op.args.len >= 2) {
                            try self.write("(");
                            try self.emitBooleanWithCornerCheck(op.args[0]);
                            try self.write(" || ");
                            try self.emitBooleanWithCornerCheck(op.args[1]);
                            try self.write(")");
                            return;
                        } else if (std.mem.eql(u8, op.name, "not") and op.args.len >= 1) {
                            try self.write("!(");
                            try self.emitBooleanWithCornerCheck(op.args[0]);
                            try self.write(")");
                            return;
                        }
                    } else if (builtin.category == .comparison) {
                        // Comparison operators - apply corner checking
                        try self.emitComparisonWithCornerCheck(op.name, op.args);
                        return;
                    }
                }
                // Fall through to regular expression
                try self.emitExpr(node);
            },
            .bool_lit => {
                try self.emitExpr(node);
            },
            else => {
                // For other expressions, just emit normally
                try self.emitExpr(node);
            },
        }
    }

    /// Emit a comparison with corner checking
    /// For equality (=): curve passes through pixel if corners don't all agree on which side
    /// For inequality (>, <, >=, <=, !=): all corners must agree
    fn emitComparisonWithCornerCheck(self: *Self, op_name: []const u8, args: []*AstNode) std.mem.Allocator.Error!void {
        if (args.len < 2) {
            try self.write("false");
            return;
        }

        const lhs = args[0];
        const rhs = args[1];

        // For equality, we check if corners straddle the curve
        // Convert LHS = RHS to checking if (LHS - RHS) changes sign across corners
        if (std.mem.eql(u8, op_name, "eq")) {
            // Equation: curve passes through if not all corners are on same side
            // v1_1 = (LHS - RHS) at corner 1 > 0
            // v1_2 = (LHS - RHS) at corner 2 > 0
            // etc.
            // Result: !(v1_1 == v1_2 && v1_2 == v1_3 && v1_3 == v1_4)
            try self.write("(!(");
            // Corner 1: (x_m, y_m)
            try self.write("((");
            try self.emitExprWithCorner(lhs, "x_m", "y_m");
            try self.write(") > (");
            try self.emitExprWithCorner(rhs, "x_m", "y_m");
            try self.write(")) == ((");
            // Corner 2: (x_m, y_p)
            try self.emitExprWithCorner(lhs, "x_m", "y_p");
            try self.write(") > (");
            try self.emitExprWithCorner(rhs, "x_m", "y_p");
            try self.write(")) && ((");
            // Corner 2 == Corner 3
            try self.emitExprWithCorner(lhs, "x_m", "y_p");
            try self.write(") > (");
            try self.emitExprWithCorner(rhs, "x_m", "y_p");
            try self.write(")) == ((");
            // Corner 3: (x_p, y_m)
            try self.emitExprWithCorner(lhs, "x_p", "y_m");
            try self.write(") > (");
            try self.emitExprWithCorner(rhs, "x_p", "y_m");
            try self.write(")) && ((");
            // Corner 3 == Corner 4
            try self.emitExprWithCorner(lhs, "x_p", "y_m");
            try self.write(") > (");
            try self.emitExprWithCorner(rhs, "x_p", "y_m");
            try self.write(")) == ((");
            // Corner 4: (x_p, y_p)
            try self.emitExprWithCorner(lhs, "x_p", "y_p");
            try self.write(") > (");
            try self.emitExprWithCorner(rhs, "x_p", "y_p");
            try self.write("))))");
        } else {
            // For inequalities: all corners must satisfy the condition
            const glsl_op = if (std.mem.eql(u8, op_name, "gt"))
                " > "
            else if (std.mem.eql(u8, op_name, "lt"))
                " < "
            else if (std.mem.eql(u8, op_name, "ge"))
                " >= "
            else if (std.mem.eql(u8, op_name, "le"))
                " <= "
            else if (std.mem.eql(u8, op_name, "neq"))
                " != "
            else
                " == ";

            try self.write("(");
            // Corner 1: (x_m, y_m)
            try self.write("((");
            try self.emitExprWithCorner(lhs, "x_m", "y_m");
            try self.write(")");
            try self.write(glsl_op);
            try self.write("(");
            try self.emitExprWithCorner(rhs, "x_m", "y_m");
            try self.write(")) && ");
            // Corner 2: (x_m, y_p)
            try self.write("((");
            try self.emitExprWithCorner(lhs, "x_m", "y_p");
            try self.write(")");
            try self.write(glsl_op);
            try self.write("(");
            try self.emitExprWithCorner(rhs, "x_m", "y_p");
            try self.write(")) && ");
            // Corner 3: (x_p, y_m)
            try self.write("((");
            try self.emitExprWithCorner(lhs, "x_p", "y_m");
            try self.write(")");
            try self.write(glsl_op);
            try self.write("(");
            try self.emitExprWithCorner(rhs, "x_p", "y_m");
            try self.write(")) && ");
            // Corner 4: (x_p, y_p)
            try self.write("((");
            try self.emitExprWithCorner(lhs, "x_p", "y_p");
            try self.write(")");
            try self.write(glsl_op);
            try self.write("(");
            try self.emitExprWithCorner(rhs, "x_p", "y_p");
            try self.write(")))");
        }
    }

    /// Emit expression (uses "x" and "y" directly)
    fn emitExpr(self: *Self, node: *const AstNode) std.mem.Allocator.Error!void {
        try self.emitExprInternal(node, null, null);
    }

    /// Emit expression with x/y replaced by corner coordinates
    fn emitExprWithCorner(self: *Self, node: *const AstNode, x_var: []const u8, y_var: []const u8) std.mem.Allocator.Error!void {
        try self.emitExprInternal(node, x_var, y_var);
    }

    /// Internal expression emitter - handles both regular and corner-substituted emission
    /// If x_var and y_var are null, emits x and y as-is. Otherwise substitutes them.
    fn emitExprInternal(self: *Self, node: *const AstNode, x_var: ?[]const u8, y_var: ?[]const u8) std.mem.Allocator.Error!void {
        switch (node.data) {
            .number => |n| {
                try self.emitFloatLiteral(n);
            },
            .bool_lit => |b| {
                try self.write(if (b) "true" else "false");
            },
            .identifier => |name| {
                // Replace x and y with corner coordinates if provided
                if (std.mem.eql(u8, name, "x") and x_var != null) {
                    try self.write(x_var.?);
                } else if (std.mem.eql(u8, name, "y") and y_var != null) {
                    try self.write(y_var.?);
                } else if (std.mem.eql(u8, name, "time_s") or std.mem.eql(u8, name, "time.s")) {
                    try self.write("time");
                } else {
                    try self.write(name);
                }
            },
            .apply => |op| {
                try self.emitApplyInternal(op.name, op.args, x_var, y_var);
            },
            .property_access => |pa| {
                // Handle axis property access
                if (pa.base.data == .identifier) {
                    const base_name = pa.base.data.identifier;
                    if (std.mem.eql(u8, base_name, "x") or std.mem.eql(u8, base_name, "axis1")) {
                        if (std.mem.eql(u8, pa.property, "min")) {
                            try self.write("axis_min.x");
                            return;
                        } else if (std.mem.eql(u8, pa.property, "max")) {
                            try self.write("axis_max.x");
                            return;
                        } else if (std.mem.eql(u8, pa.property, "res")) {
                            try self.write("resolution.x");
                            return;
                        }
                    } else if (std.mem.eql(u8, base_name, "y") or std.mem.eql(u8, base_name, "axis2")) {
                        if (std.mem.eql(u8, pa.property, "min")) {
                            try self.write("axis_min.y");
                            return;
                        } else if (std.mem.eql(u8, pa.property, "max")) {
                            try self.write("axis_max.y");
                            return;
                        } else if (std.mem.eql(u8, pa.property, "res")) {
                            try self.write("resolution.y");
                            return;
                        }
                    }
                }
                try self.emitExprInternal(pa.base, x_var, y_var);
                try self.write(".");
                try self.write(pa.property);
            },
            .tuple => |elements| {
                const n = elements.len;
                if (n == 1) {
                    // Single-element tuple - unwrap and emit inner expression
                    try self.emitExprInternal(elements[0], x_var, y_var);
                } else {
                    try self.write(switch (n) {
                        2 => "vec2(",
                        3 => "vec3(",
                        4 => "vec4(",
                        else => "vec4(",
                    });
                    for (elements, 0..) |elem, i| {
                        if (i > 0) try self.write(", ");
                        try self.emitExprInternal(elem, x_var, y_var);
                    }
                    try self.write(")");
                }
            },
            .cast => |c| {
                try self.write(c.target_type.toGlsl());
                try self.write("(");
                try self.emitExprInternal(c.operand, x_var, y_var);
                try self.write(")");
            },
            .index => |idx| {
                try self.emitExprInternal(idx.base, x_var, y_var);
                try self.write("[");
                try self.emitExprInternal(idx.index_expr, x_var, y_var);
                try self.write("]");
            },
            .if_expr => |ie| {
                try self.write("(");
                try self.emitExprInternal(ie.condition, x_var, y_var);
                try self.write(" ? ");
                try self.emitExprInternal(ie.then_branch, x_var, y_var);
                try self.write(" : ");
                if (ie.else_branch) |eb| {
                    try self.emitExprInternal(eb, x_var, y_var);
                } else {
                    try self.write("0.0");
                }
                try self.write(")");
            },
            .block => |stmts| {
                // For expression context, emit bindings as assignments and return the last expression
                // This handles patterns like: (sq: z_x * z_x + z_y * z_y, sq) in while conditions
                if (stmts.len > 0) {
                    // If there are multiple statements, wrap in a comma expression
                    if (stmts.len > 1) {
                        try self.write("(");
                        for (stmts, 0..) |stmt, i| {
                            if (i > 0) try self.write(", ");

                            // Emit bindings as assignments
                            if (stmt.data == .binding) {
                                const b = stmt.data.binding;
                                switch (b.pattern) {
                                    .single => |name| {
                                        try self.write(name);
                                        try self.write(" = ");
                                        try self.emitExprInternal(b.value, x_var, y_var);
                                    },
                                    .tuple => {
                                        // Can't handle tuple destructuring in expression context
                                        // Just emit the last expression
                                        if (i == stmts.len - 1) {
                                            try self.emitExprInternal(stmt, x_var, y_var);
                                        }
                                    },
                                }
                            } else {
                                try self.emitExprInternal(stmt, x_var, y_var);
                            }
                        }
                        try self.write(")");
                    } else {
                        // Single statement - emit directly
                        const stmt = stmts[0];
                        if (stmt.data == .binding) {
                            const b = stmt.data.binding;
                            switch (b.pattern) {
                                .single => |name| {
                                    try self.write("(");
                                    try self.write(name);
                                    try self.write(" = ");
                                    try self.emitExprInternal(b.value, x_var, y_var);
                                    try self.write(")");
                                },
                                .tuple => {
                                    // Can't handle tuple destructuring in expression context
                                    try self.emitExprInternal(stmt, x_var, y_var);
                                },
                            }
                        } else {
                            try self.emitExprInternal(stmt, x_var, y_var);
                        }
                    }
                }
            },
            else => {
                try self.write("/* unsupported */");
            },
        }
    }

    /// Emit function application (uses "x" and "y" directly)
    fn emitApply(self: *Self, name: []const u8, args: []*AstNode) std.mem.Allocator.Error!void {
        try self.emitApplyInternal(name, args, null, null);
    }

    /// Internal apply emitter - handles both regular and corner-substituted emission
    fn emitApplyInternal(self: *Self, name: []const u8, args: []*AstNode, x_var: ?[]const u8, y_var: ?[]const u8) std.mem.Allocator.Error!void {
        // Handle type casts that were parsed as function calls
        if (args.len == 1) {
            if (self.getGlslTypeName(name)) |glsl_type| {
                try self.write(glsl_type);
                try self.write("(");
                try self.emitExprInternal(args[0], x_var, y_var);
                try self.write(")");
                return;
            }
        }

        if (getBuiltin(name)) |builtin| {
            switch (builtin.emit_style) {
                .infix => {
                    if (args.len >= 2) {
                        try self.write("(");
                        try self.emitExprInternal(args[0], x_var, y_var);
                        try self.write(" ");
                        try self.write(builtin.glsl orelse name);
                        try self.write(" ");
                        try self.emitExprInternal(args[1], x_var, y_var);
                        try self.write(")");
                    }
                },
                .prefix => {
                    if (args.len >= 1) {
                        try self.write("(");
                        try self.write(builtin.glsl orelse name);
                        try self.emitExprInternal(args[0], x_var, y_var);
                        try self.write(")");
                    }
                },
                .postfix => {
                    // Handle square specially: x² → (x * x)
                    if (std.mem.eql(u8, name, "square") and args.len >= 1) {
                        try self.write("(");
                        try self.emitExprInternal(args[0], x_var, y_var);
                        try self.write(" * ");
                        try self.emitExprInternal(args[0], x_var, y_var);
                        try self.write(")");
                    }
                },
                .call => {
                    try self.write(builtin.glsl orelse name);
                    try self.write("(");
                    for (args, 0..) |arg, i| {
                        if (i > 0) try self.write(", ");
                        try self.emitExprInternal(arg, x_var, y_var);
                    }
                    try self.write(")");
                },
            }
        } else {
            // User-defined function
            try self.write(name);
            try self.write("(");
            for (args, 0..) |arg, i| {
                if (i > 0) try self.write(", ");
                try self.emitExprInternal(arg, x_var, y_var);
            }

            // If this is a nested function with captures, pass the captured variables
            if (self.nested_func_info.get(name)) |info| {
                for (info.captured_vars) |cap_var| {
                    if (args.len > 0 or info.captured_vars.len > 0) {
                        try self.write(", ");
                    }
                    try self.write(cap_var);
                }
            }

            try self.write(")");
        }
    }

    fn inferGlslType(self: *Self, node: *const AstNode) []const u8 {
        return switch (node.data) {
            .number => "float",
            .bool_lit => "bool",
            .tuple => |elements| switch (elements.len) {
                1 => {
                    // Single-element tuple - unwrap and get inner type
                    return self.inferGlslType(elements[0]);
                },
                2 => "vec2",
                3 => "vec3",
                4 => "vec4",
                else => "vec4",
            },
            .apply => |op| {
                if (getBuiltin(op.name)) |builtin| {
                    if (builtin.category == .comparison or builtin.category == .logical) {
                        return "bool";
                    }
                }
                // Check if it's a user-defined function and infer from its body
                if (self.functions.get(op.name)) |func_node| {
                    if (func_node.data == .function_def) {
                        return self.inferGlslType(func_node.data.function_def.body);
                    }
                }
                return "float";
            },
            .if_expr => |ie| {
                // Infer from then branch
                return self.inferGlslType(ie.then_branch);
            },
            .block => |stmts| {
                if (stmts.len > 0) {
                    // Return type is the type of the last statement
                    const last = stmts[stmts.len - 1];
                    // Skip bindings, look for the actual value
                    if (last.data == .binding) {
                        return "void";
                    }
                    return self.inferGlslType(last);
                }
                return "void";
            },
            .cast => |c| {
                return c.target_type.toGlsl();
            },
            else => "float",
        };
    }

    // === Utility functions ===

    fn write(self: *Self, s: []const u8) !void {
        try self.output.appendSlice(self.allocator, s);
    }

    fn writeIndent(self: *Self) !void {
        for (0..self.indent_level) |_| {
            try self.output.appendSlice(self.allocator, "    ");
        }
    }

    fn writeLine(self: *Self, s: []const u8) !void {
        try self.writeIndent();
        try self.output.appendSlice(self.allocator, s);
        try self.output.append(self.allocator, '\n');
    }

    /// Emit a float literal ensuring it has a decimal point for GLSL.
    /// GLSL requires explicit float type notation - integer literals in float contexts
    /// can cause issues. Always emit with decimal point (e.g., "1.0" not "1").
    fn emitFloatLiteral(self: *Self, n: f64) !void {
        var num_buf: [64]u8 = undefined;

        // Check if the number is a whole number (no fractional part)
        const is_whole = n == @trunc(n);

        if (is_whole and @abs(n) < MAX_WHOLE_NUMBER_MAGNITUDE) {
            // For whole numbers, explicitly add .0 to make it a float literal
            const num_str = std.fmt.bufPrint(&num_buf, "{d}.0", .{@as(i64, @intFromFloat(n))}) catch "0.0";
            try self.write(num_str);
        } else {
            // For non-whole numbers, the default formatting includes a decimal point
            const num_str = std.fmt.bufPrint(&num_buf, "{d}", .{n}) catch "0.0";
            try self.write(num_str);
            // Edge case: very large numbers might not have a decimal point
            // Check if the output contains a decimal point or 'e' (scientific notation)
            const has_decimal = std.mem.indexOfScalar(u8, num_str, '.') != null;
            const has_exponent = std.mem.indexOfScalar(u8, num_str, 'e') != null;
            if (!has_decimal and !has_exponent) {
                try self.write(".0");
            }
        }
    }

    fn genTempName(self: *Self) ![]const u8 {
        const name = try std.fmt.allocPrint(self.allocator, "_tmp{d}", .{self.temp_counter});
        self.temp_counter += 1;
        return name;
    }

    /// Map Logos type names to GLSL type names
    fn getGlslTypeName(_: *Self, name: []const u8) ?[]const u8 {
        if (std.mem.eql(u8, name, "f32")) return "float";
        if (std.mem.eql(u8, name, "f64")) return "double";
        if (std.mem.eql(u8, name, "i32")) return "int";
        if (std.mem.eql(u8, name, "i64")) return "int"; // GLSL doesn't have i64
        if (std.mem.eql(u8, name, "bool")) return "bool";
        if (std.mem.eql(u8, name, "vec2")) return "vec2";
        if (std.mem.eql(u8, name, "vec3")) return "vec3";
        if (std.mem.eql(u8, name, "vec4")) return "vec4";
        return null;
    }
};

/// Convenience function to generate GLSL from an AST
pub fn generateGlsl(allocator: std.mem.Allocator, ast_root: *const AstNode) !GenerationResult {
    var gen = GlslGenerator.init(allocator);
    defer gen.deinit();
    return gen.generate(ast_root);
}
