//! Shader management for custom GPU rendering
//!
//! SDL3 supports GPU shaders through SDL_GPU API.
//! This module provides utilities for loading and managing shaders.

const std = @import("std");
const shader_compiler = @import("../shader_compiler.zig");

// Local SDL bindings for type casting
const c = shader_compiler.sdl;

/// Shader program handle
/// Uses *anyopaque for SDL types to avoid cimport type conflicts with dvui backend
pub const ShaderProgram = struct {
    allocator: std.mem.Allocator,
    vertex_shader: ?*anyopaque = null,
    fragment_shader: ?*anyopaque = null,
    device: ?*anyopaque = null,

    pub fn init(allocator: std.mem.Allocator) ShaderProgram {
        return .{
            .allocator = allocator,
        };
    }

    pub fn deinit(self: *ShaderProgram) void {
        if (self.device) |device_opaque| {
            const device: *c.SDL_GPUDevice = @ptrCast(device_opaque);
            if (self.vertex_shader) |vs_opaque| {
                c.SDL_ReleaseGPUShader(device, @ptrCast(vs_opaque));
            }
            if (self.fragment_shader) |fs_opaque| {
                c.SDL_ReleaseGPUShader(device, @ptrCast(fs_opaque));
            }
        }
        self.vertex_shader = null;
        self.fragment_shader = null;
        self.device = null;
    }

    /// Compile and load shaders from GLSL source
    /// Note: device is *anyopaque to avoid cimport type conflicts
    pub fn loadFromGlsl(
        self: *ShaderProgram,
        device: *anyopaque,
        vertex_glsl: []const u8,
        fragment_glsl: []const u8,
    ) !void {
        // Clean up old shaders if any
        self.deinit();
        self.device = device;

        // Compile vertex shader
        self.vertex_shader = shader_compiler.compileAndCreateShader(
            self.allocator,
            device,
            vertex_glsl,
            .vertex,
        ) catch |err| {
            std.log.err("Failed to compile vertex shader: {}", .{err});
            return error.ShaderCompileFailed;
        };
        errdefer {
            if (self.vertex_shader) |vs_opaque| {
                const typed_device: *c.SDL_GPUDevice = @ptrCast(device);
                c.SDL_ReleaseGPUShader(typed_device, @ptrCast(vs_opaque));
            }
        }

        // Compile fragment shader
        self.fragment_shader = shader_compiler.compileAndCreateShader(
            self.allocator,
            device,
            fragment_glsl,
            .fragment,
        ) catch |err| {
            std.log.err("Failed to compile fragment shader: {}", .{err});
            return error.ShaderCompileFailed;
        };
    }

    /// Check if shaders are loaded and valid
    pub fn isValid(self: *const ShaderProgram) bool {
        return self.vertex_shader != null and self.fragment_shader != null;
    }
};

/// Built-in shader types
pub const ShaderType = enum {
    /// Default - basic colored rendering
    default,

    /// Gradient background
    gradient,

    /// Glow effect for lines/points
    glow,

    /// Grid pattern
    grid,

    /// Anti-aliased line rendering
    smooth_line,

    /// Custom user shader (from Logos expressions)
    custom,
};

/// Uniform data for shaders
pub const Uniforms = extern struct {
    time: f32 = 0,
    _pad0: f32 = 0,
    resolution: [2]f32 = .{ 0, 0 },
    mouse: [2]f32 = .{ 0, 0 },
    zoom: f32 = 1.0,
    _pad1: f32 = 0,
    pan: [2]f32 = .{ 0, 0 },

    // Axis bounds (default -5 to 5 for both axes)
    axis_min: [2]f32 = .{ -5, -5 },
    axis_max: [2]f32 = .{ 5, 5 },

    // Colors (vec4 aligned)
    primary_color: [4]f32 = .{ 1, 1, 1, 1 },
    secondary_color: [4]f32 = .{ 0.5, 0.5, 0.5, 1 },
    background_color: [4]f32 = .{ 0, 0, 0, 1 },

    /// Set axis bounds for both axes
    pub fn setAxisBounds(self: *Uniforms, x_min: f32, x_max: f32, y_min: f32, y_max: f32) void {
        self.axis_min = .{ x_min, y_min };
        self.axis_max = .{ x_max, y_max };
    }

    /// Set symmetric bounds (e.g., -5 to 5)
    pub fn setSymmetricBounds(self: *Uniforms, half_range: f32) void {
        self.axis_min = .{ -half_range, -half_range };
        self.axis_max = .{ half_range, half_range };
    }
};

// ============================================================================
// Built-in shader sources
// ============================================================================

