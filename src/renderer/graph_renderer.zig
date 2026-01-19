//! Graph/Plot renderer
//!
//! Renders visualizations based on parsed user input.
//! Uses dvui for display, with SDL3 GPU for custom shader rendering.
//!
//! The shader_compiler module handles GLSL → SPIRV → Native format
//! compilation for cross-platform GPU shader execution.
//!
//! Rendering flow:
//! 1. updateShader() - compile new GLSL and update pipeline
//! 2. render() - submit GPU work (non-blocking)
//! 3. pollCompletion() - check if GPU finished
//! 4. renderToPanel() - display the result in dvui

const std = @import("std");
const dvui = @import("dvui");
const SDLBackend = @import("sdl3gpu");

const session = @import("../session/session.zig");
const theme = @import("../ui/theme.zig");
const shader_compiler = @import("shader_compiler.zig");
const shaders = @import("shaders/shaders.zig");
const GpuPipeline = @import("gpu_pipeline.zig").GpuPipeline;
const types = @import("../lang/types.zig");

// SDL types from backend - must match what dvui's sdl3gpu uses
const c = SDLBackend.c;

/// Backend texture target structure - must match dvui's BackendTextureTarget
const BackendTextureTarget = extern struct {
    texture: *c.SDL_GPUTexture,
    sampler: *c.SDL_GPUSampler,
};

/// Axis margin size in pixels
const AXIS_MARGIN_BOTTOM: f32 = 40.0;
const AXIS_MARGIN_LEFT: f32 = 50.0;

