/// A position in the document as (line, column), both 0-indexed.
/// Column is measured in chars (not bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Position {
    pub line: usize,
    pub col: usize,
}

impl Position {
    pub fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }

    pub fn zero() -> Self {
        Self { line: 0, col: 0 }
    }
}

impl PartialOrd for Position {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Position {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.line.cmp(&other.line).then(self.col.cmp(&other.col))
    }
}

/// A selection in the document. `anchor` is where the selection started,
/// `head` is where the cursor currently is. When anchor == head, it's a
/// simple cursor with no selected text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Position,
    pub head: Position,
}

impl Selection {
    /// A cursor at the given position (no selected text).
    pub fn cursor(pos: Position) -> Self {
        Self {
            anchor: pos,
            head: pos,
        }
    }

    /// A selection from anchor to head.
    pub fn new(anchor: Position, head: Position) -> Self {
        Self { anchor, head }
    }

    /// The earlier of anchor/head.
    pub fn start(&self) -> Position {
        std::cmp::min(self.anchor, self.head)
    }

    /// The later of anchor/head.
    pub fn end(&self) -> Position {
        std::cmp::max(self.anchor, self.head)
    }

    /// True if no text is selected (cursor only).
    pub fn is_cursor(&self) -> bool {
        self.anchor == self.head
    }

    /// True if the selection extends backward (head before anchor).
    pub fn is_backward(&self) -> bool {
        self.head < self.anchor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_ordering() {
        let a = Position::new(0, 5);
        let b = Position::new(1, 0);
        let c = Position::new(1, 3);
        assert!(a < b);
        assert!(b < c);
        assert!(a < c);
    }

    #[test]
    fn cursor_is_cursor() {
        let sel = Selection::cursor(Position::new(2, 4));
        assert!(sel.is_cursor());
        assert!(!sel.is_backward());
        assert_eq!(sel.start(), sel.end());
    }

    #[test]
    fn forward_selection() {
        let sel = Selection::new(Position::new(0, 0), Position::new(1, 5));
        assert!(!sel.is_cursor());
        assert!(!sel.is_backward());
        assert_eq!(sel.start(), Position::new(0, 0));
        assert_eq!(sel.end(), Position::new(1, 5));
    }

    #[test]
    fn backward_selection() {
        let sel = Selection::new(Position::new(3, 2), Position::new(1, 0));
        assert!(sel.is_backward());
        assert_eq!(sel.start(), Position::new(1, 0));
        assert_eq!(sel.end(), Position::new(3, 2));
    }
}
