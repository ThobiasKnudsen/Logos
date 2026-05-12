use glyphon::{Attrs, Buffer as TextBuffer, Family, Metrics, Shaping};

use crate::ui::layout::Rect;
use crate::ui::theme::{fonts, spacing};

use super::{
    cell_delete_size, cell_header_height, output_toggle_height, shift_cell_layouts, CellInfo,
    CellLayout, Renderer,
};

impl Renderer {
    /// Update all cell buffers and compute cell layouts.
    /// Returns the computed cell layouts for hit-testing.
    pub fn update_cells(
        &mut self,
        cells: &[CellInfo],
        active_cell_index: usize,
        pane: Rect,
    ) -> Vec<CellLayout> {
        self.cached_editor_pane = pane;
        self.active_cell_index = active_cell_index;
        self.cell_playing = cells.iter().map(|c| c.is_playing).collect();

        while self.cell_buffers.len() < cells.len() {
            let mut buf = TextBuffer::new(
                &mut self.font_system,
                Metrics::new(fonts::editor_size(), fonts::editor_line_height()),
            );
            buf.set_tab_width(&mut self.font_system, 4);
            self.cell_buffers.push(buf);
        }
        self.cell_buffers.truncate(cells.len());
        self.cell_texts.truncate(cells.len());

        while self.cell_output_buffers.len() < cells.len() {
            let buf = TextBuffer::new(
                &mut self.font_system,
                Metrics::new(fonts::editor_size(), fonts::editor_line_height()),
            );
            self.cell_output_buffers.push(buf);
        }
        self.cell_output_buffers.truncate(cells.len());
        self.cell_output_texts.truncate(cells.len());
        self.cell_output_is_error.resize(cells.len(), false);

        while self.cell_output_scroll_x.len() < cells.len() {
            self.cell_output_scroll_x.push(0.0);
        }
        self.cell_output_scroll_x.truncate(cells.len());

        while self.cell_editor_scroll_x.len() < cells.len() {
            self.cell_editor_scroll_x.push(0.0);
        }
        self.cell_editor_scroll_x.truncate(cells.len());
        while self.cell_editor_scroll_y.len() < cells.len() {
            self.cell_editor_scroll_y.push(0.0);
        }
        self.cell_editor_scroll_y.truncate(cells.len());
        self.cell_content_heights.resize(cells.len(), 0.0);

        let cell_pad = spacing::cell_padding();
        let cell_spacing = spacing::cell_spacing();
        let header_h = cell_header_height();
        let sep_h = 1.0;
        let text_pad = spacing::sm();
        let container_pad = spacing::sm();

        let cell_area_width = pane.w - cell_pad * 2.0;
        let effective_width = if self.v_track_rect.is_some() {
            cell_area_width - spacing::scrollbar_width()
        } else {
            cell_area_width
        };

        let mut layouts = Vec::with_capacity(cells.len());
        let mut y_offset = cell_pad;
        let mut _perf_reshape_us: u128 = 0;
        let mut perf_set_rich_text_us: u128 = 0;
        let mut perf_shape_us: u128 = 0;
        let mut perf_measure_us: u128 = 0;
        let mut perf_output_us: u128 = 0;
        let mut perf_layout_us: u128 = 0;
        let mut reshape_count = 0u32;
        let mut reshape_reasons: Vec<String> = Vec::new();
        let uc_start = std::time::Instant::now();

        for (i, cell_info) in cells.iter().enumerate() {
            let text_changed = self
                .cell_texts
                .get(i)
                .is_none_or(|prev| *prev != cell_info.text);
            if text_changed {
                let rs = std::time::Instant::now();

                self.cell_buffers[i].set_size(&mut self.font_system, None, None);

                let spans = crate::lang::highlight::highlight(&cell_info.text);
                let mono_family = crate::ui::theme::font_family::active_family();
                let default_attrs = Attrs::new().family(Family::Name(mono_family));

                let srt = std::time::Instant::now();
                let lines_updated = Self::incremental_set_rich_text(
                    &mut self.cell_buffers[i],
                    &cell_info.text,
                    &spans,
                    default_attrs,
                );
                perf_set_rich_text_us += srt.elapsed().as_micros();
                reshape_count += 1;
                reshape_reasons.push(format!("cell[{}]:{}lines", i, lines_updated));

                let sh = std::time::Instant::now();
                self.cell_buffers[i].shape_until_scroll(&mut self.font_system, false);
                perf_shape_us += sh.elapsed().as_micros();

                if i < self.cell_texts.len() {
                    self.cell_texts[i].clone_from(&cell_info.text);
                } else {
                    self.cell_texts.push(cell_info.text.clone());
                }
                _perf_reshape_us += rs.elapsed().as_micros();
            }

            let ms = std::time::Instant::now();
            let mut content_h = Self::measure_content_height(&self.cell_buffers[i])
                .max(fonts::editor_line_height());
            if cell_info.text.ends_with('\n') {
                content_h += fonts::editor_line_height();
            }
            self.cell_content_heights[i] = content_h;

            let natural_editor_h = content_h + text_pad * 2.0;
            let editor_h = match cell_info.contracted_editor_h {
                Some(ch) => {
                    let min_h = fonts::editor_line_height() + text_pad * 2.0;
                    ch.clamp(min_h, natural_editor_h)
                }
                None => natural_editor_h,
            };
            perf_measure_us += ms.elapsed().as_micros();

            let os = std::time::Instant::now();
            let has_output = cell_info.output_text.is_some();
            let output_text_ref = cell_info.output_text.as_deref().unwrap_or("");
            let output_changed = self
                .cell_output_texts
                .get(i)
                .is_none_or(|prev| prev != output_text_ref);
            self.cell_output_is_error[i] = cell_info.is_error;
            if output_changed {
                if has_output {
                    let mono_family = crate::ui::theme::font_family::active_family();
                    self.cell_output_buffers[i].set_size(&mut self.font_system, None, None);
                    self.cell_output_buffers[i].set_text(
                        &mut self.font_system,
                        output_text_ref,
                        &Attrs::new().family(Family::Name(mono_family)),
                        Shaping::Advanced,
                        None,
                    );
                    self.cell_output_buffers[i].shape_until_scroll(&mut self.font_system, false);
                }
                if i < self.cell_output_texts.len() {
                    self.cell_output_texts[i] = output_text_ref.to_string();
                } else {
                    self.cell_output_texts.push(output_text_ref.to_string());
                }
                if i < self.cell_output_scroll_x.len() {
                    self.cell_output_scroll_x[i] = 0.0;
                }
            }

            perf_output_us += os.elapsed().as_micros();
            let ls = std::time::Instant::now();
            let output_toggle_h = if has_output {
                output_toggle_height()
            } else {
                0.0
            };

            let output_h = if has_output && !cell_info.output_collapsed {
                let line_count = output_text_ref.lines().count().clamp(1, 10) as f32;
                fonts::editor_line_height() * line_count + text_pad * 2.0
            } else {
                0.0
            };

            let output_section_h = if has_output {
                sep_h + spacing::xs() + output_toggle_h + output_h
            } else {
                0.0
            };

            let container_h =
                container_pad + header_h + sep_h + editor_h + output_section_h + container_pad;

            let screen_y = pane.y + y_offset - self.cell_scroll_y;

            let container = Rect {
                x: pane.x + cell_pad,
                y: screen_y,
                w: effective_width,
                h: container_h,
            };
            let header = Rect {
                x: container.x + container_pad,
                y: container.y + container_pad,
                w: effective_width - container_pad * 2.0,
                h: header_h,
            };
            let btn_y = container.y + (container_pad + header_h - cell_delete_size()) / 2.0;
            let play_button = Rect {
                x: header.x,
                y: btn_y,
                w: cell_delete_size(),
                h: cell_delete_size(),
            };
            let delete_button = Rect {
                x: header.x + header.w - cell_delete_size(),
                y: btn_y,
                w: cell_delete_size(),
                h: cell_delete_size(),
            };
            let copy_button = Rect {
                x: header.x + header.w - cell_delete_size() * 2.0 - spacing::xs(),
                y: btn_y,
                w: cell_delete_size(),
                h: cell_delete_size(),
            };
            // Color button is a horizontal pill sized to fit the "color"
            // label with horizontal padding; height matches the other
            // header buttons so the rounded ends are perfect semicircles.
            let color_label_w = Self::measure_label_width(&self.cell_color_label);
            let color_button_w = color_label_w + spacing::md() * 2.0;
            let color_button = Rect {
                x: copy_button.x - spacing::xs() - color_button_w,
                y: btn_y,
                w: color_button_w,
                h: cell_delete_size(),
            };
            let separator = Rect {
                x: container.x,
                y: header.y + header_h,
                w: container.w,
                h: sep_h,
            };
            let editor = Rect {
                x: container.x + container_pad,
                y: separator.y + sep_h,
                w: effective_width - container_pad * 2.0,
                h: editor_h,
            };
            let inner_w = effective_width - container_pad * 2.0;
            let zero_rect = Rect {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
            };

            let sb_w = spacing::scrollbar_width();
            let sb_margin = 2.0;

            let is_contracted = cell_info.contracted_editor_h.is_some()
                && content_h > (editor_h - text_pad * 2.0) + 1.0;
            let (editor_v_scrollbar_track, editor_v_scrollbar_thumb) = if is_contracted {
                let visible_h = editor_h - text_pad * 2.0;
                let track_x = container.x + container.w - sb_w - sb_margin;
                let track_y = editor.y;
                let track_h = editor.h;
                let track = Rect {
                    x: track_x,
                    y: track_y,
                    w: sb_w,
                    h: track_h,
                };

                let ratio = visible_h / content_h;
                let thumb_h = (track_h * ratio).max(spacing::scrollbar_thumb_min_h());
                let max_scroll = (content_h - visible_h).max(0.0);
                let scroll_y = self.cell_editor_scroll_y.get(i).copied().unwrap_or(0.0);
                let thumb_y = if max_scroll > 0.0 {
                    track.y + (scroll_y / max_scroll) * (track_h - thumb_h)
                } else {
                    track.y
                };
                let thumb = Rect {
                    x: track_x,
                    y: thumb_y,
                    w: sb_w,
                    h: thumb_h,
                };
                (track, thumb)
            } else {
                (zero_rect, zero_rect)
            };

            let editor_content_w = Self::measure_editor_content_width(&self.cell_buffers[i]);
            let v_sb_inset = if is_contracted { sb_w + sb_margin } else { 0.0 };
            let editor_visible_w = editor.w - text_pad * 2.0 - v_sb_inset;
            let (editor_h_scrollbar_track, editor_h_scrollbar_thumb) =
                if editor_content_w > editor_visible_w + 1.0 {
                    let track_x = container.x + sb_margin;
                    let track_w = container.w - sb_margin * 2.0 - v_sb_inset;
                    let track_y = editor.y + editor.h + container_pad - sb_w - sb_margin;
                    let track = Rect {
                        x: track_x,
                        y: track_y,
                        w: track_w,
                        h: sb_w,
                    };

                    let ratio = editor_visible_w / editor_content_w;
                    let thumb_w = (track.w * ratio).max(spacing::scrollbar_thumb_min_h());
                    let max_scroll = (editor_content_w - editor_visible_w).max(0.0);
                    let scroll_x = self.cell_editor_scroll_x.get(i).copied().unwrap_or(0.0);
                    let thumb_x = if max_scroll > 0.0 {
                        track.x + (scroll_x / max_scroll) * (track.w - thumb_w)
                    } else {
                        track.x
                    };
                    let thumb = Rect {
                        x: thumb_x,
                        y: track.y,
                        w: thumb_w,
                        h: sb_w,
                    };
                    (track, thumb)
                } else {
                    if i < self.cell_editor_scroll_x.len() {
                        self.cell_editor_scroll_x[i] = 0.0;
                    }
                    (zero_rect, zero_rect)
                };

            let resize_h = 6.0;
            let resize_handle = Rect {
                x: container.x,
                y: container.y + container.h - resize_h / 2.0,
                w: container.w,
                h: resize_h,
            };

            if is_contracted {
                let visible_h = editor_h - text_pad * 2.0;
                let max_scroll_y = (content_h - visible_h).max(0.0);
                if i < self.cell_editor_scroll_y.len() {
                    self.cell_editor_scroll_y[i] =
                        self.cell_editor_scroll_y[i].clamp(0.0, max_scroll_y);
                }
            } else if i < self.cell_editor_scroll_y.len() {
                self.cell_editor_scroll_y[i] = 0.0;
            }

            let (output_separator, output_toggle, output_copy_button, output_toolbar, output) =
                if has_output {
                    let osep_y = editor.y + editor_h;
                    let output_sep = Rect {
                        x: container.x,
                        y: osep_y,
                        w: container.w,
                        h: sep_h,
                    };
                    let margin = spacing::xs();
                    let row_y = osep_y + sep_h + margin;
                    let row_h = output_toggle_h;
                    let inner_x = container.x + container_pad;

                    let toggle_btn = Rect {
                        x: inner_x,
                        y: row_y,
                        w: row_h,
                        h: row_h,
                    };

                    let ob_btn_size = cell_delete_size();
                    let copy_btn = Rect {
                        x: inner_x + inner_w - ob_btn_size,
                        y: row_y + (row_h - ob_btn_size) / 2.0,
                        w: ob_btn_size,
                        h: ob_btn_size,
                    };

                    let toolbar = Rect {
                        x: inner_x,
                        y: row_y,
                        w: inner_w,
                        h: row_h,
                    };

                    let out_y = row_y + row_h;
                    let out = Rect {
                        x: inner_x,
                        y: out_y,
                        w: inner_w,
                        h: output_h,
                    };

                    (output_sep, toggle_btn, copy_btn, toolbar, out)
                } else {
                    (zero_rect, zero_rect, zero_rect, zero_rect, zero_rect)
                };

            layouts.push(CellLayout {
                cell_index: i,
                container,
                header,
                play_button,
                color_button,
                copy_button,
                delete_button,
                separator,
                editor,
                output,
                output_separator,
                output_toggle,
                output_copy_button,
                output_toolbar,
                editor_h_scrollbar_track,
                editor_h_scrollbar_thumb,
                editor_v_scrollbar_track,
                editor_v_scrollbar_thumb,
                resize_handle,
                content_height: content_h,
                plot_color: cell_info.plot_color,
            });

            perf_layout_us += ls.elapsed().as_micros();
            y_offset += container_h + cell_spacing;
        }

        let uc_total = uc_start.elapsed().as_micros();
        if reshape_count > 0 || uc_total > 500 {
            log::debug!(
                "[perf] update_cells: {}us total | reshapes={} set_rich_text={}us shape={}us measure={}us output={}us layout={}us | reasons=[{}]",
                uc_total, reshape_count, perf_set_rich_text_us, perf_shape_us, perf_measure_us, perf_output_us, perf_layout_us,
                reshape_reasons.join(", ")
            );
        }

        let add_btn_size = 28.0 * fonts::scale();
        let add_btn_x = pane.x + cell_pad + effective_width / 2.0 - add_btn_size / 2.0;
        let add_btn_y = pane.y + y_offset - self.cell_scroll_y;
        self.add_cell_rect = Rect {
            x: add_btn_x,
            y: add_btn_y,
            w: add_btn_size,
            h: add_btn_size,
        };

        y_offset += add_btn_size + cell_pad;
        self.cells_total_height = y_offset;

        if active_cell_index < cells.len() {
            let (cx, cy, ch) = compute_cursor_content_pos(
                &self.cell_buffers[active_cell_index],
                &cells[active_cell_index].text,
                cells[active_cell_index].cursor_byte,
            );
            self.cursor_content_pos = (cx, cy, ch);

            self.selection_content_rects =
                if let Some((sel_start, sel_end)) = cells[active_cell_index].selection {
                    compute_selection_rects(
                        &self.cell_buffers[active_cell_index],
                        &cells[active_cell_index].text,
                        sel_start,
                        sel_end,
                    )
                } else {
                    Vec::new()
                };

            let cursor_byte = cells[active_cell_index].cursor_byte;
            let cursor_moved =
                active_cell_index != self.prev_active_cell || cursor_byte != self.prev_cursor_byte;
            self.prev_active_cell = active_cell_index;
            self.prev_cursor_byte = cursor_byte;

            if cursor_moved {
                if cells[active_cell_index].contracted_editor_h.is_some() {
                    let visible_h = layouts
                        .get(active_cell_index)
                        .map(|l| l.editor.h - text_pad * 2.0)
                        .unwrap_or(0.0);
                    let content_h_total = self
                        .cell_content_heights
                        .get(active_cell_index)
                        .copied()
                        .unwrap_or(0.0);
                    if content_h_total > visible_h {
                        let scroll_y = &mut self.cell_editor_scroll_y[active_cell_index];
                        if cy < *scroll_y {
                            *scroll_y = cy;
                        } else if cy + ch > *scroll_y + visible_h {
                            *scroll_y = cy + ch - visible_h;
                        }
                        let max_sy = (content_h_total - visible_h).max(0.0);
                        *scroll_y = scroll_y.clamp(0.0, max_sy);
                    }
                }

                {
                    let visible_w = layouts
                        .get(active_cell_index)
                        .map(|l| l.editor.w - text_pad * 2.0)
                        .unwrap_or(0.0);
                    let scroll_x = &mut self.cell_editor_scroll_x[active_cell_index];
                    if cx < *scroll_x {
                        *scroll_x = cx;
                    } else if cx > *scroll_x + visible_w {
                        *scroll_x = cx - visible_w;
                    }
                    *scroll_x = scroll_x.max(0.0);
                }

                if let Some(active_layout) = layouts.get(active_cell_index) {
                    let cell_scroll_y_offset = self
                        .cell_editor_scroll_y
                        .get(active_cell_index)
                        .copied()
                        .unwrap_or(0.0);
                    let cursor_screen_y =
                        active_layout.editor.y + text_pad + cy - cell_scroll_y_offset;
                    let cursor_bottom = cursor_screen_y + ch;

                    let old_scroll = self.cell_scroll_y;
                    if cursor_screen_y < pane.y {
                        self.cell_scroll_y += cursor_screen_y - pane.y;
                    } else if cursor_bottom > pane.y + pane.h {
                        self.cell_scroll_y += cursor_bottom - (pane.y + pane.h);
                    }
                    let max_sy = (self.cells_total_height - pane.h).max(0.0);
                    self.cell_scroll_y = self.cell_scroll_y.clamp(0.0, max_sy);

                    let scroll_delta = self.cell_scroll_y - old_scroll;
                    if scroll_delta.abs() > 0.001 {
                        shift_cell_layouts(&mut layouts, &mut self.add_cell_rect, scroll_delta);
                    }
                }
            }
        }

        self.update_scrollbar_rects(pane);

        self.cell_layouts = layouts.clone();
        layouts
    }
}

