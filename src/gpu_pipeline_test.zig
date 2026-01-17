//! GPU Pipeline Test
//!
//! Tests the actual GPU pipeline creation that crashes on AMD RADV.
//! This tests: GLSL → SPIRV → SDL Shader → GPU Pipeline
//!
//! Run with: zig build test-gpu-pipeline
//! Or standalone: zig build run-gpu-pipeline-test

const std = @import("std");
const lexer = @import("lang/lexer.zig");
const parser = @import("lang/parser.zig");
const glsl_gen = @import("lang/glsl_gen.zig");
const shader_compiler = @import("renderer/shader_compiler.zig");
const shaders = @import("renderer/shaders/shaders.zig");

const c = shader_compiler.sdl;

// Import the shaderc cimport from shader_compiler for reflection
const shaderc_c = @cImport({
    @cInclude("shaderc/shaderc.h");
    @cInclude("SDL3/SDL.h");
    @cInclude("SDL3/SDL_gpu.h");
    @cInclude("SDL3_shadercross/SDL_shadercross.h");
});

/// Stored state to simulate app's gpu_pipeline state
var stored_vertex_shader: ?*anyopaque = null;
var stored_fragment_shader: ?*anyopaque = null;
var stored_pipeline: ?*c.SDL_GPUGraphicsPipeline = null;

