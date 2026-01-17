//! Shader Generation Pipeline Test
//!
//! Tests the complete pipeline: Logos expression → Tokens → AST → GLSL → SPIRV
//! This isolates shader generation from GPU/driver issues for debugging.
//!
//! Run with: zig build run-shader-gen-test

const std = @import("std");
const lexer = @import("lang/lexer.zig");
const parser = @import("lang/parser.zig");
const glsl_gen = @import("lang/glsl_gen.zig");
const Lexer = lexer.Lexer;
const LexerConfig = lexer.LexerConfig;

// C bindings for shaderc
const c = @cImport({
    @cInclude("shaderc/shaderc.h");
});

var global_compiler: ?c.shaderc_compiler_t = null;

fn initShaderCompiler() !void {
    if (global_compiler == null) {
        global_compiler = c.shaderc_compiler_initialize();
        if (global_compiler == null) {
            return error.CompilerInitFailed;
        }
    }
}

fn deinitShaderCompiler() void {
    if (global_compiler) |compiler| {
        c.shaderc_compiler_release(compiler);
        global_compiler = null;
    }
}

/// Compile GLSL to SPIRV using shaderc directly
/// Returns the SPIRV bytecode or an error
fn compileGlslToSpirv(allocator: std.mem.Allocator, glsl_source: []const u8) ![]u8 {
    const compiler = global_compiler orelse return error.CompilerNotInitialized;

    // Create compile options with Vulkan target
    const options = c.shaderc_compile_options_initialize();
    if (options == null) {
        return error.OutOfMemory;
    }
    defer c.shaderc_compile_options_release(options);

    // No optimization for easier debugging
    c.shaderc_compile_options_set_optimization_level(options, c.shaderc_optimization_level_zero);
    // Target Vulkan 1.0 / SPIRV 1.0 for maximum compatibility
    c.shaderc_compile_options_set_target_env(options, c.shaderc_target_env_vulkan, c.shaderc_env_version_vulkan_1_0);
    c.shaderc_compile_options_set_target_spirv(options, c.shaderc_spirv_version_1_0);

    // Compile GLSL to SPIRV
    const result = c.shaderc_compile_into_spv(
        compiler,
        glsl_source.ptr,
        glsl_source.len,
        c.shaderc_fragment_shader,
        "shader",
        "main",
        options,
    );

    if (result == null) {
        return error.ShaderCompileFailed;
    }
    defer c.shaderc_result_release(result);

    // Check compilation status
    const status = c.shaderc_result_get_compilation_status(result);
    if (status != c.shaderc_compilation_status_success) {
        const error_msg = c.shaderc_result_get_error_message(result);
        if (error_msg != null) {
            std.log.err("Shader compilation error: {s}", .{error_msg});
        }
        return error.ShaderCompileFailed;
    }

    // Get SPIRV output
    const spirv_len = c.shaderc_result_get_length(result);
    const spirv_ptr = c.shaderc_result_get_bytes(result);

    if (spirv_ptr == null or spirv_len == 0) {
        return error.ShaderCompileFailed;
    }

    // Copy to Zig-managed memory
    const output = try allocator.alloc(u8, spirv_len);
    @memcpy(output, @as([*]const u8, @ptrCast(spirv_ptr))[0..spirv_len]);

    return output;
}

/// Test result structure
const TestResult = struct {
    expression: []const u8,
    token_count: usize,
    ast_valid: bool,
    glsl_source: ?[]const u8,
    glsl_valid: bool,
    spirv_size: usize,
    spirv_valid: bool,
    error_msg: ?[]const u8,
};

