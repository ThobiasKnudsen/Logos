//! GPU Pipeline Module
//!
//! Manages the graphics pipeline for rendering custom shaders to a texture.
//! Handles pipeline creation, buffer management, and render-to-texture workflow.
//!
//! Pipeline: Vertex Shader + Fragment Shader → Graphics Pipeline → Render to Texture

const std = @import("std");
const shader_compiler = @import("shader_compiler.zig");
const shaders = @import("shaders/shaders.zig");

const c = shader_compiler.sdl;

/// Vertex for fullscreen triangle (position + texcoord)
pub const Vertex = extern struct {
    position: [2]f32,
    texcoord: [2]f32,
};

/// Fullscreen triangle vertices (covers entire screen with one triangle)
/// Using a single triangle is more efficient than a quad (no diagonal edge)
pub const fullscreen_vertices = [_]Vertex{
    .{ .position = .{ -1.0, -1.0 }, .texcoord = .{ 0.0, 1.0 } },
    .{ .position = .{ 3.0, -1.0 }, .texcoord = .{ 2.0, 1.0 } },
    .{ .position = .{ -1.0, 3.0 }, .texcoord = .{ 0.0, -1.0 } },
};

/// GPU Pipeline state for custom shader rendering
/// Uses *anyopaque for SDL types to avoid cimport conflicts with dvui backend
pub const GpuPipeline = struct {
    allocator: std.mem.Allocator,

    // GPU resources (stored as anyopaque to avoid cimport type conflicts)
    device: ?*anyopaque = null,
    pipeline: ?*anyopaque = null,
    vertex_shader: ?*anyopaque = null,
    fragment_shader: ?*anyopaque = null,

    // Render target texture
    render_texture: ?*anyopaque = null,
    render_width: u32 = 0,
    render_height: u32 = 0,

    // Sampler for the render texture
    sampler: ?*anyopaque = null,

    // Async rendering state
    pending_fence: ?*anyopaque = null,
    is_rendering: bool = false,

    // Uniform data
    uniforms: shaders.Uniforms = .{},

    // Error tracking
    last_error: ?[]const u8 = null,

    pub fn init(allocator: std.mem.Allocator) GpuPipeline {
        return .{
            .allocator = allocator,
        };
    }

    pub fn deinit(self: *GpuPipeline) void {
        self.releaseResources();
        if (self.last_error) |err| {
            self.allocator.free(err);
        }
    }

    /// Release all GPU resources
    /// Note: This should be called BEFORE the SDL backend is destroyed
    fn releaseResources(self: *GpuPipeline) void {
        // Just clear references - the SDL backend owns the device and will clean up
        // Attempting to use the device here causes crashes if backend is already destroyed
        self.pending_fence = null;
        self.pipeline = null;
        self.vertex_shader = null;
        self.fragment_shader = null;
        self.render_texture = null;
        self.sampler = null;
        self.device = null;
        self.is_rendering = false;
    }

    /// Explicitly release GPU resources while device is still valid
    /// Call this before backend.deinit()
    pub fn releaseGpuResources(self: *GpuPipeline) void {
        const device: *c.SDL_GPUDevice = @ptrCast(self.device orelse return);

        // Wait for any pending GPU work
        if (self.pending_fence) |fence_opaque| {
            const fence: *c.SDL_GPUFence = @ptrCast(fence_opaque);
            _ = c.SDL_WaitForGPUFences(device, true, @ptrCast(&fence), 1);
            c.SDL_ReleaseGPUFence(device, fence);
            self.pending_fence = null;
        }

        if (self.pipeline) |pipeline_opaque| {
            c.SDL_ReleaseGPUGraphicsPipeline(device, @ptrCast(pipeline_opaque));
            self.pipeline = null;
        }

        if (self.vertex_shader) |vs_opaque| {
            c.SDL_ReleaseGPUShader(device, @ptrCast(vs_opaque));
            self.vertex_shader = null;
        }

        if (self.fragment_shader) |fs_opaque| {
            c.SDL_ReleaseGPUShader(device, @ptrCast(fs_opaque));
            self.fragment_shader = null;
        }

        if (self.render_texture) |tex_opaque| {
            c.SDL_ReleaseGPUTexture(device, @ptrCast(tex_opaque));
            self.render_texture = null;
        }

        if (self.sampler) |samp_opaque| {
            c.SDL_ReleaseGPUSampler(device, @ptrCast(samp_opaque));
            self.sampler = null;
        }

        self.device = null;
    }

    /// Initialize the pipeline with a GPU device
    /// Note: device is *anyopaque to avoid cimport type conflicts with dvui backend
    pub fn initWithDevice(self: *GpuPipeline, device: *anyopaque) !void {
        self.device = device;

        const typed_device: *c.SDL_GPUDevice = @ptrCast(device);
        // Create sampler for render texture
        const sampler_info = c.SDL_GPUSamplerCreateInfo{
            .min_filter = c.SDL_GPU_FILTER_LINEAR,
            .mag_filter = c.SDL_GPU_FILTER_LINEAR,
            .mipmap_mode = c.SDL_GPU_SAMPLERMIPMAPMODE_LINEAR,
            .address_mode_u = c.SDL_GPU_SAMPLERADDRESSMODE_CLAMP_TO_EDGE,
            .address_mode_v = c.SDL_GPU_SAMPLERADDRESSMODE_CLAMP_TO_EDGE,
            .address_mode_w = c.SDL_GPU_SAMPLERADDRESSMODE_CLAMP_TO_EDGE,
            .mip_lod_bias = 0,
            .max_anisotropy = 1,
            .compare_op = c.SDL_GPU_COMPAREOP_NEVER,
            .min_lod = 0,
            .max_lod = 1000,
            .enable_anisotropy = false,
            .enable_compare = false,
            .props = 0,
        };

        self.sampler = c.SDL_CreateGPUSampler(typed_device, &sampler_info) orelse {
            return self.setError("Failed to create sampler");
        };

        // Compile default shaders and create pipeline
        try self.createDefaultPipeline();
    }

    /// Create the default rendering pipeline with built-in shaders
    fn createDefaultPipeline(self: *GpuPipeline) !void {
        const device = self.device orelse return error.NoDevice;

        // Compile vertex shader
        self.vertex_shader = shader_compiler.compileAndCreateShader(
            self.allocator,
            device,
            shaders.default_vertex_glsl,
            .vertex,
        ) catch |err| {
            std.log.err("Failed to compile vertex shader: {}", .{err});
            return self.setError("Vertex shader compilation failed");
        };

        // Compile fragment shader
        self.fragment_shader = shader_compiler.compileAndCreateShader(
            self.allocator,
            device,
            shaders.default_fragment_glsl,
            .fragment,
        ) catch |err| {
            std.log.err("Failed to compile fragment shader: {}", .{err});
            return self.setError("Fragment shader compilation failed");
        };

        try self.createPipeline();
    }

    /// Create graphics pipeline from current shaders
    fn createPipeline(self: *GpuPipeline) !void {
        const device: *c.SDL_GPUDevice = @ptrCast(self.device orelse return error.NoDevice);
        const vs: *c.SDL_GPUShader = @ptrCast(self.vertex_shader orelse return error.NoVertexShader);
        const fs: *c.SDL_GPUShader = @ptrCast(self.fragment_shader orelse return error.NoFragmentShader);

        // Wait for any pending GPU work before releasing old pipeline
        if (self.pending_fence) |fence_opaque| {
            const fence: *c.SDL_GPUFence = @ptrCast(fence_opaque);
            _ = c.SDL_WaitForGPUFences(device, true, @ptrCast(&fence), 1);
            c.SDL_ReleaseGPUFence(device, fence);
            self.pending_fence = null;
            self.is_rendering = false;
        }

        // WORKAROUND: Don't release old pipeline here - causes AMD RADV Vulkan crash
        // The caller should set self.pipeline = null before calling createPipeline

        // Color target description
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

        // Pipeline create info
        const pipeline_info = c.SDL_GPUGraphicsPipelineCreateInfo{
            .vertex_shader = vs,
            .fragment_shader = fs,
            .vertex_input_state = .{
                // No vertex input needed - we generate vertices in the shader
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

        self.pipeline = c.SDL_CreateGPUGraphicsPipeline(device, &pipeline_info) orelse {
            return self.setError("Failed to create graphics pipeline");
        };

        std.log.info("Graphics pipeline created successfully", .{});
    }

    /// Ensure render texture exists and has correct size
    pub fn ensureRenderTexture(self: *GpuPipeline, width: u32, height: u32) !void {
        if (width == 0 or height == 0) return;

        const device: *c.SDL_GPUDevice = @ptrCast(self.device orelse return error.NoDevice);

        // Check if we need to recreate
        if (self.render_texture != null and self.render_width == width and self.render_height == height) {
            return;
        }

        // Release old texture
        if (self.render_texture) |tex_opaque| {
            c.SDL_ReleaseGPUTexture(device, @ptrCast(tex_opaque));
        }

        // Create new render target texture
        const texture_info = c.SDL_GPUTextureCreateInfo{
            .type = c.SDL_GPU_TEXTURETYPE_2D,
            .format = c.SDL_GPU_TEXTUREFORMAT_R8G8B8A8_UNORM,
            .usage = c.SDL_GPU_TEXTUREUSAGE_COLOR_TARGET | c.SDL_GPU_TEXTUREUSAGE_SAMPLER,
            .width = width,
            .height = height,
            .layer_count_or_depth = 1,
            .num_levels = 1,
            .sample_count = c.SDL_GPU_SAMPLECOUNT_1,
            .props = 0,
        };

        self.render_texture = c.SDL_CreateGPUTexture(device, &texture_info) orelse {
            return self.setError("Failed to create render texture");
        };

        self.render_width = width;
        self.render_height = height;

        std.log.info("Render texture created: {}x{}", .{ width, height });
    }

    /// Update the fragment shader with new GLSL code
    pub fn updateFragmentShader(self: *GpuPipeline, glsl_source: []const u8) !void {
        const device_opaque = self.device orelse return error.NoDevice;
        const device: *c.SDL_GPUDevice = @ptrCast(device_opaque);

        // Compile new fragment shader first (before touching any GPU state)
        const new_shader = shader_compiler.compileAndCreateShader(
            self.allocator,
            device_opaque,
            glsl_source,
            .fragment,
        ) catch |err| {
            std.log.err("Failed to compile new fragment shader: {}", .{err});
            return self.setError("Fragment shader compilation failed");
        };

        // Wait for any pending GPU work to complete before releasing resources
        if (self.pending_fence) |fence_opaque| {
            const fence: *c.SDL_GPUFence = @ptrCast(fence_opaque);
            _ = c.SDL_WaitForGPUFences(device, true, @ptrCast(&fence), 1);
            c.SDL_ReleaseGPUFence(device, fence);
            self.pending_fence = null;
            self.is_rendering = false;
        }

        // WORKAROUND: Don't release old pipeline or shader - causes Vulkan driver crash
        // on AMD RADV driver. This leaks memory but is acceptable for shader hot-reload.
        // Resources will be cleaned up when the device is destroyed at app shutdown.
        self.pipeline = null;

        // Set new shader
        self.fragment_shader = new_shader;

        // Create new pipeline with new shader
        try self.createPipeline();

        std.log.info("Fragment shader updated successfully", .{});
    }

    /// Render a frame to the render texture (non-blocking)
    /// Returns true if render was submitted, false if already rendering
    pub fn renderFrame(self: *GpuPipeline) !bool {
        if (self.is_rendering) return false;

        const device: *c.SDL_GPUDevice = @ptrCast(self.device orelse return error.NoDevice);
        const pipeline: *c.SDL_GPUGraphicsPipeline = @ptrCast(self.pipeline orelse return error.NoPipeline);
        const render_texture: *c.SDL_GPUTexture = @ptrCast(self.render_texture orelse return error.NoRenderTexture);

        // Acquire command buffer
        const cmd = c.SDL_AcquireGPUCommandBuffer(device) orelse {
            return self.setError("Failed to acquire command buffer");
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
            return self.setError("Failed to begin render pass");
        };

        // Bind pipeline
        c.SDL_BindGPUGraphicsPipeline(render_pass, pipeline);

        // Push uniforms
        c.SDL_PushGPUFragmentUniformData(cmd, 0, &self.uniforms, @sizeOf(shaders.Uniforms));

        // Draw fullscreen triangle (3 vertices, generated in shader)
        c.SDL_DrawGPUPrimitives(render_pass, 3, 1, 0, 0);

        // End render pass
        c.SDL_EndGPURenderPass(render_pass);

        // Submit with fence for async completion tracking
        self.pending_fence = c.SDL_SubmitGPUCommandBufferAndAcquireFence(cmd);
        if (self.pending_fence == null) {
            return self.setError("Failed to submit command buffer");
        }

        self.is_rendering = true;
        return true;
    }

    /// Poll for render completion (non-blocking)
    /// Returns true if rendering is complete
    pub fn pollCompletion(self: *GpuPipeline) bool {
        if (!self.is_rendering) return true;

        const device: *c.SDL_GPUDevice = @ptrCast(self.device orelse return true);
        const fence: *c.SDL_GPUFence = @ptrCast(self.pending_fence orelse return true);

        if (c.SDL_QueryGPUFence(device, fence)) {
            c.SDL_ReleaseGPUFence(device, fence);
            self.pending_fence = null;
            self.is_rendering = false;
            return true;
        }

        return false;
    }

    /// Update uniform values
    pub fn updateUniforms(self: *GpuPipeline, time: f32, width: f32, height: f32) void {
        self.uniforms.time = time;
        self.uniforms.resolution = .{ width, height };
    }

    /// Set axis bounds for coordinate mapping
    pub fn setAxisBounds(self: *GpuPipeline, x_min: f32, x_max: f32, y_min: f32, y_max: f32) void {
        self.uniforms.axis_min = .{ x_min, y_min };
        self.uniforms.axis_max = .{ x_max, y_max };
    }

    /// Get the render texture for display
    pub fn getRenderTexture(self: *GpuPipeline) ?*c.SDL_GPUTexture {
        return @ptrCast(self.render_texture);
    }

    /// Get render dimensions
    pub fn getRenderSize(self: *GpuPipeline) struct { width: u32, height: u32 } {
        return .{ .width = self.render_width, .height = self.render_height };
    }

    /// Render to an external texture (for dvui integration)
    /// The texture must have SDL_GPU_TEXTUREUSAGE_COLOR_TARGET flag
    pub fn renderToExternalTexture(self: *GpuPipeline, external_texture: *anyopaque, width: u32, height: u32) !bool {
        if (self.is_rendering) return false;

        const device: *c.SDL_GPUDevice = @ptrCast(self.device orelse return error.NoDevice);
        const pipeline: *c.SDL_GPUGraphicsPipeline = @ptrCast(self.pipeline orelse return error.NoPipeline);
        const target_texture: *c.SDL_GPUTexture = @ptrCast(external_texture);

        // Update resolution in uniforms
        self.uniforms.resolution = .{ @floatFromInt(width), @floatFromInt(height) };

        // Acquire command buffer
        const cmd = c.SDL_AcquireGPUCommandBuffer(device) orelse {
            return self.setError("Failed to acquire command buffer");
        };

        // Set up color target
        var color_target = std.mem.zeroes(c.SDL_GPUColorTargetInfo);
        color_target.texture = target_texture;
        color_target.clear_color = .{ .r = 0.0, .g = 0.0, .b = 0.0, .a = 1.0 };
        color_target.load_op = c.SDL_GPU_LOADOP_CLEAR;
        color_target.store_op = c.SDL_GPU_STOREOP_STORE;

        // Begin render pass
        const render_pass = c.SDL_BeginGPURenderPass(cmd, &color_target, 1, null) orelse {
            _ = c.SDL_SubmitGPUCommandBuffer(cmd);
            return self.setError("Failed to begin render pass");
        };

        // Bind pipeline
        c.SDL_BindGPUGraphicsPipeline(render_pass, pipeline);

        // Push uniforms
        c.SDL_PushGPUFragmentUniformData(cmd, 0, &self.uniforms, @sizeOf(shaders.Uniforms));

        // Draw fullscreen triangle (3 vertices, generated in shader)
        c.SDL_DrawGPUPrimitives(render_pass, 3, 1, 0, 0);

        // End render pass
        c.SDL_EndGPURenderPass(render_pass);

        // Submit with fence for async completion tracking
        self.pending_fence = c.SDL_SubmitGPUCommandBufferAndAcquireFence(cmd);
        if (self.pending_fence == null) {
            return self.setError("Failed to submit command buffer");
        }

        self.is_rendering = true;
        return true;
    }

    /// Helper to set error and return error type
    fn setError(self: *GpuPipeline, msg: []const u8) error{PipelineError} {
        if (self.last_error) |old| {
            self.allocator.free(old);
        }
        self.last_error = self.allocator.dupe(u8, msg) catch null;
        const sdl_error = c.SDL_GetError();
        if (sdl_error != null and sdl_error[0] != 0) {
            std.log.err("{s}: {s}", .{ msg, sdl_error });
        } else {
            std.log.err("{s}", .{msg});
        }
        return error.PipelineError;
    }
};

// Tests
test "GpuPipeline init/deinit" {
    var pipeline = GpuPipeline.init(std.testing.allocator);
    defer pipeline.deinit();

    // Without device, should have no resources
    try std.testing.expect(pipeline.device == null);
    try std.testing.expect(pipeline.pipeline == null);
}
