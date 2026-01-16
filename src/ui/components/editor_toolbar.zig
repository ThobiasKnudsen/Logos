//! Editor toolbar - contains play/pause buttons and controls
//!
//! Positioned above the text editor panel. Controls lexing/parsing
//! and shader compilation.

const std = @import("std");
const dvui = @import("dvui");
const theme = @import("../theme.zig");
const session = @import("../../session/session.zig");
const lexer_mod = @import("../../lang/lexer.zig");
const parser_mod = @import("../../lang/parser.zig");
const glsl_gen = @import("../../lang/glsl_gen.zig");

// Entypo icons from dvui
const entypo = dvui.entypo;

pub const EditorToolbar = struct {
    /// Playback state
    state: PlayState = .stopped,

    /// Shared lexer instance (initialized on first use)
    lexer: ?lexer_mod.Lexer = null,

    /// Last lexing error message (if any)
    lex_error: ?[]const u8 = null,

    const PlayState = enum {
        stopped, // Show play button, stop is grey
        running, // Show pause button, stop is red
        paused, // Show continue button, stop is red
    };

    const base_toolbar_height: f32 = 32;
    const base_icon_size: f32 = 18;
    const button_padding: f32 = 4;

    fn getToolbarHeight() f32 {
        return base_toolbar_height * theme.fonts.getScale();
    }

    fn getIconSize() f32 {
        return base_icon_size * theme.fonts.getScale();
    }

    // Colors
    const stop_red = dvui.Color{ .r = 220, .g = 60, .b = 60, .a = 255 };
    const stop_red_hover = dvui.Color{ .r = 240, .g = 80, .b = 80, .a = 255 };
    const pause_orange = dvui.Color{ .r = 230, .g = 160, .b = 60, .a = 255 };

    pub fn init() EditorToolbar {
        return .{};
    }

    pub fn deinit(self: *EditorToolbar) void {
        if (self.lexer) |*lex| {
            lex.deinit();
        }
    }

    /// Ensure the lexer is initialized
    fn ensureLexer(self: *EditorToolbar, allocator: std.mem.Allocator) !*lexer_mod.Lexer {
        if (self.lexer == null) {
            self.lexer = try lexer_mod.Lexer.init(allocator, lexer_mod.LexerConfig.logosPatterns());
        }
        return &self.lexer.?;
    }

    /// Render the toolbar and return whether play was clicked
    pub fn render(self: *EditorToolbar, active_session: *session.TabSession) bool {
        var play_clicked = false;

        // Get scaled sizes
        const toolbar_height = getToolbarHeight();
        const icon_size = getIconSize();
        const scale = theme.fonts.getScale();
        const corner_radius = theme.radius.sm * scale;

        // Outer container to hold toolbar + separator line
        var outer = dvui.box(@src(), .{ .dir = .vertical }, .{
            .expand = .horizontal,
        });
        defer outer.deinit();

        // Toolbar container
        var toolbar = dvui.box(@src(), .{ .dir = .horizontal }, .{
            .expand = .horizontal,
            .min_size_content = .{ .h = toolbar_height },
            .color_fill = theme.colors.toolbar_bg,
            .background = true,
            .padding = .{ .x = button_padding, .y = button_padding, .w = button_padding, .h = button_padding },
        });

        // Status indicator (left side)
        self.renderStatus(active_session);

        // Spacer to push buttons to the right
        {
            var fill = dvui.box(@src(), .{}, .{ .expand = .horizontal });
            fill.deinit();
        }

        // Play/Pause/Continue button
        const button_icon = switch (self.state) {
            .stopped => entypo.controller_play,
            .running => entypo.controller_pause,
            .paused => entypo.controller_next, // Triangle with bar - "continue/resume"
        };
        const button_name = switch (self.state) {
            .stopped => "play",
            .running => "pause",
            .paused => "continue",
        };
        const button_color = switch (self.state) {
            .stopped => theme.colors.toolbar_button_active, // green
            .running => pause_orange,
            .paused => theme.colors.accent_info, // blue for continue
        };

        if (dvui.buttonIcon(
            @src(),
            button_name,
            button_icon,
            .{},
            .{},
            .{
                .color_fill = button_color,
                .color_fill_hover = theme.colors.toolbar_button_hover,
                .corner_radius = .{ .x = corner_radius, .y = corner_radius, .w = corner_radius, .h = corner_radius },
                .padding = .{ .x = 4, .y = 4, .w = 4, .h = 4 },
                .margin = .{ .x = 2, .y = 2, .w = 2, .h = 2 },
                .min_size_content = .{ .w = icon_size, .h = icon_size },
            },
        )) {
            switch (self.state) {
                .stopped => {
                    // Start running
                    self.state = .running;
                    play_clicked = true;
                    self.tokenizeContent(active_session);
                },
                .running => {
                    // Pause
                    self.state = .paused;
                },
                .paused => {
                    // Continue running
                    self.state = .running;
                },
            }
        }

        // Stop button - circular, red when running/paused, grey when stopped
        const is_active = self.state != .stopped;
        const stop_color = if (is_active) stop_red else theme.colors.toolbar_button;
        const stop_hover = if (is_active) stop_red_hover else theme.colors.toolbar_button_hover;
        const stop_radius = 12 * scale;

        if (dvui.buttonIcon(
            @src(),
            "stop",
            entypo.controller_stop,
            .{},
            .{},
            .{
                .color_fill = stop_color,
                .color_fill_hover = stop_hover,
                // Circular button (high corner radius)
                .corner_radius = .{ .x = stop_radius, .y = stop_radius, .w = stop_radius, .h = stop_radius },
                .padding = .{ .x = 4, .y = 4, .w = 4, .h = 4 },
                .margin = .{ .x = 2, .y = 2, .w = 2, .h = 2 },
                .min_size_content = .{ .w = icon_size, .h = icon_size },
            },
        )) {
            self.state = .stopped;
        }

        // Close toolbar before drawing separator
        toolbar.deinit();

        // Bottom separator line
        renderBottomSeparator();

        return play_clicked;
    }

    /// Renders a horizontal separator line at the bottom of the toolbar
    fn renderBottomSeparator() void {
        var sep = dvui.box(@src(), .{}, .{
            .min_size_content = .{ .h = 1 },
            .expand = .horizontal,
            .color_fill = theme.colors.border,
            .background = true,
        });
        sep.deinit();
    }

    fn renderStatus(self: *EditorToolbar, active_session: *session.TabSession) void {
        _ = self;
        const parse_status = active_session.parse_state.status;
        const status_text = switch (parse_status) {
            .idle => "Ready",
            .lexing => "Lexing...",
            .parsing => "Parsing...",
            .ready => "OK",
            .err => "Error",
        };

        const status_color = switch (parse_status) {
            .idle => theme.colors.text_muted,
            .lexing, .parsing => theme.colors.accent_info,
            .ready => theme.colors.accent_secondary,
            .err => theme.colors.accent_primary,
        };

        // Get scaled font for toolbar
        const toolbar_font = dvui.Font.theme(.body).withSize(theme.fonts.smallSize());
        const icon_scale = theme.fonts.getScale();
        const scaled_icon_size: f32 = 14 * icon_scale;

        // Show status icon for ready/error states
        if (parse_status == .ready) {
            dvui.icon(@src(), "check", entypo.check, .{}, .{
                .color_text = theme.colors.accent_secondary,
                .min_size_content = .{ .w = scaled_icon_size, .h = scaled_icon_size },
                .gravity_y = 0.5,
            });
        } else if (parse_status == .err) {
            dvui.icon(@src(), "error", entypo.circle_with_cross, .{}, .{
                .color_text = theme.colors.accent_primary,
                .min_size_content = .{ .w = scaled_icon_size, .h = scaled_icon_size },
                .gravity_y = 0.5,
            });
        }

        dvui.labelNoFmt(@src(), status_text, .{}, .{
            .color_text = status_color,
            .font = toolbar_font,
            .padding = .{ .x = 4, .y = 4, .w = 4, .h = 4 },
            .gravity_y = 0.5,
        });

        // Show token count if available
        if (active_session.parse_state.tokens) |tokens| {
            var buf: [32]u8 = undefined;
            const count_text = std.fmt.bufPrint(&buf, "({d} tokens)", .{tokens.len}) catch "(?? tokens)";
            dvui.labelNoFmt(@src(), count_text, .{}, .{
                .color_text = theme.colors.text_muted,
                .font = toolbar_font,
                .padding = .{ .x = 4, .y = 4, .w = 4, .h = 4 },
                .gravity_y = 0.5,
            });
        }

        // Show AST status if available
        if (active_session.parse_state.ast != null and parse_status == .ready) {
            dvui.labelNoFmt(@src(), "(AST ready)", .{}, .{
                .color_text = theme.colors.accent_secondary,
                .font = toolbar_font,
                .padding = .{ .x = 4, .y = 4, .w = 4, .h = 4 },
                .gravity_y = 0.5,
            });
        }

        // Show error message if any
        if (active_session.parse_state.error_message) |err_msg| {
            dvui.labelNoFmt(@src(), err_msg, .{}, .{
                .color_text = theme.colors.accent_primary,
                .font = toolbar_font,
                .padding = .{ .x = 4, .y = 4, .w = 4, .h = 4 },
                .gravity_y = 0.5,
            });
        }
    }

    /// Tokenize the content and store tokens in parse state
    fn tokenizeContent(self: *EditorToolbar, active_session: *session.TabSession) void {
        const content = active_session.content.items;

        // Start new parse cycle
        active_session.parse_state.startNewParse();

        if (content.len == 0) {
            active_session.parse_state.freeTokens();
            active_session.parse_state.status = .ready;
            return;
        }

        // Get or create lexer
        const lex = self.ensureLexer(active_session.allocator) catch |err| {
            std.log.err("Failed to init lexer: {}", .{err});
            active_session.parse_state.status = .err;
            active_session.parse_state.error_message = "Failed to initialize lexer";
            return;
        };

        // Update status
        active_session.parse_state.status = .lexing;

        // Free previous tokens
        active_session.parse_state.freeTokens();

        // Tokenize
        const tokens = lex.tokenize(content) catch |err| {
            std.log.err("Lexer error: {}", .{err});
            active_session.parse_state.status = .err;
            active_session.parse_state.error_message = "Lexer error";
            return;
        };

        active_session.parse_state.tokens = tokens;

        std.log.info("Tokenized {} bytes into {} tokens", .{ content.len, tokens.len });

        // Now parse the tokens into an AST
        self.parseTokens(active_session);
    }

    /// Parse tokens into an AST
    fn parseTokens(self: *EditorToolbar, active_session: *session.TabSession) void {
        const tokens = active_session.parse_state.tokens orelse {
            active_session.parse_state.status = .err;
            active_session.parse_state.error_message = "No tokens to parse";
            return;
        };

        if (tokens.len == 0) {
            active_session.parse_state.status = .ready;
            return;
        }

        // DEBUG: Print all tokens
        debugPrintTokens(tokens, active_session.content.items);

        active_session.parse_state.status = .parsing;

        // Use the parse arena for AST allocation
        const arena_allocator = active_session.parse_state.parse_arena.allocator();

        // Create parser and parse
        var parser = parser_mod.Parser.init(arena_allocator, tokens);
        defer parser.deinit();

        const ast = parser.parse() catch |err| {
            std.log.err("Parse error: {}", .{err});
            active_session.parse_state.status = .err;

            // Get first error message if available
            if (parser.errors.items.len > 0) {
                const first_err = parser.errors.items[0];

                // Format error with line/column info
                const content = active_session.content.items;
                const loc = session.parse_state.byteOffsetToLocation(content, first_err.byte_start);

                // Create formatted message with location
                const formatted = std.fmt.allocPrint(
                    active_session.parse_state.parse_arena.allocator(),
                    "{s} (Ln {d}, Col {d})",
                    .{ first_err.message, loc.line, loc.column },
                ) catch first_err.message;

                active_session.parse_state.error_message = formatted;

                // DEBUG: Print error context
                std.debug.print("\n========== PARSE ERROR ==========\n", .{});
                std.debug.print("Error: {s}\n", .{first_err.message});
                std.debug.print("At byte offset: {d}-{d}\n", .{ first_err.byte_start, first_err.byte_end });
                std.debug.print("Location: Ln {d}, Col {d}\n", .{ loc.line, loc.column });

                // Show the problematic token/area
                if (first_err.byte_start < content.len) {
                    const context_start = if (first_err.byte_start > 20) first_err.byte_start - 20 else 0;
                    const context_end = @min(first_err.byte_end + 20, content.len);
                    std.debug.print("Context: ...{s}...\n", .{content[context_start..context_end]});
                }
                std.debug.print("=================================\n\n", .{});

                // Copy errors to parse state
                for (parser.errors.items) |parse_err| {
                    active_session.parse_state.errors.append(active_session.allocator, parse_err) catch {};
                }
            } else {
                active_session.parse_state.error_message = "Parse error";
            }
            return;
        };

        // Store the AST
        active_session.parse_state.ast = ast;

        std.log.info("Parsed {} tokens into AST", .{tokens.len});

        // DEBUG: Print AST when play is clicked
        ast.debugPrintTree();

        // Generate GLSL shaders
        self.generateGlsl(active_session);
    }

    /// Generate GLSL shaders from the AST
    fn generateGlsl(_: *EditorToolbar, active_session: *session.TabSession) void {
        const ast = active_session.parse_state.ast orelse {
            active_session.parse_state.status = .err;
            active_session.parse_state.error_message = "No AST to generate GLSL from";
            return;
        };

        // Free any previously generated shaders
        active_session.parse_state.freeGeneratedShaders();

        // Generate GLSL
        var generator = glsl_gen.GlslGenerator.init(active_session.allocator);
        defer generator.deinit();

        const result = generator.generate(ast) catch |err| {
            std.log.err("GLSL generation error: {}", .{err});
            active_session.parse_state.status = .err;
            active_session.parse_state.error_message = "GLSL generation failed";
            return;
        };

        if (result.shaders.len == 0) {
            std.log.info("No root outputs found - nothing to render", .{});
            active_session.parse_state.status = .ready;
            active_session.parse_state.error_message = "No outputs to render";
            return;
        }

        // Store the generated shaders
        active_session.parse_state.generated_shaders = result.shaders;
        active_session.parse_state.status = .ready;

        std.log.info("Generated {} GLSL fragment shader(s)", .{result.shaders.len});

        // DEBUG: Print generated shaders
        debugPrintShaders(result.shaders);
    }

    /// Debug print all generated shaders
    fn debugPrintShaders(shaders: []const glsl_gen.GeneratedShader) void {
        std.debug.print("\n========== GENERATED GLSL SHADERS ==========\n", .{});
        std.debug.print("Total shaders: {d}\n\n", .{shaders.len});

        for (shaders, 0..) |shader, i| {
            std.debug.print("--- Shader {d} ({s}) ---\n", .{
                i,
                @tagName(shader.output_type),
            });
            std.debug.print("{s}\n", .{shader.source});
            std.debug.print("--- End Shader {d} ---\n\n", .{i});
        }
        std.debug.print("=============================================\n\n", .{});
    }

    /// Debug print all tokens with their types and positions
    fn debugPrintTokens(tokens: []const session.parse_state.Token, content: []const u8) void {
        std.debug.print("\n========== TOKENS DEBUG ==========\n", .{});
        std.debug.print("Total tokens: {d}\n\n", .{tokens.len});

        for (tokens, 0..) |token, i| {
            const loc = session.parse_state.byteOffsetToLocation(content, token.byte_start);

            // Skip whitespace for cleaner output
            if (token.token_type == .whitespace) continue;

            // Escape newlines in token text for display
            var display_text: [64]u8 = undefined;
            var display_len: usize = 0;
            for (token.text) |c| {
                if (display_len >= 60) {
                    display_text[display_len] = '.';
                    display_len += 1;
                    display_text[display_len] = '.';
                    display_len += 1;
                    display_text[display_len] = '.';
                    display_len += 1;
                    break;
                }
                if (c == '\n') {
                    display_text[display_len] = '\\';
                    display_len += 1;
                    display_text[display_len] = 'n';
                    display_len += 1;
                } else if (c == '\t') {
                    display_text[display_len] = '\\';
                    display_len += 1;
                    display_text[display_len] = 't';
                    display_len += 1;
                } else {
                    display_text[display_len] = c;
                    display_len += 1;
                }
            }

            std.debug.print("[{d:4}] {s:12} Ln{d:3}:Col{d:3}  \"{s}\"\n", .{
                i,
                @tagName(token.token_type),
                loc.line,
                loc.column,
                display_text[0..display_len],
            });
        }
        std.debug.print("==================================\n\n", .{});
    }
};