/// Run a complete shader generation test for an expression
fn testExpression(allocator: std.mem.Allocator, expression: []const u8) !TestResult {
    var result = TestResult{
        .expression = expression,
        .token_count = 0,
        .ast_valid = false,
        .glsl_source = null,
        .glsl_valid = false,
        .spirv_size = 0,
        .spirv_valid = false,
        .error_msg = null,
    };

    // Step 1: Tokenize
    std.debug.print("\n============================================================\n", .{});
    std.debug.print("Testing expression: {s}\n", .{expression});
    std.debug.print("============================================================\n\n", .{});

    var lex = Lexer.init(allocator, LexerConfig.logosPatterns()) catch |err| {
        result.error_msg = @errorName(err);
        return result;
    };
    defer lex.deinit();

    const tokens = lex.tokenize(expression) catch |err| {
        result.error_msg = @errorName(err);
        return result;
    };
    defer allocator.free(tokens);

    result.token_count = tokens.len;

    std.debug.print("STEP 1: TOKENIZATION\n", .{});
    std.debug.print("----------------------------------------\n", .{});
    std.debug.print("Token count: {d}\n\n", .{tokens.len});
    for (tokens, 0..) |tok, i| {
        std.debug.print("  [{d:3}] {s:12} \"{s}\"\n", .{ i, @tagName(tok.token_type), tok.text });
    }
    std.debug.print("\n", .{});

    // Step 2: Parse
    std.debug.print("STEP 2: PARSING\n", .{});
    std.debug.print("----------------------------------------\n", .{});

    const parse_result = parser.parseTokens(allocator, tokens) catch |err| {
        result.error_msg = @errorName(err);
        std.debug.print("Parse error: {s}\n", .{@errorName(err)});
        return result;
    };

    if (parse_result.errors.len > 0) {
        std.debug.print("Parse errors:\n", .{});
        for (parse_result.errors) |perr| {
            std.debug.print("  - {s}\n", .{perr.message});
        }
        result.error_msg = "ParseErrors";
        allocator.free(parse_result.errors);
        return result;
    }
    defer allocator.free(parse_result.errors);

    result.ast_valid = true;
    std.debug.print("AST generated successfully\n", .{});
    parse_result.ast.debugPrintTree();

    // Step 3: Generate GLSL
    std.debug.print("STEP 3: GLSL GENERATION\n", .{});
    std.debug.print("----------------------------------------\n", .{});

    var gen = glsl_gen.GlslGenerator.init(allocator);
    defer gen.deinit();

    const gen_result = gen.generate(parse_result.ast) catch |err| {
        result.error_msg = @errorName(err);
        std.debug.print("GLSL generation error: {s}\n", .{@errorName(err)});
        return result;
    };
    defer {
        for (gen_result.shaders) |shader| {
            allocator.free(shader.source);
        }
        allocator.free(gen_result.shaders);
        allocator.free(gen_result.errors);
    }

    if (gen_result.shaders.len == 0) {
        result.error_msg = "NoShadersGenerated";
        std.debug.print("No shaders generated\n", .{});
        return result;
    }

    const shader = gen_result.shaders[0];
    result.glsl_source = try allocator.dupe(u8, shader.source);
    result.glsl_valid = true;

    std.debug.print("Generated GLSL ({d} bytes, type: {s}):\n\n", .{ shader.source.len, @tagName(shader.output_type) });
    std.debug.print("{s}\n", .{shader.source});

    // Step 4: Compile to SPIRV
    std.debug.print("STEP 4: SPIRV COMPILATION\n", .{});
    std.debug.print("----------------------------------------\n", .{});

    const spirv = compileGlslToSpirv(allocator, shader.source) catch |err| {
        result.error_msg = @errorName(err);
        std.debug.print("SPIRV compilation error: {s}\n", .{@errorName(err)});
        return result;
    };
    defer allocator.free(spirv);

    result.spirv_size = spirv.len;

    // Validate SPIRV magic number
    if (spirv.len >= 4 and
        spirv[0] == 0x03 and
        spirv[1] == 0x02 and
        spirv[2] == 0x23 and
        spirv[3] == 0x07)
    {
        result.spirv_valid = true;
        std.debug.print("SPIRV compiled successfully: {d} bytes\n", .{spirv.len});
        std.debug.print("Magic number: 0x{X:0>2}{X:0>2}{X:0>2}{X:0>2} (valid)\n", .{ spirv[3], spirv[2], spirv[1], spirv[0] });
    } else {
        result.error_msg = "InvalidSpirvMagic";
        std.debug.print("Invalid SPIRV magic number\n", .{});
    }

    return result;
}

