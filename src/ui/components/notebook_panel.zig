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
    // Shared styling constants - base sizes that scale proportionally
    const base_unit: f32 = 4; // All sizes derived from this base unit for consistent ratios
    const cell_spacing: f32 = base_unit * 3; // 12px between cells
    const cell_padding: f32 = base_unit * 2; // 8px inside cells
    const header_height: f32 = base_unit * 6; // 24px header
    const button_size: f32 = base_unit * 5; // 20px buttons

    // Corner radius constants for consistent rounding
    const small_corner_radius: f32 = base_unit * 1; // 4px - for small buttons
    const medium_corner_radius: f32 = base_unit * 2; // 8px - for cells and larger buttons
    const large_corner_radius: f32 = base_unit * 3; // 12px - for prominent elements

    // Custom theme for text entry with transparent focus
    const no_focus_theme = blk: {
        var t = dvui.Theme.builtin.adwaita_dark;
        t.focus = dvui.Color{ .r = 0, .g = 0, .b = 0, .a = 0 };
        break :blk t;
    };

    // Global lexer instance (initialized lazily)
    var lexer: ?lexer_mod.Lexer = null;

    // Color editor state (for RGBA editing popup)
    var color_editor_cell_id: ?usize = null;
    var color_editor_text: std.ArrayList(u8) = undefined;
    var color_editor_initialized: bool = false;
    var color_editor_from_rect: dvui.Rect.Natural = .{};

    // Deferred deletion (set during render, executed after render loop)
    var cell_to_delete: ?usize = null;

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

    /// Calculate relative luminance and return appropriate text color (black or white)
    /// Uses WCAG relative luminance formula for optimal contrast
    fn getContrastingTextColor(bg_color: dvui.Color) dvui.Color {
        // Convert to linear RGB (0.0 to 1.0)
        const r = @as(f32, @floatFromInt(bg_color.r)) / 255.0;
        const g = @as(f32, @floatFromInt(bg_color.g)) / 255.0;
        const b = @as(f32, @floatFromInt(bg_color.b)) / 255.0;

        // Calculate relative luminance (WCAG formula)
        const luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;

        // If luminance > 0.5, use dark text; otherwise use light text
        if (luminance > 0.5) {
            return dvui.Color{ .r = 0, .g = 0, .b = 0, .a = 255 }; // Black
        } else {
            return dvui.Color{ .r = 255, .g = 255, .b = 255, .a = 255 }; // White
        }
    }

    /// Render the notebook panel with all cells
    pub fn render(active_session: *session.TabSession, graph_renderer: *renderer.GraphRenderer) void {
        _ = graph_renderer; // TODO: Will be used for per-cell rendering in Phase 4

        // Reset deferred deletion
        cell_to_delete = null;

        const scale = theme.fonts.getScale();
        const scaled_spacing = cell_spacing * scale;
        const scaled_padding = cell_padding * scale;
        const scaled_corner = base_unit * scale; // Consistent corner radius

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
            renderCell(active_session, cell, i, scaled_corner, scaled_spacing);
        }

        // Plus button to add new cell
        renderPlusButton(active_session, scale);

        // Execute deferred deletion after render loop completes
        if (cell_to_delete) |index| {
            active_session.removeCell(index) catch |err| {
                std.log.warn("Could not remove cell at index {d}: {}", .{ index, err });
            };
        }
    }

    /// Render a single cell
    fn renderCell(
        active_session: *session.TabSession,
        cell: *code_cell.CodeCell,
        cell_index: usize,
        corner_radius: f32,
        spacing: f32,
    ) void {
        _ = corner_radius; // Unused, we use constants now
        const scale = theme.fonts.getScale();
        const scaled_cell_corner = large_corner_radius * scale;

        // Cell container box with rounded corners and border
        var cell_box = dvui.box(@src(), .{ .dir = .vertical }, .{
            .id_extra = cell.id,
            .expand = .horizontal,
            .background = true,
            .color_fill = theme.colors.bg_elevated,
            .border = .{ .x = 1 * scale, .y = 1 * scale, .w = 1 * scale, .h = 1 * scale },
            .color_border = theme.colors.border,
            .corner_radius = .{ .x = scaled_cell_corner, .y = scaled_cell_corner, .w = scaled_cell_corner, .h = scaled_cell_corner },
            .padding = .{ .x = spacing, .y = spacing, .w = spacing, .h = spacing },
            .margin = .{ .x = 0, .y = 0, .w = 0, .h = spacing },
        });
        defer cell_box.deinit();

        // Cell header with controls
        renderCellHeader(active_session, cell, cell_index);

        // Separator line between header and editor (extends full width)
        {
            const separator_height = 1 * scale;
            const separator_margin = (base_unit * 0.75) * scale;
            var separator = dvui.box(@src(), .{}, .{
                .id_extra = cell.id + 5000,
                .expand = .horizontal,
                .background = true,
                .color_fill = theme.colors.border,
                .min_size_content = .{ .h = separator_height },
                .margin = .{ .x = -spacing, .y = separator_margin, .w = -spacing, .h = separator_margin },
            });
            separator.deinit();
        }

        // Cell editor
        renderCellEditor(active_session, cell, cell_index);

        // Cell output (if any)
        if (cell.output) |output| {
            // Separator line between editor and output (extends full width)
            {
                const separator_height = 1 * scale;
                const separator_margin = (base_unit * 0.75) * scale;
                var separator = dvui.box(@src(), .{}, .{
                    .id_extra = cell.id + 6000,
                    .expand = .horizontal,
                    .background = true,
                    .color_fill = theme.colors.border,
                    .min_size_content = .{ .h = separator_height },
                    .margin = .{ .x = -spacing, .y = separator_margin, .w = -spacing, .h = separator_margin },
                });
                separator.deinit();
            }

            renderCellOutput(cell.id, output);
        }
    }

    /// Render cell header with color picker and copy button
    fn renderCellHeader(active_session: *session.TabSession, cell: *code_cell.CodeCell, cell_index: usize) void {
        const scale = theme.fonts.getScale();
        const scaled_header_height = header_height * scale;
        const scaled_icon_size = button_size * scale;
        const scaled_btn_corner = small_corner_radius * scale; // Consistent small corner radius
        const scaled_btn_padding = base_unit * scale;
        const scaled_btn_margin = (base_unit / 2) * scale;

        var header = dvui.box(@src(), .{ .dir = .horizontal }, .{
            .id_extra = cell.id,
            .expand = .horizontal,
            .min_size_content = .{ .h = scaled_header_height },
        });
        defer header.deinit();

        // Spacer (push buttons to the right)
        {
            var spacer = dvui.box(@src(), .{}, .{ .expand = .horizontal, .id_extra = cell.id });
            spacer.deinit();
        }

        // Color picker button (right side) - wrapped in scope to close before other elements
        {
            const color_vec4 = cell.getColorVec4();
            const color = dvui.Color{
                .r = @intFromFloat(color_vec4[0] * 255),
                .g = @intFromFloat(color_vec4[1] * 255),
                .b = @intFromFloat(color_vec4[2] * 255),
                .a = 255,
            };

            var color_btn: dvui.ButtonWidget = undefined;
            color_btn.init(@src(), .{}, .{
                .id_extra = cell.id,
                .color_fill = color,
                .corner_radius = .{ .x = scaled_btn_corner, .y = scaled_btn_corner, .w = scaled_btn_corner, .h = scaled_btn_corner },
                .padding = .{ .x = scaled_btn_padding, .y = scaled_btn_padding, .w = scaled_btn_padding, .h = scaled_btn_padding },
                .margin = .{ .x = scaled_btn_margin, .y = scaled_btn_margin, .w = scaled_btn_margin, .h = scaled_btn_margin },
                .min_size_content = .{ .w = scaled_icon_size, .h = scaled_icon_size },
            });
            defer color_btn.deinit();

            color_btn.processEvents();
            color_btn.drawBackground();

            // Draw "Color" label inside the button with contrasting text color (larger font)
            const text_color = getContrastingTextColor(color);
            dvui.labelNoFmt(@src(), "Color", .{}, .{
                .id_extra = cell.id,
                .color_text = text_color,
                .font = theme.fonts.uiFont(),
            });

            if (color_btn.clicked()) {
                // Open color editor for this cell (convert to natural coordinates)
                const btn_rect_scale = color_btn.data().borderRectScale();
                const btn_rect_natural = btn_rect_scale.r.toNatural();
                openColorEditor(cell, btn_rect_natural);
            }
        }

        // Render color editor popup if this cell is being edited
        if (color_editor_cell_id) |editing_id| {
            if (editing_id == cell.id) {
                renderColorEditor(active_session, cell);
            }
        }

        // Copy button - small icon button
        const entypo = dvui.entypo;
        if (dvui.buttonIcon(@src(), "copy", entypo.copy, .{}, .{}, .{
            .id_extra = cell.id + 1,
            .color_fill = dvui.Color{ .r = 0, .g = 0, .b = 0, .a = 0 }, // Transparent
            .color_fill_hover = theme.colors.toolbar_button_hover,
            .corner_radius = .{ .x = scaled_btn_corner, .y = scaled_btn_corner, .w = scaled_btn_corner, .h = scaled_btn_corner },
            .padding = .{ .x = scaled_btn_padding, .y = scaled_btn_padding, .w = scaled_btn_padding, .h = scaled_btn_padding },
            .margin = .{ .x = scaled_btn_margin, .y = scaled_btn_margin, .w = scaled_btn_margin, .h = scaled_btn_margin },
            .min_size_content = .{ .w = scaled_icon_size, .h = scaled_icon_size },
        })) {
            // Copy cell content to clipboard
            dvui.clipboardTextSet(cell.content.items);
            std.log.info("Copied cell {d} to clipboard", .{cell.id});
        }

        // Delete button (rightmost) - icon button with red icon
        if (dvui.buttonIcon(@src(), "delete", entypo.circle_with_cross, .{}, .{}, .{
            .id_extra = cell.id + 2,
            .color_fill = dvui.Color{ .r = 0, .g = 0, .b = 0, .a = 0 }, // Transparent background
            .color_fill_hover = dvui.Color{ .r = 220, .g = 80, .b = 80, .a = 100 }, // Slight red tint on hover
            .color_text = dvui.Color{ .r = 220, .g = 80, .b = 80, .a = 255 }, // Red icon
            .corner_radius = .{ .x = scaled_btn_corner, .y = scaled_btn_corner, .w = scaled_btn_corner, .h = scaled_btn_corner },
            .padding = .{ .x = scaled_btn_padding, .y = scaled_btn_padding, .w = scaled_btn_padding, .h = scaled_btn_padding },
            .margin = .{ .x = scaled_btn_margin, .y = scaled_btn_margin, .w = scaled_btn_margin, .h = scaled_btn_margin },
            .min_size_content = .{ .w = scaled_icon_size, .h = scaled_icon_size },
        })) {
            // Defer deletion until after render loop completes
            cell_to_delete = cell_index;
            std.log.info("Marked cell {d} for deletion", .{cell.id});
        }
    }

    /// Render cell editor (TextEntryWidget)
    fn renderCellEditor(active_session: *session.TabSession, cell: *code_cell.CodeCell, cell_index: usize) void {
        const scaled_font = theme.fonts.editorFont();
        const scale = theme.fonts.getScale();

        // Check if this cell is currently focused
        const is_focused = cell_index == active_session.active_cell_index;

        // Calculate minimum height for exactly one line of text (padding applied separately)
        const line_height = scaled_font.lineHeight();
        const scaled_padding_x = base_unit * 2 * scale; // 8px horizontal padding
        const scaled_padding_y = base_unit * 0.5 * scale; // 2px vertical padding (minimal, consistent)
        const scaled_text_corner = small_corner_radius * scale;

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
            .min_size_content = .{ .h = line_height }, // Just one line, padding added separately
            .margin = .{},
            .padding = .{ .x = scaled_padding_x, .y = scaled_padding_y, .w = scaled_padding_x, .h = scaled_padding_y },
            .border = .{},
            .corner_radius = .{ .x = scaled_text_corner, .y = scaled_text_corner, .w = scaled_text_corner, .h = scaled_text_corner },
            .background = true,
            .color_fill = theme.colors.bg_elevated,
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
    fn renderCellOutput(cell_id: usize, output: code_cell.CellOutput) void {
        const scale = theme.fonts.getScale();
        const scaled_output_margin = base_unit * 2 * scale; // 8px base margin

        var output_box = dvui.box(@src(), .{ .dir = .vertical }, .{
            .expand = .horizontal,
            .margin = .{ .x = 0, .y = scaled_output_margin, .w = 0, .h = 0 },
        });
        defer output_box.deinit();

        const scaled_text_padding_x = base_unit * 2 * scale;
        const scaled_text_padding_y = base_unit * scale;

        // "Output" label at the top
        dvui.labelNoFmt(@src(), "Output", .{}, .{
            .id_extra = cell_id + 7000,
            .color_text = theme.colors.text_secondary,
            .font = theme.fonts.smallFont(),
            .padding = .{ .x = scaled_text_padding_x, .y = 0, .w = scaled_text_padding_x, .h = scaled_text_padding_y },
        });

        // Render text output if present
        if (output.text) |text| {
            dvui.labelNoFmt(@src(), text, .{}, .{
                .color_text = theme.colors.text_secondary,
                .font = theme.fonts.editorFont(),
                .padding = .{ .x = scaled_text_padding_x, .y = scaled_text_padding_y, .w = scaled_text_padding_x, .h = scaled_text_padding_y },
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
                .padding = .{ .x = scaled_text_padding_x, .y = scaled_text_padding_y, .w = scaled_text_padding_x, .h = scaled_text_padding_y },
            });
        }

        // Render error if present
        if (output.error_msg) |err_msg| {
            dvui.labelNoFmt(@src(), err_msg, .{}, .{
                .color_text = theme.colors.accent_primary,
                .padding = .{ .x = scaled_text_padding_x, .y = scaled_text_padding_y, .w = scaled_text_padding_x, .h = scaled_text_padding_y },
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
        const scaled_button_size = button_size * scale * 1.4; // Even larger button for bigger +
        const scaled_margin = base_unit * 3 * scale; // 12px base margin

        var button_container = dvui.box(@src(), .{ .dir = .horizontal }, .{
            .expand = .horizontal,
            .margin = .{ .x = 0, .y = scaled_margin, .w = 0, .h = scaled_margin },
        });
        defer button_container.deinit();

        // Left spacer to center button
        {
            var spacer = dvui.box(@src(), .{}, .{ .expand = .horizontal });
            spacer.deinit();
        }

        // Centered add button - rounded with border
        if (dvui.button(@src(), "+", .{}, .{
            .min_size_content = .{ .w = scaled_button_size, .h = scaled_button_size },
            .color_fill = theme.colors.bg_elevated,
            .color_fill_hover = theme.colors.toolbar_button_hover,
            .border = .{ .x = 2 * scale, .y = 2 * scale, .w = 2 * scale, .h = 2 * scale },
            .color_border = theme.colors.border,
            .corner_radius = .{ .x = scaled_button_size, .y = scaled_button_size, .w = scaled_button_size, .h = scaled_button_size },
            .font = theme.fonts.editorFont(),
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

        // Right spacer to center button
        {
            var spacer = dvui.box(@src(), .{}, .{ .expand = .horizontal });
            spacer.deinit();
        }
    }

    /// Open color editor for a cell
    fn openColorEditor(cell: *code_cell.CodeCell, from_rect: dvui.Rect.Natural) void {
        color_editor_from_rect = from_rect;
        if (!color_editor_initialized) {
            color_editor_text = .{ .items = &.{}, .capacity = 0 };
            color_editor_initialized = true;
        }

        // Set current cell as the one being edited
        color_editor_cell_id = cell.id;

        // Initialize text buffer with current color values as tuple (r, g, b, a)
        const rgba = cell.getColorVec4();

        // Clear and fill buffer with tuple format
        color_editor_text.clearRetainingCapacity();

        const allocator = std.heap.page_allocator;

        std.fmt.format(color_editor_text.writer(allocator), "({d:.3}, {d:.3}, {d:.3}, {d:.3})", .{ rgba[0], rgba[1], rgba[2], rgba[3] }) catch {};
    }

    /// Render color editor popup
    fn renderColorEditor(active_session: *session.TabSession, cell: *code_cell.CodeCell) void {
        const scale = theme.fonts.getScale();
        const scaled_popup_width = 240 * scale;
        const scaled_padding = base_unit * 2 * scale;

        // Create a floating menu popup with proper width
        var float_menu = dvui.floatingMenu(@src(), .{ .from = color_editor_from_rect }, .{
            .id_extra = cell.id,
            .min_size_content = .{ .w = scaled_popup_width },
        });
        defer float_menu.deinit();

        // Check for Escape key to close menu without applying
        const evts = dvui.events();
        for (evts) |*e| {
            if (e.evt == .key) {
                const ke = e.evt.key;
                if (ke.code == .escape and ke.action == .down) {
                    color_editor_cell_id = null;
                    e.handled = true;
                    return;
                }
            }
        }

        var vbox = dvui.box(@src(), .{ .dir = .vertical }, .{
            .id_extra = cell.id,
            .expand = .horizontal,
            .padding = .{ .x = scaled_padding, .y = scaled_padding, .w = scaled_padding, .h = scaled_padding },
        });
        defer vbox.deinit();

        dvui.labelNoFmt(@src(), "(red, green, blue, alpha)", .{}, .{
            .color_text = theme.colors.text_primary,
        });

        const scaled_input_width = 200 * scale;
        const scaled_button_margin = base_unit * 2 * scale;
        const scaled_button_width = 60 * scale;

        // Single input field for tuple format (wrapped in scope to deinit before buttons)
        {
            var text_entry = dvui.textEntry(@src(), .{
                .text = .{
                    .array_list = .{
                        .backing = &color_editor_text,
                        .allocator = std.heap.page_allocator,
                    },
                },
            }, .{
                .id_extra = cell.id,
                .expand = .horizontal,
                .min_size_content = .{ .w = scaled_input_width },
            });
            defer text_entry.deinit();
        }

        // Button row with proper spacing
        var button_row = dvui.box(@src(), .{ .dir = .horizontal }, .{
            .id_extra = cell.id + 100,
            .expand = .horizontal,
            .margin = .{ .x = 0, .y = scaled_button_margin, .w = 0, .h = 0 },
        });
        defer button_row.deinit();

        // Apply button (left aligned with fixed width)
        if (dvui.button(@src(), "Apply", .{}, .{
            .id_extra = cell.id,
            .min_size_content = .{ .w = scaled_button_width },
        })) {
            applyColorChanges(active_session, cell);
            color_editor_cell_id = null;
        }

        // Spacer to push cancel button to the right
        {
            var spacer = dvui.box(@src(), .{}, .{ .expand = .horizontal, .id_extra = cell.id + 200 });
            spacer.deinit();
        }

        // Cancel button (right aligned with fixed width)
        if (dvui.button(@src(), "Cancel", .{}, .{
            .id_extra = cell.id + 1000,
            .min_size_content = .{ .w = scaled_button_width },
        })) {
            color_editor_cell_id = null;
        }
    }

    /// Apply color changes from tuple text input
    fn applyColorChanges(active_session: *session.TabSession, cell: *code_cell.CodeCell) void {
        // Parse tuple format: (r, g, b, a)
        const text = color_editor_text.items;

        // Default values if parsing fails
        var r: f32 = 1.0;
        var g: f32 = 1.0;
        var b: f32 = 1.0;
        var a: f32 = 1.0;

        // Strip parentheses and whitespace
        const trimmed = std.mem.trim(u8, text, " \t\n\r()");

        // Split by commas
        var iter = std.mem.splitScalar(u8, trimmed, ',');
        var idx: usize = 0;

        while (iter.next()) |component| : (idx += 1) {
            const value = std.fmt.parseFloat(f32, std.mem.trim(u8, component, " \t")) catch {
                std.log.warn("Failed to parse color component {}: '{s}'", .{ idx, component });
                continue;
            };

            switch (idx) {
                0 => r = value,
                1 => g = value,
                2 => b = value,
                3 => a = value,
                else => break,
            }
        }

        // Update cell color
        cell.setColorFromFloat(r, g, b, a);
        active_session.is_modified = true;
        active_session.render_state.needs_update = true;

        std.log.info("Updated cell {} color to ({d:.3}, {d:.3}, {d:.3}, {d:.3})", .{ cell.id, r, g, b, a });
    }
};
