//! Shader Compiler Module
//!
//! Provides GLSL to SPIRV compilation via shaderc and
//! cross-platform shader creation via SDL_ShaderCross.
//!
//! Pipeline: GLSL → shaderc → SPIRV → SDL_ShaderCross → Native format

const std = @import("std");

// C bindings for shaderc, SDL, and SDL_ShaderCross (all in one cimport for type compatibility)
const c = @cImport({
    @cInclude("shaderc/shaderc.h");
    @cInclude("SDL3/SDL.h");
    @cInclude("SDL3/SDL_gpu.h");
    @cInclude("SDL3_shadercross/SDL_shadercross.h");
});

// Re-export SDL for external use (GPU operations)
pub const sdl = c;

/// Shader stage enum
pub const ShaderStage = enum {
    vertex,
    fragment,
    compute,

    fn toShadercKind(self: ShaderStage) c.shaderc_shader_kind {
        return switch (self) {
            .vertex => c.shaderc_vertex_shader,
            .fragment => c.shaderc_fragment_shader,
            .compute => c.shaderc_compute_shader,
        };
    }

    fn toShaderCrossStage(self: ShaderStage) c.SDL_ShaderCross_ShaderStage {
        return switch (self) {
            .vertex => c.SDL_SHADERCROSS_SHADERSTAGE_VERTEX,
            .fragment => c.SDL_SHADERCROSS_SHADERSTAGE_FRAGMENT,
            .compute => c.SDL_SHADERCROSS_SHADERSTAGE_COMPUTE,
        };
    }
};

/// Shader compilation error
pub const ShaderError = error{
    CompilerInitFailed,
    ShaderCompileFailed,
    ShaderCrossInitFailed,
    ReflectionFailed,
    ShaderCreationFailed,
    OutOfMemory,
};

/// Compiled SPIRV bytecode
pub const SpirvBytecode = struct {
    data: []const u8,
    allocator: std.mem.Allocator,

    pub fn deinit(self: *SpirvBytecode) void {
        self.allocator.free(self.data);
    }
};

/// Shader resource info from reflection
pub const ShaderResourceInfo = struct {
    num_samplers: u32,
    num_storage_textures: u32,
    num_storage_buffers: u32,
    num_uniform_buffers: u32,
};

/// Global shaderc compiler instance (thread-safe, reusable)
var global_compiler: ?c.shaderc_compiler_t = null;
var shadercross_initialized: bool = false;

/// Initialize the shader compiler subsystem
/// Must be called before any shader compilation
pub fn init() ShaderError!void {
    if (global_compiler == null) {
        global_compiler = c.shaderc_compiler_initialize();
        if (global_compiler == null) {
            return ShaderError.CompilerInitFailed;
        }
    }

    if (!shadercross_initialized) {
        if (!c.SDL_ShaderCross_Init()) {
            return ShaderError.ShaderCrossInitFailed;
        }
        shadercross_initialized = true;
    }
}

/// Cleanup shader compiler resources
pub fn deinit() void {
    if (shadercross_initialized) {
        c.SDL_ShaderCross_Quit();
        shadercross_initialized = false;
    }

    if (global_compiler) |compiler| {
        c.shaderc_compiler_release(compiler);
        global_compiler = null;
    }
}