/// Default vertex shader - fullscreen quad
pub const default_vertex_glsl =
    \\#version 450
    \\
    \\layout(location = 0) out vec2 out_texcoord;
    \\
    \\// Fullscreen triangle vertices
    \\vec2 positions[3] = vec2[](
    \\    vec2(-1.0, -1.0),
    \\    vec2( 3.0, -1.0),
    \\    vec2(-1.0,  3.0)
    \\);
    \\
    \\vec2 texcoords[3] = vec2[](
    \\    vec2(0.0, 1.0),
    \\    vec2(2.0, 1.0),
    \\    vec2(0.0, -1.0)
    \\);
    \\
    \\void main() {
    \\    gl_Position = vec4(positions[gl_VertexIndex], 0.0, 1.0);
    \\    out_texcoord = texcoords[gl_VertexIndex];
    \\}
;

/// Default fragment shader - gradient background
pub const default_fragment_glsl =
    \\#version 450
    \\
    \\layout(location = 0) in vec2 in_texcoord;
    \\layout(location = 0) out vec4 out_color;
    \\
    \\layout(set = 0, binding = 0) uniform Uniforms {
    \\    float time;
    \\    float _pad0;
    \\    vec2 resolution;
    \\    vec2 mouse;
    \\    float zoom;
    \\    float _pad1;
    \\    vec2 pan;
    \\    vec2 axis_min;
    \\    vec2 axis_max;
    \\    vec4 primary_color;
    \\    vec4 secondary_color;
    \\    vec4 background_color;
    \\};
    \\
    \\void main() {
    \\    vec2 uv = in_texcoord;
    \\    
    \\    // Dark gradient background
    \\    vec3 bg = mix(
    \\        background_color.rgb,
    \\        background_color.rgb * 1.2,
    \\        uv.y
    \\    );
    \\    
    \\    // Subtle grid
    \\    vec2 world = mix(axis_min, axis_max, uv);
    \\    vec2 grid = abs(fract(world) - 0.5);
    \\    float line = min(grid.x, grid.y);
    \\    float grid_alpha = smoothstep(0.02, 0.0, line) * 0.15;
    \\    
    \\    // Axis lines
    \\    float axis_x = smoothstep(0.02, 0.0, abs(world.y));
    \\    float axis_y = smoothstep(0.02, 0.0, abs(world.x));
    \\    float axis_alpha = max(axis_x, axis_y) * 0.4;
    \\    
    \\    vec3 final_color = mix(bg, secondary_color.rgb, grid_alpha);
    \\    final_color = mix(final_color, primary_color.rgb, axis_alpha);
    \\    
    \\    out_color = vec4(final_color, 1.0);
    \\}
;

/// Template for custom Logos expressions
/// The placeholder LOGOS_EXPRESSION will be replaced with generated code
pub const custom_fragment_template =
    \\#version 450
    \\
    \\layout(location = 0) in vec2 in_texcoord;
    \\layout(location = 0) out vec4 out_color;
    \\
    \\layout(set = 0, binding = 0) uniform Uniforms {
    \\    float time;
    \\    float _pad0;
    \\    vec2 resolution;
    \\    vec2 mouse;
    \\    float zoom;
    \\    float _pad1;
    \\    vec2 pan;
    \\    vec2 axis_min;
    \\    vec2 axis_max;
    \\    vec4 primary_color;
    \\    vec4 secondary_color;
    \\    vec4 background_color;
    \\};
    \\
    \\void main() {
    \\    vec2 uv = in_texcoord;
    \\    vec2 world = mix(axis_min, axis_max, uv);
    \\    float x = world.x;
    \\    float y = world.y;
    \\    
    \\    // LOGOS_EXPRESSION_START
    \\    vec4 result = vec4(0.0, 0.0, 0.0, 1.0);
    \\    // LOGOS_EXPRESSION_END
    \\    
    \\    out_color = result;
    \\}
;

/// Load a built-in shader by type
/// Note: device is *anyopaque to avoid cimport type conflicts
pub fn loadBuiltinShader(
    allocator: std.mem.Allocator,
    device: *anyopaque,
    shader_type: ShaderType,
) !ShaderProgram {
    var program = ShaderProgram.init(allocator);

    const fragment_glsl = switch (shader_type) {
        .default, .gradient => default_fragment_glsl,
        .grid => default_fragment_glsl, // TODO: specialized grid shader
        .glow => default_fragment_glsl, // TODO: glow shader
        .smooth_line => default_fragment_glsl, // TODO: line shader
        .custom => return error.UseLoadFromGlsl,
    };

    try program.loadFromGlsl(device, default_vertex_glsl, fragment_glsl);
    return program;
}
