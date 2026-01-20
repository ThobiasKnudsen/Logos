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

/// Grid rendering configuration
const GRID_LINE_COLOR: dvui.Color = dvui.Color.fromHex("#FFFFFF66");
const GRID_LABEL_COLOR: dvui.Color = dvui.Color.fromHex("#888888");
const GRID_LABEL_PADDING: f32 = 4.0;
const DEFAULT_GRID_LINES_PER_AXIS: usize = 5;

/// Target pixels between grid lines (used to calculate grid density)
const TARGET_PIXELS_PER_GRID_LINE: f32 = 100.0;

/// Edge zone size for axis-specific zoom (in pixels)
const AXIS_ZONE_SIZE: f32 = 40.0;

/// Mouse interaction zone
pub const MouseZone = enum {
    /// Center area - pan/zoom both axes
    center,
    /// Bottom edge - zoom axis1 (X) only
    axis1_edge,
    /// Left edge - zoom axis2 (Y) only
    axis2_edge,
};

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
    current_mouse_zone: MouseZone,

    /// Flag to wait for first render after shader update
    needs_initial_render: bool,

    /// Track if we've successfully rendered at least once
    has_rendered_once: bool,

    /// Flag to trigger re-render when view changes (zoom/pan)
    needs_view_update: bool,

    /// Grid configuration
    num_grid_lines_axis1: usize,
    num_grid_lines_axis2: usize,

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
            .current_mouse_zone = .center,
            .needs_initial_render = false,
            .has_rendered_once = false,
            .needs_view_update = false,
            .num_grid_lines_axis1 = DEFAULT_GRID_LINES_PER_AXIS,
            .num_grid_lines_axis2 = DEFAULT_GRID_LINES_PER_AXIS,
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

            // Try to get detailed error message from shader compiler
            if (shader_compiler.getLastError()) |detailed_error| {
                self.setError(detailed_error);
            } else {
                self.setError("Shader compilation failed");
            }
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
        // Use full panel dimensions (no margins)
        const panel_width = @as(u32, @intFromFloat(@max(1, width)));
        const panel_height = @as(u32, @intFromFloat(@max(1, height)));

        // Update frame counter
        self.frame_count +%= 1;

        // Update grid line counts based on panel dimensions
        self.updateGridDensity(width, height);

        // Sync axis state to GPU pipeline uniforms
        self.syncAxisStateToUniforms();

        // Always ensure we have a render target (even before shader is loaded)
        self.ensureRenderTarget(panel_width, panel_height);

        // Try to render if initialized and shader is active
        if (self.is_initialized and self.current_glsl != null) {
            // Poll for completion FIRST so we can start a new render this frame if the previous one finished
            if (self.pollCompletion()) {
                // Mark that we've rendered successfully
                self.has_rendered_once = true;
            }

            // Only submit render if not currently rendering
            // This ensures continuous rendering to the texture target
            const is_rendering = self.isRendering();

            if (!is_rendering) {
                // Clear the view update flag since we're actually rendering now
                self.needs_view_update = false;
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
        }

        // Render visual content with grid overlay
        self.renderVisualContent(width, height);
    }

    /// Update grid line density based on panel dimensions
    /// More pixels = more grid lines to maintain roughly constant spacing
    fn updateGridDensity(self: *GraphRenderer, width: f32, height: f32) void {
        // Calculate number of grid lines based on panel dimensions
        // Target ~100 pixels between each grid line
        self.num_grid_lines_axis1 = @max(2, @as(usize, @intFromFloat(width / TARGET_PIXELS_PER_GRID_LINE)));
        self.num_grid_lines_axis2 = @max(2, @as(usize, @intFromFloat(height / TARGET_PIXELS_PER_GRID_LINE)));
    }

    /// Sync axis state to GPU pipeline uniforms
    fn syncAxisStateToUniforms(self: *GraphRenderer) void {
        // Note: axis2 (Y-axis) is inverted for GPU because screen coordinates
        // have Y increasing downward, but mathematical plots have Y increasing upward
        self.gpu_pipeline.setAxisBounds(
            @floatCast(self.axis_state.axis1.start),
            @floatCast(self.axis_state.axis1.end),
            @floatCast(self.axis_state.axis2.end), // swapped: max becomes min
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

    /// Render visual content (texture + grid overlay)
    fn renderVisualContent(self: *GraphRenderer, width: f32, height: f32) void {
        // Create a container that fills the entire panel
        var container = dvui.box(@src(), .{}, .{
            .expand = .both,
            .min_size_content = .{ .w = width, .h = height },
        });
        defer container.deinit();

        // Get the panel's position for grid overlay (before any child boxes)
        const panel_rs = container.data().contentRectScale();

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

        // Render the texture (or black background if not available)
        self.renderTextureOrBlack(width, height);

        // Overlay grid lines and labels on top of the texture
        // Pass the panel position so grid draws at correct location without creating new layout
        self.renderGridOverlay(width, height, panel_rs.r.x, panel_rs.r.y);
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

    /// Calculate a "nice" step size for grid lines using 1-2-5 sequence
    /// Returns a step that gives roughly the target number of lines
    fn calculateNiceStep(range: f64, target_lines: usize) f64 {
        if (range <= 0) return 1.0;

        // Calculate rough step
        const rough_step = range / @as(f64, @floatFromInt(target_lines));

        // Find order of magnitude (power of 10)
        const magnitude = @floor(std.math.log10(rough_step));
        const power = std.math.pow(f64, 10.0, magnitude);

        // Normalize to 1-10 range
        const normalized = rough_step / power;

        // Round to nearest 1, 2, or 5
        const nice_normalized: f64 = if (normalized < 1.5)
            1.0
        else if (normalized < 3.5)
            2.0
        else if (normalized < 7.5)
            5.0
        else
            10.0;

        return nice_normalized * power;
    }

    /// Format a number with appropriate decimal places based on step size
    fn formatGridLabel(buf: []u8, value: f64, step: f64) []const u8 {
        // Determine decimal places based on step magnitude
        const step_magnitude = @floor(std.math.log10(@abs(step)));
        const decimals: usize = if (step_magnitude >= 0)
            0
        else
            @intFromFloat(@min(6.0, -step_magnitude));

        return switch (decimals) {
            0 => std.fmt.bufPrint(buf, "{d:.0}", .{value}) catch "?",
            1 => std.fmt.bufPrint(buf, "{d:.1}", .{value}) catch "?",
            2 => std.fmt.bufPrint(buf, "{d:.2}", .{value}) catch "?",
            3 => std.fmt.bufPrint(buf, "{d:.3}", .{value}) catch "?",
            4 => std.fmt.bufPrint(buf, "{d:.4}", .{value}) catch "?",
            5 => std.fmt.bufPrint(buf, "{d:.5}", .{value}) catch "?",
            else => std.fmt.bufPrint(buf, "{d:.6}", .{value}) catch "?",
        };
    }

    /// Render grid overlay with lines and labels
    /// panel_x and panel_y are the absolute screen coordinates of the panel origin
    fn renderGridOverlay(self: *GraphRenderer, width: f32, height: f32, panel_x: f32, panel_y: f32) void {
        const font = dvui.themeGet().font_body.larger(-2);

        // Get axis ranges
        const axis1_min = self.axis_state.axis1.start;
        const axis1_max = self.axis_state.axis1.end;
        const axis1_range = axis1_max - axis1_min;

        const axis2_min = self.axis_state.axis2.start;
        const axis2_max = self.axis_state.axis2.end;
        const axis2_range = axis2_max - axis2_min;

        // Calculate nice step sizes
        const axis1_step = calculateNiceStep(axis1_range, self.num_grid_lines_axis1);
        const axis2_step = calculateNiceStep(axis2_range, self.num_grid_lines_axis2);

        // Find first grid line position for axis1 (round up to nearest step)
        const axis1_first = @ceil(axis1_min / axis1_step) * axis1_step;

        // Find first grid line position for axis2 (round up to nearest step)
        const axis2_first = @ceil(axis2_min / axis2_step) * axis2_step;

        // Draw vertical grid lines (constant X values) at nice numbers
        var axis1_value = axis1_first;
        var line_count: usize = 0;
        const max_lines: usize = 50; // Safety limit

        while (axis1_value <= axis1_max and line_count < max_lines) : ({
            axis1_value += axis1_step;
            line_count += 1;
        }) {
            // Convert world coordinate to screen coordinate
            const norm_x = (axis1_value - axis1_min) / axis1_range;
            const local_x = @as(f32, @floatCast(norm_x)) * width;

            // Skip if outside visible area (with small margin)
            if (local_x < -1 or local_x > width + 1) continue;

            // Offset by panel position to get absolute screen coordinates
            const screen_x = panel_x + local_x;
            const screen_y_start = panel_y;
            const screen_y_end = panel_y + height;

            // Draw vertical line
            const line_start = dvui.Point.Physical{ .x = screen_x, .y = screen_y_start };
            const line_end = dvui.Point.Physical{ .x = screen_x, .y = screen_y_end };
            dvui.Path.stroke(.{ .points = &.{ line_start, line_end } }, .{
                .color = GRID_LINE_COLOR,
                .thickness = 1,
            });

            // Draw label at bottom right of line
            var label_buf: [32]u8 = undefined;
            const label_text = formatGridLabel(&label_buf, axis1_value, axis1_step);
            const label_size = font.textSize(label_text);

            const label_x = screen_x + GRID_LABEL_PADDING;
            const label_y = panel_y + height - label_size.h - GRID_LABEL_PADDING;

            const label_rs = dvui.RectScale{
                .r = dvui.Rect.Physical.fromPoint(.{ .x = label_x, .y = label_y }).toSize(label_size.scale(1.0, dvui.Size.Physical)),
                .s = 1.0,
            };

            dvui.renderText(.{
                .font = font,
                .text = label_text,
                .rs = label_rs,
                .color = GRID_LABEL_COLOR,
            }) catch {};
        }

        // Draw horizontal grid lines (constant Y values) at nice numbers
        var axis2_value = axis2_first;
        line_count = 0;

        while (axis2_value <= axis2_max and line_count < max_lines) : ({
            axis2_value += axis2_step;
            line_count += 1;
        }) {
            // Convert world coordinate to screen coordinate (Y is inverted)
            const norm_y = (axis2_value - axis2_min) / axis2_range;
            const local_y = height - (@as(f32, @floatCast(norm_y)) * height);

            // Skip if outside visible area (with small margin)
            if (local_y < -1 or local_y > height + 1) continue;

            // Offset by panel position to get absolute screen coordinates
            const screen_y = panel_y + local_y;
            const screen_x_start = panel_x;
            const screen_x_end = panel_x + width;

            // Draw horizontal line
            const line_start = dvui.Point.Physical{ .x = screen_x_start, .y = screen_y };
            const line_end = dvui.Point.Physical{ .x = screen_x_end, .y = screen_y };
            dvui.Path.stroke(.{ .points = &.{ line_start, line_end } }, .{
                .color = GRID_LINE_COLOR,
                .thickness = 1,
            });

            // Draw label at left side above the line
            var label_buf: [32]u8 = undefined;
            const label_text = formatGridLabel(&label_buf, axis2_value, axis2_step);
            const label_size = font.textSize(label_text);

            const label_x = panel_x + GRID_LABEL_PADDING;
            const label_y = screen_y - label_size.h - GRID_LABEL_PADDING;

            const label_rs = dvui.RectScale{
                .r = dvui.Rect.Physical.fromPoint(.{ .x = label_x, .y = label_y }).toSize(label_size.scale(1.0, dvui.Size.Physical)),
                .s = 1.0,
            };

            dvui.renderText(.{
                .font = font,
                .text = label_text,
                .rs = label_rs,
                .color = GRID_LABEL_COLOR,
            }) catch {};
        }

        // Display hover coordinates in top-right corner if available
        if (self.hover_x != null and self.hover_y != null) {
            var hover_buf: [64]u8 = undefined;
            const hover_text = std.fmt.bufPrint(&hover_buf, "({d:.3}, {d:.3})", .{ self.hover_x.?, self.hover_y.? }) catch "";
            const hover_size = font.textSize(hover_text);

            const hover_x = panel_x + width - hover_size.w - GRID_LABEL_PADDING * 2;
            const hover_y = panel_y + GRID_LABEL_PADDING;

            const hover_rs = dvui.RectScale{
                .r = dvui.Rect.Physical.fromPoint(.{ .x = hover_x, .y = hover_y }).toSize(hover_size.scale(1.0, dvui.Size.Physical)),
                .s = 1.0,
            };

            dvui.renderText(.{
                .font = font,
                .text = hover_text,
                .rs = hover_rs,
                .color = dvui.Color.fromHex("#ffaa00"),
            }) catch {};
        }
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

    /// Determine which zone the mouse is in based on position
    fn detectMouseZone(x: f32, y: f32, _: f32, panel_height: f32) MouseZone {
        const in_left_zone = x < AXIS_ZONE_SIZE;
        const in_bottom_zone = y > (panel_height - AXIS_ZONE_SIZE);

        // Corner: prefer axis1 (bottom) if in both zones
        if (in_bottom_zone) return .axis1_edge;
        if (in_left_zone) return .axis2_edge;
        return .center;
    }

    /// Handle mouse move in the render area
    /// Call this with coordinates relative to the render panel
    pub fn handleMouseMove(self: *GraphRenderer, x: f32, y: f32, panel_width: f32, panel_height: f32) void {
        // Convert to world coordinates (full panel dimensions)
        const world = self.axis_state.screenToWorld(x, y, panel_width, panel_height);
        self.hover_x = world.x;
        self.hover_y = world.y;

        // Update current mouse zone for cursor and zoom behavior
        self.current_mouse_zone = detectMouseZone(x, y, panel_width, panel_height);

        // Handle dragging for pan
        if (self.is_dragging) {
            const dx = (x - self.last_mouse_x) / panel_width;
            const dy = (y - self.last_mouse_y) / panel_height;

            // Pan in world coordinates (based on drag zone)
            // Note: panBy and pan already handle coordinate system conversions
            switch (self.current_mouse_zone) {
                .center => {
                    const pan_x = dx * self.axis_state.axis1.span();
                    const pan_y = dy * self.axis_state.axis2.span();
                    self.axis_state.panBy(pan_x, pan_y);
                },
                .axis1_edge => {
                    // Drag on X-axis zone: pan only X
                    const pan_x = dx * self.axis_state.axis1.span();
                    self.axis_state.axis1.pan(-pan_x);
                },
                .axis2_edge => {
                    // Drag on Y-axis zone: pan only Y
                    const pan_y = dy * self.axis_state.axis2.span();
                    self.axis_state.axis2.pan(-pan_y);
                },
            }

            // Trigger shader re-render with new axis bounds
            self.needs_view_update = true;
        }

        self.last_mouse_x = x;
        self.last_mouse_y = y;
    }

    /// Get the appropriate cursor for the current mouse zone
    pub fn getCursor(self: *const GraphRenderer) dvui.enums.Cursor {
        if (self.is_dragging) {
            return switch (self.current_mouse_zone) {
                .center => .arrow_all,
                .axis1_edge => .arrow_w_e, // Horizontal (west-east)
                .axis2_edge => .arrow_n_s, // Vertical (north-south)
            };
        }
        return switch (self.current_mouse_zone) {
            .center => .arrow,
            .axis1_edge => .arrow_w_e, // Horizontal (west-east)
            .axis2_edge => .arrow_n_s, // Vertical (north-south)
        };
    }

    /// Handle mouse button down
    pub fn handleMouseDown(self: *GraphRenderer, x: f32, y: f32, _: f32, _: f32) void {
        self.is_dragging = true;
        self.last_mouse_x = x;
        self.last_mouse_y = y;
    }

    /// Handle mouse button up
    pub fn handleMouseUp(self: *GraphRenderer) void {
        self.is_dragging = false;
    }

    /// Handle mouse scroll for zoom
    pub fn handleScroll(self: *GraphRenderer, delta: f32, x: f32, y: f32, panel_width: f32, panel_height: f32) void {
        // Detect mouse zone based on scroll position
        const mouse_zone = detectMouseZone(x, y, panel_width, panel_height);

        // Convert to world coordinates (full panel dimensions)
        const world = self.axis_state.screenToWorld(x, y, panel_width, panel_height);

        // Zoom factor (positive delta = zoom in)
        const factor: f64 = if (delta > 0) 0.9 else 1.1;

        // Zoom based on current mouse zone
        switch (mouse_zone) {
            .center => {
                // Zoom both axes
                self.axis_state.zoomAt(factor, world.x, world.y);
            },
            .axis1_edge => {
                // Zoom only X axis (axis1)
                self.axis_state.axis1.zoom(factor, world.x);
            },
            .axis2_edge => {
                // Zoom only Y axis (axis2)
                self.axis_state.axis2.zoom(factor, world.y);
            },
        }

        // Trigger shader re-render with new axis bounds
        self.needs_view_update = true;
    }

    /// Reset axis ranges to defaults
    pub fn resetAxisRanges(self: *GraphRenderer) void {
        self.axis_state = .{};
        self.needs_view_update = true;
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

    /// Set number of grid lines for axis1 (X-axis, vertical lines)
    pub fn setNumGridLinesAxis1(self: *GraphRenderer, count: usize) void {
        self.num_grid_lines_axis1 = count;
    }

    /// Set number of grid lines for axis2 (Y-axis, horizontal lines)
    pub fn setNumGridLinesAxis2(self: *GraphRenderer, count: usize) void {
        self.num_grid_lines_axis2 = count;
    }

    /// Get number of grid lines for axis1
    pub fn getNumGridLinesAxis1(self: *GraphRenderer) usize {
        return self.num_grid_lines_axis1;
    }

    /// Get number of grid lines for axis2
    pub fn getNumGridLinesAxis2(self: *GraphRenderer) usize {
        return self.num_grid_lines_axis2;
    }
};
