use glyphon::{
    cosmic_text::LineEnding, Attrs, AttrsList, Buffer as TextBuffer, BufferLine,
    Color as GlyphonColor, Shaping,
};

use super::Renderer;

impl Renderer {
    /// Invalidate cached cell texts so the next sync forces re-highlighting.
    /// Used when the syntax theme changes without the text itself changing.
    pub fn invalidate_cell_texts(&mut self) {
        self.cell_texts.clear();
    }

    /// Force every cell editor and output buffer to re-shape from scratch on
    /// the next sync. Used when the active font family changes — clearing the
    /// cached text alone isn't enough because the incremental-shape path
    /// preserves each line's existing `AttrsList` (still referencing the old
    /// family). Wiping the buffer lines forces a full rebuild with the new
    /// default attrs.
    pub fn invalidate_cell_fonts(&mut self) {
        self.cell_texts.clear();
        self.cell_output_texts.clear();
        for buf in &mut self.cell_buffers {
            buf.lines.clear();
            buf.set_redraw(true);
        }
        for buf in &mut self.cell_output_buffers {
            buf.lines.clear();
            buf.set_redraw(true);
        }
    }

    /// Update a cosmic-text buffer incrementally: only reshape lines that
    /// actually changed.  Returns the number of lines that were reshaped.
    pub(super) fn incremental_set_rich_text(
        buffer: &mut TextBuffer,
        new_text: &str,
        spans: &[crate::lang::highlight::HighlightSpan],
        default_attrs: Attrs,
    ) -> usize {
        let new_lines: Vec<&str> = new_text.split('\n').collect();
        let new_count = new_lines.len();
        let old_count = buffer.lines.len();

        let top_match = {
            let limit = old_count.min(new_count);
            let mut m = 0;
            while m < limit && buffer.lines[m].text() == new_lines[m] {
                m += 1;
            }
            m
        };

        let bottom_match = {
            let limit = (old_count - top_match).min(new_count - top_match);
            let mut m = 0;
            while m < limit
                && buffer.lines[old_count - 1 - m].text() == new_lines[new_count - 1 - m]
            {
                m += 1;
            }
            m
        };

        let old_mid_start = top_match;
        let old_mid_end = old_count - bottom_match;
        let new_mid_start = top_match;
        let new_mid_end = new_count - bottom_match;
        let new_mid_count = new_mid_end - new_mid_start;

        let mut line_byte_starts = Vec::with_capacity(new_count);
        let mut offset = 0usize;
        for (idx, line) in new_lines.iter().enumerate() {
            line_byte_starts.push(offset);
            offset += line.len();
            if idx < new_count - 1 {
                offset += 1; // '\n'
            }
        }

        let mut replacement = Vec::with_capacity(new_mid_count);
        for idx in new_mid_start..new_mid_end {
            let line_text = new_lines[idx];
            let ending = if idx < new_count - 1 {
                LineEnding::Lf
            } else {
                LineEnding::None
            };
            let line_start = line_byte_starts[idx];
            let line_end = line_start + line_text.len();

            let mut attrs_list = AttrsList::new(&default_attrs);
            for span in spans {
                if span.end <= line_start || span.start >= line_end {
                    continue;
                }
                let local_start = span.start.saturating_sub(line_start);
                let local_end = (span.end - line_start).min(line_text.len());
                if local_start < local_end {
                    let a = default_attrs.clone().color(GlyphonColor::rgba(
                        span.color.r,
                        span.color.g,
                        span.color.b,
                        span.color.a,
                    ));
                    attrs_list.add_span(local_start..local_end, &a);
                }
            }
            replacement.push(BufferLine::new(
                line_text,
                ending,
                attrs_list,
                Shaping::Advanced,
            ));
        }

        let lines_updated = replacement.len();
        buffer.lines.splice(old_mid_start..old_mid_end, replacement);

        if top_match > 0 && old_count != new_count {
            let prev = top_match - 1;
            let ending = if prev < new_count - 1 {
                LineEnding::Lf
            } else {
                LineEnding::None
            };
            buffer.lines[prev].set_ending(ending);
        }

        buffer.set_redraw(true);
        lines_updated
    }

    /// Measure the total content width of an output text buffer.
    pub(super) fn measure_output_content_width(buf: &TextBuffer) -> f32 {
        let mut max_right = 0.0_f32;
        for run in buf.layout_runs() {
            for glyph in run.glyphs.iter() {
                max_right = max_right.max(glyph.x + glyph.w);
            }
        }
        max_right
    }

    /// Measure the total content width of an editor text buffer.
    pub(super) fn measure_editor_content_width(buf: &TextBuffer) -> f32 {
        let mut max_right = 0.0_f32;
        for run in buf.layout_runs() {
            for glyph in run.glyphs.iter() {
                max_right = max_right.max(glyph.x + glyph.w);
            }
        }
        max_right
    }

    pub(super) fn measure_content_height(buf: &TextBuffer) -> f32 {
        let mut max_bottom = 0.0_f32;
        for run in buf.layout_runs() {
            max_bottom = max_bottom.max(run.line_top + run.line_height);
        }
        max_bottom
    }
}