/// Print a summary of test results
fn printSummary(results: []const TestResult) void {
    std.debug.print("\n============================================================\n", .{});
    std.debug.print("TEST SUMMARY\n", .{});
    std.debug.print("============================================================\n\n", .{});

    var passed: usize = 0;
    var failed: usize = 0;

    for (results) |r| {
        const status = if (r.spirv_valid) "✓ PASS" else "✗ FAIL";
        std.debug.print("{s}: {s}\n", .{ status, r.expression });
        if (!r.spirv_valid) {
            if (r.error_msg) |msg| {
                std.debug.print("   Error: {s}\n", .{msg});
            }
        } else {
            std.debug.print("   Tokens: {d}, GLSL: {d} bytes, SPIRV: {d} bytes\n", .{
                r.token_count,
                if (r.glsl_source) |g| g.len else 0,
                r.spirv_size,
            });
        }

        if (r.spirv_valid) {
            passed += 1;
        } else {
            failed += 1;
        }
    }

    std.debug.print("\nTotal: {d} passed, {d} failed\n", .{ passed, failed });
}

// ============================================================================
// Test cases
// ============================================================================

// The main problematic expression that crashes
test "x²+y²>1 shader generation" {
    // Use arena allocator since AST nodes don't have proper cleanup yet
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    // Initialize shader compiler
    try initShaderCompiler();
    defer deinitShaderCompiler();

    const result = try testExpression(allocator, "x²+y²>1");

    try std.testing.expect(result.ast_valid);
    try std.testing.expect(result.glsl_valid);
    try std.testing.expect(result.spirv_valid);
    try std.testing.expectEqual(@as(usize, 7), result.token_count);
}

test "simple x+y shader generation" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    try initShaderCompiler();
    defer deinitShaderCompiler();

    const result = try testExpression(allocator, "x+y");

    try std.testing.expect(result.ast_valid);
    try std.testing.expect(result.glsl_valid);
    try std.testing.expect(result.spirv_valid);
}

test "x²+y² (without comparison)" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    try initShaderCompiler();
    defer deinitShaderCompiler();

    const result = try testExpression(allocator, "x²+y²");

    try std.testing.expect(result.ast_valid);
    try std.testing.expect(result.glsl_valid);
    try std.testing.expect(result.spirv_valid);
}

test "circle equation x²+y²=1" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    try initShaderCompiler();
    defer deinitShaderCompiler();

    const result = try testExpression(allocator, "x²+y²=1");

    try std.testing.expect(result.ast_valid);
    try std.testing.expect(result.glsl_valid);
    try std.testing.expect(result.spirv_valid);
}

test "sin(x)+cos(y)>0" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    try initShaderCompiler();
    defer deinitShaderCompiler();

    const result = try testExpression(allocator, "sin(x)+cos(y)>0");

    try std.testing.expect(result.ast_valid);
    try std.testing.expect(result.glsl_valid);
    try std.testing.expect(result.spirv_valid);
}

test "complex boolean: x²+y²>1 and x>0" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    try initShaderCompiler();
    defer deinitShaderCompiler();

    const result = try testExpression(allocator, "x²+y²>1 and x>0");

    try std.testing.expect(result.ast_valid);
    try std.testing.expect(result.glsl_valid);
    try std.testing.expect(result.spirv_valid);
}

// ============================================================================
// Main entry point for standalone execution
// ============================================================================

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    // Initialize shader compiler
    try initShaderCompiler();
    defer deinitShaderCompiler();

    const test_expressions = [_][]const u8{
        // Basic expressions
        "x+y",
        "x*y",

        // Square operator (the problematic one)
        "x²",
        "x²+y²",

        // Comparisons (these generate boolean shaders)
        "x>0",
        "x²+y²>1", // The main problematic expression!
        "x²+y²=1",
        "x²+y²<1",

        // Complex expressions
        "sin(x)+cos(y)",
        "sin(x)>0.5",
        "x²+y²>1 and x>0",
        "x²+y²>1 or y<0",
    };

    var results: std.ArrayList(TestResult) = .{ .items = &.{}, .capacity = 0 };
    defer {
        for (results.items) |r| {
            if (r.glsl_source) |s| allocator.free(s);
        }
        results.deinit(allocator);
    }

    for (test_expressions) |expr| {
        const result = testExpression(allocator, expr) catch |err| {
            std.debug.print("Fatal error testing '{s}': {s}\n", .{ expr, @errorName(err) });
            continue;
        };
        try results.append(allocator, result);
    }

    printSummary(results.items);
}
