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
    const compiler = global_compiler orelse {
        // Auto-initialize if not done
        try init();
        return compileGlslToSpirv(allocator, glsl_source, stage);
    };

    // Create compile options with Vulkan target
    const options = c.shaderc_compile_options_initialize();
    if (options == null) {
        return ShaderError.OutOfMemory;
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
        stage.toShadercKind(),
        "shader", // filename for error messages
        "main", // entry point
        options,
    );

    if (result == null) {
        return ShaderError.ShaderCompileFailed;
    }
    defer c.shaderc_result_release(result);

    // Check compilation status
    const status = c.shaderc_result_get_compilation_status(result);
    if (status != c.shaderc_compilation_status_success) {
        const error_msg = c.shaderc_result_get_error_message(result);
        if (error_msg != null) {
            std.log.err("Shader compilation error: {s}", .{error_msg});
        }
        return ShaderError.ShaderCompileFailed;
    }

    // Get SPIRV output
    const spirv_len = c.shaderc_result_get_length(result);
    const spirv_ptr = c.shaderc_result_get_bytes(result);

    if (spirv_ptr == null or spirv_len == 0) {
        return ShaderError.ShaderCompileFailed;
    }

    // Copy to Zig-managed memory
    const output = allocator.alloc(u8, spirv_len) catch return ShaderError.OutOfMemory;
    errdefer allocator.free(output);

    @memcpy(output, @as([*]const u8, @ptrCast(spirv_ptr))[0..spirv_len]);

    return SpirvBytecode{
        .data = output,
        .allocator = allocator,
    };
}

/// Reflect SPIRV bytecode to get resource info
pub fn reflectSpirv(spirv: []const u8) ShaderError!ShaderResourceInfo {
    const metadata = c.SDL_ShaderCross_ReflectGraphicsSPIRV(
        spirv.ptr,
        spirv.len,
        0,
    ) orelse return ShaderError.ReflectionFailed;
    defer c.SDL_free(metadata);

    return ShaderResourceInfo{
        .num_samplers = metadata.resource_info.num_samplers,
        .num_storage_textures = metadata.resource_info.num_storage_textures,
        .num_storage_buffers = metadata.resource_info.num_storage_buffers,
        .num_uniform_buffers = metadata.resource_info.num_uniform_buffers,
    };
}

/// Create an SDL GPU shader from SPIRV bytecode
/// Uses SDL_CreateGPUShader directly with SPIRV format (same method as dvui backend)
/// Note: device and return are *anyopaque to avoid cimport type conflicts with dvui backend
pub fn createGpuShader(
    device: *anyopaque,
    spirv: []const u8,
    stage: ShaderStage,
) ShaderError!*anyopaque {
    const typed_device: *c.SDL_GPUDevice = @ptrCast(@alignCast(device));

    // Reflect to get resource info
    const metadata = c.SDL_ShaderCross_ReflectGraphicsSPIRV(
        spirv.ptr,
        spirv.len,
        0,
    ) orelse return ShaderError.ReflectionFailed;
    defer c.SDL_free(metadata);

    // Use SDL_CreateGPUShader directly with SPIRV format (same as dvui backend)
    // This is more compatible than SDL_ShaderCross_CompileGraphicsShaderFromSPIRV
    var shader_info = std.mem.zeroes(c.SDL_GPUShaderCreateInfo);
    shader_info.code = spirv.ptr;
    shader_info.code_size = spirv.len;
    shader_info.entrypoint = "main";
    shader_info.format = c.SDL_GPU_SHADERFORMAT_SPIRV;
    shader_info.stage = switch (stage) {
        .vertex => c.SDL_GPU_SHADERSTAGE_VERTEX,
        .fragment => c.SDL_GPU_SHADERSTAGE_FRAGMENT,
        .compute => @as(c.SDL_GPUShaderStage, 0),
    };
    shader_info.num_samplers = metadata.*.resource_info.num_samplers;
    shader_info.num_storage_textures = metadata.*.resource_info.num_storage_textures;
    shader_info.num_storage_buffers = metadata.*.resource_info.num_storage_buffers;
    shader_info.num_uniform_buffers = metadata.*.resource_info.num_uniform_buffers;

    std.log.info("Calling SDL_CreateGPUShader (SPIRV format, {d} uniforms)...", .{metadata.*.resource_info.num_uniform_buffers});

    const gpu_shader = c.SDL_CreateGPUShader(typed_device, &shader_info) orelse {
        const err_msg = c.SDL_GetError();
        if (err_msg != null and err_msg[0] != 0) {
            std.log.err("SDL_CreateGPUShader failed: {s}", .{err_msg});
        }
        return ShaderError.ShaderCreationFailed;
    };

    std.log.info("SDL_CreateGPUShader succeeded", .{});
    return @ptrCast(gpu_shader);
}

/// Compile GLSL and create GPU shader in one step
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
    \\layout(set = 0, binding = 0) uniform Uniforms {
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
