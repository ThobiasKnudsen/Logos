//! Text editor panel - left side of the split view
//!
//! Multi-line text area where users write their expressions/code
//! that gets parsed and visualized in the graph panel.
//! Includes syntax highlighting via manual tokenization.

const std = @import("std");
const dvui = @import("dvui");
const theme = @import("../theme.zig");
const session = @import("../../session/session.zig");
const lexer_mod = @import("../../lang/lexer.zig");

pub const EditorPanel = struct {
    // Shared styling constants - use theme where possible
    const gutter_width: f32 = 48;
    const content_padding_x: f32 = 8;
    const content_padding_y: f32 = 4;

    // Custom theme for text entry with transparent focus (to hide focus border)
    const no_focus_theme = blk: {
        var t = dvui.Theme.builtin.adwaita_dark;
        t.focus = dvui.Color{ .r = 0, .g = 0, .b = 0, .a = 0 }; // Transparent focus
        break :blk t;
    };

    // Global lexer instance (initialized lazily)
    // Uses page_allocator since it's a long-lived global resource
    var lexer: ?lexer_mod.Lexer = null;

    /// Initialize the global lexer if not already done
    fn ensureLexer(_: std.mem.Allocator) ?*lexer_mod.Lexer {
        if (lexer == null) {
            // Use page_allocator for the global lexer since it lives for the app lifetime
            lexer = lexer_mod.Lexer.init(std.heap.page_allocator, lexer_mod.LexerConfig.logosPatterns()) catch |err| {
                std.log.err("Failed to initialize lexer: {}", .{err});
                return null;
            };
        }
        return &lexer.?;
    }

    pub fn render(active_session: *session.TabSession) void {
        // Outer scroll area - handles scrolling for both line numbers and text
        var scroll = dvui.scrollArea(@src(), .{}, .{
            .expand = .both,
            .background = false,
        });
        defer scroll.deinit();

        // Content row containing line numbers and text entry
        var content_row = dvui.box(@src(), .{ .dir = .horizontal }, .{
            .expand = .both,
        });
        defer content_row.deinit();

        // Line number gutter
        renderLineNumbers(active_session);

        // Vertical separator line between gutter and text
        renderSeparator();

        // Main text area with syntax highlighting
        renderTextAreaWithHighlighting(active_session);
    }

    fn renderLineNumbers(active_session: *session.TabSession) void {
        // Get the mono font with current size from centralized theme
        const scaled_font = theme.fonts.editorFont();
        const line_height = scaled_font.lineHeight();

        var gutter = dvui.box(@src(), .{ .dir = .vertical }, .{
            .min_size_content = .{ .w = gutter_width },
            .padding = .{ .x = content_padding_x, .y = content_padding_y, .w = 4, .h = content_padding_y },
        });
        defer gutter.deinit();

        const line_count = countLines(active_session.content.items);
        for (1..line_count + 1) |line_num| {
            var buf: [16]u8 = undefined;
            const line_str = std.fmt.bufPrint(&buf, "{d: >3}", .{line_num}) catch "???";
            // Use mono font with scaled size and explicit min height matching TextLayoutWidget's line height
            dvui.labelNoFmt(@src(), line_str, .{}, .{
                .id_extra = line_num,
                .font = scaled_font,
                .color_text = theme.colors.text_muted, // Use theme color
                .padding = .{},
                .margin = .{},
                .min_size_content = .{ .h = line_height }, // Match TextLayoutWidget line height
            });
        }
    }

    fn renderSeparator() void {
        var sep = dvui.box(@src(), .{}, .{
            .min_size_content = .{ .w = 1 },
            .expand = .vertical,
            .color_fill = theme.colors.border, // Use theme color
            .background = true,
        });
        sep.deinit();
    }

    fn renderTextAreaWithHighlighting(active_session: *session.TabSession) void {
        // Get scaled font from centralized theme
        const scaled_font = theme.fonts.editorFont();

        // Create TextEntryWidget manually to avoid automatic draw() call
        // (dvui.textEntry() calls processEvents() and draw() automatically)
        var text_entry = dvui.widgetAlloc(dvui.TextEntryWidget);
        text_entry.init(@src(), .{
            .text = .{
                .array_list = .{
                    .backing = &active_session.content,
                    .allocator = active_session.allocator,
                },
            },
            .multiline = true,
            .scroll_vertical = false, // Let parent scrollArea handle scrolling
            .scroll_horizontal = false,
        }, .{
            .expand = .both,
            .margin = .{}, // Remove default 4px margin
            .padding = .{ .x = content_padding_x, .y = content_padding_y, .w = content_padding_x, .h = content_padding_y },
            .border = .{}, // No border
            .corner_radius = .{}, // No corner radius
            .background = false, // No background drawing
            .color_border = dvui.Color{ .r = 0, .g = 0, .b = 0, .a = 0 }, // Transparent border
            .font = scaled_font, // Use scaled mono font
            .theme = &no_focus_theme, // Use theme with transparent focus
        });
        text_entry.data().was_allocated_on_widget_stack = true;
        defer text_entry.deinit();

        // Process input events (keyboard, mouse, selection)
        text_entry.processEvents();

        // Mark as modified if text changed
        if (text_entry.text_changed) {
            active_session.is_modified = true;
            active_session.render_state.needs_update = true;
        }

        // Get the content from the text entry widget's internal buffer
        // This is the authoritative source that matches what the widget expects
        const content = text_entry.text[0..text_entry.len];

        // Try to get the lexer for syntax highlighting
        if (ensureLexer(active_session.allocator)) |lex| {
            // Tokenize and render with colors using custom draw sequence
            renderWithSyntaxHighlighting(text_entry, lex, content);
        } else {
            // Fallback: use normal widget draw (no highlighting)
            text_entry.draw();
        }

        // Update cursor position in session for status bar display
        active_session.updateCursorPosition(text_entry.textLayout.selection.cursor);
    }

    /// Render text with syntax highlighting, replacing the widget's default text rendering
    fn renderWithSyntaxHighlighting(
        text_entry: *dvui.TextEntryWidget,
        lex: *lexer_mod.Lexer,
        content: []const u8,
    ) void {
        // Draw background, focus ring, etc.
        text_entry.drawBeforeText();

        // Handle empty content - still need to call addTextDone for proper layout
        if (content.len == 0) {
            text_entry.textLayout.addTextDone(text_entry.data().options.strip());
            text_entry.drawAfterText();
            return;
        }

        // Tokenize the content
        const tokens = lex.tokenize(content) catch {
            // On error, fall back to plain text with default color
            text_entry.textLayout.addText(content, text_entry.data().options.strip());
            text_entry.textLayout.addTextDone(text_entry.data().options.strip());
            text_entry.drawAfterText();
            return;
        };
        defer lex.allocator.free(tokens);

        // Get base options from the widget for consistent styling
        const base_opts = text_entry.data().options.strip();

        // Render each token with its syntax color
        for (tokens) |token| {
            const color = theme.syntax.colorForToken(token.token_type, token.text);
            text_entry.textLayout.addText(token.text, base_opts.override(.{ .color_text = color }));
        }

        // Finalize text layout - marks text as done so deinit doesn't add it again
        text_entry.textLayout.addTextDone(base_opts);

        // Draw cursor and selection overlay
        text_entry.drawAfterText();
    }

    fn countLines(text: []const u8) usize {
        if (text.len == 0) return 1;

        var count: usize = 1;
        for (text) |c| {
            if (c == '\n') count += 1;
        }
        return count;
    }
};
