//! Resizable split view - divides space between editor and graph renderer
//!
//! Left panel: Text editor
//! Right panel: Graph/plot rendering with custom shaders

const std = @import("std");
const dvui = @import("dvui");
const theme = @import("../theme.zig");
const session = @import("../../session/session.zig");
const renderer = @import("../../renderer/renderer.zig");

const EditorPanel = @import("editor_panel.zig").EditorPanel;

pub const SplitView = struct {
    /// Split ratio (0.0 to 1.0) - portion of width for left panel
    split_ratio: f32 = 0.45,

    const handle_width: f32 = 6;
    const separator_color = dvui.Color{ .r = 60, .g = 70, .b = 85, .a = 255 };
    const separator_hover_color = dvui.Color{ .r = 99, .g = 130, .b = 170, .a = 255 };

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

        // Left panel - Editor (first pane)
        if (paned.showFirst()) {
            var left = dvui.box(@src(), .{ .dir = .vertical }, .{
                .expand = .both,
                .color_fill = dvui.Color{ .r = 25, .g = 30, .b = 38, .a = 255 },
                .background = true,
            });
            defer left.deinit();

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
            graph_renderer.renderToPanel(rs.r.w, rs.r.h);
        }

        // Draw custom separator line over the handle area (full height)
        drawSeparatorLine(paned);
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
