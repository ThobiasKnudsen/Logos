//! Graph/Plot renderer
//!
//! Renders visualizations based on parsed user input.
//! Uses dvui for display, with SDL3 for custom rendering.

const std = @import("std");
const dvui = @import("dvui");

const session = @import("../session/session.zig");
const theme = @import("../ui/theme.zig");

pub const GraphRenderer = struct {
    allocator: std.mem.Allocator,

    /// Cached render data
    cached_content_hash: u64,

    pub fn init(allocator: std.mem.Allocator) GraphRenderer {
        return .{
            .allocator = allocator,
            .cached_content_hash = 0,
        };
    }

    pub fn deinit(self: *GraphRenderer) void {
        _ = self;
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
    }

    /// Render the graph to the dvui panel
    pub fn renderToPanel(self: *GraphRenderer, width: f32, height: f32) void {
        _ = self;
        _ = width;
        _ = height;

        // Placeholder content using dvui
        var center = dvui.box(@src(), .{ .dir = .vertical }, .{
            .expand = .both,
        });
        defer center.deinit();

        dvui.labelNoFmt(@src(), "Graph Renderer", .{}, .{
            .font = .theme(.title),
            .margin = .{ .x = 0, .y = 20, .w = 0, .h = 10 },
        });
        dvui.labelNoFmt(@src(), "Custom rendering area", .{}, .{});
        dvui.labelNoFmt(@src(), "", .{}, .{ .margin = .{ .y = 10 } });
        dvui.labelNoFmt(@src(), "Write expressions in the editor", .{}, .{});
        dvui.labelNoFmt(@src(), "to see visualizations here", .{}, .{});
    }
};
