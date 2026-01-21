//! Notebook Panel - Multi-cell notebook interface
//!
//! Renders a scrollable list of code cells, each with its own
//! editor, output display, and controls.

const std = @import("std");
const dvui = @import("dvui");
const theme = @import("../theme.zig");
const session = @import("../../session/session.zig");
const code_cell = @import("../../session/code_cell.zig");
const renderer = @import("../../renderer/renderer.zig");
const lexer_mod = @import("../../lang/lexer.zig");

pub const NotebookPanel = struct {
    // Shared styling constants
    const cell_spacing: f32 = 12;
    const cell_padding: f32 = 12;
    const header_height: f32 = 32;
    const button_size: f32 = 24;

    // Custom theme for text entry with transparent focus
    const no_focus_theme = blk: {
        var t = dvui.Theme.builtin.adwaita_dark;
        t.focus = dvui.Color{ .r = 0, .g = 0, .b = 0, .a = 0 };
        break :blk t;
    };

    // Global lexer instance (initialized lazily)
    var lexer: ?lexer_mod.Lexer = null;

    /// Initialize the global lexer if not already done
    fn ensureLexer(_: std.mem.Allocator) ?*lexer_mod.Lexer {
        if (lexer == null) {
            lexer = lexer_mod.Lexer.init(std.heap.page_allocator, lexer_mod.LexerConfig.logosPatterns()) catch |err| {
                std.log.err("Failed to initialize lexer: {}", .{err});
                return null;
            };
        }
        return &lexer.?;
    }

    /// Render the notebook panel with all cells
    pub fn render(active_session: *session.TabSession, graph_renderer: *renderer.GraphRenderer) void {
        _ = graph_renderer; // TODO: Will be used for per-cell rendering in Phase 4

        const scale = theme.fonts.getScale();
        const scaled_spacing = cell_spacing * scale;
        const scaled_padding = cell_padding * scale;
        const corner_radius = theme.radius.md * scale;

        // Outer scroll area for all cells
        var scroll = dvui.scrollArea(@src(), .{}, .{
            .expand = .both,
            .background = false,
        });
        defer scroll.deinit();

        // Vertical container for cells
        var container = dvui.box(@src(), .{ .dir = .vertical }, .{
            .expand = .horizontal,
            .padding = .{ .x = scaled_padding, .y = scaled_padding, .w = scaled_padding, .h = scaled_padding },
        });
        defer container.deinit();

        // Render each cell
        for (active_session.cells.items, 0..) |*cell, i| {
            renderCell(active_session, cell, i, corner_radius, scaled_spacing);
        }

        // Plus button to add new cell
        renderPlusButton(active_session, scale);
    }

    /// Render a single cell
    fn renderCell(
        active_session: *session.TabSession,
        cell: *code_cell.CodeCell,
        cell_index: usize,
        corner_radius: f32,
        spacing: f32,
    ) void {
        const is_active = cell_index == active_session.active_cell_index;

        // Cell container box with rounded corners
        var cell_box = dvui.box(@src(), .{ .dir = .vertical }, .{
            .id_extra = cell.id,
            .expand = .horizontal,
            .background = true,
            .color_fill = if (is_active) theme.colors.bg_elevated else theme.colors.bg_secondary,
            .corner_radius = .{ .x = corner_radius, .y = corner_radius, .w = corner_radius, .h = corner_radius },
            .padding = .{ .x = spacing, .y = spacing, .w = spacing, .h = spacing },
            .margin = .{ .x = 0, .y = 0, .w = 0, .h = spacing },
        });
        defer cell_box.deinit();

        // Cell header with controls
        renderCellHeader(active_session, cell, cell_index);

        // Cell editor
        renderCellEditor(active_session, cell, cell_index);

        // Cell output (if any)
        if (cell.output) |output| {
            renderCellOutput(output);
        }
    }

    /// Render cell header with color picker and copy button
    fn renderCellHeader(active_session: *session.TabSession, cell: *code_cell.CodeCell, cell_index: usize) void {
        _ = cell_index;

        var header = dvui.box(@src(), .{ .dir = .horizontal }, .{
            .id_extra = cell.id,
            .expand = .horizontal,
            .min_size_content = .{ .h = header_height },
        });
        defer header.deinit();

        // Color picker button (left side)
        const color_vec4 = cell.getColorVec4();
        const color = dvui.Color{
            .r = @intFromFloat(color_vec4[0] * 255),
            .g = @intFromFloat(color_vec4[1] * 255),
            .b = @intFromFloat(color_vec4[2] * 255),
            .a = 255,
        };

        if (dvui.button(@src(), "Color", .{}, .{
            .id_extra = cell.id,
            .color_fill = color,
            .min_size_content = .{ .w = button_size, .h = button_size },
            .corner_radius = .{ .x = 4, .y = 4, .w = 4, .h = 4 },
        })) {
            // Cycle through color palette on click
            const palette = [_][3]u8{
                .{ 255, 85, 0 }, // Orange
                .{ 0, 170, 255 }, // Blue
                .{ 255, 0, 0 }, // Red
                .{ 0, 255, 127 }, // Spring green
                .{ 255, 0, 255 }, // Magenta
                .{ 255, 255, 0 }, // Yellow
                .{ 0, 255, 255 }, // Cyan
                .{ 170, 85, 255 }, // Purple
            };

            // Find current color index and cycle to next
            var current_idx: usize = 0;
            for (palette, 0..) |palette_color, i| {
                if (std.mem.eql(u8, &cell.color, &palette_color)) {
                    current_idx = i;
                    break;
                }
            }
            const next_idx = (current_idx + 1) % palette.len;
            cell.setColor(palette[next_idx]);
            active_session.is_modified = true;
            std.log.info("Changed cell {d} color to palette index {d}", .{ cell.id, next_idx });
        }

        // Spacer
        {
            var spacer = dvui.box(@src(), .{}, .{ .expand = .horizontal, .id_extra = cell.id });
            spacer.deinit();
        }

        // Copy button (right side)
        if (dvui.button(@src(), "Copy", .{}, .{
            .id_extra = cell.id,
            .background = false,
            .color_text = theme.colors.text_muted,
            .min_size_content = .{ .w = button_size, .h = button_size },
        })) {
            // Copy cell content to clipboard
            dvui.clipboardTextSet(cell.content.items);
            std.log.info("Copied cell {d} to clipboard", .{cell.id});
        }
    }

    /// Render cell editor (TextEntryWidget)
    fn renderCellEditor(active_session: *session.TabSession, cell: *code_cell.CodeCell, cell_index: usize) void {
        const scaled_font = theme.fonts.editorFont();

        // Check if this cell is currently focused
        const is_focused = cell_index == active_session.active_cell_index;

        // Create TextEntryWidget
        var text_entry = dvui.widgetAlloc(dvui.TextEntryWidget);
        text_entry.init(@src(), .{
            .text = .{
                .array_list = .{
                    .backing = &cell.content,
                    .allocator = active_session.allocator,
                },
            },
            .multiline = true,
            .scroll_vertical = false,
            .scroll_horizontal = false,
        }, .{
            .id_extra = cell.id,
            .expand = .horizontal,
            .min_size_content = .{ .h = 100 },
            .margin = .{},
            .padding = .{ .x = 8, .y = 8, .w = 8, .h = 8 },
            .border = .{},
            .corner_radius = .{ .x = 4, .y = 4, .w = 4, .h = 4 },
            .background = true,
            .color_fill = theme.colors.editor_bg,
            .font = scaled_font,
            .theme = &no_focus_theme,
        });
        text_entry.data().was_allocated_on_widget_stack = true;
        defer text_entry.deinit();

        // Process input events
        text_entry.processEvents();

        // Mark session as modified if text changed
        if (text_entry.text_changed) {
            active_session.is_modified = true;
            active_session.render_state.needs_update = true;
        }

        // Get content for rendering
        const content = text_entry.text[0..text_entry.len];

        // Render with syntax highlighting
        if (ensureLexer(active_session.allocator)) |lex| {
            renderWithSyntaxHighlighting(text_entry, lex, content);
        } else {
            text_entry.draw();
        }

        // Update cursor position if this is the active cell
        if (is_focused) {
            active_session.updateCursorPosition(text_entry.textLayout.selection.cursor);
        }

        // Handle Enter key for cell finalization
        handleEnterKey(active_session, cell, cell_index);
    }

    /// Render text with syntax highlighting
    fn renderWithSyntaxHighlighting(
        text_entry: *dvui.TextEntryWidget,
        lex: *lexer_mod.Lexer,
        content: []const u8,
    ) void {
        text_entry.drawBeforeText();

        if (content.len == 0) {
            text_entry.textLayout.addTextDone(text_entry.data().options.strip());
            text_entry.drawAfterText();
            return;
        }

        const tokens = lex.tokenize(content) catch {
            text_entry.textLayout.addText(content, text_entry.data().options.strip());
            text_entry.textLayout.addTextDone(text_entry.data().options.strip());
            text_entry.drawAfterText();
            return;
        };
        defer lex.allocator.free(tokens);

        const base_opts = text_entry.data().options.strip();

        for (tokens) |token| {
            const color = theme.syntax.colorForTokenType(token.token_type);
            text_entry.textLayout.addText(token.text, base_opts.override(.{ .color_text = color }));
        }

        text_entry.textLayout.addTextDone(base_opts);
        text_entry.drawAfterText();
    }

    /// Render cell output (text and/or plot)
    fn renderCellOutput(output: code_cell.CellOutput) void {
        var output_box = dvui.box(@src(), .{ .dir = .vertical }, .{
            .expand = .horizontal,
            .margin = .{ .x = 0, .y = 8, .w = 0, .h = 0 },
        });
        defer output_box.deinit();

        // Render text output if present
        if (output.text) |text| {
            dvui.labelNoFmt(@src(), text, .{}, .{
                .color_text = theme.colors.text_secondary,
                .font = theme.fonts.editorFont(),
                .padding = .{ .x = 8, .y = 4, .w = 8, .h = 4 },
            });
        }

        // Render plot output if present
        if (output.shader) |shader| {
            // Show shader info - inline rendering will be added in a future update
            const info_text = std.fmt.allocPrint(
                std.heap.page_allocator,
                "[Plot #{d}: {s} output]",
                .{ shader.index, @tagName(shader.output_type) },
            ) catch "[Plot output]";
            defer std.heap.page_allocator.free(info_text);

            dvui.labelNoFmt(@src(), info_text, .{}, .{
                .color_text = theme.colors.accent_info,
                .padding = .{ .x = 8, .y = 4, .w = 8, .h = 4 },
            });
        }

        // Render error if present
        if (output.error_msg) |err_msg| {
            dvui.labelNoFmt(@src(), err_msg, .{}, .{
                .color_text = theme.colors.accent_primary,
                .padding = .{ .x = 8, .y = 4, .w = 8, .h = 4 },
            });
        }
    }

    /// Handle Enter key for cell finalization
    fn handleEnterKey(active_session: *session.TabSession, cell: *code_cell.CodeCell, cell_index: usize) void {
        // Check if Enter was pressed
        var enter_pressed = false;
        const evts = dvui.events();
        for (evts) |*e| {
            if (e.evt == .key) {
                const ke = e.evt.key;
                if (ke.code == .enter and ke.action == .down) {
                    // Check if this is the last cell and cell has content that produces output
                    const is_last_cell = cell_index == active_session.cells.items.len - 1;
                    const has_output_content = cell.content.items.len > 0 and looksLikeOutputExpression(cell.content.items);

                    if (is_last_cell and has_output_content and cell.output != null) {
                        // Cell has output and it's the last cell - auto-create new cell
                        enter_pressed = true;
                        e.handled = true;
                    }
                }
            }
        }

        if (enter_pressed) {
            // Finalize current cell
            active_session.finalizeCell(cell_index);

            // Create new cell
            _ = active_session.addCell() catch {
                std.log.err("Failed to add new cell after Enter", .{});
                return;
            };

            // Set new cell as active
            active_session.active_cell_index = active_session.cells.items.len - 1;

            std.log.info("Auto-created new cell after Enter in last cell", .{});
        }
    }

    /// Check if content looks like it would produce an output expression
    /// This is a simple heuristic - more sophisticated detection happens during parsing
    fn looksLikeOutputExpression(content: []const u8) bool {
        // Trim whitespace
        const trimmed = std.mem.trim(u8, content, " \t\n\r");

        if (trimmed.len == 0) return false;

        // Check if it starts with keywords that define things (not outputs)
        if (std.mem.startsWith(u8, trimmed, "let ") or
            std.mem.startsWith(u8, trimmed, "const ") or
            std.mem.startsWith(u8, trimmed, "fn ") or
            std.mem.startsWith(u8, trimmed, "function "))
        {
            return false;
        }

        // If it contains '=' without comparison operators, it might be an assignment
        // But for now, be lenient and assume most content produces output
        return true;
    }

    /// Render plus button to add new cell
    fn renderPlusButton(active_session: *session.TabSession, scale: f32) void {
        const scaled_button_size = button_size * scale;

        var button_container = dvui.box(@src(), .{ .dir = .horizontal }, .{
            .expand = .horizontal,
            .gravity_x = 0.5, // Center horizontally
            .margin = .{ .x = 0, .y = 12, .w = 0, .h = 12 },
        });
        defer button_container.deinit();

        if (dvui.button(@src(), "+ New Cell", .{}, .{
            .min_size_content = .{ .w = scaled_button_size * 4, .h = scaled_button_size },
            .color_fill = theme.colors.toolbar_button,
            .color_fill_hover = theme.colors.toolbar_button_hover,
            .corner_radius = .{ .x = 8, .y = 8, .w = 8, .h = 8 },
        })) {
            // Add new cell
            _ = active_session.addCell() catch {
                std.log.err("Failed to add new cell", .{});
            };
            // Set new cell as active
            if (active_session.cells.items.len > 0) {
                active_session.active_cell_index = active_session.cells.items.len - 1;
            }
        }
    }
};