pub const GraphRenderer = struct {
    allocator: std.mem.Allocator,

    /// Cached render data
    cached_content_hash: u64,

    /// GPU pipeline for custom shader rendering
    gpu_pipeline: GpuPipeline,

    /// Initialization state
    is_initialized: bool,
    init_attempted: bool,

    /// Current generated GLSL (null if none)
    current_glsl: ?[]const u8,

    /// Error message from last operation
    last_error: ?[]const u8,

    /// Animation state
    start_time: i64,
    is_animating: bool,

    /// Frame counter for render status
    frame_count: u64,

    /// Backend reference for texture operations
    backend: ?*SDLBackend.SDLBackend,

    /// Render texture for dvui display
    render_target: ?dvui.TextureTarget,

    /// Axis state for pan/zoom and coordinate display
    axis_state: types.AxisState,

    /// Mouse interaction state
    is_dragging: bool,
    last_mouse_x: f32,
    last_mouse_y: f32,
    hover_x: ?f64,
    hover_y: ?f64,

    /// Flag to wait for first render after shader update
    needs_initial_render: bool,

    /// Track if we've successfully rendered at least once
    has_rendered_once: bool,

    pub fn init(allocator: std.mem.Allocator) GraphRenderer {
        return .{
            .allocator = allocator,
            .cached_content_hash = 0,
            .gpu_pipeline = GpuPipeline.init(allocator),
            .is_initialized = false,
            .init_attempted = false,
            .current_glsl = null,
            .last_error = null,
            .start_time = std.time.milliTimestamp(),
            .is_animating = false,
            .frame_count = 0,
            .backend = null,
            .render_target = null,
            .axis_state = .{},
            .is_dragging = false,
            .last_mouse_x = 0,
            .last_mouse_y = 0,
            .hover_x = null,
            .hover_y = null,
            .needs_initial_render = false,
            .has_rendered_once = false,
        };
    }

    /// Initialize with GPU device and backend (call after SDL is ready)
    pub fn initWithDevice(self: *GraphRenderer, backend: *SDLBackend.SDLBackend) void {
        if (self.init_attempted) return;
        self.init_attempted = true;

        self.backend = backend;

        // Initialize shader compiler
        shader_compiler.init() catch |err| {
            std.log.warn("Shader compiler initialization failed: {}", .{err});
            self.setError("Shader compiler init failed");
            return;
        };

        // Initialize GPU pipeline with device
        self.gpu_pipeline.initWithDevice(backend.device) catch |err| {
            std.log.warn("GPU pipeline initialization failed: {}", .{err});
            self.setError("GPU pipeline init failed");
            return;
        };

        self.is_initialized = true;
        std.log.info("GraphRenderer initialized with GPU pipeline", .{});
    }

    pub fn deinit(self: *GraphRenderer) void {
        if (self.current_glsl) |glsl| {
            self.allocator.free(glsl);
        }
        if (self.last_error) |err| {
            self.allocator.free(err);
        }
        // Note: GPU resources are released via releaseGpuResources() which must be
        // called while the SDL backend is still valid (before backend.deinit())
        self.gpu_pipeline.deinit();
        if (self.is_initialized) {
            shader_compiler.deinit();
        }
    }

    /// Release GPU resources explicitly - call before SDL backend is destroyed
    pub fn releaseGpuResources(self: *GraphRenderer) void {
        // Note: render_target cleanup is handled by dvui's texture system
        // We just need to clear our reference
        self.render_target = null;
        self.gpu_pipeline.releaseGpuResources();
        self.backend = null;
    }

    /// Update the graph based on session content
    pub fn update(self: *GraphRenderer, active_session: *session.TabSession) void {
        // Check if we need to re-render
        if (!active_session.render_state.needs_update) {
            // Check content hash to see if content changed
            const new_hash = std.hash.Wyhash.hash(0, active_session.content.items);
            if (new_hash == self.cached_content_hash) {
                return;
            }
            self.cached_content_hash = new_hash;
        }

        active_session.render_state.needs_update = false;

        // TODO: When AST/parser is ready, generate GLSL here
        // For now, we just mark content as processed
    }

    /// Update the shader from generated GLSL code
    /// Called when the user clicks "Play" and GLSL is generated from the AST
    pub fn updateShader(self: *GraphRenderer, glsl_source: []const u8) !void {
        std.log.info("[GraphRenderer] ========== UPDATE SHADER START ==========", .{});
        std.log.info("[GraphRenderer] GLSL source length: {} bytes", .{glsl_source.len});
        std.log.info("[GraphRenderer] GLSL source:\n{s}", .{glsl_source});

        // Clear previous error
        self.clearError();

        // Store the GLSL source
        if (self.current_glsl) |old| {
            self.allocator.free(old);
        }
        self.current_glsl = try self.allocator.dupe(u8, glsl_source);

        if (!self.is_initialized) {
            std.log.err("[GraphRenderer] Cannot update shader: not initialized", .{});
            return error.NotInitialized;
        }

        std.log.info("[GraphRenderer] Calling gpu_pipeline.updateFragmentShader()...", .{});

        // Update the GPU pipeline with new shader
        self.gpu_pipeline.updateFragmentShader(glsl_source) catch |err| {
            std.log.err("[GraphRenderer] Shader update FAILED: {}", .{err});
            self.setError("Shader update failed");
            return err;
        };

        // IMPORTANT: Invalidate render target so it gets recreated with new shader
        // The old texture still contains output from the previous shader
        std.log.info("[GraphRenderer] Invalidating render target to force recreation with new shader", .{});
        self.render_target = null;

        // Mark that we need to wait for first render to complete and reset rendered flag
        self.needs_initial_render = true;
        self.has_rendered_once = false;

        std.log.info("[GraphRenderer] Shader updated successfully ✓", .{});
        std.log.info("[GraphRenderer] ========== UPDATE SHADER END ==========", .{});
    }

    /// Run automated shader test - parses "x²+y²=9" and tries to render
    /// Call this after initialization to test without user interaction
    pub fn runShaderTest(self: *GraphRenderer, allocator: std.mem.Allocator) !void {
        std.log.info("Running shader test: x²+y²=9", .{});

        const lexer = @import("../lang/lexer.zig");
        const parser = @import("../lang/parser.zig");
        const glsl_gen = @import("../lang/glsl_gen.zig");

        const test_expr = "x²+y²=9";

        // Tokenize
        var lex = lexer.Lexer.init(allocator, lexer.LexerConfig.logosPatterns()) catch |err| {
            std.log.err("Lexer init failed: {}", .{err});
            return err;
        };
        defer lex.deinit();
        const tokens = lex.tokenize(test_expr) catch |err| {
            std.log.err("Tokenization failed: {}", .{err});
            return err;
        };
        defer allocator.free(tokens);

        // Parse (NOTE: AST nodes are intentionally not freed to avoid complicating the test)
        var parse = parser.Parser.init(allocator, tokens);
        const ast = parse.parse() catch |err| {
            std.log.err("Parsing failed: {}", .{err});
            return err;
        };

        // Generate GLSL
        var gen = glsl_gen.GlslGenerator.init(allocator);
        defer gen.deinit();
        const result = gen.generate(ast) catch |err| {
            std.log.err("GLSL generation failed: {}", .{err});
            return err;
        };
        defer {
            for (result.shaders) |shader| {
                allocator.free(shader.source);
            }
            allocator.free(result.shaders);
            allocator.free(result.errors);
        }

        if (result.shaders.len == 0) {
            std.log.err("No shaders generated", .{});
            return error.NoShaders;
        }

        // Update with the generated shader
        self.updateShader(result.shaders[0].source) catch |err| {
            std.log.err("Generated shader update failed: {}", .{err});
            return err;
        };

        std.log.info("Shader test passed!", .{});
    }


    /// Poll for render completion (non-blocking)
    pub fn pollCompletion(self: *GraphRenderer) bool {
        return self.gpu_pipeline.pollCompletion();
    }

    /// Check if currently rendering
    pub fn isRendering(self: *GraphRenderer) bool {
        return self.gpu_pipeline.is_rendering;
    }

    /// Start/stop animation
    pub fn setAnimating(self: *GraphRenderer, animating: bool) void {
        self.is_animating = animating;
        if (animating) {
            self.start_time = std.time.milliTimestamp();
        }
    }

    /// Set axis bounds for coordinate mapping
    pub fn setAxisBounds(self: *GraphRenderer, x_min: f32, x_max: f32, y_min: f32, y_max: f32) void {
        self.gpu_pipeline.setAxisBounds(x_min, x_max, y_min, y_max);
    }

    /// Render the graph to the dvui panel
    pub fn renderToPanel(self: *GraphRenderer, width: f32, height: f32) void {
        // Calculate render area (excluding margins)
        const render_width = @max(1, width - AXIS_MARGIN_LEFT);
        const render_height = @max(1, height - AXIS_MARGIN_BOTTOM);
        const panel_width = @as(u32, @intFromFloat(render_width));
        const panel_height = @as(u32, @intFromFloat(render_height));

        // Update frame counter
        self.frame_count +%= 1;

        // Sync axis state to GPU pipeline uniforms
        self.syncAxisStateToUniforms();

        // Always ensure we have a render target (even before shader is loaded)
        self.ensureRenderTarget(panel_width, panel_height);

        // Try to render if initialized and shader is active
        if (self.is_initialized and self.current_glsl != null) {
            // Only submit render if:
            // 1. Not currently rendering, AND
            // 2. Either animating OR we haven't rendered once yet (for static expressions)
            const is_rendering = self.isRendering();
            const should_render = !is_rendering and (self.is_animating or !self.has_rendered_once);

            if (should_render) {
                if (self.render_target) |target| {
                    // Get the underlying SDL texture from dvui's texture target
                    const backend_target: *BackendTextureTarget = @ptrCast(@alignCast(target.ptr));

                    // Update uniforms with time and resolution
                    const elapsed = @as(f32, @floatFromInt(std.time.milliTimestamp() - self.start_time)) / 1000.0;
                    self.gpu_pipeline.updateUniforms(
                        elapsed,
                        @floatFromInt(panel_width),
                        @floatFromInt(panel_height),
                    );

                    // Render to the external texture
                    _ = self.gpu_pipeline.renderToExternalTexture(
                        backend_target.texture,
                        panel_width,
                        panel_height,
                    ) catch |err| {
                        std.log.err("[GraphRenderer] Render failed: {}", .{err});
                    };

                    // Mark first render after shader update as complete
                    if (self.needs_initial_render) {
                        self.needs_initial_render = false;
                    }
                }
            }

            // Poll for completion
            if (self.pollCompletion()) {
                // Mark that we've rendered successfully
                self.has_rendered_once = true;

                // Render complete - trigger continuous re-render if animating
                if (self.is_animating) {
                    if (self.render_target) |target| {
                        const backend_target: *BackendTextureTarget = @ptrCast(@alignCast(target.ptr));
                        _ = self.gpu_pipeline.renderToExternalTexture(
                            backend_target.texture,
                            panel_width,
                            panel_height,
                        ) catch {};
                    }
                }
            }
        }

        // Render visual content with axis margins (this will DISPLAY the texture)
        self.renderVisualContentWithAxes(width, height);
    }

    /// Sync axis state to GPU pipeline uniforms
    fn syncAxisStateToUniforms(self: *GraphRenderer) void {
        // Note: axis2 (Y-axis) is inverted for GPU because screen coordinates
        // have Y increasing downward, but mathematical plots have Y increasing upward
        self.gpu_pipeline.setAxisBounds(
            @floatCast(self.axis_state.axis1.start),
            @floatCast(self.axis_state.axis1.end),
            @floatCast(self.axis_state.axis2.end),   // swapped: max becomes min
            @floatCast(self.axis_state.axis2.start), // swapped: min becomes max
        );
    }

    /// Ensure render target exists and has correct size
    fn ensureRenderTarget(self: *GraphRenderer, width: u32, height: u32) void {
        if (width == 0 or height == 0) return;

        // Check if we need to recreate
        if (self.render_target) |rt| {
            if (rt.width == width and rt.height == height) {
                return; // Size matches, keep existing target
            }
            // Size changed, mark for recreation
            std.log.info("[GraphRenderer] Render target size changed: {}x{} -> {}x{}, recreating", .{
                rt.width,
                rt.height,
                width,
                height,
            });
            // Note: We can't easily destroy dvui textures mid-frame, so we'll leak the old one
            // dvui's texture cache handles cleanup normally
            self.render_target = null;
        }

        // Create new render target
        std.log.info("[GraphRenderer] Creating new render target: {}x{}", .{ width, height });
        self.render_target = dvui.TextureTarget.create(width, height, .nearest) catch |err| {
            std.log.err("[GraphRenderer] Failed to create render target: {}", .{err});
            return;
        };
        std.log.info("[GraphRenderer] Render target created: ptr={*}, EMPTY/UNINITIALIZED", .{self.render_target.?.ptr});
    }

    /// Render visual content with axis margins
    fn renderVisualContentWithAxes(self: *GraphRenderer, width: f32, height: f32) void {
        // Create main container
        var main_container = dvui.box(@src(), .{ .dir = .horizontal }, .{
            .expand = .both,
        });
        defer main_container.deinit();

        // Left axis margin (Y axis labels)
        self.renderLeftAxisMargin(height);

        // Right side: render area + bottom margin
        {
            var right_container = dvui.box(@src(), .{ .dir = .vertical }, .{
                .expand = .both,
            });
            defer right_container.deinit();

            // Render area
            self.renderMainContent(width - AXIS_MARGIN_LEFT, height - AXIS_MARGIN_BOTTOM);

            // Bottom axis margin (X axis labels)
            self.renderBottomAxisMargin(width);
        }
    }

    /// Render the left axis margin (Y axis / axis2)
    fn renderLeftAxisMargin(self: *GraphRenderer, height: f32) void {
        var margin_box = dvui.box(@src(), .{ .dir = .vertical }, .{
            .min_size_content = .{ .w = AXIS_MARGIN_LEFT, .h = 0 },
            .max_size_content = .{ .w = AXIS_MARGIN_LEFT, .h = height - AXIS_MARGIN_BOTTOM },
            .expand = .vertical,
            .color_fill = dvui.Color.fromHex("#1a1a2e"),
            .background = true,
        });
        defer margin_box.deinit();

        // Show axis2 label with range
        var max_buf: [32]u8 = undefined;
        const max_label = std.fmt.bufPrint(&max_buf, "{d:.1}", .{self.axis_state.axis2.end}) catch "?";
        dvui.labelNoFmt(@src(), max_label, .{}, .{
            .color_text = dvui.Color.fromHex("#888888"),
            .font = .theme(.mono),
            .gravity_x = 1.0,
        });

        // Spacer
        _ = dvui.spacer(@src(), .{ .expand = .vertical });

        // Y axis label
        dvui.labelNoFmt(@src(), "axis2", .{}, .{
            .color_text = dvui.Color.fromHex("#666666"),
            .font = .theme(.mono),
            .gravity_x = 0.5,
        });

        // Spacer
        _ = dvui.spacer(@src(), .{ .expand = .vertical });

        var min_buf: [32]u8 = undefined;
        const min_label = std.fmt.bufPrint(&min_buf, "{d:.1}", .{self.axis_state.axis2.start}) catch "?";
        dvui.labelNoFmt(@src(), min_label, .{}, .{
            .color_text = dvui.Color.fromHex("#888888"),
            .font = .theme(.mono),
            .gravity_x = 1.0,
        });
    }

    /// Render the bottom axis margin (X axis / axis1)
    fn renderBottomAxisMargin(self: *GraphRenderer, width: f32) void {
        var margin_box = dvui.box(@src(), .{ .dir = .horizontal }, .{
            .min_size_content = .{ .w = 0, .h = AXIS_MARGIN_BOTTOM },
            .max_size_content = .{ .w = width - AXIS_MARGIN_LEFT, .h = AXIS_MARGIN_BOTTOM },
            .expand = .horizontal,
            .color_fill = dvui.Color.fromHex("#1a1a2e"),
            .background = true,
        });
        defer margin_box.deinit();

        // Show min value
        var min_buf: [32]u8 = undefined;
        const min_label = std.fmt.bufPrint(&min_buf, "{d:.1}", .{self.axis_state.axis1.start}) catch "?";
        dvui.labelNoFmt(@src(), min_label, .{}, .{
            .color_text = dvui.Color.fromHex("#888888"),
            .font = .theme(.mono),
            .gravity_y = 0.0,
        });

        // Spacer
        _ = dvui.spacer(@src(), .{ .expand = .horizontal });

        // Show hover coordinates or axis label
        if (self.hover_x != null and self.hover_y != null) {
            var hover_label: [64]u8 = undefined;
            const hover_text = std.fmt.bufPrint(&hover_label, "({d:.2}, {d:.2})", .{ self.hover_x.?, self.hover_y.? }) catch "";
            dvui.labelNoFmt(@src(), hover_text, .{}, .{
                .color_text = dvui.Color.fromHex("#ffaa00"),
                .font = .theme(.mono),
                .gravity_y = 0.5,
            });
        } else {
            dvui.labelNoFmt(@src(), "axis1", .{}, .{
                .color_text = dvui.Color.fromHex("#666666"),
                .font = .theme(.mono),
                .gravity_y = 0.5,
            });
        }

        // Spacer
        _ = dvui.spacer(@src(), .{ .expand = .horizontal });

        // Show max value
        var max_buf: [32]u8 = undefined;
        const max_label = std.fmt.bufPrint(&max_buf, "{d:.1}", .{self.axis_state.axis1.end}) catch "?";
        dvui.labelNoFmt(@src(), max_label, .{}, .{
            .color_text = dvui.Color.fromHex("#888888"),
            .font = .theme(.mono),
            .gravity_y = 0.0,
        });
    }

    /// Render the main content area (shader output or status)
    fn renderMainContent(self: *GraphRenderer, width: f32, height: f32) void {
        // Create a container that fills the render area
        var container = dvui.box(@src(), .{ .dir = .vertical }, .{
            .expand = .both,
            .min_size_content = .{ .w = width, .h = height },
        });
        defer container.deinit();

        // Show error if present
        if (self.last_error) |err_msg| {
            self.renderErrorState(err_msg);
            return;
        }

        // Show status based on current state
        if (!self.is_initialized) {
            self.renderInitializingState();
            return;
        }

        // Always render the texture (black if no shader loaded)
        self.renderTextureOrBlack(width, height);
    }

    fn renderErrorState(_: *GraphRenderer, err_msg: []const u8) void {
        // Center the error message
        var center = dvui.box(@src(), .{}, .{ .expand = .both, .gravity_x = 0.5, .gravity_y = 0.5 });
        defer center.deinit();

        dvui.labelNoFmt(@src(), "⚠ Shader Error", .{}, .{
            .font = .theme(.title),
            .color_text = dvui.Color.fromHex("#ff4444"),
            .margin = .{ .x = 0, .y = 10, .w = 0, .h = 10 },
        });
        dvui.labelNoFmt(@src(), err_msg, .{}, .{
            .color_text = dvui.Color.fromHex("#ff6666"),
        });
    }

    fn renderInitializingState(_: *GraphRenderer) void {
        var center = dvui.box(@src(), .{}, .{ .expand = .both, .gravity_x = 0.5, .gravity_y = 0.5 });
        defer center.deinit();

        dvui.labelNoFmt(@src(), "⏳ Initializing...", .{}, .{
            .font = .theme(.title),
            .color_text = dvui.Color.fromHex("#ffaa00"),
        });
    }

    /// Render the texture (or black background if texture not available)
    fn renderTextureOrBlack(self: *GraphRenderer, width: f32, height: f32) void {
        _ = width;
        _ = height;

        // If we have a render target AND it's been rendered at least once, display it
        // Don't display uninitialized textures (they contain garbage GPU memory)
        if (self.render_target) |target| {
            // Wait for the first render to complete before displaying
            if (!self.has_rendered_once) {
                self.renderBlackBackground();
                return;
            }

            // Convert TextureTarget to Texture for display
            const texture = dvui.textureFromTarget(target) catch |err| {
                std.log.err("[GraphRenderer] Failed to convert texture target: {}", .{err});
                self.renderBlackBackground();
                return;
            };

            // Get current rect for texture placement
            // Using a box to define the area, then render texture manually
            var content_box = dvui.box(@src(), .{}, .{
                .expand = .both,
            });
            defer content_box.deinit();

            // Get the rect-scale of the content box for rendering
            const rs = content_box.data().contentRectScale();

            // Render the texture to fill the box
            dvui.renderTexture(texture, rs, .{}) catch |err| {
                std.log.err("[GraphRenderer] Failed to render texture: {}", .{err});
                self.renderBlackBackground();
            };
        } else {
            // No render target yet - show black background
            self.renderBlackBackground();
        }
    }

    /// Render a black background when texture is not available
    fn renderBlackBackground(self: *GraphRenderer) void {
        _ = self;
        var bg = dvui.box(@src(), .{}, .{
            .expand = .both,
            .color_fill = dvui.Color{ .r = 0, .g = 0, .b = 0, .a = 255 },
            .background = true,
        });
        bg.deinit();
    }


    fn renderIdleState(_: *GraphRenderer) void {
        var center = dvui.box(@src(), .{}, .{ .expand = .both, .gravity_x = 0.5, .gravity_y = 0.5 });
        defer center.deinit();

        dvui.labelNoFmt(@src(), "📊 Graph Renderer", .{}, .{
            .font = .theme(.title),
            .color_text = dvui.Color.fromHex("#888888"),
            .margin = .{ .x = 0, .y = 0, .w = 0, .h = 10 },
        });
        dvui.labelNoFmt(@src(), "GPU pipeline ready", .{}, .{
            .color_text = dvui.Color.fromHex("#666666"),
        });
        dvui.labelNoFmt(@src(), "", .{}, .{ .margin = .{ .y = 10 } });
        dvui.labelNoFmt(@src(), "Write expressions in the editor", .{}, .{
            .color_text = dvui.Color.fromHex("#666666"),
        });
        dvui.labelNoFmt(@src(), "and click Play to render", .{}, .{
            .color_text = dvui.Color.fromHex("#666666"),
        });
    }


    /// Test shader compilation with a sample GLSL
    pub fn testCompilation(self: *GraphRenderer) !void {
        try self.updateShader(shaders.default_fragment_glsl);
        self.is_animating = true;
    }

    /// Helper to set error message
    fn setError(self: *GraphRenderer, msg: []const u8) void {
        if (self.last_error) |old| {
            self.allocator.free(old);
        }
        self.last_error = self.allocator.dupe(u8, msg) catch null;
    }

    /// Clear error message
    fn clearError(self: *GraphRenderer) void {
        if (self.last_error) |old| {
            self.allocator.free(old);
            self.last_error = null;
        }
    }

    // ========================================================================
    // Mouse Interaction API
    // ========================================================================

    /// Handle mouse move in the render area
    /// Call this with coordinates relative to the render panel
    pub fn handleMouseMove(self: *GraphRenderer, x: f32, y: f32, panel_width: f32, panel_height: f32) void {
        // Calculate render area dimensions (excluding margins)
        const render_width = panel_width - AXIS_MARGIN_LEFT;
        const render_height = panel_height - AXIS_MARGIN_BOTTOM;

        // Check if mouse is in render area (not in margins)
        if (x >= AXIS_MARGIN_LEFT and y < render_height) {
            // Convert to render area coordinates
            const rx = x - AXIS_MARGIN_LEFT;
            const ry = y;

            // Convert to world coordinates
            const world = self.axis_state.screenToWorld(rx, ry, render_width, render_height);
            self.hover_x = world.x;
            self.hover_y = world.y;

            // Handle dragging for pan
            if (self.is_dragging) {
                const dx = (x - self.last_mouse_x) / render_width;
                const dy = (y - self.last_mouse_y) / render_height;

                // Pan in world coordinates
                const pan_x = dx * self.axis_state.axis1.span();
                const pan_y = dy * self.axis_state.axis2.span();
                self.axis_state.panBy(pan_x, -pan_y); // Invert Y for screen coords
            }
        } else {
            self.hover_x = null;
            self.hover_y = null;
        }

        self.last_mouse_x = x;
        self.last_mouse_y = y;
    }

    /// Handle mouse button down
    pub fn handleMouseDown(self: *GraphRenderer, x: f32, y: f32, _: f32, panel_height: f32) void {
        // Check if in render area (not in margins)
        const render_height = panel_height - AXIS_MARGIN_BOTTOM;
        if (x >= AXIS_MARGIN_LEFT and y < render_height) {
            self.is_dragging = true;
            self.last_mouse_x = x;
            self.last_mouse_y = y;
        }
    }

    /// Handle mouse button up
    pub fn handleMouseUp(self: *GraphRenderer) void {
        self.is_dragging = false;
    }

    /// Handle mouse scroll for zoom
    pub fn handleScroll(self: *GraphRenderer, delta: f32, x: f32, y: f32, panel_width: f32, panel_height: f32) void {
        // Calculate render area dimensions
        const render_width = panel_width - AXIS_MARGIN_LEFT;
        const render_height = panel_height - AXIS_MARGIN_BOTTOM;

        // Check if in render area
        if (x >= AXIS_MARGIN_LEFT and y < render_height) {
            // Convert to render area coordinates
            const rx = x - AXIS_MARGIN_LEFT;
            const ry = y;

            // Convert to world coordinates
            const world = self.axis_state.screenToWorld(rx, ry, render_width, render_height);

            // Zoom factor (positive delta = zoom in)
            const factor: f64 = if (delta > 0) 0.9 else 1.1;

            self.axis_state.zoomAt(factor, world.x, world.y);
        }
    }

    /// Reset axis ranges to defaults
    pub fn resetAxisRanges(self: *GraphRenderer) void {
        self.axis_state = .{};
    }

    /// Set axis1 (X axis) range
    pub fn setAxis1Range(self: *GraphRenderer, start: f64, end: f64) !void {
        try self.axis_state.axis1.setStart(start);
        try self.axis_state.axis1.setEnd(end);
    }

    /// Set axis2 (Y axis) range
    pub fn setAxis2Range(self: *GraphRenderer, start: f64, end: f64) !void {
        try self.axis_state.axis2.setStart(start);
        try self.axis_state.axis2.setEnd(end);
    }

    /// Get current axis1 range
    pub fn getAxis1(self: *GraphRenderer) types.RangeVar {
        return self.axis_state.axis1;
    }

    /// Get current axis2 range
    pub fn getAxis2(self: *GraphRenderer) types.RangeVar {
        return self.axis_state.axis2;
    }
};
