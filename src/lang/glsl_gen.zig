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

        // Second pass: find all root-level outputs (non-void expressions)
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
    fn collectDefinitions(self: *Self, node: *const AstNode) !void {
        switch (node.data) {
            .block => |stmts| {
                for (stmts) |stmt| {
                    try self.collectDefinitions(stmt);
                }
            },
            .function_def => |fd| {
                try self.functions.put(fd.name, node);
                // Also collect any nested functions from the body
                try self.collectDefinitions(fd.body);
            },
            .binding => |b| {
                // Check if the value is a function definition (nested function via binding)
                if (b.value.data == .function_def) {
                    const nested_fd = b.value.data.function_def;
                    try self.functions.put(nested_fd.name, b.value);
                    try self.collectDefinitions(nested_fd.body);
                } else {
                    // Recurse into the value for any nested function definitions
                    try self.collectDefinitions(b.value);
                }
            },
            .if_expr => |ie| {
                try self.collectDefinitions(ie.then_branch);
                if (ie.else_branch) |eb| {
                    try self.collectDefinitions(eb);
                }
            },
            .while_loop => |wl| {
                try self.collectDefinitions(wl.body);
            },
            .for_loop => |fl| {
                try self.collectDefinitions(fl.body);
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
            .apply => |op| {
                // Check if it's a comparison or logical operation
                if (getBuiltin(op.name)) |builtin| {
                    if (builtin.category == .comparison or builtin.category == .logical) {
                        return .boolean;
                    }
                }
                // Check for tuple-returning functions (like mandelbrot returning vec4)
                // For now, assume user-defined functions might return color
                if (self.functions.get(op.name)) |_| {
                    // Could analyze return type, for now assume color if 4-tuple
                    return .color;
                }
                return .scalar;
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

    /// Generate a complete fragment shader for one output
    fn generateShader(self: *Self, output: *const AstNode, output_type: OutputType, root: *const AstNode) !void {
        // Emit shader header
        try self.emitHeader();

        // Emit uniform block
        try self.emitUniforms();

        // Emit helper functions (from root definitions)
        try self.emitHelperFunctions(root);

        // Emit main function
        try self.emitMainFunction(output, output_type);
    }

    fn emitHeader(self: *Self) !void {
        try self.writeLine("#version 450");
        try self.writeLine("");
    }

    fn emitUniforms(self: *Self) !void {
        try self.writeLine("layout(set = 3, binding = 0, std140) uniform FragmentUBO {");
        self.indent_level += 1;
        try self.writeLine("float time;");
        try self.writeLine("float padding;");
        try self.writeLine("vec2 res;");
        try self.writeLine("float min_x;");
        try self.writeLine("float max_x;");
        try self.writeLine("float min_y;");
        try self.writeLine("float max_y;");
        self.indent_level -= 1;
        try self.writeLine("} fubo;");
        try self.writeLine("");
        try self.writeLine("layout(location = 0) in vec2 frag_uv;");
        try self.writeLine("layout(location = 0) out vec4 out_color;");
        try self.writeLine("");
    }

    fn emitHelperFunctions(self: *Self, root: *const AstNode) !void {
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

        // Now emit all collected functions (including nested ones)
        var iter = self.functions.iterator();
        while (iter.next()) |entry| {
            const func_name = entry.key_ptr.*;
            const func_node = entry.value_ptr.*;

            if (!self.emitted_functions.contains(func_name)) {
                if (func_node.data == .function_def) {
                    const fd = func_node.data.function_def;
                    try self.emitFunction(fd.name, fd.params, fd.body);
                    try self.emitted_functions.put(fd.name, {});
                }
            }
        }
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

    fn emitFunction(self: *Self, name: []const u8, params: []const []const u8, body: *const AstNode) !void {
        // Infer return type from body
        const return_type = self.inferGlslType(body);

        try self.write(return_type);
        try self.write(" ");
        try self.write(name);
        try self.write("(");

        for (params, 0..) |param, i| {
            if (i > 0) try self.write(", ");
            try self.write("float "); // Assume float params for now
            try self.write(param);
        }

        try self.write(") {\n");
        self.indent_level += 1;

        // Emit function body
        try self.emitFunctionBody(body);

        self.indent_level -= 1;
        try self.writeLine("}");
        try self.writeLine("");
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

        // Set up axis variables
        try self.writeLine("float axis1 = frag_uv.x;");
        try self.writeLine("float axis2 = frag_uv.y;");
        try self.writeLine("float x = frag_uv.x;");
        try self.writeLine("float y = frag_uv.y;");
        try self.writeLine("float time_s = fubo.time;");
        try self.writeLine("");

        // Emit the output expression
        try self.writeIndent();

        switch (output_type) {
            .color => {
                try self.write("vec4 result = ");
                try self.emitExpr(output);
                try self.write(";\n");
                try self.writeLine("out_color = result;");
            },
            .boolean => {
                // For boolean expressions, use corner checking for proper curve rendering
                // Calculate half-pixel size for corner sampling
                try self.writeLine("// Half-pixel offsets for corner sampling");
                try self.writeLine("float dx = (fubo.max_x - fubo.min_x) / (2.0 * fubo.res.x);");
                try self.writeLine("float dy = (fubo.max_y - fubo.min_y) / (2.0 * fubo.res.y);");
                try self.writeLine("");
                try self.writeLine("// Corner coordinates");
                try self.writeLine("float x_m = x - dx;  // x minus");
                try self.writeLine("float x_p = x + dx;  // x plus");
                try self.writeLine("float y_m = y - dy;  // y minus");
                try self.writeLine("float y_p = y + dy;  // y plus");
                try self.writeLine("");

                // Emit the corner-checked boolean expression
                try self.write("bool cond = ");
                try self.emitBooleanWithCornerCheck(output);
                try self.write(";\n");

                // Generate random color from seed
                const r = @as(f32, @floatFromInt((self.color_seed >> 0) & 0xFF)) / 255.0;
                const g = @as(f32, @floatFromInt((self.color_seed >> 8) & 0xFF)) / 255.0;
                const b = @as(f32, @floatFromInt((self.color_seed >> 16) & 0xFF)) / 255.0;

                try self.writeIndent();
                var color_buf: [128]u8 = undefined;
                const color_str = std.fmt.bufPrint(&color_buf, "vec3 region_color = vec3({d:.3}, {d:.3}, {d:.3});\n", .{ r, g, b }) catch "vec3 region_color = vec3(1.0, 0.0, 1.0);\n";
                try self.write(color_str);
                try self.writeLine("out_color = cond ? vec4(region_color, 1.0) : vec4(0.0);");
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

    /// Emit an expression with x and y replaced by corner coordinates
    fn emitExprWithCorner(self: *Self, node: *const AstNode, x_var: []const u8, y_var: []const u8) std.mem.Allocator.Error!void {
        switch (node.data) {
            .number => |n| {
                var num_buf: [64]u8 = undefined;
                const num_str = std.fmt.bufPrint(&num_buf, "{d}", .{n}) catch "0.0";
                try self.write(num_str);
            },
            .bool_lit => |b| {
                try self.write(if (b) "true" else "false");
            },
            .identifier => |name| {
                // Replace x and y with corner coordinates
                if (std.mem.eql(u8, name, "x")) {
                    try self.write(x_var);
                } else if (std.mem.eql(u8, name, "y")) {
                    try self.write(y_var);
                } else if (std.mem.eql(u8, name, "time_s") or std.mem.eql(u8, name, "time.s")) {
                    try self.write("fubo.time");
                } else {
                    try self.write(name);
                }
            },
            .apply => |op| {
                try self.emitApplyWithCorner(op.name, op.args, x_var, y_var);
            },
            .property_access => |pa| {
                // Handle axis property access
                if (pa.base.data == .identifier) {
                    const base_name = pa.base.data.identifier;
                    if (std.mem.eql(u8, base_name, "x") or std.mem.eql(u8, base_name, "axis1")) {
                        if (std.mem.eql(u8, pa.property, "min")) {
                            try self.write("fubo.min_x");
                            return;
                        } else if (std.mem.eql(u8, pa.property, "max")) {
                            try self.write("fubo.max_x");
                            return;
                        } else if (std.mem.eql(u8, pa.property, "res")) {
                            try self.write("fubo.res.x");
                            return;
                        }
                    } else if (std.mem.eql(u8, base_name, "y") or std.mem.eql(u8, base_name, "axis2")) {
                        if (std.mem.eql(u8, pa.property, "min")) {
                            try self.write("fubo.min_y");
                            return;
                        } else if (std.mem.eql(u8, pa.property, "max")) {
                            try self.write("fubo.max_y");
                            return;
                        } else if (std.mem.eql(u8, pa.property, "res")) {
                            try self.write("fubo.res.y");
                            return;
                        }
                    }
                }
                try self.emitExprWithCorner(pa.base, x_var, y_var);
                try self.write(".");
                try self.write(pa.property);
            },
            .tuple => |elements| {
                const n = elements.len;
                try self.write(switch (n) {
                    2 => "vec2(",
                    3 => "vec3(",
                    4 => "vec4(",
                    else => "vec4(",
                });
                for (elements, 0..) |elem, i| {
                    if (i > 0) try self.write(", ");
                    try self.emitExprWithCorner(elem, x_var, y_var);
                }
                try self.write(")");
            },
            .cast => |c| {
                try self.write(c.target_type.toGlsl());
                try self.write("(");
                try self.emitExprWithCorner(c.operand, x_var, y_var);
                try self.write(")");
            },
            .if_expr => |ie| {
                try self.write("(");
                try self.emitExprWithCorner(ie.condition, x_var, y_var);
                try self.write(" ? ");
                try self.emitExprWithCorner(ie.then_branch, x_var, y_var);
                try self.write(" : ");
                if (ie.else_branch) |eb| {
                    try self.emitExprWithCorner(eb, x_var, y_var);
                } else {
                    try self.write("0.0");
                }
                try self.write(")");
            },
            else => {
                try self.write("/* unsupported */");
            },
        }
    }

    /// Emit an apply expression with corner coordinate substitution
    fn emitApplyWithCorner(self: *Self, name: []const u8, args: []*AstNode, x_var: []const u8, y_var: []const u8) std.mem.Allocator.Error!void {
        // Handle type casts
        if (args.len == 1) {
            if (self.getGlslTypeName(name)) |glsl_type| {
                try self.write(glsl_type);
                try self.write("(");
                try self.emitExprWithCorner(args[0], x_var, y_var);
                try self.write(")");
                return;
            }
        }

        if (getBuiltin(name)) |builtin| {
            switch (builtin.emit_style) {
                .infix => {
                    if (args.len >= 2) {
                        try self.write("(");
                        try self.emitExprWithCorner(args[0], x_var, y_var);
                        try self.write(" ");
                        try self.write(builtin.glsl orelse name);
                        try self.write(" ");
                        try self.emitExprWithCorner(args[1], x_var, y_var);
                        try self.write(")");
                    }
                },
                .prefix => {
                    if (args.len >= 1) {
                        try self.write("(");
                        try self.write(builtin.glsl orelse name);
                        try self.emitExprWithCorner(args[0], x_var, y_var);
                        try self.write(")");
                    }
                },
                .postfix => {
                    if (std.mem.eql(u8, name, "square") and args.len >= 1) {
                        try self.write("(");
                        try self.emitExprWithCorner(args[0], x_var, y_var);
                        try self.write(" * ");
                        try self.emitExprWithCorner(args[0], x_var, y_var);
                        try self.write(")");
                    }
                },
                .call => {
                    try self.write(builtin.glsl orelse name);
                    try self.write("(");
                    for (args, 0..) |arg, i| {
                        if (i > 0) try self.write(", ");
                        try self.emitExprWithCorner(arg, x_var, y_var);
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
                try self.emitExprWithCorner(arg, x_var, y_var);
            }
            try self.write(")");
        }
    }

    fn emitExpr(self: *Self, node: *const AstNode) std.mem.Allocator.Error!void {
        switch (node.data) {
            .number => |n| {
                var num_buf: [64]u8 = undefined;
                const num_str = std.fmt.bufPrint(&num_buf, "{d}", .{n}) catch "0.0";
                try self.write(num_str);
            },
            .bool_lit => |b| {
                try self.write(if (b) "true" else "false");
            },
            .identifier => |name| {
                // Map special identifiers
                if (std.mem.eql(u8, name, "time_s") or std.mem.eql(u8, name, "time.s")) {
                    try self.write("fubo.time");
                } else {
                    try self.write(name);
                }
            },
            .apply => |op| {
                try self.emitApply(op.name, op.args);
            },
            .property_access => |pa| {
                // Check if this is an axis property access (x.min, y.max, etc.)
                if (pa.base.data == .identifier) {
                    const base_name = pa.base.data.identifier;
                    // Map axis properties to uniforms
                    if (std.mem.eql(u8, base_name, "x") or std.mem.eql(u8, base_name, "axis1")) {
                        if (std.mem.eql(u8, pa.property, "min")) {
                            try self.write("fubo.min_x");
                            return;
                        } else if (std.mem.eql(u8, pa.property, "max")) {
                            try self.write("fubo.max_x");
                            return;
                        } else if (std.mem.eql(u8, pa.property, "res")) {
                            try self.write("fubo.res.x");
                            return;
                        }
                    } else if (std.mem.eql(u8, base_name, "y") or std.mem.eql(u8, base_name, "axis2")) {
                        if (std.mem.eql(u8, pa.property, "min")) {
                            try self.write("fubo.min_y");
                            return;
                        } else if (std.mem.eql(u8, pa.property, "max")) {
                            try self.write("fubo.max_y");
                            return;
                        } else if (std.mem.eql(u8, pa.property, "res")) {
                            try self.write("fubo.res.y");
                            return;
                        }
                    }
                }
                // Regular property access
                try self.emitExpr(pa.base);
                try self.write(".");
                try self.write(pa.property);
            },
            .tuple => |elements| {
                const n = elements.len;
                try self.write(switch (n) {
                    2 => "vec2(",
                    3 => "vec3(",
                    4 => "vec4(",
                    else => "vec4(",
                });
                for (elements, 0..) |elem, i| {
                    if (i > 0) try self.write(", ");
                    try self.emitExpr(elem);
                }
                try self.write(")");
            },
            .cast => |c| {
                try self.write(c.target_type.toGlsl());
                try self.write("(");
                try self.emitExpr(c.operand);
                try self.write(")");
            },
            .index => |idx| {
                try self.emitExpr(idx.base);
                try self.write("[");
                try self.emitExpr(idx.index_expr);
                try self.write("]");
            },
            .if_expr => |ie| {
                // Ternary operator
                try self.write("(");
                try self.emitExpr(ie.condition);
                try self.write(" ? ");
                try self.emitExpr(ie.then_branch);
                try self.write(" : ");
                if (ie.else_branch) |eb| {
                    try self.emitExpr(eb);
                } else {
                    try self.write("0.0");
                }
                try self.write(")");
            },
            .block => |stmts| {
                // For expression context, we just emit the last expression
                if (stmts.len > 0) {
                    try self.emitExpr(stmts[stmts.len - 1]);
                }
            },
            else => {
                try self.write("/* unsupported */");
            },
        }
    }

    fn emitApply(self: *Self, name: []const u8, args: []*AstNode) std.mem.Allocator.Error!void {
        // Handle type casts that were parsed as function calls
        if (args.len == 1) {
            if (self.getGlslTypeName(name)) |glsl_type| {
                try self.write(glsl_type);
                try self.write("(");
                try self.emitExpr(args[0]);
                try self.write(")");
                return;
            }
        }

        if (getBuiltin(name)) |builtin| {
            switch (builtin.emit_style) {
                .infix => {
                    if (args.len >= 2) {
                        try self.write("(");
                        try self.emitExpr(args[0]);
                        try self.write(" ");
                        try self.write(builtin.glsl orelse name);
                        try self.write(" ");
                        try self.emitExpr(args[1]);
                        try self.write(")");
                    }
                },
                .prefix => {
                    if (args.len >= 1) {
                        try self.write("(");
                        try self.write(builtin.glsl orelse name);
                        try self.emitExpr(args[0]);
                        try self.write(")");
                    }
                },
                .postfix => {
                    // Handle square specially: x² → (x * x)
                    if (std.mem.eql(u8, name, "square") and args.len >= 1) {
                        try self.write("(");
                        try self.emitExpr(args[0]);
                        try self.write(" * ");
                        try self.emitExpr(args[0]);
                        try self.write(")");
                    }
                },
                .call => {
                    try self.write(builtin.glsl orelse name);
                    try self.write("(");
                    for (args, 0..) |arg, i| {
                        if (i > 0) try self.write(", ");
                        try self.emitExpr(arg);
                    }
                    try self.write(")");
                },
            }
        } else {
            // User-defined function or unknown
            try self.write(name);
            try self.write("(");
            for (args, 0..) |arg, i| {
                if (i > 0) try self.write(", ");
                try self.emitExpr(arg);
            }
            try self.write(")");
        }
    }

    fn inferGlslType(self: *Self, node: *const AstNode) []const u8 {
        return switch (node.data) {
            .number => "float",
            .bool_lit => "bool",
            .tuple => |elements| switch (elements.len) {
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
