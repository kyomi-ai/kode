use ropey::Rope;

use crate::Position;

/// The text buffer, wrapping a ropey::Rope with position conversion helpers.
#[derive(Debug, Clone)]
pub struct Buffer {
    rope: Rope,
    version: u64,
}

impl Buffer {
    /// Create a buffer from a string.
    pub fn from_text(text: &str) -> Self {
        Self {
            rope: Rope::from_str(text),
            version: 0,
        }
    }

    /// Create an empty buffer.
    pub fn new() -> Self {
        Self {
            rope: Rope::new(),
            version: 0,
        }
    }

    /// The underlying rope.
    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    /// Document version, incremented on every edit.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Total number of chars in the document.
    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    /// Total number of lines (always >= 1).
    pub fn len_lines(&self) -> usize {
        self.rope.len_lines()
    }

    /// True if the document is empty.
    pub fn is_empty(&self) -> bool {
        self.rope.len_chars() == 0
    }

    /// Get the full text as a String.
    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    /// Get text of a specific line (0-indexed), including trailing newline if present.
    pub fn line(&self, line_idx: usize) -> ropey::RopeSlice<'_> {
        self.rope.line(line_idx)
    }

    /// Number of chars in a given line (excluding trailing line-ending chars).
    /// Handles \n, \r\n, and lone \r.
    pub fn line_len(&self, line_idx: usize) -> usize {
        let line = self.rope.line(line_idx);
        let len = line.len_chars();
        if len == 0 {
            return 0;
        }
        // Strip \n (covers both \n and \r\n cases via the \r check below)
        let len = if line.char(len - 1) == '\n' { len - 1 } else { len };
        // Strip a preceding \r (handles \r\n pairs) or a lone \r as line separator
        if len > 0 && line.char(len - 1) == '\r' {
            len - 1
        } else {
            len
        }
    }

    /// Total number of bytes in the document.
    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    /// Convert a char offset to a byte offset.
    pub fn char_to_byte(&self, char_idx: usize) -> usize {
        let idx = char_idx.min(self.len_chars());
        self.rope.char_to_byte(idx)
    }

    /// Convert a byte offset to a char offset.
    pub fn byte_to_char(&self, byte_idx: usize) -> usize {
        let idx = byte_idx.min(self.len_bytes());
        self.rope.byte_to_char(idx)
    }

    /// Get the char offset of the start of a line.
    pub fn line_to_char(&self, line_idx: usize) -> usize {
        let line = line_idx.min(self.len_lines().saturating_sub(1));
        self.rope.line_to_char(line)
    }

    /// Convert a Position (line, col) to a char offset in the rope.
    /// Clamps to valid range.
    pub fn pos_to_char(&self, pos: Position) -> usize {
        let line = pos.line.min(self.len_lines().saturating_sub(1));
        let line_start = self.rope.line_to_char(line);
        let max_col = self.line_len(line);
        let col = pos.col.min(max_col);
        line_start + col
    }

    /// Convert a char offset to a Position (line, col).
    pub fn char_to_pos(&self, char_idx: usize) -> Position {
        let idx = char_idx.min(self.len_chars());
        let line = self.rope.char_to_line(idx);
        let line_start = self.rope.line_to_char(line);
        Position::new(line, idx - line_start)
    }

    /// Clamp a Position to valid document bounds.
    pub fn clamp_pos(&self, pos: Position) -> Position {
        let line = pos.line.min(self.len_lines().saturating_sub(1));
        let max_col = self.line_len(line);
        Position::new(line, pos.col.min(max_col))
    }

    /// Insert text at a char offset. Returns the new version.
    pub(crate) fn insert(&mut self, char_idx: usize, text: &str) -> u64 {
        let idx = char_idx.min(self.len_chars());
        self.rope.insert(idx, text);
        self.version += 1;
        self.version
    }

    /// Delete a range of chars [start, end). Returns the deleted text and new version.
    pub(crate) fn delete(&mut self, start: usize, end: usize) -> (String, u64) {
        let s = start.min(self.len_chars());
        let e = end.min(self.len_chars());
        if s >= e {
            return (String::new(), self.version);
        }
        let deleted: String = self.rope.slice(s..e).to_string();
        self.rope.remove(s..e);
        self.version += 1;
        (deleted, self.version)
    }

    /// Replace a range [start, end) with new text. Returns deleted text and new version.
    pub(crate) fn replace(&mut self, start: usize, end: usize, text: &str) -> (String, u64) {
        let s = start.min(self.len_chars());
        let e = end.min(self.len_chars());
        let deleted: String = if s < e {
            let d = self.rope.slice(s..e).to_string();
            self.rope.remove(s..e);
            d
        } else {
            String::new()
        };
        self.rope.insert(s, text);
        self.version += 1;
        (deleted, self.version)
    }
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_buffer() {
        let buf = Buffer::new();
        assert!(buf.is_empty());
        assert_eq!(buf.len_chars(), 0);
        assert_eq!(buf.len_lines(), 1);
        assert_eq!(buf.text(), "");
    }

    #[test]
    fn from_str_basic() {
        let buf = Buffer::from_text("hello\nworld");
        assert_eq!(buf.len_lines(), 2);
        assert_eq!(buf.line_len(0), 5); // "hello" without \n
        assert_eq!(buf.line_len(1), 5); // "world"
        assert_eq!(buf.len_chars(), 11);
    }

    #[test]
    fn pos_to_char_and_back() {
        let buf = Buffer::from_text("abc\ndef\nghi");
        assert_eq!(buf.pos_to_char(Position::new(0, 0)), 0);
        assert_eq!(buf.pos_to_char(Position::new(0, 3)), 3);
        assert_eq!(buf.pos_to_char(Position::new(1, 0)), 4);
        assert_eq!(buf.pos_to_char(Position::new(2, 2)), 10);

        assert_eq!(buf.char_to_pos(0), Position::new(0, 0));
        assert_eq!(buf.char_to_pos(4), Position::new(1, 0));
        assert_eq!(buf.char_to_pos(10), Position::new(2, 2));
    }

    #[test]
    fn pos_clamping() {
        let buf = Buffer::from_text("ab\ncd");
        // Line past end
        assert_eq!(buf.clamp_pos(Position::new(99, 0)), Position::new(1, 0));
        // Col past end of line
        assert_eq!(buf.clamp_pos(Position::new(0, 99)), Position::new(0, 2));
    }

    #[test]
    fn insert_and_version() {
        let mut buf = Buffer::from_text("hello");
        assert_eq!(buf.version(), 0);
        buf.insert(5, " world");
        assert_eq!(buf.version(), 1);
        assert_eq!(buf.text(), "hello world");
    }

    #[test]
    fn delete_range() {
        let mut buf = Buffer::from_text("hello world");
        let (deleted, _) = buf.delete(5, 11);
        assert_eq!(deleted, " world");
        assert_eq!(buf.text(), "hello");
    }

    #[test]
    fn replace_range() {
        let mut buf = Buffer::from_text("hello world");
        let (deleted, _) = buf.replace(6, 11, "rust");
        assert_eq!(deleted, "world");
        assert_eq!(buf.text(), "hello rust");
    }

    #[test]
    fn unicode_positions() {
        let buf = Buffer::from_text("café\n日本語");
        assert_eq!(buf.len_lines(), 2);
        assert_eq!(buf.line_len(0), 4); // c-a-f-é = 4 chars
        assert_eq!(buf.line_len(1), 3); // 日本語 = 3 chars
        assert_eq!(buf.pos_to_char(Position::new(1, 2)), 7); // 5 (café\n) + 2
    }

    #[test]
    fn emoji_handling() {
        let buf = Buffer::from_text("hi 👋🏽 there");
        // 👋🏽 is 2 chars (wave + skin tone modifier)
        let text = buf.text();
        assert_eq!(text, "hi 👋🏽 there");
    }

    // ── CRLF / lone-CR bugs ───────────────────────────────────────────────

    /// line_len must exclude the \r in a CRLF line, not just the \n.
    /// Currently returns 4 for "foo\r\n" (includes \r); should return 3.
    #[test]
    fn crlf_line_len_excludes_cr() {
        let buf = Buffer::from_text("foo\r\nbar");
        assert_eq!(buf.line_len(0), 3, "CRLF: line_len should be 3, not 4");
    }

    /// With the current bug, pos_to_char(line=0, col=line_len(0)) resolves to
    /// the '\n' char itself (offset 4), not the position before \r\n (offset 3).
    #[test]
    fn crlf_pos_to_char_at_line_end_is_before_cr() {
        let buf = Buffer::from_text("foo\r\nbar");
        let end_col = buf.line_len(0);
        let offset = buf.pos_to_char(Position::new(0, end_col));
        // Expected: offset 3 (before \r). Current bug: offset 4 (\n itself).
        assert_eq!(offset, 3, "CRLF: end-of-line offset should be before \\r");
    }

    /// Lone \r is a line separator in ropey. line_len must not include it.
    /// Currently returns 4 for "foo\r" (ropey line 0 = "foo\r", len=4, no \n to strip).
    #[test]
    fn lone_cr_line_len_excludes_cr() {
        let buf = Buffer::from_text("foo\rbar");
        assert_eq!(buf.line_len(0), 3, "lone \\r: line_len should be 3, not 4");
    }

    /// With the lone-\r bug, pos_to_char at max col jumps to the first char of
    /// the *next* line because col 4 on line 0 of "foo\rbar" = char offset 4 = 'b'.
    #[test]
    fn lone_cr_pos_to_char_stays_on_same_line() {
        let buf = Buffer::from_text("foo\rbar");
        let end_col = buf.line_len(0);
        let offset = buf.pos_to_char(Position::new(0, end_col));
        // Expected: offset 3 (before \r). Current bug: offset 4 = 'b' (next line!).
        assert_eq!(offset, 3, "lone \\r: end-of-line offset must not escape into next line");
    }
}