/// Returns (content_x, content_y, line_height) relative to buffer origin.
fn compute_cursor_content_pos(
    text_buffer: &TextBuffer,
    text: &str,
    cursor_byte: usize,
) -> (f32, f32, f32) {
    let clamped = cursor_byte.min(text.len());
    let before = &text[..clamped];
    let line_idx = before.matches('\n').count();
    let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let col_byte = clamped - line_start;

    for run in text_buffer.layout_runs() {
        if run.line_i == line_idx {
            for glyph in run.glyphs.iter() {
                if glyph.start >= col_byte {
                    return (glyph.x, run.line_top, run.line_height);
                }
            }
            let x = run.glyphs.last().map(|g| g.x + g.w).unwrap_or(0.0);
            return (x, run.line_top, run.line_height);
        }
    }

    let mut last_top = 0.0_f32;
    let mut last_height = fonts::editor_line_height();
    let mut last_line_i = 0;
    for run in text_buffer.layout_runs() {
        last_top = run.line_top;
        last_height = run.line_height;
        last_line_i = run.line_i;
    }
    let extra = (line_idx.saturating_sub(last_line_i)) as f32;
    (0.0, last_top + last_height * extra, last_height)
}

/// Compute content-relative (x, y, w, h) rectangles for a text selection.
fn compute_selection_rects(
    text_buffer: &TextBuffer,
    text: &str,
    sel_start: usize,
    sel_end: usize,
) -> Vec<(f32, f32, f32, f32)> {
    let start = sel_start.min(text.len());
    let end = sel_end.min(text.len());
    if start >= end {
        return Vec::new();
    }

    let before_start = &text[..start];
    let start_line = before_start.matches('\n').count();
    let start_line_begin = before_start.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let start_col_byte = start - start_line_begin;

    let before_end = &text[..end];
    let end_line = before_end.matches('\n').count();
    let end_line_begin = before_end.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end_col_byte = end - end_line_begin;

    let mut rects = Vec::new();
    let min_sel_width = 6.0;

    for run in text_buffer.layout_runs() {
        if run.line_i < start_line || run.line_i > end_line {
            continue;
        }

        let col_start = if run.line_i == start_line {
            start_col_byte
        } else {
            0
        };
        let col_end = if run.line_i == end_line {
            end_col_byte
        } else {
            usize::MAX
        };

        if col_start == col_end && run.line_i == start_line && run.line_i == end_line {
            continue;
        }

        let x_start = if col_start == 0 {
            0.0
        } else {
            let mut x = run.glyphs.last().map(|g| g.x + g.w).unwrap_or(0.0);
            for glyph in run.glyphs.iter() {
                if glyph.start >= col_start {
                    x = glyph.x;
                    break;
                }
            }
            x
        };

        let x_end = if col_end == usize::MAX {
            run.glyphs.last().map(|g| g.x + g.w).unwrap_or(0.0) + min_sel_width
        } else {
            let mut x = run.glyphs.last().map(|g| g.x + g.w).unwrap_or(0.0);
            for glyph in run.glyphs.iter() {
                if glyph.start >= col_end {
                    x = glyph.x;
                    break;
                }
            }
            x
        };

        let w = (x_end - x_start).max(if run.line_i != end_line {
            min_sel_width
        } else {
            0.0
        });
        if w > 0.0 {
            rects.push((x_start, run.line_top, w, run.line_height));
        }
    }

    rects
}
