//! Resizable split view - divides space between editor and graph renderer
//!
//! Left panel: Text editor with toolbar
//! Right panel: Graph/plot rendering with custom shaders

const std = @import("std");
const dvui = @import("dvui");
const theme = @import("../theme.zig");
const session = @import("../../session/session.zig");
const renderer = @import("../../renderer/renderer.zig");

const EditorPanel = @import("editor_panel.zig").EditorPanel;
const EditorToolbar = @import("editor_toolbar.zig").EditorToolbar;

pub const SplitView = struct {
    /// Split ratio (0.0 to 1.0) - portion of width for left panel
    split_ratio: f32 = 0.45,

    /// Absolute divider position in pixels (null = use split_ratio)
    divider_position: ?f32 = null,

    /// Last known container width for detecting resizes
    last_container_width: ?f32 = null,

    /// Last known split ratio for detecting user drags
    last_split_ratio: f32 = 0.45,

    /// Last known render panel dimensions for aspect ratio adjustment
    last_render_width: ?f32 = null,
    last_render_height: ?f32 = null,

    /// Reference scale for each axis (maintained across resizes)
    reference_pixels_per_unit_x: ?f64 = null,
    reference_pixels_per_unit_y: ?f64 = null,

    /// Editor toolbar state
    editor_toolbar: EditorToolbar = EditorToolbar.init(),

    const handle_width: f32 = 6;
    const separator_color = dvui.Color{ .r = 60, .g = 70, .b = 85, .a = 255 };
    const separator_hover_color = dvui.Color{ .r = 99, .g = 130, .b = 170, .a = 255 };

    pub fn deinit(self: *SplitView) void {
        self.editor_toolbar.deinit();
    }

    pub fn render(
        self: *SplitView,
        active_session: *session.TabSession,
        graph_renderer: *renderer.GraphRenderer,
    ) void {
        // Use dvui's paned widget for split view
        var paned = dvui.paned(@src(), .{
            .direction = .horizontal,
            .collapsed_size = 200,
            .split_ratio = &self.split_ratio,
            .handle_size = handle_width,
            .handle_margin = 0,
        }, .{
            .expand = .both,
        });
        defer paned.deinit();

        // Get container dimensions from the paned widget
        const container_width = paned.data().contentRectScale().r.w;

        // Detect what changed: window resize vs user drag
        const width_changed = if (self.last_container_width) |last_width|
            @abs(container_width - last_width) > 1.0
        else
            false;

        const ratio_changed = @abs(self.split_ratio - self.last_split_ratio) > 0.001;

        if (width_changed and !ratio_changed) {
            // Window resized but user didn't drag - maintain absolute divider position
            if (self.divider_position) |div_pos| {
                if (container_width > 0) {
                    self.split_ratio = @max(0.1, @min(0.9, div_pos / container_width));
                }
            }
        }

        // Update tracked values (always update divider position from current split_ratio)
        self.divider_position = self.split_ratio * container_width;
        self.last_container_width = container_width;
        self.last_split_ratio = self.split_ratio;

        // Left panel - Editor with toolbar (first pane)
        if (paned.showFirst()) {
            var left = dvui.box(@src(), .{ .dir = .vertical }, .{
                .expand = .both,
                .color_fill = dvui.Color{ .r = 25, .g = 30, .b = 38, .a = 255 },
                .background = true,
            });
            defer left.deinit();

            // Toolbar at top - pass graph_renderer so Play button can trigger rendering
            _ = self.editor_toolbar.render(active_session, graph_renderer);

            // Editor panel below toolbar
            EditorPanel.render(active_session);
        }

        // Right panel - Graph renderer (second pane)
        if (paned.showSecond()) {
            var right = dvui.box(@src(), .{ .dir = .vertical }, .{
                .expand = .both,
                .color_fill = dvui.Color{ .r = 20, .g = 24, .b = 30, .a = 255 },
                .background = true,
            });
            defer right.deinit();

            // Get panel dimensions for rendering
            const rs = right.data().contentRectScale();
            const panel_width = rs.r.w;
            const panel_height = rs.r.h;

            // Adjust axis ranges to maintain aspect ratio (prevent shape distortion)
            self.adjustAxisRanges(graph_renderer, panel_width, panel_height);

            // Handle mouse events for pan/zoom/hover
            self.handleRenderPanelMouse(graph_renderer, right, panel_width, panel_height);

            // Render the graph
            graph_renderer.renderToPanel(panel_width, panel_height);
        }

        // Draw custom separator line over the handle area (full height)
        drawSeparatorLine(paned);
    }

    /// Adjust axis ranges when panel size changes to maintain scale without drift
    /// Preserves independent axis scaling (allows different zoom on X vs Y)
    fn adjustAxisRanges(
        self: *SplitView,
        graph_renderer: *renderer.GraphRenderer,
        panel_width: f32,
        panel_height: f32,
    ) void {
        // Skip if dimensions are invalid
        if (panel_width <= 0 or panel_height <= 0) return;

        // Check if panel size changed
        const panel_size_changed = if (self.last_render_width) |last_w|
            if (self.last_render_height) |last_h|
                @abs(last_w - panel_width) > 1.0 or @abs(last_h - panel_height) > 1.0
            else
                true
        else
            true;

        // Get current axis ranges
        var axis1 = graph_renderer.getAxis1();
        var axis2 = graph_renderer.getAxis2();

        const span1 = axis1.span();
        const span2 = axis2.span();

        // Calculate current pixels-per-unit for each axis
        const pixels_per_unit_x = @as(f64, panel_width) / span1;
        const pixels_per_unit_y = @as(f64, panel_height) / span2;

        // Initialize reference on first run
        if (self.reference_pixels_per_unit_x == null or self.reference_pixels_per_unit_y == null) {
            self.reference_pixels_per_unit_x = pixels_per_unit_x;
            self.reference_pixels_per_unit_y = pixels_per_unit_y;
            self.last_render_width = panel_width;
            self.last_render_height = panel_height;
            return; // Don't adjust on first run, just initialize
        }

        // If panel size changed, adjust axes to maintain their independent scales
        if (panel_size_changed) {
            const reference_scale_x = self.reference_pixels_per_unit_x.?;
            const reference_scale_y = self.reference_pixels_per_unit_y.?;

            // Calculate center points to maintain view center
            const center1 = (axis1.start + axis1.end) / 2.0;
            const center2 = (axis2.start + axis2.end) / 2.0;

            // Calculate new spans based on independent reference scales
            const new_span1 = @as(f64, panel_width) / reference_scale_x;
            const new_span2 = @as(f64, panel_height) / reference_scale_y;

            // Update axis ranges
            axis1.start = center1 - new_span1 / 2.0;
            axis1.end = center1 + new_span1 / 2.0;
            axis2.start = center2 - new_span2 / 2.0;
            axis2.end = center2 + new_span2 / 2.0;

            graph_renderer.setAxis1Range(axis1.start, axis1.end) catch {};
            graph_renderer.setAxis2Range(axis2.start, axis2.end) catch {};

            // Update tracked dimensions
            self.last_render_width = panel_width;
            self.last_render_height = panel_height;
        } else {
            // Panel size didn't change, but user may have zoomed
            // Update reference scales to reflect current zoom levels
            self.reference_pixels_per_unit_x = pixels_per_unit_x;
            self.reference_pixels_per_unit_y = pixels_per_unit_y;
        }
    }

    /// Handle mouse events in the render panel for pan/zoom/hover
    fn handleRenderPanelMouse(
        _: *SplitView,
        graph_renderer: *renderer.GraphRenderer,
        box_widget: *dvui.BoxWidget,
        panel_width: f32,
        panel_height: f32,
    ) void {
        const panel_rs = box_widget.data().contentRectScale();

        // Process all events and check if they match this widget
        const evts = dvui.events();
        for (evts) |*e| {
            // Only process events that match our widget
            if (!dvui.eventMatchSimple(e, box_widget.data()))
                continue;

            switch (e.evt) {
                .mouse => |me| {
                    // Convert to panel-local coordinates
                    const local_x = me.p.x - panel_rs.r.x;
                    const local_y = me.p.y - panel_rs.r.y;

                    switch (me.action) {
                        .position => {
                            // Update hover position and handle dragging
                            graph_renderer.handleMouseMove(local_x, local_y, panel_width, panel_height);
                            dvui.cursorSet(graph_renderer.getCursor());
                        },
                        .press => {
                            // Start dragging on left button press
                            if (me.button == .left) {
                                graph_renderer.handleMouseDown(local_x, local_y, panel_width, panel_height);
                                e.handled = true;
                            }
                        },
                        .release => {
                            // End dragging on button release
                            if (me.button == .left and graph_renderer.is_dragging) {
                                graph_renderer.handleMouseUp();
                                e.handled = true;
                            }
                        },
                        .wheel_y => |delta| {
                            // Handle zoom with mouse wheel
                            graph_renderer.handleScroll(delta, local_x, local_y, panel_width, panel_height);
                            e.handled = true;
                        },
                        else => {},
                    }
                },
                else => {},
            }
        }
    }

    fn drawSeparatorLine(paned: *dvui.PanedWidget) void {
        const rs = paned.data().contentRectScale();
        const handle_gap = paned.handleGap() * rs.s;

        // Calculate separator position (physical coordinates)
        const r = rs.r;
        const split_x = r.x + (r.w - handle_gap) * paned.split_ratio.* + handle_gap / 2 - 1;

        // Check if mouse is near the separator for hover effect
        const is_hovered = dvui.captured(paned.data().id) or paned.mouse_dist <= handle_width / 2;
        const color = if (is_hovered) separator_hover_color else separator_color;

        // Draw vertical line spanning full height (using Physical rect for rendering)
        const sep_rect: dvui.Rect.Physical = .{
            .x = split_x,
            .y = r.y,
            .w = 2,
            .h = r.h,
        };
        sep_rect.fill(.{}, .{ .color = color });
    }
};
