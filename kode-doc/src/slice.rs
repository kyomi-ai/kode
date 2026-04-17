//! Slice — a fragment of document content with open sides.
//!
//! A [`Slice`] represents a portion of a document that may have been cut
//! through node boundaries. The `open_start` and `open_end` fields record
//! how many node levels are "open" (not closed) at each side. This metadata
//! is essential for correct insertion — open sides get joined with the
//! surrounding document structure rather than creating new nested nodes.

use crate::fragment::Fragment;

/// A slice of document content that may be "open" on either side.
///
/// When content is cut from the middle of the tree, the slice records
/// how many node levels are open (not closed) at the start and end.
/// This is essential for correct insertion — open sides get joined
/// with the surrounding nodes.
///
/// Example: cutting "llo wo" from `<p>Hello</p><p>World</p>` produces:
/// - content: Fragment([text("llo"), text("wo")])
/// - open_start: 1 (we cut through the first paragraph)
/// - open_end: 1 (we cut through the second paragraph)
///
/// When inserting this slice, the replace algorithm uses the open sides
/// to join "llo" with the content before the insertion point and "wo"
/// with the content after.
#[derive(Clone, Debug)]
pub struct Slice {
    /// The content of this slice.
    pub content: Fragment,
    /// How many node levels are open (not closed) at the start.
    pub open_start: usize,
    /// How many node levels are open (not closed) at the end.
    pub open_end: usize,
}

impl Slice {
    /// Create a new slice.
    ///
    /// # Panics
    ///
    /// Panics if `open_start` or `open_end` exceed the nesting depth of the
    /// content on their respective sides.
    pub fn new(content: Fragment, open_start: usize, open_end: usize) -> Self {
        assert!(
            open_start <= max_open_start(&content),
            "open_start ({open_start}) exceeds content nesting depth ({})",
            max_open_start(&content)
        );
        assert!(
            open_end <= max_open_end(&content),
            "open_end ({open_end}) exceeds content nesting depth ({})",
            max_open_end(&content)
        );
        Slice {
            content,
            open_start,
            open_end,
        }
    }

    /// An empty slice (no content, no open sides).
    pub fn empty() -> Self {
        Slice {
            content: Fragment::empty(),
            open_start: 0,
            open_end: 0,
        }
    }

    /// The "net" size of the slice — content size minus the open tokens.
    ///
    /// This is the number of tokens that will be added to the document
    /// when the slice is inserted (open sides join with existing content
    /// rather than adding new open/close tokens).
    pub fn size(&self) -> usize {
        self.content.size().saturating_sub(self.open_start + self.open_end)
    }

    /// Whether this slice has no content.
    pub fn is_empty(&self) -> bool {
        self.content.size() == 0
    }
}

/// Maximum valid `open_start` for a fragment — the nesting depth on the left side.
fn max_open_start(content: &Fragment) -> usize {
    if content.child_count() == 0 {
        return 0;
    }
    let first = content.child(0);
    // is_leaf() covers text nodes, hard breaks, images, etc.
    if first.node_type.is_leaf() {
        return 0;
    }
    1 + max_open_start(&first.content)
}

/// Maximum valid `open_end` for a fragment — the nesting depth on the right side.
fn max_open_end(content: &Fragment) -> usize {
    if content.child_count() == 0 {
        return 0;
    }
    let last = content.child(content.child_count() - 1);
    if last.node_type.is_leaf() {
        return 0;
    }
    1 + max_open_end(&last.content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;
    use crate::node_type::NodeType;

    #[test]
    fn empty_slice() {
        let s = Slice::empty();
        assert_eq!(s.size(), 0);
        assert!(s.is_empty());
        assert_eq!(s.open_start, 0);
        assert_eq!(s.open_end, 0);
    }

    #[test]
    fn slice_with_text_content() {
        // Simple text fragment, no open sides.
        let content = Fragment::from_node(Node::new_text("Hello"));
        let s = Slice::new(content, 0, 0);
        assert_eq!(s.size(), 5);
        assert!(!s.is_empty());
    }

    #[test]
    fn slice_with_open_sides() {
        // Fragment containing a paragraph with text — open_start=1, open_end=1
        // means we cut through the paragraph on both sides.
        // <p>llo wo</p> has size 1 + 6 + 1 = 8
        // Net size = 8 - 1 - 1 = 6 (the text content)
        let content = Fragment::from_node(Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("llo wo")),
        ));
        let s = Slice::new(content, 1, 1);
        assert_eq!(s.size(), 6);
        assert!(!s.is_empty());
    }

    #[test]
    fn slice_with_two_open_paragraphs() {
        // Two paragraphs: [<p>llo</p>, <p>wo</p>]
        // <p>llo</p> = 1+3+1 = 5, <p>wo</p> = 1+2+1 = 4, total = 9
        // open_start=1, open_end=1 (cut through both paragraphs)
        // Net size = 9 - 1 - 1 = 7
        let p1 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("llo")),
        );
        let p2 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("wo")),
        );
        let content = Fragment::from_vec(vec![p1, p2]);
        let s = Slice::new(content, 1, 1);
        assert_eq!(s.size(), 7);
    }

    #[test]
    fn slice_zero_open_with_full_blocks() {
        // A full paragraph — open_start=0, open_end=0.
        let content = Fragment::from_node(Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hello")),
        ));
        let s = Slice::new(content, 0, 0);
        // 1 + 5 + 1 = 7
        assert_eq!(s.size(), 7);
    }

    #[test]
    #[should_panic(expected = "open_start (2) exceeds content nesting depth")]
    fn slice_panics_on_excessive_open_start() {
        // Text-only content has max depth 0.
        let content = Fragment::from_node(Node::new_text("Hi"));
        Slice::new(content, 2, 0);
    }

    #[test]
    #[should_panic(expected = "open_end (2) exceeds content nesting depth")]
    fn slice_panics_on_excessive_open_end() {
        let content = Fragment::from_node(Node::new_text("Hi"));
        Slice::new(content, 0, 2);
    }
}