/// Compile GLSL source code to SPIRV bytecode
pub fn compileGlslToSpirv(
    allocator: std.mem.Allocator,
    glsl_source: []const u8,
    stage: ShaderStage,
) ShaderError!SpirvBytecode {
    std.log.info("[ShaderCompiler] Compiling GLSL to SPIRV ({s} stage, {} bytes)...", .{ @tagName(stage), glsl_source.len });

    const compiler = global_compiler orelse {
        // Auto-initialize if not done
        std.log.info("[ShaderCompiler] Auto-initializing compiler...", .{});
        try init();
        return compileGlslToSpirv(allocator, glsl_source, stage);
    };

    // Create compile options with Vulkan target
    const options = c.shaderc_compile_options_initialize();
    if (options == null) {
        std.log.err("[ShaderCompiler] Failed to initialize compile options", .{});
        return ShaderError.OutOfMemory;
    }
    defer c.shaderc_compile_options_release(options);

    // No optimization for easier debugging
    c.shaderc_compile_options_set_optimization_level(options, c.shaderc_optimization_level_zero);
    // Target Vulkan 1.0 / SPIRV 1.0 for maximum compatibility
    c.shaderc_compile_options_set_target_env(options, c.shaderc_target_env_vulkan, c.shaderc_env_version_vulkan_1_0);
    c.shaderc_compile_options_set_target_spirv(options, c.shaderc_spirv_version_1_0);

    std.log.debug("[ShaderCompiler] Calling shaderc_compile_into_spv...", .{});

    // Compile GLSL to SPIRV bytecode
    // Note: SDL3 GPU API accepts SPIRV input and handles platform-specific translation internally
    const result = c.shaderc_compile_into_spv(
        compiler,
        glsl_source.ptr,
        glsl_source.len,
        stage.toShadercKind(),
        "shader", // filename for error messages
        "main", // entry point
        options,
    );

    if (result == null) {
        std.log.err("[ShaderCompiler] shaderc_compile_into_spv returned null", .{});
        return ShaderError.ShaderCompileFailed;
    }
    defer c.shaderc_result_release(result);

    // Check compilation status
    const status = c.shaderc_result_get_compilation_status(result);
    if (status != c.shaderc_compilation_status_success) {
        const error_msg = c.shaderc_result_get_error_message(result);
        if (error_msg != null) {
            std.log.err("[ShaderCompiler] Compilation error: {s}", .{error_msg});
        }
        std.log.err("[ShaderCompiler] Compilation status: {}", .{status});
        return ShaderError.ShaderCompileFailed;
    }

    // Get SPIRV output
    const spirv_len = c.shaderc_result_get_length(result);
    const spirv_ptr = c.shaderc_result_get_bytes(result);

    std.log.debug("[ShaderCompiler] SPIRV output size: {} bytes", .{spirv_len});

    if (spirv_ptr == null or spirv_len == 0) {
        std.log.err("[ShaderCompiler] SPIRV output is null or empty", .{});
        return ShaderError.ShaderCompileFailed;
    }

    // Copy to Zig-managed memory
    const output = allocator.alloc(u8, spirv_len) catch {
        std.log.err("[ShaderCompiler] Failed to allocate memory for SPIRV", .{});
        return ShaderError.OutOfMemory;
    };
    errdefer allocator.free(output);

    @memcpy(output, @as([*]const u8, @ptrCast(spirv_ptr))[0..spirv_len]);

    // Verify SPIRV magic number
    if (spirv_len >= 4) {
        const is_valid = output[0] == 0x03 and output[1] == 0x02 and output[2] == 0x23 and output[3] == 0x07;
        std.log.info("[ShaderCompiler] SPIRV compilation success ✓ (magic number valid: {})", .{is_valid});
    } else {
        std.log.warn("[ShaderCompiler] SPIRV too small to verify magic number", .{});
    }

    return SpirvBytecode{
        .data = output,
        .allocator = allocator,
    };
}

/// Note: Reflection is now handled internally by SDL_ShaderCross_CompileGraphicsShaderFromSPIRV
/// This function is kept for backward compatibility but returns empty info
pub fn reflectSpirv(spirv: []const u8) ShaderError!ShaderResourceInfo {
    _ = spirv;
    // SDL_shadercross no longer exposes reflection API - it's handled internally
    return ShaderResourceInfo{
        .num_samplers = 0,
        .num_storage_textures = 0,
        .num_storage_buffers = 0,
        .num_uniform_buffers = 0,
    };
}

/// Shader resource info for pipeline creation
pub const ShaderResources = struct {
    num_samplers: u32 = 0,
    num_storage_textures: u32 = 0,
    num_storage_buffers: u32 = 0,
    num_uniform_buffers: u32 = 0,
};

/// Create an SDL GPU shader from SPIRV bytecode
/// Uses SDL_CreateGPUShader directly with specified resource counts
/// Note: device and return are *anyopaque to avoid cimport type conflicts with dvui backend
pub fn createGpuShader(
    device: *anyopaque,
    spirv: []const u8,
    stage: ShaderStage,
) ShaderError!*anyopaque {
    // Default resources - minimal shader with no resources
    const resources = ShaderResources{};
    return createGpuShaderWithResources(device, spirv, stage, resources);
}

