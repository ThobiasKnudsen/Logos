pub mod cell;

pub use cell::CodeCell;

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
        self.text
            .split('\n')
            .nth(target_line)
            .unwrap_or("")
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
}
