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

    /// Collect function definitions and constants from the AST
    fn collectDefinitions(self: *Self, node: *const AstNode) !void {
        switch (node.data) {
            .block => |stmts| {
                for (stmts) |stmt| {
                    try self.collectDefinitions(stmt);
                }
            },
            .function_def => |fd| {
                try self.functions.put(fd.name, node);
            },
            .binding => |b| {
                // Constants/variables will be handled during emission
                _ = b;
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
                            // Check if it's a simple constant
                            if (self.isConstantBinding(b.value)) {
                                try self.emitConstantDefine(b.pattern, b.value);
                            }
                        },
                        .function_def => |fd| {
                            if (!self.emitted_functions.contains(fd.name)) {
                                try self.emitFunction(fd.name, fd.params, fd.body);
                                try self.emitted_functions.put(fd.name, {});
                            }
                        },
                        else => {},
                    }
                }
            },
            else => {},
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
                // Emit all statements, return the last one
                for (stmts, 0..) |stmt, i| {
                    const is_last = i == stmts.len - 1;
                    if (is_last) {
                        try self.writeIndent();
                        try self.write("return ");
                        try self.emitExpr(stmt);
                        try self.write(";\n");
                    } else {
                        try self.emitStatement(stmt);
                    }
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
                try self.writeIndent();
                switch (b.pattern) {
                    .single => |name| {
                        const var_type = self.inferGlslType(b.value);
                        try self.write(var_type);
                        try self.write(" ");
                        try self.write(name);
                        try self.write(" = ");
                        try self.emitExpr(b.value);
                        try self.write(";\n");
                        try self.defined_vars.put(name, {});
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
                            try self.write("float ");
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
                try self.write("bool cond = ");
                try self.emitExpr(output);
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
                try self.emitExpr(pa.base);
                try self.write(".");
                // Map property names
                if (std.mem.eql(u8, pa.property, "min")) {
                    try self.write("x"); // Would need context for axis
                } else if (std.mem.eql(u8, pa.property, "max")) {
                    try self.write("y");
                } else if (std.mem.eql(u8, pa.property, "res")) {
                    try self.write("z");
                } else {
                    try self.write(pa.property);
                }
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
        _ = self;
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
                return "float";
            },
            .if_expr => |ie| {
                // Infer from branches
                _ = ie;
                return "float"; // Simplified
            },
            .block => |stmts| {
                if (stmts.len > 0) {
                    return "float"; // Would need recursion
                }
                return "void";
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
};

/// Convenience function to generate GLSL from an AST
pub fn generateGlsl(allocator: std.mem.Allocator, ast_root: *const AstNode) !GenerationResult {
    var gen = GlslGenerator.init(allocator);
    defer gen.deinit();
    return gen.generate(ast_root);
}
