const std = @import("std");
const dvui = @import("dvui");
const SDLBackend = @import("sdl3gpu");

const ui = @import("ui/ui.zig");
const session = @import("session/session.zig");
const renderer = @import("renderer/renderer.zig");

pub const App = struct {
    allocator: std.mem.Allocator,
    session_manager: session.SessionManager,
    graph_renderer: renderer.GraphRenderer,

    pub fn init(allocator: std.mem.Allocator) !App {
        var session_manager = session.SessionManager.init(allocator);
        errdefer session_manager.deinit();

        // Create initial session
        try session_manager.createSession("Untitled");

        return .{
            .allocator = allocator,
            .session_manager = session_manager,
            .graph_renderer = renderer.GraphRenderer.init(allocator),
        };
    }

    pub fn deinit(self: *App) void {
        ui.views.deinit();
        self.graph_renderer.deinit();
        self.session_manager.deinit();
    }

    pub fn run(self: *App) !void {
        SDLBackend.enableSDLLogging();

        // Init SDL backend (creates OS window)
        var backend = try SDLBackend.initWindow(.{
            .allocator = self.allocator,
            .title = "Logos",
            .size = .{ .w = 1400, .h = 900 },
            .min_size = .{ .w = 800, .h = 600 },
            .vsync = true,
        });
        defer backend.deinit();

        // Init dvui Window
        var win = try dvui.Window.init(@src(), self.allocator, backend.backend(), .{
            .theme = switch (backend.preferredColorScheme() orelse .dark) {
                .light => dvui.Theme.builtin.adwaita_light,
                .dark => dvui.Theme.builtin.adwaita_dark,
            },
        });
        defer win.deinit();

        // Initialize graph renderer with the backend now that SDL is ready
        self.graph_renderer.initWithDevice(&backend);

        main_loop: while (true) {
            // Continuous rendering - get current time directly
            const nstime = std.time.nanoTimestamp();

            // Begin dvui frame
            try win.begin(nstime);

            // Poll SDL events without blocking
            _ = try backend.addAllEvents(&win);

            // Render the main UI
            const keep_running = ui.views.mainView(self);
            if (!keep_running) break :main_loop;

            // Update graph renderer with current session's content
            if (self.session_manager.activeSession()) |active| {
                self.graph_renderer.update(active);
            }

            // End dvui frame
            _ = try win.end(.{});

            // Cursor management
            try backend.setCursor(win.cursorRequested());
            try backend.textInputRect(win.textInputRequested());

            // Render to screen
            try backend.renderPresent();
        }

        // Release GPU resources before backend is destroyed
        // This must happen while SDL device is still valid
        self.graph_renderer.releaseGpuResources();
    }
};
