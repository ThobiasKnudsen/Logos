//! Shader management for custom GPU rendering
//!
//! SDL3 supports GPU shaders through SDL_GPU API.
//! This module provides utilities for loading and managing shaders.

const std = @import("std");
const sdl = @import("sdl3");

/// Shader program handle
pub const ShaderProgram = struct {
    // SDL3 GPU shader handles would go here
    // For now this is a placeholder - SDL3's GPU API
    // requires more setup
    vertex_shader: ?*anyopaque,
    fragment_shader: ?*anyopaque,

    pub fn init() ShaderProgram {
        return .{
            .vertex_shader = null,
            .fragment_shader = null,
        };
    }

    pub fn deinit(self: *ShaderProgram) void {
        // Cleanup shader resources
        _ = self;
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
};

/// Load a built-in shader by type
pub fn loadBuiltinShader(shader_type: ShaderType) !ShaderProgram {
    _ = shader_type;
    // TODO: Implement actual shader loading when using SDL3 GPU API
    // For now, return a placeholder
    return ShaderProgram.init();
}

/// Uniform data for shaders
pub const Uniforms = struct {
    time: f32 = 0,
    resolution: [2]f32 = .{ 0, 0 },
    mouse: [2]f32 = .{ 0, 0 },
    zoom: f32 = 1.0,
    pan: [2]f32 = .{ 0, 0 },

    // Colors
    primary_color: [4]f32 = .{ 1, 1, 1, 1 },
    secondary_color: [4]f32 = .{ 0.5, 0.5, 0.5, 1 },
    background_color: [4]f32 = .{ 0, 0, 0, 1 },
};
