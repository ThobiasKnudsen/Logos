const std = @import("std");
const dvui = @import("dvui");
const SDLBackend = @import("sdl3gpu");

const ui = @import("ui/ui.zig");
const theme = @import("ui/theme.zig");
const session = @import("session/session.zig");
const renderer = @import("renderer/renderer.zig");

pub const App = struct {
    allocator: std.mem.Allocator,
    session_manager: session.SessionManager,
    graph_renderer: renderer.GraphRenderer,

    pub fn init(allocator: std.mem.Allocator) !App {
        // Load syntax theme from .logos.theme.json (creates file if missing)
        theme.syntax.init();

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

        // Register DejaVu Sans fonts for broad Unicode coverage
        // (Greek π, arrows →, math ≤≥≠, Latin Extended ŋ, and more)
        try win.addFont("DejaVu Sans", @embedFile("fonts/DejaVuSans.ttf"), null);
        try win.addFont("DejaVu Sans Mono", @embedFile("fonts/DejaVuSansMono.ttf"), null);

        // Override theme fonts to use DejaVu Sans
        const body_font: dvui.Font = .find(.{ .family = "DejaVu Sans" });
        const mono_font: dvui.Font = .find(.{ .family = "DejaVu Sans Mono" });
        win.theme.font_body = body_font;
        win.theme.font_heading = body_font;
        win.theme.font_title = body_font.withSize(20);
        win.theme.font_mono = mono_font;

        // Initialize graph renderer with the backend now that SDL is ready
        self.graph_renderer.initWithDevice(&backend);

        var interrupted = false;
        var debug_logged = false;

        main_loop: while (true) {
            // beginWait coordinates with waitTime below to run frames only when needed
            const nstime = win.beginWait(interrupted);

            // Begin dvui frame
            try win.begin(nstime);

            // One-time font debug: verify glyph rendering for Unicode characters
            if (!debug_logged) {
                debug_logged = true;
                const theme_mono = dvui.themeGet().font_mono;
                std.log.info("FONT DEBUG: mono family='{s}' size={d}", .{
                    dvui.Font.string(&theme_mono.family),
                    @as(u32, @intFromFloat(theme_mono.size)),
                });

                // Test if the font can actually render target codepoints
                if (win.fonts.getOrCreate(self.allocator, theme_mono)) |entry| {
                    const test_codepoints = [_]struct { cp: u32, name: []const u8 }{
                        .{ .cp = 0x41, .name = "A (ASCII)" },
                        .{ .cp = 0x03C0, .name = "π (Greek pi)" },
                        .{ .cp = 0x2192, .name = "→ (right arrow)" },
                        .{ .cp = 0x014B, .name = "ŋ (eng)" },
                        .{ .cp = 0x2265, .name = "≥ (gte)" },
                        .{ .cp = 0x2264, .name = "≤ (lte)" },
                        .{ .cp = 0x2260, .name = "≠ (neq)" },
                    };
                    for (test_codepoints) |tc| {
                        if (entry.glyphInfoGetOrReplacement(self.allocator, tc.cp)) |gi| {
                            std.log.info("FONT DEBUG: {s} advance={d} w={d} h={d}", .{
                                tc.name,
                                @as(u32, @intFromFloat(gi.advance)),
                                @as(u32, @intFromFloat(gi.w)),
                                @as(u32, @intFromFloat(gi.h)),
                            });
                        } else |err| {
                            std.log.err("FONT DEBUG: {s} ERROR: {}", .{ tc.name, err });
                        }
                    }
                } else |err| {
                    std.log.err("FONT DEBUG: getOrCreate failed: {}", .{err});
                }
            }

            // Poll SDL events without blocking
            _ = try backend.addAllEvents(&win);

            // Render the main UI
            const keep_running = ui.views.mainView(self);
            if (!keep_running) break :main_loop;

            // Update graph renderer with current session's content
            if (self.session_manager.activeSession()) |active| {
                self.graph_renderer.update(active);
            }

            // If the graph renderer is animating, request continuous redraws
            if (self.graph_renderer.is_animating) {
                win.refreshWindow(@src(), null);
            }

            // End dvui frame - returns wait time hint (null = wait indefinitely)
            const end_micros = try win.end(.{});

            // Cursor management
            try backend.setCursor(win.cursorRequested());
            try backend.textInputRect(win.textInputRequested());

            // Render to screen
            try backend.renderPresent();

            // Wait for events or timeout — sleeps when idle, spins when animating
            const wait_event_micros = win.waitTime(end_micros);
            interrupted = try backend.waitEventTimeout(wait_event_micros);
        }

        // Release GPU resources before backend is destroyed
        // This must happen while SDL device is still valid
        self.graph_renderer.releaseGpuResources();
    }
};