/// Test creating a GPU pipeline with dynamically generated shaders
/// This replicates EXACTLY what the app's updateFragmentShader does:
/// - Does NOT release old shaders
/// - Does NOT release old pipeline
/// - Just creates new ones and overwrites the pointers
fn testPipelineCreation(allocator: std.mem.Allocator, device: *c.SDL_GPUDevice, expression: []const u8) !void {
    std.debug.print("\n============================================================\n", .{});
    std.debug.print("Testing pipeline for: {s}\n", .{expression});
    std.debug.print("============================================================\n\n", .{});

    // Step 1: Generate GLSL from expression
    std.debug.print("Step 1: Tokenize...\n", .{});
    var lex = try lexer.Lexer.init(allocator, lexer.LexerConfig.logosPatterns());
    defer lex.deinit();
    const tokens = try lex.tokenize(expression);
    defer allocator.free(tokens);
    std.debug.print("  Tokens: {d}\n", .{tokens.len});

    std.debug.print("Step 2: Parse...\n", .{});
    const parse_result = try parser.parseTokens(allocator, tokens);
    defer allocator.free(parse_result.errors);
    std.debug.print("  AST valid: true\n", .{});

    std.debug.print("Step 3: Generate GLSL...\n", .{});
    var gen = glsl_gen.GlslGenerator.init(allocator);
    defer gen.deinit();
    const gen_result = try gen.generate(parse_result.ast);
    defer {
        for (gen_result.shaders) |shader| {
            allocator.free(shader.source);
        }
        allocator.free(gen_result.shaders);
        allocator.free(gen_result.errors);
    }

    if (gen_result.shaders.len == 0) {
        return error.NoShadersGenerated;
    }
    const fragment_glsl = gen_result.shaders[0].source;
    std.debug.print("  Fragment GLSL: {d} bytes\n", .{fragment_glsl.len});

    // Step 4: Compile vertex shader (same as default) - like updateFragmentShader
    std.debug.print("Step 4: Compile vertex shader...\n", .{});
    const new_vertex = try shader_compiler.compileAndCreateShader(
        allocator,
        device,
        shaders.default_vertex_glsl,
        .vertex,
    );
    std.debug.print("  Vertex shader created: {*}\n", .{new_vertex});

    // Step 5: Compile fragment shader (generated)
    std.debug.print("Step 5: Compile fragment shader...\n", .{});
    const new_fragment = try shader_compiler.compileAndCreateShader(
        allocator,
        device,
        fragment_glsl,
        .fragment,
    );
    std.debug.print("  Fragment shader created: {*}\n", .{new_fragment});

    // *** CRITICAL: Replicate app behavior - DON'T release old resources ***
    // This is exactly what updateFragmentShader does in gpu_pipeline.zig
    std.debug.print("  NOT releasing old shaders/pipeline (like app does)...\n", .{});
    stored_pipeline = null; // Just null the pointer, don't release
    stored_vertex_shader = new_vertex;
    stored_fragment_shader = new_fragment;

    // Step 6: Create pipeline - THIS IS WHERE IT CRASHES ON AMD RADV
    std.debug.print("Step 6: Create GPU pipeline...\n", .{});

    const vs: *c.SDL_GPUShader = @ptrCast(new_vertex);
    const fs: *c.SDL_GPUShader = @ptrCast(new_fragment);

    var color_targets: [1]c.SDL_GPUColorTargetDescription = .{
        .{
            .format = c.SDL_GPU_TEXTUREFORMAT_R8G8B8A8_UNORM,
            .blend_state = .{
                .src_color_blendfactor = c.SDL_GPU_BLENDFACTOR_SRC_ALPHA,
                .dst_color_blendfactor = c.SDL_GPU_BLENDFACTOR_ONE_MINUS_SRC_ALPHA,
                .color_blend_op = c.SDL_GPU_BLENDOP_ADD,
                .src_alpha_blendfactor = c.SDL_GPU_BLENDFACTOR_ONE,
                .dst_alpha_blendfactor = c.SDL_GPU_BLENDFACTOR_ONE_MINUS_SRC_ALPHA,
                .alpha_blend_op = c.SDL_GPU_BLENDOP_ADD,
                .color_write_mask = 0xF,
                .enable_blend = true,
                .enable_color_write_mask = false,
            },
        },
    };

    const pipeline_info = c.SDL_GPUGraphicsPipelineCreateInfo{
        .vertex_shader = vs,
        .fragment_shader = fs,
        .vertex_input_state = .{
            .vertex_buffer_descriptions = null,
            .num_vertex_buffers = 0,
            .vertex_attributes = null,
            .num_vertex_attributes = 0,
        },
        .primitive_type = c.SDL_GPU_PRIMITIVETYPE_TRIANGLELIST,
        .rasterizer_state = .{
            .fill_mode = c.SDL_GPU_FILLMODE_FILL,
            .cull_mode = c.SDL_GPU_CULLMODE_NONE,
            .front_face = c.SDL_GPU_FRONTFACE_COUNTER_CLOCKWISE,
            .depth_bias_constant_factor = 0,
            .depth_bias_clamp = 0,
            .depth_bias_slope_factor = 0,
            .enable_depth_bias = false,
            .enable_depth_clip = false,
        },
        .multisample_state = .{
            .sample_count = c.SDL_GPU_SAMPLECOUNT_1,
            .sample_mask = 0,
            .enable_mask = false,
        },
        .depth_stencil_state = .{
            .compare_op = c.SDL_GPU_COMPAREOP_ALWAYS,
            .back_stencil_state = std.mem.zeroes(c.SDL_GPUStencilOpState),
            .front_stencil_state = std.mem.zeroes(c.SDL_GPUStencilOpState),
            .compare_mask = 0,
            .write_mask = 0,
            .enable_depth_test = false,
            .enable_depth_write = false,
            .enable_stencil_test = false,
        },
        .target_info = .{
            .color_target_descriptions = &color_targets,
            .num_color_targets = 1,
            .depth_stencil_format = c.SDL_GPU_TEXTUREFORMAT_INVALID,
            .has_depth_stencil_target = false,
        },
        .props = 0,
    };

    std.debug.print("  Calling SDL_CreateGPUGraphicsPipeline...\n", .{});
    const pipeline = c.SDL_CreateGPUGraphicsPipeline(device, &pipeline_info);

    if (pipeline == null) {
        const err = c.SDL_GetError();
        if (err != null and err[0] != 0) {
            std.debug.print("  ERROR: {s}\n", .{err});
        }
        return error.PipelineCreationFailed;
    }

    std.debug.print("  Pipeline created successfully: {*}\n", .{pipeline});
    stored_pipeline = pipeline;

    // *** DON'T cleanup - leak resources like the app does ***
    std.debug.print("\n✓ Pipeline test PASSED for: {s}\n", .{expression});
}

