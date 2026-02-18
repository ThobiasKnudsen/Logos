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

    // Global lexer instance (initialized lazily)
    var lexer: ?lexer_mod.Lexer = null;

    // Color editor state (for RGBA editing popup)
    var color_editor_cell_id: ?usize = null;
    var color_editor_text: std.ArrayList(u8) = undefined;
    var color_editor_initialized: bool = false;
    var color_editor_from_rect: dvui.Rect.Natural = .{};

    // Deferred deletion (set during render, executed after render loop)
    var cell_to_delete: ?usize = null;

    // Deferred focus: when a new cell is created (e.g. via Enter auto-play),
    // we need to focus its TextEntryWidget on the next frame.
    var pending_focus_cell: bool = false;

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

    // Colors for play/stop buttons
    const play_green = dvui.Color{ .r = 60, .g = 200, .b = 80, .a = 255 };
    const play_green_hover = dvui.Color{ .r = 80, .g = 220, .b = 100, .a = 255 };
    const stop_red = dvui.Color{ .r = 220, .g = 60, .b = 60, .a = 255 };
    const stop_red_hover = dvui.Color{ .r = 240, .g = 80, .b = 80, .a = 255 };
    const error_red_border = dvui.Color{ .r = 220, .g = 60, .b = 60, .a = 200 };

    /// Render the notebook panel with all cells
    pub fn render(active_session: *session.TabSession, graph_renderer: *renderer.GraphRenderer) void {
        _ = graph_renderer;

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

        // Render each cell using index-based iteration.
        // NOTE: We snapshot the cell count at the start. handleEnterKey may add
        // new cells via addCell(), but we must NOT render those in the same frame
        // because (a) the cells slice may be reallocated and (b) the new cell
        // could pick up stale keyboard events.
        const cell_count = active_session.cells.items.len;
        var i: usize = 0;
        while (i < cell_count) : (i += 1) {
            renderCell(active_session, &active_session.cells.items[i], i, scaled_corner, scaled_spacing);
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

        // Red border when validation error, normal otherwise
        const border_color = if (cell.has_validation_error) error_red_border else theme.colors.border;
        const border_width = if (cell.has_validation_error) 2 * scale else 1 * scale;

        // Cell container box with rounded corners and border
        var cell_box = dvui.box(@src(), .{ .dir = .vertical }, .{
            .id_extra = cell.id,
            .expand = .horizontal,
            .background = true,
            .color_fill = theme.colors.bg_elevated,
            .border = .{ .x = border_width, .y = border_width, .w = border_width, .h = border_width },
            .color_border = border_color,
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
            const separator_margin = (base_unit * 0.5) * scale;
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

        // Cell editor (may add new cells via handleEnterKey, which can reallocate cells array)
        renderCellEditor(active_session, cell, cell_index);

        // IMPORTANT: Re-fetch cell pointer after renderCellEditor because handleEnterKey
        // may call addCell() which can reallocate the cells ArrayList, invalidating
        // the original 'cell' pointer passed to this function.
        if (cell_index >= active_session.cells.items.len) return;
        const current_cell = &active_session.cells.items[cell_index];

        // Show validation error in output section
        if (current_cell.has_validation_error) {
            // Separator line
            {
                const separator_height = 1 * scale;
                const separator_margin = (base_unit * 0.75) * scale;
                var separator = dvui.box(@src(), .{}, .{
                    .id_extra = current_cell.id + 6500,
                    .expand = .horizontal,
                    .background = true,
                    .color_fill = error_red_border,
                    .min_size_content = .{ .h = separator_height },
                    .margin = .{ .x = -spacing, .y = separator_margin, .w = -spacing, .h = separator_margin },
                });
                separator.deinit();
            }

            // Error message
            const err_msg = current_cell.validation_error orelse "Validation error";
            const scaled_text_padding_x = base_unit * 2 * scale;
            const scaled_text_padding_y = base_unit * scale;
            dvui.labelNoFmt(@src(), err_msg, .{}, .{
                .id_extra = current_cell.id + 6600,
                .color_text = error_red_border,
                .font = theme.fonts.smallFont(),
                .padding = .{ .x = scaled_text_padding_x, .y = scaled_text_padding_y, .w = scaled_text_padding_x, .h = scaled_text_padding_y },
            });
        }

        // Cell output (if any, and no validation error)
        if (!current_cell.has_validation_error) {
            if (current_cell.output) |output| {
                // Separator line between editor and output (extends full width)
                {
                    const separator_height = 1 * scale;
                    const separator_margin = (base_unit * 0.75) * scale;
                    var separator = dvui.box(@src(), .{}, .{
                        .id_extra = current_cell.id + 6000,
                        .expand = .horizontal,
                        .background = true,
                        .color_fill = theme.colors.border,
                        .min_size_content = .{ .h = separator_height },
                        .margin = .{ .x = -spacing, .y = separator_margin, .w = -spacing, .h = separator_margin },
                    });
                    separator.deinit();
                }

                renderCellOutput(current_cell.id, output);
            }
        }
    }

    /// Render cell header with play/stop button, color picker, and copy button
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
            .margin = .{ .x = 0, .y = -(base_unit * 1.5 * scale), .w = 0, .h = 0 },
        });
        defer header.deinit();

        const entypo = dvui.entypo;

        // Only show play/color buttons when cell has plottable output
        if (cell.output != null and cell.output.?.shader != null) {
            // Play/Stop button (left side)
            {
                const play_icon = if (cell.is_playing) entypo.controller_stop else entypo.controller_play;
                const play_label = if (cell.is_playing) "stop" else "play";
                const play_color = if (cell.is_playing) stop_red else play_green;
                const play_hover = if (cell.is_playing) stop_red_hover else play_green_hover;

                if (dvui.buttonIcon(@src(), play_label, play_icon, .{}, .{}, .{
                    .id_extra = cell.id + 3,
                    .color_fill = play_color,
                    .color_fill_hover = play_hover,
                    .corner_radius = .{ .x = scaled_btn_corner, .y = scaled_btn_corner, .w = scaled_btn_corner, .h = scaled_btn_corner },
                    .padding = .{ .x = scaled_btn_padding, .y = scaled_btn_padding, .w = scaled_btn_padding, .h = scaled_btn_padding },
                    .margin = .{ .x = -(base_unit * scale), .y = scaled_btn_margin, .w = scaled_btn_margin, .h = scaled_btn_margin },
                    .min_size_content = .{ .w = scaled_icon_size, .h = scaled_icon_size },
                })) {
                    if (cell.is_playing) {
                        // Stop
                        cell.is_playing = false;
                        std.log.info("Stopped cell {d}", .{cell.id});
                    } else {
                        // Play: validate and parse
                        active_session.validateAndParseCell(cell_index);

                        if (!cell.has_validation_error) {
                            cell.is_playing = true;
                            std.log.info("Playing cell {d}", .{cell.id});

                            // Replay dependent cells
                            active_session.replayDependentCells(cell_index);
                        }
                    }
                }
            }
        }

        // Spacer (push buttons to the right)
        {
            var spacer = dvui.box(@src(), .{}, .{ .expand = .horizontal, .id_extra = cell.id });
            spacer.deinit();
        }

        // Only show color button when cell has plottable output
        if (cell.output != null and cell.output.?.shader != null) {
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
                    .padding = .{ .x = scaled_btn_padding, .y = scaled_btn_padding * 0.5, .w = scaled_btn_padding, .h = scaled_btn_padding * 0.5 },
                    .margin = .{ .x = scaled_btn_margin, .y = scaled_btn_margin, .w = scaled_btn_margin, .h = scaled_btn_margin },
                    .min_size_content = .{ .w = scaled_icon_size, .h = scaled_icon_size },
                });
                defer color_btn.deinit();

                color_btn.processEvents();
                color_btn.drawBackground();

                // Draw "Color" label inside the button with contrasting text color (smaller font)
                const text_color = getContrastingTextColor(color);
                dvui.labelNoFmt(@src(), "Color", .{}, .{
                    .id_extra = cell.id,
                    .color_text = text_color,
                    .font = theme.fonts.smallFont(),
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
        }

        // Copy button - small icon button
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
            .margin = .{ .x = scaled_btn_margin, .y = scaled_btn_margin, .w = -(base_unit * scale), .h = scaled_btn_margin },
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

        // Create a runtime theme from the window's actual theme (which has DejaVu fonts)
        // with transparent focus. Using a comptime theme from adwaita_dark would use
        // Vera Sans Mono which lacks many Unicode glyphs (π, →, ŋ, etc.)
        var cell_theme = dvui.themeGet();
        cell_theme.focus = dvui.Color{ .r = 0, .g = 0, .b = 0, .a = 0 };

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
            .theme = &cell_theme,
        });
        text_entry.data().was_allocated_on_widget_stack = true;
        defer text_entry.deinit();

        // If this cell was just created (e.g. via Enter auto-play) and needs focus,
        // explicitly focus the TextEntryWidget so the user can start typing immediately.
        if (is_focused and pending_focus_cell) {
            dvui.focusWidget(text_entry.data().id, null, null);
            pending_focus_cell = false;
        }

        // Capture content and cursor BEFORE processEvents (for Enter key detection).
        // processEvents will insert a newline if Enter is pressed, so we need
        // the pre-Enter state to decide if we should auto-play.
        const content_before = active_session.allocator.dupe(u8, cell.content.items) catch null;
        defer if (content_before) |cb| active_session.allocator.free(cb);
        const cursor_before = text_entry.textLayout.selection.cursor;

        // Process input events
        text_entry.processEvents();

        // Detect if Enter key was pressed (content grew by at least 1 and a newline was added)
        var enter_pressed = false;
        if (content_before) |cb| {
            if (cell.content.items.len > cb.len) {
                // Check if a newline was inserted (Enter key)
                const diff = cell.content.items.len - cb.len;
                if (diff >= 1) {
                    // Look for a newline in the newly inserted region
                    const insert_start = @min(cursor_before, cell.content.items.len);
                    const insert_end = @min(insert_start + diff, cell.content.items.len);
                    for (cell.content.items[insert_start..insert_end]) |c| {
                        if (c == '\n') {
                            enter_pressed = true;
                            break;
                        }
                    }
                }
            }
        }

        // Mark session as modified if text changed, validate, and stop if playing
        if (text_entry.text_changed) {
            active_session.is_modified = true;
            active_session.render_state.needs_update = true;

            // If cell was playing, stop it on edit (but not if Enter auto-play will handle it)
            if (cell.is_playing and !enter_pressed) {
                cell.is_playing = false;
                std.log.info("Cell {d} stopped due to edit", .{cell.id});
            }

            // Validate the cell content (skip if Enter auto-play will handle it)
            if (!enter_pressed) {
                active_session.validateAndParseCell(cell_index);

                // NOTE: We intentionally do NOT call replayDependentCells here.
                // Replaying on every keystroke causes severe performance issues
                // (each keystroke re-parses and re-generates shaders for ALL
                // dependent playing cells). Dependent cells are replayed when
                // explicitly played (play button or Enter auto-play) instead.
            }
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

        // Handle Enter key for cell finalization (uses pre-Enter content/cursor)
        if (enter_pressed) {
            if (content_before) |cb| {
                handleEnterKey(active_session, cell, cell_index, cb, cursor_before);
            }
        }
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
            const color = theme.syntax.colorForToken(token.token_type, token.text);
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
            // Validate UTF-8 before passing to dvui to prevent crashes
            if (text.len > 0 and std.unicode.utf8ValidateSlice(text)) {
                dvui.labelNoFmt(@src(), text, .{}, .{
                    .color_text = theme.colors.text_secondary,
                    .font = theme.fonts.editorFont(),
                    .padding = .{ .x = scaled_text_padding_x, .y = scaled_text_padding_y, .w = scaled_text_padding_x, .h = scaled_text_padding_y },
                });
            } else if (text.len > 0) {
                dvui.labelNoFmt(@src(), "[invalid UTF-8 output]", .{}, .{
                    .color_text = theme.colors.accent_primary,
                    .font = theme.fonts.editorFont(),
                    .padding = .{ .x = scaled_text_padding_x, .y = scaled_text_padding_y, .w = scaled_text_padding_x, .h = scaled_text_padding_y },
                });
            }
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
            if (err_msg.len > 0 and std.unicode.utf8ValidateSlice(err_msg)) {
                dvui.labelNoFmt(@src(), err_msg, .{}, .{
                    .color_text = theme.colors.accent_primary,
                    .padding = .{ .x = scaled_text_padding_x, .y = scaled_text_padding_y, .w = scaled_text_padding_x, .h = scaled_text_padding_y },
                });
            }
        }
    }

    /// Handle Enter key: if cursor is on last line and content produces plottable
    /// output, auto-play the cell and create a new cell. Otherwise, let the
    /// newline through normally (for function definitions, bindings, etc.).
    ///
    /// Called AFTER text_entry.processEvents() has inserted the newline.
    /// content_before_enter and cursor_before_enter reflect state BEFORE the
    /// newline was added.
    fn handleEnterKey(active_session: *session.TabSession, cell: *code_cell.CodeCell, cell_index: usize, content_before_enter: []const u8, cursor_before_enter: usize) void {
        // Check if cursor was on the last line of the cell content (before newline insertion)
        const is_on_last_line = isOnLastLine(content_before_enter, cursor_before_enter);

        if (!is_on_last_line) return;

        // Quick pre-filter: skip obviously non-output lines
        const last_line = getLastLine(content_before_enter);
        if (last_line.len == 0 or !looksLikeOutputExpression(last_line)) return;

        // Validate the cell to determine if it actually produces output.
        // The cell content currently has the newline from processEvents — that's
        // fine, the parser handles trailing whitespace.
        active_session.validateAndParseCell(cell_index);

        // If the cell has no plottable output (e.g. function definition, binding),
        // leave the newline in place and return — Enter acts normally.
        if (cell.has_validation_error or cell.output == null or cell.output.?.shader == null) {
            return;
        }

        // Cell has plottable output — remove the newline that processEvents inserted
        // and finalize the cell.
        {
            const insert_pos = @min(cursor_before_enter, cell.content.items.len);
            if (insert_pos < cell.content.items.len and cell.content.items[insert_pos] == '\n') {
                // Remove the newline by shifting content left
                std.mem.copyForwards(
                    u8,
                    cell.content.items[insert_pos..cell.content.items.len - 1],
                    cell.content.items[insert_pos + 1 .. cell.content.items.len],
                );
                cell.content.items.len -= 1;
            } else if (cell.content.items.len > 0 and cell.content.items[cell.content.items.len - 1] == '\n') {
                // Fallback: remove trailing newline
                cell.content.items.len -= 1;
            }
        }

        // Auto-play the cell
        cell.is_playing = true;
        std.log.info("Auto-playing cell {d} on Enter", .{cell.id});

        // If last cell, create a new one and focus it
        const is_last_cell = cell_index == active_session.cells.items.len - 1;
        if (is_last_cell) {
            _ = active_session.addCell() catch {
                std.log.err("Failed to add new cell after Enter", .{});
                return;
            };
            active_session.active_cell_index = active_session.cells.items.len - 1;
            // Request focus for the new cell on the next frame
            pending_focus_cell = true;
            std.log.info("Auto-created new cell after Enter in last cell", .{});
        }

        // Replay dependent cells AFTER the new cell has been created
        // (so we don't accidentally try to replay it)
        active_session.replayDependentCells(cell_index);
    }

    /// Check if a cursor position is on the last line of content
    fn isOnLastLine(content: []const u8, cursor: usize) bool {
        // If cursor is at or past the end, it's on the last line
        if (cursor >= content.len) return true;

        // Check if there's a newline after the cursor
        for (content[cursor..]) |c| {
            if (c == '\n') return false;
        }
        return true;
    }

    /// Get the last line of content
    fn getLastLine(content: []const u8) []const u8 {
        if (content.len == 0) return "";

        // Find the last newline
        var last_newline: ?usize = null;
        for (content, 0..) |c, i| {
            if (c == '\n') last_newline = i;
        }

        if (last_newline) |nl| {
            return std.mem.trim(u8, content[nl + 1 ..], " \t\r");
        }
        return std.mem.trim(u8, content, " \t\r\n");
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
