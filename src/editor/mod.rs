pub mod autocomplete;

/// A simple text buffer with cursor and selection for basic editing.
///
/// This is a starter implementation using a plain `String`.
/// Future: replace with a rope (e.g. `ropey`) for large documents,
/// add undo/redo, multi-cursor, etc.
pub struct Buffer {
    text: String,
    /// Cursor position as byte offset into `text`.
    cursor: usize,
    /// When set, selection spans from this anchor to `cursor`.
    /// The visible selection range is `min(anchor, cursor)..max(anchor, cursor)`.
    selection_anchor: Option<usize>,
}

impl Buffer {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            selection_anchor: None,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor_byte_offset(&self) -> usize {
        self.cursor
    }

    pub fn set_text(&mut self, text: &str) {
        self.text = text.to_string();
        self.cursor = self.text.len();
        self.selection_anchor = None;
    }

    // -----------------------------------------------------------------------
    // Selection
    // -----------------------------------------------------------------------

    /// Returns the ordered (start, end) byte range of the selection, or `None`.
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        self.selection_anchor.and_then(|anchor| {
            let start = anchor.min(self.cursor);
            let end = anchor.max(self.cursor);
            if start == end {
                None
            } else {
                Some((start, end))
            }
        })
    }

    /// Returns the selected text, or `None` if nothing is selected.
    pub fn selected_text(&self) -> Option<&str> {
        self.selection_range().map(|(s, e)| &self.text[s..e])
    }

    /// Select the entire buffer contents.
    pub fn select_all(&mut self) {
        self.selection_anchor = Some(0);
        self.cursor = self.text.len();
    }

    pub fn clear_selection(&mut self) {
        self.selection_anchor = None;
    }

    /// Begin extending a selection from the current cursor, if not already started.
    fn begin_selection(&mut self) {
        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        }
    }

    /// Delete the selected range and collapse the cursor. Returns `true` if
    /// something was deleted.
    fn delete_selection(&mut self) -> bool {
        if let Some((start, end)) = self.selection_range() {
            self.text.drain(start..end);
            self.cursor = start;
            self.selection_anchor = None;
            true
        } else {
            self.selection_anchor = None;
            false
        }
    }

    // -----------------------------------------------------------------------
    // Editing (selection-aware)
    // -----------------------------------------------------------------------

    pub fn insert(&mut self, ch: char) {
        self.delete_selection();
        self.text.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    /// Insert a multi-character string at the cursor, replacing any selection.
    pub fn insert_text(&mut self, s: &str) {
        self.delete_selection();
        self.text.insert_str(self.cursor, s);
        self.cursor += s.len();
    }

    /// Insert a tab character, replacing any selection.
    pub fn insert_tab(&mut self) {
        self.insert('\t');
    }

    /// Insert a newline with auto-indentation based on bracket nesting depth.
    /// Counts unmatched `(`, `[`, `{` before the cursor to determine indent level.
    pub fn insert_newline_auto_indent(&mut self) {
        self.delete_selection();
        let depth = self.paren_depth_at_cursor();
        let indent = "\t".repeat(depth);
        let to_insert = format!("\n{}", indent);
        self.text.insert_str(self.cursor, &to_insert);
        self.cursor += to_insert.len();
    }

    /// Count the net nesting depth of brackets before the cursor.
    fn paren_depth_at_cursor(&self) -> usize {
        let before = &self.text[..self.cursor];
        let mut depth: i32 = 0;
        for ch in before.chars() {
            match ch {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                _ => {}
            }
        }
        depth.max(0) as usize
    }

    pub fn backspace(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        if self.cursor == 0 {
            return false;
        }
        // Find the previous char boundary.
        let prev = self.text[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.text.drain(prev..self.cursor);
        self.cursor = prev;
        true
    }

    /// Replace bytes in `start..end` with `replacement`. Indices are clamped
    /// to the text length and snapped to char boundaries; if `start > end`
    /// after clamping, this is a no-op.
    pub fn replace_range(&mut self, start: usize, end: usize, replacement: &str) {
        self.clear_selection();
        let len = self.text.len();
        let mut start = start.min(len);
        let mut end = end.min(len);
        while start > 0 && !self.text.is_char_boundary(start) {
            start -= 1;
        }
        while end < len && !self.text.is_char_boundary(end) {
            end += 1;
        }
        if start > end {
            return;
        }
        self.text.drain(start..end);
        self.text.insert_str(start, replacement);
        self.cursor = start + replacement.len();
    }

    pub fn delete(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        if self.cursor >= self.text.len() {
            return false;
        }
        let next = self.cursor
            + self.text[self.cursor..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
        self.text.drain(self.cursor..next);
        true
    }

    // -----------------------------------------------------------------------
    // Movement (with optional selection extension)
    // -----------------------------------------------------------------------

    pub fn move_left(&mut self, extend: bool) -> bool {
        if extend {
            self.begin_selection();
        } else if let Some((start, _)) = self.selection_range() {
            self.cursor = start;
            self.clear_selection();
            return true;
        } else {
            self.clear_selection();
        }
        if self.cursor == 0 {
            return false;
        }
        self.cursor = self.text[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
        true
    }

    pub fn move_right(&mut self, extend: bool) -> bool {
        if extend {
            self.begin_selection();
        } else if let Some((_, end)) = self.selection_range() {
            self.cursor = end;
            self.clear_selection();
            return true;
        } else {
            self.clear_selection();
        }
        if self.cursor >= self.text.len() {
            return false;
        }
        self.cursor += self.text[self.cursor..]
            .chars()
            .next()
            .map(|c| c.len_utf8())
            .unwrap_or(0);
        true
    }

    pub fn move_up(&mut self, extend: bool) -> bool {
        if extend {
            self.begin_selection();
        } else {
            self.clear_selection();
        }
        let (line, col) = self.line_col();
        if line == 0 {
            return false;
        }
        let target_line = line - 1;
        self.cursor = self.byte_offset_at(target_line, col);
        true
    }

    pub fn move_down(&mut self, extend: bool) -> bool {
        if extend {
            self.begin_selection();
        } else {
            self.clear_selection();
        }
        let (line, col) = self.line_col();
        let line_count = self.text.lines().count().max(1);
        // Account for trailing newline.
        let total_lines = if self.text.ends_with('\n') {
            line_count + 1
        } else {
            line_count
        };
        if line + 1 >= total_lines {
            return false;
        }
        self.cursor = self.byte_offset_at(line + 1, col);
        true
    }

    pub fn move_home(&mut self, extend: bool) -> bool {
        if extend {
            self.begin_selection();
        } else {
            self.clear_selection();
        }
        let (line, _) = self.line_col();
        let new_cursor = self.byte_offset_at(line, 0);
        if new_cursor == self.cursor {
            return false;
        }
        self.cursor = new_cursor;
        true
    }

    pub fn move_end(&mut self, extend: bool) -> bool {
        if extend {
            self.begin_selection();
        } else {
            self.clear_selection();
        }
        let (line, _) = self.line_col();
        let line_text = self.line_text(line);
        let line_len = line_text.chars().count();
        let new_cursor = self.byte_offset_at(line, line_len);
        if new_cursor == self.cursor {
            return false;
        }
        self.cursor = new_cursor;
        true
    }

    // --- Helpers ---

    /// Returns (line_index, column_as_char_count) for the current cursor.
    fn line_col(&self) -> (usize, usize) {
        let before_cursor = &self.text[..self.cursor];
        let line = before_cursor.matches('\n').count();
        let line_start = before_cursor.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let col = before_cursor[line_start..].chars().count();
        (line, col)
    }

    /// Returns the byte offset for a given (line, char_column), clamped to line length.
    fn byte_offset_at(&self, target_line: usize, target_col: usize) -> usize {
        let mut offset = 0;
        for (i, line) in self.text.split('\n').enumerate() {
            if i == target_line {
                let col_clamped = target_col.min(line.chars().count());
                let byte_col: usize = line.chars().take(col_clamped).map(|c| c.len_utf8()).sum();
                return offset + byte_col;
            }
            offset += line.len() + 1; // +1 for the '\n'
        }
        // Past last line — clamp to end.
        self.text.len()
    }

    fn line_text(&self, target_line: usize) -> &str {
        self.text.split('\n').nth(target_line).unwrap_or("")
    }

    /// Set cursor to a specific byte offset, clamping to text length and
    /// snapping to a valid char boundary. Clears any selection.
    pub fn set_cursor_byte(&mut self, byte: usize) {
        self.cursor = byte.min(self.text.len());
        while self.cursor > 0 && !self.text.is_char_boundary(self.cursor) {
            self.cursor -= 1;
        }
        self.selection_anchor = None;
    }

    /// Set cursor to a specific byte offset while extending the selection.
    /// If no selection anchor exists, the current cursor becomes the anchor
    /// before moving (used for shift+click and drag-to-select).
    pub fn set_cursor_byte_extend(&mut self, byte: usize) {
        self.begin_selection();
        self.cursor = byte.min(self.text.len());
        while self.cursor > 0 && !self.text.is_char_boundary(self.cursor) {
            self.cursor -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(text: &str) -> Buffer {
        let mut b = Buffer::new();
        b.set_text(text);
        b
    }

    #[test]
    fn set_text_places_cursor_at_end() {
        let b = buffer("hello");
        assert_eq!(b.cursor_byte_offset(), 5);
    }

    #[test]
    fn insert_advances_cursor_by_utf8_len() {
        let mut b = Buffer::new();
        b.insert('π');
        assert_eq!(b.text(), "π");
        assert_eq!(b.cursor_byte_offset(), 'π'.len_utf8());
    }

    #[test]
    fn backspace_deletes_full_codepoint() {
        let mut b = buffer("aπb");
        b.set_cursor_byte(b.text().len());
        b.backspace(); // remove 'b'
        assert_eq!(b.text(), "aπ");
        b.backspace(); // remove 'π' (2 bytes) atomically
        assert_eq!(b.text(), "a");
    }

    #[test]
    fn move_left_right_respect_codepoint_boundaries() {
        let mut b = buffer("πq");
        b.set_cursor_byte(0);
        b.move_right(false);
        assert_eq!(b.cursor_byte_offset(), 'π'.len_utf8());
        b.move_left(false);
        assert_eq!(b.cursor_byte_offset(), 0);
    }

    #[test]
    fn delete_selection_via_backspace() {
        let mut b = buffer("hello world");
        b.set_cursor_byte(0);
        b.set_cursor_byte_extend(5);
        assert_eq!(b.selected_text(), Some("hello"));
        b.backspace(); // selection-delete path
        assert_eq!(b.text(), " world");
    }

    #[test]
    fn replace_range_clamps_oob_indices() {
        let mut b = buffer("abc");
        b.replace_range(1, 9999, "XY"); // end past length
        assert_eq!(b.text(), "aXY");
        b.replace_range(9999, 9999, "Z"); // both past length
        assert_eq!(b.text(), "aXYZ");
    }

    #[test]
    fn replace_range_snaps_to_char_boundaries() {
        let mut b = buffer("aπb"); // 'π' is 2 bytes, so byte 1..2 is mid-codepoint
        b.replace_range(1, 2, "X"); // start=1 (boundary), end=2 (mid-π) → snaps end to 3
        assert_eq!(b.text(), "aXb");
    }

    #[test]
    fn replace_range_noop_if_start_after_end() {
        let mut b = buffer("hello");
        b.replace_range(3, 1, "X"); // inverted indices → no-op
        assert_eq!(b.text(), "hello");
    }

    #[test]
    fn select_all_spans_full_buffer() {
        let mut b = buffer("hello");
        b.select_all();
        assert_eq!(b.selected_text(), Some("hello"));
    }

    #[test]
    fn move_home_goes_to_line_start() {
        let mut b = buffer("foo\nbar");
        b.move_end(false);
        b.move_home(false);
        let line_start = b.text().find('\n').unwrap() + 1;
        assert_eq!(b.cursor_byte_offset(), line_start);
    }

    #[test]
    fn paren_depth_for_auto_indent() {
        // `(` and `{` both unmatched → depth 2 → two tabs of indent on newline.
        let mut b = buffer("f({a := 1");
        b.set_cursor_byte(b.text().len());
        b.insert_newline_auto_indent();
        assert!(b.text().ends_with("\n\t\t"), "got: {:?}", b.text());

        // Single unmatched `(` → 1 tab.
        let mut b = buffer("if (x > 0");
        b.set_cursor_byte(b.text().len());
        b.insert_newline_auto_indent();
        assert!(b.text().ends_with("\n\t"), "got: {:?}", b.text());
    }

    #[test]
    fn set_cursor_byte_snaps_to_char_boundary() {
        let mut b = buffer("aπb");
        b.set_cursor_byte(2); // mid-π → snaps to 1
        assert_eq!(b.cursor_byte_offset(), 1);
    }
}