/// Simulate a frame render pass (like the app does)
fn simulateFrameRendering(device: *c.SDL_GPUDevice, pipeline: *c.SDL_GPUGraphicsPipeline) !void {
    // Create a texture to render to (like the app's render target)
    const texture_info = c.SDL_GPUTextureCreateInfo{
        .type = c.SDL_GPU_TEXTURETYPE_2D,
        .format = c.SDL_GPU_TEXTUREFORMAT_R8G8B8A8_UNORM,
        .usage = c.SDL_GPU_TEXTUREUSAGE_COLOR_TARGET | c.SDL_GPU_TEXTUREUSAGE_SAMPLER,
        .width = 256,
        .height = 256,
        .layer_count_or_depth = 1,
        .num_levels = 1,
        .sample_count = c.SDL_GPU_SAMPLECOUNT_1,
        .props = 0,
    };

    const render_texture = c.SDL_CreateGPUTexture(device, &texture_info) orelse {
        return error.TextureCreationFailed;
    };
    defer c.SDL_ReleaseGPUTexture(device, render_texture);

    // Acquire command buffer
    const cmd = c.SDL_AcquireGPUCommandBuffer(device) orelse {
        return error.CommandBufferFailed;
    };

    // Set up color target
    var color_target = std.mem.zeroes(c.SDL_GPUColorTargetInfo);
    color_target.texture = render_texture;
    color_target.clear_color = .{ .r = 0.0, .g = 0.0, .b = 0.0, .a = 1.0 };
    color_target.load_op = c.SDL_GPU_LOADOP_CLEAR;
    color_target.store_op = c.SDL_GPU_STOREOP_STORE;

    // Begin render pass
    const render_pass = c.SDL_BeginGPURenderPass(cmd, &color_target, 1, null) orelse {
        _ = c.SDL_SubmitGPUCommandBuffer(cmd);
        return error.RenderPassFailed;
    };

    // Bind pipeline and draw
    c.SDL_BindGPUGraphicsPipeline(render_pass, pipeline);
    c.SDL_DrawGPUPrimitives(render_pass, 3, 1, 0, 0);

    // End render pass
    c.SDL_EndGPURenderPass(render_pass);

    // Submit with fence
    const fence = c.SDL_SubmitGPUCommandBufferAndAcquireFence(cmd);
    if (fence != null) {
        _ = c.SDL_WaitForGPUFences(device, true, @ptrCast(&fence), 1);
        c.SDL_ReleaseGPUFence(device, fence);
    }
}

/// Create a pipeline from shaders
fn createPipelineFromShaders(device: *c.SDL_GPUDevice, vertex_shader: *anyopaque, fragment_shader: *anyopaque) !*c.SDL_GPUGraphicsPipeline {
    const vs: *c.SDL_GPUShader = @ptrCast(vertex_shader);
    const fs: *c.SDL_GPUShader = @ptrCast(fragment_shader);

    var color_targets: [1]c.SDL_GPUColorTargetDescription = .{
        .{
            .format = c.SDL_GPU_TEXTUREFORMAT_R8G8B8A8_UNORM,
            .blend_state = .{
                .src_color_blendfactor = c.SDL_GPU_BLENDFACTOR_SRC_ALPHA,
                .dst_color_blendfactor = c.SDL_GPU_BLENDFACTOR_ONE_MINUS_SRC_ALPHA,
                .color_blend_op = c.SDL_GPU_BLENDOP_ADD,
                .src_alpha_blendfactor = c.SDL_GPU_BLENDFACTOR_ONE,
                .dst_alpha_blendfactor = c.SDL_GPU_BLENDFACTOR_ONE_MINUS_SRC_ALPHA,
                .alpha_blend_op = c.SDL_GPU_BLENDOP_ADD,
                .color_write_mask = 0xF,
                .enable_blend = true,
                .enable_color_write_mask = false,
            },
        },
    };

    const pipeline_info = c.SDL_GPUGraphicsPipelineCreateInfo{
        .vertex_shader = vs,
        .fragment_shader = fs,
        .vertex_input_state = .{
            .vertex_buffer_descriptions = null,
            .num_vertex_buffers = 0,
            .vertex_attributes = null,
            .num_vertex_attributes = 0,
        },
        .primitive_type = c.SDL_GPU_PRIMITIVETYPE_TRIANGLELIST,
        .rasterizer_state = .{
            .fill_mode = c.SDL_GPU_FILLMODE_FILL,
            .cull_mode = c.SDL_GPU_CULLMODE_NONE,
            .front_face = c.SDL_GPU_FRONTFACE_COUNTER_CLOCKWISE,
            .depth_bias_constant_factor = 0,
            .depth_bias_clamp = 0,
            .depth_bias_slope_factor = 0,
            .enable_depth_bias = false,
            .enable_depth_clip = false,
        },
        .multisample_state = .{
            .sample_count = c.SDL_GPU_SAMPLECOUNT_1,
            .sample_mask = 0,
            .enable_mask = false,
        },
        .depth_stencil_state = .{
            .compare_op = c.SDL_GPU_COMPAREOP_ALWAYS,
            .back_stencil_state = std.mem.zeroes(c.SDL_GPUStencilOpState),
            .front_stencil_state = std.mem.zeroes(c.SDL_GPUStencilOpState),
            .compare_mask = 0,
            .write_mask = 0,
            .enable_depth_test = false,
            .enable_depth_write = false,
            .enable_stencil_test = false,
        },
        .target_info = .{
            .color_target_descriptions = &color_targets,
            .num_color_targets = 1,
            .depth_stencil_format = c.SDL_GPU_TEXTUREFORMAT_INVALID,
            .has_depth_stencil_target = false,
        },
        .props = 0,
    };

    const pipeline = c.SDL_CreateGPUGraphicsPipeline(device, &pipeline_info);
    if (pipeline == null) {
        const err = c.SDL_GetError();
        if (err != null and err[0] != 0) {
            std.debug.print("Pipeline creation failed: {s}\n", .{err});
        }
        return error.PipelineCreationFailed;
    }

    return pipeline.?;
}

