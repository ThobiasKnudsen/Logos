//! Renderer module - graph/plot visualization with custom shaders
//!
//! Uses SDL3 GPU for custom shader rendering to textures.
//! Supports runtime shader compilation via GLSL → SPIRV → Native format pipeline.
//!
//! Key components:
//! - GraphRenderer: High-level rendering interface
//! - GpuPipeline: Graphics pipeline and render-to-texture management
//! - shader_compiler: GLSL to SPIRV compilation
//! - shaders: Built-in shader definitions

pub const GraphRenderer = @import("graph_renderer.zig").GraphRenderer;
pub const GpuPipeline = @import("gpu_pipeline.zig").GpuPipeline;
pub const TextureTarget = @import("texture_target.zig").TextureTarget;
pub const shaders = @import("shaders/shaders.zig");
pub const shader_compiler = @import("shader_compiler.zig");