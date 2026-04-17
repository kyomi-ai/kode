use crate::buffer::Buffer;
use crate::history::History;
use crate::selection::{Position, Selection};
use crate::transaction::{EditStep, Transaction};

/// The editor state machine. Ties together buffer, selection, and history.
/// This is the primary public API for kode-core.
#[derive(Debug)]
pub struct Editor {
    buffer: Buffer,
    selection: Selection,
    history: History,
    /// Desired column for vertical movement (remembered across up/down).
    sticky_col: Option<usize>,
}

impl Editor {
    /// Create an editor with the given text.
    pub fn new(text: &str) -> Self {
        Self {
            buffer: Buffer::from_text(text),
            selection: Selection::cursor(Position::zero()),
            history: History::new(),
            sticky_col: None,
        }
    }

    /// Create an empty editor.
    pub fn empty() -> Self {
        Self::new("")
    }

    // ── Accessors ────────────────────────────────────────────────────────

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn text(&self) -> String {
        self.buffer.text()
    }

    pub fn selection(&self) -> Selection {
        self.selection
    }

    pub fn cursor(&self) -> Position {
        self.selection.head
    }

    pub fn version(&self) -> u64 {
        self.buffer.version()
    }

    pub fn is_dirty(&self) -> bool {
        self.history.is_dirty()
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn mark_clean(&mut self) {
        self.history.mark_clean();
    }

    /// Get the selected text, or empty string if cursor only.
    pub fn selected_text(&self) -> String {
        if self.selection.is_cursor() {
            return String::new();
        }
        let start = self.buffer.pos_to_char(self.selection.start());
        let end = self.buffer.pos_to_char(self.selection.end());
        self.buffer.rope().slice(start..end).to_string()
    }

    // ── Editing commands ─────────────────────────────────────────────────

    /// Insert text at the cursor. If there's a selection, replace it.
    pub fn insert(&mut self, text: &str) {
        let cursor_before = self.cursor();
        let (offset, step) = if self.selection.is_cursor() {
            let offset = self.buffer.pos_to_char(self.cursor());
            let step = EditStep::insert(offset, text);
            (offset, step)
        } else {
            let start = self.buffer.pos_to_char(self.selection.start());
            let end = self.buffer.pos_to_char(self.selection.end());
            let deleted: String = self.buffer.rope().slice(start..end).to_string();
            let step = EditStep::replace(start, deleted, text.to_string());
            (start, step)
        };

        self.apply_step(&step);

        let new_offset = offset + text.chars().count();
        let new_pos = self.buffer.char_to_pos(new_offset);
        self.selection = Selection::cursor(new_pos);
        self.sticky_col = None;

        let tx = Transaction::single(step).with_cursors(cursor_before, new_pos);
        self.history.push(tx);
    }

    /// Delete the character before the cursor (backspace).
    pub fn backspace(&mut self) {
        if !self.selection.is_cursor() {
            self.delete_selection();
            return;
        }

        let offset = self.buffer.pos_to_char(self.cursor());
        if offset == 0 {
            return;
        }

        let cursor_before = self.cursor();
        let prev_char: String = self.buffer.rope().slice((offset - 1)..offset).to_string();
        let step = EditStep::delete(offset - 1, prev_char);

        self.apply_step(&step);
        let new_pos = self.buffer.char_to_pos(offset - 1);
        self.selection = Selection::cursor(new_pos);
        self.sticky_col = None;

        let tx = Transaction::single(step).with_cursors(cursor_before, new_pos);
        self.history.push(tx);
    }

    /// Delete the character after the cursor (forward delete).
    pub fn delete_forward(&mut self) {
        if !self.selection.is_cursor() {
            self.delete_selection();
            return;
        }

        let offset = self.buffer.pos_to_char(self.cursor());
        if offset >= self.buffer.len_chars() {
            return;
        }

        let cursor_before = self.cursor();
        let next_char: String = self.buffer.rope().slice(offset..(offset + 1)).to_string();
        let step = EditStep::delete(offset, next_char);

        self.apply_step(&step);
        // Cursor stays at same offset
        let new_pos = self.buffer.char_to_pos(offset);
        self.selection = Selection::cursor(new_pos);
        self.sticky_col = None;

        let tx = Transaction::single(step).with_cursors(cursor_before, new_pos);
        self.history.push(tx);
    }

    /// Delete the current selection. No-op if cursor only.
    pub fn delete_selection(&mut self) {
        if self.selection.is_cursor() {
            return;
        }

        let cursor_before = self.cursor();
        let start = self.buffer.pos_to_char(self.selection.start());
        let end = self.buffer.pos_to_char(self.selection.end());
        let deleted: String = self.buffer.rope().slice(start..end).to_string();
        let step = EditStep::delete(start, deleted);

        self.apply_step(&step);
        let new_pos = self.buffer.char_to_pos(start);
        self.selection = Selection::cursor(new_pos);
        self.sticky_col = None;

        let tx = Transaction::single(step).with_cursors(cursor_before, new_pos);
        self.history.push(tx);
    }

    /// Insert a newline at the cursor.
    pub fn insert_newline(&mut self) {
        self.insert("\n");
    }

    /// Apply a pre-built transaction atomically. All steps are applied
    /// and a single history entry is created for undo.
    ///
    /// Steps must contain **sequential offsets**: each step's offset is relative
    /// to the buffer state after all previous steps have been applied.
    pub fn apply_transaction(&mut self, tx: Transaction) {
        if tx.steps.is_empty() {
            return;
        }
        let cursor_before = self.cursor();
        for step in &tx.steps {
            self.apply_step(step);
        }
        // Place cursor at end of last inserted text
        let cursor_after = tx.steps.last().map(|step| {
            self.buffer.char_to_pos(step.offset + step.inserted_len())
        }).unwrap_or(cursor_before);
        self.selection = Selection::cursor(self.buffer.clamp_pos(cursor_after));
        self.sticky_col = None;

        let tx = tx.with_cursors(cursor_before, cursor_after);
        self.history.push(tx);
    }

    // ── Cursor movement ──────────────────────────────────────────────────

    /// Set the cursor to a position, collapsing any selection.
    pub fn set_cursor(&mut self, pos: Position) {
        let clamped = self.buffer.clamp_pos(pos);
        self.selection = Selection::cursor(clamped);
        self.sticky_col = None;
    }

    /// Set a selection range.
    pub fn set_selection(&mut self, anchor: Position, head: Position) {
        let anchor = self.buffer.clamp_pos(anchor);
        let head = self.buffer.clamp_pos(head);
        self.selection = Selection::new(anchor, head);
        self.sticky_col = None;
    }

    /// Extend the selection to a new head position (shift+arrow behavior).
    pub fn extend_selection(&mut self, head: Position) {
        let head = self.buffer.clamp_pos(head);
        self.selection = Selection::new(self.selection.anchor, head);
    }

    /// Select all text.
    pub fn select_all(&mut self) {
        let last_line = self.buffer.len_lines().saturating_sub(1);
        let last_col = self.buffer.line_len(last_line);
        self.selection = Selection::new(
            Position::zero(),
            Position::new(last_line, last_col),
        );
        self.sticky_col = None;
    }

    /// Move cursor left by one character.
    pub fn move_left(&mut self) {
        if !self.selection.is_cursor() {
            // Collapse to start of selection
            self.set_cursor(self.selection.start());
            return;
        }
        let offset = self.buffer.pos_to_char(self.cursor());
        if offset > 0 {
            self.set_cursor(self.buffer.char_to_pos(offset - 1));
        }
    }

    /// Move cursor right by one character.
    pub fn move_right(&mut self) {
        if !self.selection.is_cursor() {
            self.set_cursor(self.selection.end());
            return;
        }
        let offset = self.buffer.pos_to_char(self.cursor());
        if offset < self.buffer.len_chars() {
            self.set_cursor(self.buffer.char_to_pos(offset + 1));
        }
    }

    /// Move cursor up by one line.
    pub fn move_up(&mut self) {
        let pos = self.cursor();
        if pos.line == 0 {
            self.set_cursor(Position::new(0, 0));
            return;
        }
        let target_col = self.sticky_col.unwrap_or(pos.col);
        let new_line = pos.line - 1;
        let max_col = self.buffer.line_len(new_line);
        let new_pos = Position::new(new_line, target_col.min(max_col));
        self.selection = Selection::cursor(new_pos);
        self.sticky_col = Some(target_col);
    }

    /// Move cursor down by one line.
    pub fn move_down(&mut self) {
        let pos = self.cursor();
        let last_line = self.buffer.len_lines().saturating_sub(1);
        if pos.line >= last_line {
            let max_col = self.buffer.line_len(last_line);
            self.set_cursor(Position::new(last_line, max_col));
            return;
        }
        let target_col = self.sticky_col.unwrap_or(pos.col);
        let new_line = pos.line + 1;
        let max_col = self.buffer.line_len(new_line);
        let new_pos = Position::new(new_line, target_col.min(max_col));
        self.selection = Selection::cursor(new_pos);
        self.sticky_col = Some(target_col);
    }

    /// Move cursor to start of current line.
    pub fn move_to_line_start(&mut self) {
        self.set_cursor(Position::new(self.cursor().line, 0));
    }

    /// Move cursor to end of current line.
    pub fn move_to_line_end(&mut self) {
        let line = self.cursor().line;
        let col = self.buffer.line_len(line);
        self.set_cursor(Position::new(line, col));
    }

    /// Move cursor to start of document.
    pub fn move_to_start(&mut self) {
        self.set_cursor(Position::zero());
    }

    /// Move cursor to end of document.
    pub fn move_to_end(&mut self) {
        let last_line = self.buffer.len_lines().saturating_sub(1);
        let last_col = self.buffer.line_len(last_line);
        self.set_cursor(Position::new(last_line, last_col));
    }

    // ── Word movement ────────────────────────────────────────────────────

    /// Move cursor left to the start of the previous word.
    pub fn move_word_left(&mut self) {
        if !self.selection.is_cursor() {
            self.set_cursor(self.selection.start());
            return;
        }
        let pos = self.find_word_boundary_left();
        self.set_cursor(pos);
    }

    /// Move cursor right to the end of the next word.
    pub fn move_word_right(&mut self) {
        if !self.selection.is_cursor() {
            self.set_cursor(self.selection.end());
            return;
        }
        let pos = self.find_word_boundary_right();
        self.set_cursor(pos);
    }

    // ── Selection extension ──────────────────────────────────────────────

    /// Extend selection one character left (Shift+Left).
    pub fn extend_selection_left(&mut self) {
        let offset = self.buffer.pos_to_char(self.selection.head);
        if offset > 0 {
            let new_head = self.buffer.char_to_pos(offset - 1);
            self.selection = Selection::new(self.selection.anchor, new_head);
            self.sticky_col = None;
        }
    }

    /// Extend selection one character right (Shift+Right).
    pub fn extend_selection_right(&mut self) {
        let offset = self.buffer.pos_to_char(self.selection.head);
        if offset < self.buffer.len_chars() {
            let new_head = self.buffer.char_to_pos(offset + 1);
            self.selection = Selection::new(self.selection.anchor, new_head);
            self.sticky_col = None;
        }
    }

    /// Extend selection up one line (Shift+Up).
    pub fn extend_selection_up(&mut self) {
        let head = self.selection.head;
        if head.line == 0 {
            self.extend_selection(Position::new(0, 0));
            return;
        }
        let target_col = self.sticky_col.unwrap_or(head.col);
        let new_line = head.line - 1;
        let max_col = self.buffer.line_len(new_line);
        let new_head = Position::new(new_line, target_col.min(max_col));
        self.selection = Selection::new(self.selection.anchor, new_head);
        self.sticky_col = Some(target_col);
    }

    /// Extend selection down one line (Shift+Down).
    pub fn extend_selection_down(&mut self) {
        let head = self.selection.head;
        let last_line = self.buffer.len_lines().saturating_sub(1);
        if head.line >= last_line {
            let max_col = self.buffer.line_len(last_line);
            self.extend_selection(Position::new(last_line, max_col));
            return;
        }
        let target_col = self.sticky_col.unwrap_or(head.col);
        let new_line = head.line + 1;
        let max_col = self.buffer.line_len(new_line);
        let new_head = Position::new(new_line, target_col.min(max_col));
        self.selection = Selection::new(self.selection.anchor, new_head);
        self.sticky_col = Some(target_col);
    }

    /// Extend selection to word boundary left (Shift+Ctrl+Left).
    pub fn extend_selection_word_left(&mut self) {
        let pos = self.find_word_boundary_left();
        self.selection = Selection::new(self.selection.anchor, pos);
        self.sticky_col = None;
    }

    /// Extend selection to word boundary right (Shift+Ctrl+Right).
    pub fn extend_selection_word_right(&mut self) {
        let pos = self.find_word_boundary_right();
        self.selection = Selection::new(self.selection.anchor, pos);
        self.sticky_col = None;
    }

    /// Extend selection to line start (Shift+Home).
    pub fn extend_selection_to_line_start(&mut self) {
        let head = self.selection.head;
        self.selection = Selection::new(self.selection.anchor, Position::new(head.line, 0));
        self.sticky_col = None;
    }

    /// Extend selection to line end (Shift+End).
    pub fn extend_selection_to_line_end(&mut self) {
        let head = self.selection.head;
        let col = self.buffer.line_len(head.line);
        self.selection = Selection::new(self.selection.anchor, Position::new(head.line, col));
        self.sticky_col = None;
    }

    /// Extend selection to document start (Ctrl+Shift+Home).
    pub fn extend_selection_to_start(&mut self) {
        self.selection = Selection::new(self.selection.anchor, Position::zero());
        self.sticky_col = None;
    }

    /// Extend selection to document end (Ctrl+Shift+End).
    pub fn extend_selection_to_end(&mut self) {
        let last_line = self.buffer.len_lines().saturating_sub(1);
        let last_col = self.buffer.line_len(last_line);
        self.selection = Selection::new(self.selection.anchor, Position::new(last_line, last_col));
        self.sticky_col = None;
    }

    /// Move cursor up by N lines (PageUp).
    pub fn page_up(&mut self, page_lines: usize) {
        let pos = self.cursor();
        let target_col = self.sticky_col.unwrap_or(pos.col);
        let new_line = pos.line.saturating_sub(page_lines);
        let max_col = self.buffer.line_len(new_line);
        let new_pos = Position::new(new_line, target_col.min(max_col));
        self.selection = Selection::cursor(new_pos);
        self.sticky_col = Some(target_col);
    }

    /// Move cursor down by N lines (PageDown).
    pub fn page_down(&mut self, page_lines: usize) {
        let pos = self.cursor();
        let last_line = self.buffer.len_lines().saturating_sub(1);
        let target_col = self.sticky_col.unwrap_or(pos.col);
        let new_line = (pos.line + page_lines).min(last_line);
        let max_col = self.buffer.line_len(new_line);
        let new_pos = Position::new(new_line, target_col.min(max_col));
        self.selection = Selection::cursor(new_pos);
        self.sticky_col = Some(target_col);
    }

    // ── Smart selection ──────────────────────────────────────────────────

    /// Select the word at the cursor (double-click behavior).
    pub fn select_word(&mut self) {
        let offset = self.buffer.pos_to_char(self.cursor());
        let text = self.buffer.text();
        let chars: Vec<char> = text.chars().collect();

        if chars.is_empty() {
            return;
        }

        let idx = offset.min(chars.len().saturating_sub(1));
        let is_word = |c: char| c.is_alphanumeric() || c == '_';

        // If on whitespace or punctuation, select just that char
        if !is_word(chars[idx]) {
            let start = self.buffer.char_to_pos(idx);
            let end = self.buffer.char_to_pos(idx + 1);
            self.selection = Selection::new(start, end);
            self.sticky_col = None;
            return;
        }

        // Find word boundaries
        let mut start = idx;
        while start > 0 && is_word(chars[start - 1]) {
            start -= 1;
        }
        let mut end = idx;
        while end < chars.len() && is_word(chars[end]) {
            end += 1;
        }

        self.selection = Selection::new(
            self.buffer.char_to_pos(start),
            self.buffer.char_to_pos(end),
        );
        self.sticky_col = None;
    }

    /// Select the entire current line (triple-click behavior).
    pub fn select_line(&mut self) {
        let line = self.cursor().line;
        let line_start = Position::new(line, 0);
        let last_line = self.buffer.len_lines().saturating_sub(1);
        let line_end = if line < last_line {
            // Include the newline — select to start of next line
            Position::new(line + 1, 0)
        } else {
            Position::new(line, self.buffer.line_len(line))
        };
        self.selection = Selection::new(line_start, line_end);
        self.sticky_col = None;
    }

    // ── Word-level deletion ──────────────────────────────────────────────

    /// Delete from cursor to start of previous word (Ctrl+Backspace).
    pub fn delete_word_back(&mut self) {
        if !self.selection.is_cursor() {
            self.delete_selection();
            return;
        }
        let word_start = self.find_word_boundary_left();
        let cursor = self.cursor();
        if word_start != cursor {
            self.set_selection(word_start, cursor);
            self.delete_selection();
        }
    }

    /// Delete from cursor to end of next word (Ctrl+Delete).
    pub fn delete_word_forward(&mut self) {
        if !self.selection.is_cursor() {
            self.delete_selection();
            return;
        }
        let word_end = self.find_word_boundary_right();
        let cursor = self.cursor();
        if word_end != cursor {
            self.set_selection(cursor, word_end);
            self.delete_selection();
        }
    }

    // ── Indentation ──────────────────────────────────────────────────────

    /// Insert a tab (2 spaces) at the cursor, or indent all selected lines.
    pub fn indent(&mut self) {
        if self.selection.is_cursor() {
            self.insert("  ");
            return;
        }
        // Indent all lines in selection
        let start_line = self.selection.start().line;
        let end_line = self.selection.end().line;
        let mut steps = Vec::new();
        let mut offset_delta: isize = 0;
        for line in start_line..=end_line {
            let line_start = self.buffer.line_to_char(line);
            let adjusted = (line_start as isize + offset_delta) as usize;
            steps.push(EditStep::insert(adjusted, "  "));
            offset_delta += 2;
        }
        if !steps.is_empty() {
            self.apply_transaction(Transaction::new(steps));
        }
    }

    /// Remove one level of indentation (2 spaces) from the current or selected lines.
    pub fn outdent(&mut self) {
        let start_line = self.selection.start().line;
        let end_line = self.selection.end().line;
        let mut steps = Vec::new();
        let mut offset_delta: isize = 0;
        for line in start_line..=end_line {
            let line_text = self.buffer.line(line).to_string();
            let spaces = line_text.chars().take(2).take_while(|&c| c == ' ').count();
            if spaces > 0 {
                let line_start = self.buffer.line_to_char(line);
                let adjusted = (line_start as isize + offset_delta) as usize;
                let removed: String = line_text.chars().take(spaces).collect();
                steps.push(EditStep::delete(adjusted, removed));
                offset_delta -= spaces as isize;
            }
        }
        if !steps.is_empty() {
            self.apply_transaction(Transaction::new(steps));
        }
    }

    // ── Line operations ──────────────────────────────────────────────────

    /// Duplicate the current line or all selected lines (Ctrl+D).
    pub fn duplicate_lines(&mut self) {
        let start_line = self.selection.start().line;
        let end_line = self.selection.end().line;

        // Collect the text of all lines to duplicate
        let mut lines_text = String::new();
        for line in start_line..=end_line {
            let line_content = self.buffer.line(line).to_string();
            lines_text.push_str(&line_content);
        }
        // Ensure it ends with a newline
        if !lines_text.ends_with('\n') {
            lines_text.push('\n');
        }

        // Insert the duplicate after the last selected line
        let insert_after = if end_line < self.buffer.len_lines().saturating_sub(1) {
            self.buffer.line_to_char(end_line + 1)
        } else {
            // At end of doc — insert newline first
            self.buffer.len_chars()
        };

        let cursor_before = self.cursor();
        let step = EditStep::insert(insert_after, &lines_text);
        let tx = Transaction::single(step);
        self.apply_transaction(tx);

        // Move cursor to the duplicated region
        let _new_line = end_line + 1 + (self.cursor().line.saturating_sub(end_line).saturating_sub(1));
        let cursor_after = Position::new(cursor_before.line + (end_line - start_line + 1), cursor_before.col);
        self.set_cursor(self.buffer.clamp_pos(cursor_after));
    }

    /// Get the position at the start of the word before/at the cursor.
    /// Used by the autocomplete system to determine the prefix to replace.
    pub fn word_start_before_cursor(&self) -> Position {
        self.find_word_boundary_left()
    }

    // ── Private helpers ──────────────────────────────────────────────────

    /// Find the position of the word boundary to the left of the cursor head.
    fn find_word_boundary_left(&self) -> Position {
        let offset = self.buffer.pos_to_char(self.selection.head);
        if offset == 0 {
            return Position::zero();
        }
        let text = self.buffer.text();
        let chars: Vec<char> = text.chars().collect();
        let mut pos = offset;

        // Skip whitespace
        while pos > 0 && chars[pos - 1].is_whitespace() {
            pos -= 1;
        }
        // Skip word chars
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        if pos > 0 && is_word(chars[pos - 1]) {
            while pos > 0 && is_word(chars[pos - 1]) {
                pos -= 1;
            }
        } else if pos > 0 {
            // Skip punctuation
            while pos > 0 && !chars[pos - 1].is_whitespace() && !is_word(chars[pos - 1]) {
                pos -= 1;
            }
        }

        self.buffer.char_to_pos(pos)
    }

    /// Find the position of the word boundary to the right of the cursor head.
    fn find_word_boundary_right(&self) -> Position {
        let offset = self.buffer.pos_to_char(self.selection.head);
        let total = self.buffer.len_chars();
        if offset >= total {
            return self.buffer.char_to_pos(total);
        }
        let text = self.buffer.text();
        let chars: Vec<char> = text.chars().collect();
        let mut pos = offset;

        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        // Skip current word chars
        if pos < chars.len() && is_word(chars[pos]) {
            while pos < chars.len() && is_word(chars[pos]) {
                pos += 1;
            }
        } else if pos < chars.len() && !chars[pos].is_whitespace() {
            // Skip punctuation
            while pos < chars.len() && !chars[pos].is_whitespace() && !is_word(chars[pos]) {
                pos += 1;
            }
        }
        // Skip whitespace
        while pos < chars.len() && chars[pos].is_whitespace() {
            pos += 1;
        }

        self.buffer.char_to_pos(pos)
    }

    // ── Undo/Redo ────────────────────────────────────────────────────────

    /// Undo the last transaction.
    pub fn undo(&mut self) {
        if let Some(inverse) = self.history.undo() {
            for step in &inverse.steps {
                self.apply_step(step);
            }
            // Use stored cursor position if available, else compute from steps
            let pos = inverse
                .cursor_after
                .unwrap_or_else(|| {
                    inverse.steps.last().map(|step| {
                        self.buffer.char_to_pos(step.offset + step.inserted_len())
                    }).unwrap_or(Position::zero())
                });
            self.selection = Selection::cursor(self.buffer.clamp_pos(pos));
            self.sticky_col = None;
        }
    }

    /// Redo the last undone transaction.
    pub fn redo(&mut self) {
        if let Some(tx) = self.history.redo() {
            for step in &tx.steps {
                self.apply_step(step);
            }
            let pos = tx
                .cursor_after
                .unwrap_or_else(|| {
                    tx.steps.last().map(|step| {
                        self.buffer.char_to_pos(step.offset + step.inserted_len())
                    }).unwrap_or(Position::zero())
                });
            self.selection = Selection::cursor(self.buffer.clamp_pos(pos));
            self.sticky_col = None;
        }
    }

    // ── Internal ─────────────────────────────────────────────────────────

    /// Apply a single edit step to the buffer.
    fn apply_step(&mut self, step: &EditStep) {
        if !step.deleted.is_empty() && !step.inserted.is_empty() {
            self.buffer.replace(step.offset, step.offset + step.deleted_len(), &step.inserted);
        } else if !step.inserted.is_empty() {
            self.buffer.insert(step.offset, &step.inserted);
        } else if !step.deleted.is_empty() {
            self.buffer.delete(step.offset, step.offset + step.deleted_len());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_editor() {
        let ed = Editor::new("hello");
        assert_eq!(ed.text(), "hello");
        assert_eq!(ed.cursor(), Position::zero());
        assert!(!ed.is_dirty());
    }

    #[test]
    fn insert_at_cursor() {
        let mut ed = Editor::empty();
        ed.insert("hello");
        assert_eq!(ed.text(), "hello");
        assert_eq!(ed.cursor(), Position::new(0, 5));
        assert!(ed.is_dirty());
    }

    #[test]
    fn insert_multiline() {
        let mut ed = Editor::empty();
        ed.insert("line1\nline2");
        assert_eq!(ed.text(), "line1\nline2");
        assert_eq!(ed.cursor(), Position::new(1, 5));
    }

    #[test]
    fn backspace() {
        let mut ed = Editor::new("abc");
        ed.set_cursor(Position::new(0, 3));
        ed.backspace();
        assert_eq!(ed.text(), "ab");
        assert_eq!(ed.cursor(), Position::new(0, 2));
    }

    #[test]
    fn backspace_at_start() {
        let mut ed = Editor::new("abc");
        ed.set_cursor(Position::new(0, 0));
        ed.backspace();
        assert_eq!(ed.text(), "abc"); // no change
    }

    #[test]
    fn backspace_joins_lines() {
        let mut ed = Editor::new("abc\ndef");
        ed.set_cursor(Position::new(1, 0));
        ed.backspace();
        assert_eq!(ed.text(), "abcdef");
        assert_eq!(ed.cursor(), Position::new(0, 3));
    }

    #[test]
    fn delete_forward() {
        let mut ed = Editor::new("abc");
        ed.set_cursor(Position::new(0, 0));
        ed.delete_forward();
        assert_eq!(ed.text(), "bc");
        assert_eq!(ed.cursor(), Position::new(0, 0));
    }

    #[test]
    fn delete_selection() {
        let mut ed = Editor::new("hello world");
        ed.set_selection(Position::new(0, 5), Position::new(0, 11));
        ed.delete_selection();
        assert_eq!(ed.text(), "hello");
        assert_eq!(ed.cursor(), Position::new(0, 5));
    }

    #[test]
    fn insert_replaces_selection() {
        let mut ed = Editor::new("hello world");
        ed.set_selection(Position::new(0, 6), Position::new(0, 11));
        ed.insert("rust");
        assert_eq!(ed.text(), "hello rust");
        assert_eq!(ed.cursor(), Position::new(0, 10));
    }

    #[test]
    fn undo_redo() {
        let mut ed = Editor::empty();
        ed.insert("hello world");
        assert_eq!(ed.text(), "hello world");

        ed.undo();
        assert_eq!(ed.text(), "");

        ed.redo();
        assert_eq!(ed.text(), "hello world");
    }

    #[test]
    fn undo_coalesced_typing() {
        let mut ed = Editor::empty();
        ed.insert("h");
        ed.insert("e");
        ed.insert("l");
        ed.insert("l");
        ed.insert("o");

        // All coalesced into one undo
        ed.undo();
        assert_eq!(ed.text(), "");
    }

    #[test]
    fn undo_newline_breaks_coalescing() {
        let mut ed = Editor::empty();
        ed.insert("a");
        ed.insert("b");
        ed.insert("\n");
        ed.insert("c");

        // "c" is separate from "\n" which is separate from "ab"
        ed.undo();
        assert_eq!(ed.text(), "ab\n");
        ed.undo();
        assert_eq!(ed.text(), "ab");
        ed.undo();
        assert_eq!(ed.text(), "");
    }

    #[test]
    fn cursor_movement() {
        let mut ed = Editor::new("abc\ndef\nghi");

        ed.move_to_end();
        assert_eq!(ed.cursor(), Position::new(2, 3));

        ed.move_to_start();
        assert_eq!(ed.cursor(), Position::new(0, 0));

        ed.move_right();
        assert_eq!(ed.cursor(), Position::new(0, 1));

        ed.move_to_line_end();
        assert_eq!(ed.cursor(), Position::new(0, 3));

        ed.move_down();
        assert_eq!(ed.cursor(), Position::new(1, 3));

        ed.move_to_line_start();
        assert_eq!(ed.cursor(), Position::new(1, 0));
    }

    #[test]
    fn sticky_column_on_vertical_movement() {
        let mut ed = Editor::new("long line\nhi\nlong line");
        ed.set_cursor(Position::new(0, 8));

        ed.move_down(); // "hi" only has 2 chars
        assert_eq!(ed.cursor(), Position::new(1, 2));

        ed.move_down(); // back to long line, should restore col 8
        assert_eq!(ed.cursor(), Position::new(2, 8));
    }

    #[test]
    fn select_all() {
        let mut ed = Editor::new("abc\ndef");
        ed.select_all();
        assert_eq!(ed.selected_text(), "abc\ndef");
    }

    #[test]
    fn move_left_collapses_selection() {
        let mut ed = Editor::new("hello");
        ed.set_selection(Position::new(0, 1), Position::new(0, 4));
        ed.move_left();
        assert!(ed.selection().is_cursor());
        assert_eq!(ed.cursor(), Position::new(0, 1));
    }

    #[test]
    fn move_right_collapses_selection() {
        let mut ed = Editor::new("hello");
        ed.set_selection(Position::new(0, 1), Position::new(0, 4));
        ed.move_right();
        assert!(ed.selection().is_cursor());
        assert_eq!(ed.cursor(), Position::new(0, 4));
    }

    #[test]
    fn dirty_tracking() {
        let mut ed = Editor::new("hello");
        assert!(!ed.is_dirty());

        ed.insert(" world");
        assert!(ed.is_dirty());

        ed.mark_clean();
        assert!(!ed.is_dirty());

        ed.insert("!");
        assert!(ed.is_dirty());

        ed.undo();
        assert!(!ed.is_dirty());
    }

    #[test]
    fn unicode_editing() {
        let mut ed = Editor::new("café");
        ed.set_cursor(Position::new(0, 4));
        ed.backspace();
        assert_eq!(ed.text(), "caf");

        ed.insert("é");
        assert_eq!(ed.text(), "café");
    }

    #[test]
    fn empty_doc_operations() {
        let mut ed = Editor::empty();
        ed.backspace(); // no-op
        ed.delete_forward(); // no-op
        ed.move_left(); // no-op
        ed.move_up(); // no-op
        assert_eq!(ed.text(), "");
        assert_eq!(ed.cursor(), Position::zero());
    }

    #[test]
    fn extend_selection() {
        let mut ed = Editor::new("hello world");
        ed.set_cursor(Position::new(0, 0));
        ed.extend_selection(Position::new(0, 5));
        assert_eq!(ed.selected_text(), "hello");
        assert!(!ed.selection().is_cursor());
    }

    #[test]
    fn multiple_undo_redo_cycles() {
        let mut ed = Editor::empty();
        ed.insert("a");
        ed.insert("b");
        ed.insert("c");
        // All coalesced
        assert_eq!(ed.text(), "abc");

        ed.undo();
        assert_eq!(ed.text(), "");

        ed.redo();
        assert_eq!(ed.text(), "abc");

        // New edit clears redo
        ed.insert("d");
        assert!(!ed.can_redo());
        assert_eq!(ed.text(), "abcd");
    }

    #[test]
    fn apply_transaction_multi_step() {
        let mut ed = Editor::new("Hello\nWorld");
        // Steps with sequential offsets (each relative to buffer after previous step)
        // Step 1: replace "Hello" (offset 0, 5 chars) with "> Hello" (7 chars)
        // Step 2: replace "World" (offset 8 in post-step-1 buffer) with "> World"
        let tx = Transaction::new(vec![
            EditStep::replace(0, "Hello".to_string(), "> Hello".to_string()),
            EditStep::replace(8, "World".to_string(), "> World".to_string()),
        ]);
        ed.apply_transaction(tx);
        assert_eq!(ed.text(), "> Hello\n> World");

        // Undo should restore original
        ed.undo();
        assert_eq!(ed.text(), "Hello\nWorld");

        // Redo should re-apply
        ed.redo();
        assert_eq!(ed.text(), "> Hello\n> World");
    }

    #[test]
    fn apply_transaction_empty() {
        let mut ed = Editor::new("hello");
        ed.apply_transaction(Transaction::new(vec![]));
        assert_eq!(ed.text(), "hello");
        // Should not create an undo entry
        assert!(!ed.can_undo());
    }

    #[test]
    fn undo_forward_delete_cursor_position() {
        let mut ed = Editor::new("hello world");
        ed.set_cursor(Position::new(0, 5));
        ed.delete_forward(); // delete " "
        ed.delete_forward(); // delete "w"
        ed.delete_forward(); // delete "o"
        assert_eq!(ed.text(), "hellorld");

        ed.undo();
        // Cursor should be back at col 5 (where forward-deleting began)
        assert_eq!(ed.cursor(), Position::new(0, 5));
    }

    // ── New command tests ────────────────────────────────────────────

    #[test]
    fn word_movement() {
        let mut ed = Editor::new("hello world foo");
        ed.set_cursor(Position::new(0, 0));

        ed.move_word_right();
        assert_eq!(ed.cursor(), Position::new(0, 6)); // after "hello "

        ed.move_word_right();
        assert_eq!(ed.cursor(), Position::new(0, 12)); // after "world "

        ed.move_word_left();
        assert_eq!(ed.cursor(), Position::new(0, 6)); // back to "world"
    }

    #[test]
    fn select_word() {
        let mut ed = Editor::new("hello world");
        ed.set_cursor(Position::new(0, 7)); // inside "world"
        ed.select_word();
        assert_eq!(ed.selected_text(), "world");
    }

    #[test]
    fn select_line() {
        let mut ed = Editor::new("line1\nline2\nline3");
        ed.set_cursor(Position::new(1, 2));
        ed.select_line();
        assert_eq!(ed.selected_text(), "line2\n");
    }

    #[test]
    fn extend_selection_directions() {
        let mut ed = Editor::new("abc\ndef");
        ed.set_cursor(Position::new(0, 1));

        ed.extend_selection_right();
        assert_eq!(ed.selected_text(), "b");

        ed.extend_selection_right();
        assert_eq!(ed.selected_text(), "bc");

        ed.extend_selection_left();
        assert_eq!(ed.selected_text(), "b");

        ed.extend_selection_down();
        // From anchor (0,1) to head (1,2) — sticky_col is 2 from prior extends
        assert_eq!(ed.selected_text(), "bc\nde");
    }

    #[test]
    fn delete_word_back() {
        let mut ed = Editor::new("hello world");
        ed.set_cursor(Position::new(0, 11));
        ed.delete_word_back();
        assert_eq!(ed.text(), "hello ");
    }

    #[test]
    fn delete_word_forward() {
        let mut ed = Editor::new("hello world");
        ed.set_cursor(Position::new(0, 0));
        ed.delete_word_forward();
        assert_eq!(ed.text(), "world");
    }

    #[test]
    fn indent_outdent() {
        let mut ed = Editor::new("line1\nline2");
        ed.set_selection(Position::new(0, 0), Position::new(1, 5));
        ed.indent();
        assert_eq!(ed.text(), "  line1\n  line2");

        // Undo should be atomic
        ed.undo();
        assert_eq!(ed.text(), "line1\nline2");

        // Re-indent then outdent
        ed.set_selection(Position::new(0, 0), Position::new(1, 5));
        ed.indent();
        ed.set_selection(Position::new(0, 0), Position::new(1, 7));
        ed.outdent();
        assert_eq!(ed.text(), "line1\nline2");
    }

    #[test]
    fn duplicate_lines() {
        let mut ed = Editor::new("line1\nline2\nline3");
        ed.set_cursor(Position::new(1, 0)); // on line2
        ed.duplicate_lines();
        assert_eq!(ed.text(), "line1\nline2\nline2\nline3");
    }

    #[test]
    fn extend_selection_word() {
        let mut ed = Editor::new("hello world");
        ed.set_cursor(Position::new(0, 0));
        ed.extend_selection_word_right();
        // From start: skips "hello", skips space → lands at 6
        assert_eq!(ed.selected_text(), "hello ");
    }

    #[test]
    fn extend_selection_to_line_bounds() {
        let mut ed = Editor::new("hello world");
        ed.set_cursor(Position::new(0, 5));
        ed.extend_selection_to_line_start();
        assert_eq!(ed.selected_text(), "hello");

        ed.set_cursor(Position::new(0, 5));
        ed.extend_selection_to_line_end();
        assert_eq!(ed.selected_text(), " world");
    }

    #[test]
    fn word_start_before_cursor_at_end_of_dotted() {
        let mut ed = Editor::new("foo.bar");
        ed.set_cursor(Position::new(0, 7)); // end of "bar"
        let pos = ed.word_start_before_cursor();
        assert_eq!(pos, Position::new(0, 4)); // start of "bar"
    }

    #[test]
    fn word_start_before_cursor_at_col_zero() {
        let mut ed = Editor::new("hello");
        ed.set_cursor(Position::new(0, 0));
        let pos = ed.word_start_before_cursor();
        assert_eq!(pos, Position::new(0, 0));
    }

    #[test]
    fn word_start_before_cursor_middle_of_word() {
        let mut ed = Editor::new("hello");
        ed.set_cursor(Position::new(0, 3)); // middle of "hello"
        let pos = ed.word_start_before_cursor();
        assert_eq!(pos, Position::new(0, 0)); // start of "hello"
    }
}