var test_window: ?*c.SDL_Window = null;

/// Initialize SDL and get GPU device (with window, like dvui does)
fn initSdlGpu() !*c.SDL_GPUDevice {
    // Initialize SDL with video (required for GPU)
    if (!c.SDL_Init(c.SDL_INIT_VIDEO)) {
        const err = c.SDL_GetError();
        if (err != null and err[0] != 0) {
            std.debug.print("SDL_Init failed: {s}\n", .{err});
        } else {
            std.debug.print("SDL_Init failed: unknown error\n", .{});
        }
        return error.SdlInitFailed;
    }

    // Create a window first (like dvui does)
    test_window = c.SDL_CreateWindow(
        "GPU Pipeline Test",
        800,
        600,
        c.SDL_WINDOW_HIDDEN, // Hidden so it doesn't show during tests
    );

    if (test_window == null) {
        const err = c.SDL_GetError();
        if (err != null and err[0] != 0) {
            std.debug.print("SDL_CreateWindow failed: {s}\n", .{err});
        }
        c.SDL_Quit();
        return error.WindowCreationFailed;
    }
    std.debug.print("Test window created (hidden)\n", .{});

    // Create GPU device
    const device = c.SDL_CreateGPUDevice(
        c.SDL_GPU_SHADERFORMAT_SPIRV,
        true, // debug mode
        null, // no specific device name
    );

    if (device == null) {
        const err = c.SDL_GetError();
        if (err != null and err[0] != 0) {
            std.debug.print("SDL_CreateGPUDevice failed: {s}\n", .{err});
        } else {
            std.debug.print("SDL_CreateGPUDevice failed: unknown error\n", .{});
        }
        c.SDL_DestroyWindow(test_window);
        c.SDL_Quit();
        return error.GpuDeviceCreationFailed;
    }

    // Claim the window for GPU rendering (like dvui does)
    if (!c.SDL_ClaimWindowForGPUDevice(device, test_window)) {
        const err = c.SDL_GetError();
        if (err != null and err[0] != 0) {
            std.debug.print("SDL_ClaimWindowForGPUDevice failed: {s}\n", .{err});
        }
        c.SDL_DestroyGPUDevice(device);
        c.SDL_DestroyWindow(test_window);
        c.SDL_Quit();
        return error.ClaimWindowFailed;
    }
    std.debug.print("Window claimed for GPU device\n", .{});

    // Get device info
    const driver = c.SDL_GetGPUDeviceDriver(device);
    std.debug.print("GPU Device created\n", .{});
    if (driver != null) {
        std.debug.print("  Driver: {s}\n", .{driver});
    } else {
        std.debug.print("  Driver: unknown\n", .{});
    }

    return device.?;
}

