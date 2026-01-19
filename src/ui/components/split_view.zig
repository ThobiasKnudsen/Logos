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

            // Handle mouse events for pan/zoom/hover
            self.handleRenderPanelMouse(graph_renderer, right, panel_width, panel_height);

            // Render the graph
            graph_renderer.renderToPanel(panel_width, panel_height);
        }

        // Draw custom separator line over the handle area (full height)
        drawSeparatorLine(paned);
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
