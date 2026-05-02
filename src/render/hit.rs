use crate::ui::layout::Rect;

use super::{CellLayout, Renderer};

impl Renderer {
    /// Returns the cell layouts for hit-testing.
    pub fn cell_layouts(&self) -> &[CellLayout] {
        &self.cell_layouts
    }

    /// Hit-test a screen position against a cell's text buffer, returning the
    /// byte offset into the cell's text. Returns `None` if the cell index is
    /// out of range or has no layout.
    pub fn hit_test_cell(&self, cell_index: usize, screen_x: f32, screen_y: f32) -> Option<usize> {
        let cl = self
            .cell_layouts
            .iter()
            .find(|c| c.cell_index == cell_index)?;
        if cell_index >= self.cell_buffers.len() {
            return None;
        }
        let buf = &self.cell_buffers[cell_index];
        let text_pad = crate::ui::theme::spacing::sm();

        let scroll_x = self
            .cell_editor_scroll_x
            .get(cell_index)
            .copied()
            .unwrap_or(0.0);
        let scroll_y = self
            .cell_editor_scroll_y
            .get(cell_index)
            .copied()
            .unwrap_or(0.0);
        let cx = screen_x - cl.editor.x - text_pad + scroll_x;
        let cy = screen_y - cl.editor.y - text_pad + scroll_y;

        let mut best_run = None;
        let mut last_run_line_top = 0.0_f32;
        let mut last_run_line_height = crate::ui::theme::fonts::editor_line_height();

        for run in buf.layout_runs() {
            last_run_line_top = run.line_top;
            last_run_line_height = run.line_height;

            if cy >= run.line_top && cy < run.line_top + run.line_height {
                best_run = Some((run.line_i, run.glyphs.to_vec()));
                break;
            }
            if run.line_top + run.line_height <= cy {
                best_run = Some((run.line_i, run.glyphs.to_vec()));
            }
        }

        if cy > last_run_line_top + last_run_line_height {
            let text = if cell_index < self.cell_texts.len() {
                &self.cell_texts[cell_index]
            } else {
                return Some(0);
            };
            return Some(text.len());
        }

        if cy < 0.0 {
            return Some(0);
        }

        let (line_i, glyphs) = best_run?;

        let text = if cell_index < self.cell_texts.len() {
            &self.cell_texts[cell_index]
        } else {
            return Some(0);
        };

        let line_start_byte: usize = text
            .split('\n')
            .take(line_i)
            .map(|l| l.len() + 1)
            .sum();

        if glyphs.is_empty() {
            return Some(line_start_byte);
        }

        if cx <= glyphs[0].x {
            return Some(line_start_byte);
        }

        for glyph in &glyphs {
            let mid = glyph.x + glyph.w / 2.0;
            if cx < mid {
                return Some(line_start_byte + glyph.start);
            }
        }

        if let Some(last) = glyphs.last() {
            Some(line_start_byte + last.end)
        } else {
            Some(line_start_byte)
        }
    }

    /// Returns the add-cell button rect for hit-testing.
    pub fn add_cell_button_rect(&self) -> Rect {
        self.add_cell_rect
    }
}