fn deinitSdlGpu(device: *c.SDL_GPUDevice) void {
    if (test_window) |window| {
        c.SDL_ReleaseWindowFromGPUDevice(device, window);
        c.SDL_DestroyWindow(window);
        test_window = null;
    }
    c.SDL_DestroyGPUDevice(device);
    c.SDL_Quit();
}

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    std.debug.print("\n", .{});
    std.debug.print("============================================================\n", .{});
    std.debug.print("GPU PIPELINE TEST\n", .{});
    std.debug.print("Tests actual GPU pipeline creation (crashes on AMD RADV)\n", .{});
    std.debug.print("============================================================\n\n", .{});

    // Initialize SDL GPU
    std.debug.print("Initializing SDL GPU...\n", .{});
    const device = initSdlGpu() catch |err| {
        std.debug.print("Failed to initialize SDL GPU: {s}\n", .{@errorName(err)});
        std.debug.print("\nNote: This test requires a display and GPU.\n", .{});
        std.debug.print("If running headless, set SDL_VIDEODRIVER=dummy\n", .{});
        return err;
    };
    defer deinitSdlGpu(device);

    // Initialize shader compiler
    std.debug.print("Initializing shader compiler...\n", .{});
    try shader_compiler.init();
    defer shader_compiler.deinit();

    // CRITICAL TEST: Replicate exact app scenario
    // 1. Create default pipeline first (like app startup)
    // 2. Then create dynamic pipeline (like when user types expression)
    // This is the scenario that crashes on AMD RADV
    std.debug.print("\n>>> CRITICAL TEST: App scenario replication <<<\n", .{});
    std.debug.print("Creating default pipeline first (like app startup)...\n", .{});

    // Create default shaders and pipeline (stored in global vars like app does)
    stored_vertex_shader = try shader_compiler.compileAndCreateShader(
        allocator,
        device,
        shaders.default_vertex_glsl,
        .vertex,
    );
    stored_fragment_shader = try shader_compiler.compileAndCreateShader(
        allocator,
        device,
        shaders.default_fragment_glsl,
        .fragment,
    );

    stored_pipeline = try createPipelineFromShaders(device, stored_vertex_shader.?, stored_fragment_shader.?);
    std.debug.print("Default pipeline created: {*}\n\n", .{stored_pipeline.?});

    // CRITICAL: Simulate what app does - render some frames with default pipeline
    std.debug.print("Simulating frame rendering (like app does)...\n", .{});
    try simulateFrameRendering(device, stored_pipeline.?);
    try simulateFrameRendering(device, stored_pipeline.?);
    try simulateFrameRendering(device, stored_pipeline.?);
    std.debug.print("Frame rendering complete\n\n", .{});

    // Now test creating NEW pipelines while default exists (don't release default!)
    // This is exactly what updateFragmentShader does

    // Test expressions
    const test_expressions = [_][]const u8{
        "x+y", // Simple - should work
        "x²+y²", // Without comparison
        "x²+y²>1", // THE PROBLEMATIC ONE
        "x²+y²=1", // Equality
        "sin(x)+cos(y)>0", // Complex
    };

    var passed: usize = 0;
    var failed: usize = 0;

    for (test_expressions) |expr| {
        testPipelineCreation(allocator, device, expr) catch |err| {
            std.debug.print("\n✗ Pipeline test FAILED for: {s}\n", .{expr});
            std.debug.print("  Error: {s}\n", .{@errorName(err)});
            failed += 1;
            continue;
        };
        passed += 1;
    }

    // NOTE: Not cleaning up resources - testing the leak scenario like the app

    std.debug.print("\n============================================================\n", .{});
    std.debug.print("TEST SUMMARY\n", .{});
    std.debug.print("============================================================\n", .{});
    std.debug.print("Passed: {d}\n", .{passed});
    std.debug.print("Failed: {d}\n", .{failed});
    std.debug.print("============================================================\n\n", .{});

    if (failed > 0) {
        return error.TestsFailed;
    }
}

// Also expose as Zig test for `zig build test-gpu-pipeline`
test "GPU pipeline creation for x²+y²>1" {
    // Use arena to avoid AST memory leak issues
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    // Try to initialize SDL GPU
    const device = initSdlGpu() catch |err| {
        // Skip test if GPU not available (e.g., headless CI)
        std.debug.print("Skipping GPU test - no GPU available: {s}\n", .{@errorName(err)});
        return;
    };
    defer deinitSdlGpu(device);

    try shader_compiler.init();
    defer shader_compiler.deinit();

    // Test the problematic expression
    try testPipelineCreation(allocator, device, "x²+y²>1");
}
