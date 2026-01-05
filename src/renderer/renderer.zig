//! Renderer module - graph/plot visualization with custom shaders
//!
//! Uses SDL3 textures as render targets for custom GPU rendering.
//! The texture can then be displayed in the dvui panel.

pub const GraphRenderer = @import("graph_renderer.zig").GraphRenderer;
pub const TextureTarget = @import("texture_target.zig").TextureTarget;
pub const shaders = @import("shaders/shaders.zig");