/// Create an SDL GPU shader from SPIRV bytecode with explicit resource counts
pub fn createGpuShaderWithResources(
    device: *anyopaque,
    spirv: []const u8,
    stage: ShaderStage,
    resources: ShaderResources,
) ShaderError!*anyopaque {
    const typed_device: *c.SDL_GPUDevice = @ptrCast(@alignCast(device));

    const sdl_stage: c.SDL_GPUShaderStage = switch (stage) {
        .vertex => c.SDL_GPU_SHADERSTAGE_VERTEX,
        .fragment => c.SDL_GPU_SHADERSTAGE_FRAGMENT,
        .compute => @as(c.SDL_GPUShaderStage, 0),
    };

    std.log.info("[ShaderCompiler] Creating GPU shader (stage={}, uniform_buffers={}, spirv_size={})...", .{
        @intFromEnum(stage),
        resources.num_uniform_buffers,
        spirv.len,
    });

    // Create shader directly using SDL_CreateGPUShader
    const shader_info = c.SDL_GPUShaderCreateInfo{
        .code_size = spirv.len,
        .code = spirv.ptr,
        .entrypoint = "main",
        .format = c.SDL_GPU_SHADERFORMAT_SPIRV,
        .stage = sdl_stage,
        .num_samplers = resources.num_samplers,
        .num_storage_textures = resources.num_storage_textures,
        .num_storage_buffers = resources.num_storage_buffers,
        .num_uniform_buffers = resources.num_uniform_buffers,
        .props = 0,
    };

    std.log.debug("[ShaderCompiler] Calling SDL_CreateGPUShader...", .{});

    const gpu_shader = c.SDL_CreateGPUShader(typed_device, &shader_info) orelse {
        const err_msg = c.SDL_GetError();
        if (err_msg != null and err_msg[0] != 0) {
            std.log.err("[ShaderCompiler] SDL_CreateGPUShader FAILED: {s}", .{err_msg});
        } else {
            std.log.err("[ShaderCompiler] SDL_CreateGPUShader FAILED (no error message)", .{});
        }
        return ShaderError.ShaderCreationFailed;
    };

    std.log.info("[ShaderCompiler] SDL_CreateGPUShader succeeded ✓", .{});
    return @ptrCast(gpu_shader);
}

/// Compile GLSL and create GPU shader in one step (minimal resources)
/// Note: device is *anyopaque to avoid cimport type conflicts with dvui backend
pub fn compileAndCreateShader(
    allocator: std.mem.Allocator,
    device: *anyopaque,
    glsl_source: []const u8,
    stage: ShaderStage,
) ShaderError!*anyopaque {
    var spirv = try compileGlslToSpirv(allocator, glsl_source, stage);
    defer spirv.deinit();

    return createGpuShader(device, spirv.data, stage);
}

/// Compile GLSL and create GPU shader with explicit resource counts
pub fn compileAndCreateShaderWithResources(
    allocator: std.mem.Allocator,
    device: *anyopaque,
    glsl_source: []const u8,
    stage: ShaderStage,
    resources: ShaderResources,
) ShaderError!*anyopaque {
    var spirv = try compileGlslToSpirv(allocator, glsl_source, stage);
    defer spirv.deinit();

    return createGpuShaderWithResources(device, spirv.data, stage, resources);
}

/// Get supported shader formats for the current platform
pub fn getSupportedFormats() sdl.SDL_GPUShaderFormat {
    return @bitCast(c.SDL_ShaderCross_GetSPIRVShaderFormats());
}

// ============================================================================
// Test shaders for validation
// ============================================================================

/// Simple passthrough vertex shader for testing
pub const test_vertex_shader =
    \\#version 450
    \\
    \\layout(location = 0) in vec2 in_position;
    \\layout(location = 1) in vec2 in_texcoord;
    \\
    \\layout(location = 0) out vec2 out_texcoord;
    \\
    \\void main() {
    \\    gl_Position = vec4(in_position, 0.0, 1.0);
    \\    out_texcoord = in_texcoord;
    \\}
;

/// Simple color output fragment shader for testing
pub const test_fragment_shader =
    \\#version 450
    \\
    \\layout(location = 0) in vec2 in_texcoord;
    \\layout(location = 0) out vec4 out_color;
    \\
    \\layout(push_constant) uniform PushConstants {
    \\    float time;
    \\    vec2 resolution;
    \\};
    \\
    \\void main() {
    \\    vec2 uv = in_texcoord;
    \\    out_color = vec4(uv, 0.5 + 0.5 * sin(time), 1.0);
    \\}
;

test "compile vertex shader to SPIRV" {
    try init();
    defer deinit();

    var spirv = try compileGlslToSpirv(
        std.testing.allocator,
        test_vertex_shader,
        .vertex,
    );
    defer spirv.deinit();

    // SPIRV magic number: 0x07230203
    try std.testing.expect(spirv.data.len >= 4);
    try std.testing.expectEqual(@as(u8, 0x03), spirv.data[0]);
    try std.testing.expectEqual(@as(u8, 0x02), spirv.data[1]);
    try std.testing.expectEqual(@as(u8, 0x23), spirv.data[2]);
    try std.testing.expectEqual(@as(u8, 0x07), spirv.data[3]);
}

test "compile fragment shader to SPIRV" {
    try init();
    defer deinit();

    var spirv = try compileGlslToSpirv(
        std.testing.allocator,
        test_fragment_shader,
        .fragment,
    );
    defer spirv.deinit();

    // SPIRV magic number: 0x07230203
    try std.testing.expect(spirv.data.len >= 4);
    try std.testing.expectEqual(@as(u8, 0x03), spirv.data[0]);
    try std.testing.expectEqual(@as(u8, 0x02), spirv.data[1]);
    try std.testing.expectEqual(@as(u8, 0x23), spirv.data[2]);
    try std.testing.expectEqual(@as(u8, 0x07), spirv.data[3]);
}
