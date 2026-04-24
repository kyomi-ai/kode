//! Document state for the WYSIWYG editor.
//!
//! [`DocState`] is the WYSIWYG-mode equivalent of a markdown editor state. It
//! holds the current document tree, selection (cursor position), and edit
//! history for undo/redo. It provides the editing API that the WYSIWYG
//! component calls for keyboard events and toolbar actions.
//!
//! All edits go through [`Transform`] internally, which means:
//! - Every edit produces a step map for position mapping
//! - Every edit is invertible (undo)
//! - The document tree is always structurally valid

mod clipboard;
mod editing;
mod formatting;
mod selection;
mod undo;

#[cfg(test)]
mod tests;

use crate::fragment::Fragment;
use crate::node::Node;
use crate::node_type::NodeType;
use crate::parse::parse_markdown;
use crate::serialize::serialize_markdown;

// ── Helpers ────────────────────────────────────────────────────────────────

/// Ensure the document has at least one block child.
///
/// An empty `Doc` has no valid cursor position for the editor, so we
/// always bootstrap one empty `Paragraph` — matching ProseMirror's
/// invariant that every document must contain at least one block node.
///
/// This is applied at the **editor layer** (`DocState`), not in the
/// parser, so the parser can still faithfully represent an empty input.
fn ensure_min_content(doc: Node) -> Node {
    if doc.child_count() == 0 {
        Node::branch(
            NodeType::Doc,
            Fragment::from_node(Node::branch(NodeType::Paragraph, Fragment::empty())),
        )
    } else {
        doc
    }
}

// ── Selection ──────────────────────────────────────────────────────────────

/// Cursor/selection in the document.
///
/// A selection has an `anchor` (where the user started selecting) and a `head`
/// (where the cursor currently is). When `anchor == head`, the selection is
/// collapsed to a cursor.
#[derive(Clone, Debug, PartialEq)]
pub struct Selection {
    /// The anchor position (where the selection started).
    pub anchor: usize,
    /// The head position (where the cursor is / selection extends to).
    pub head: usize,
}

impl Selection {
    /// Create a cursor (collapsed selection) at a position.
    pub fn cursor(pos: usize) -> Self {
        Selection {
            anchor: pos,
            head: pos,
        }
    }

    /// Create a range selection.
    pub fn range(anchor: usize, head: usize) -> Self {
        Selection { anchor, head }
    }

    /// Whether this is a cursor (no range selected).
    pub fn is_cursor(&self) -> bool {
        self.anchor == self.head
    }

    /// The leftmost position.
    pub fn from(&self) -> usize {
        self.anchor.min(self.head)
    }

    /// The rightmost position.
    pub fn to(&self) -> usize {
        self.anchor.max(self.head)
    }
}

// ── FormattingState ────────────────────────────────────────────────────────

/// Formatting state for toolbar active buttons.
///
/// Reflects the inline marks and block type at the current cursor position.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormattingState {
    /// Whether bold (strong) mark is active.
    pub bold: bool,
    /// Whether italic (em) mark is active.
    pub italic: bool,
    /// Whether inline code mark is active.
    pub code: bool,
    /// Whether strikethrough mark is active.
    pub strikethrough: bool,
    /// Heading level (0 if not a heading).
    pub heading_level: u8,
    /// Whether the cursor is inside a bullet list item.
    pub bullet_list: bool,
    /// Whether the cursor is inside an ordered list item.
    pub ordered_list: bool,
    /// Whether the cursor is inside a blockquote.
    pub blockquote: bool,
}

// ── HistoryEntry ───────────────────────────────────────────────────────────

/// An entry in the undo/redo history.
pub(super) struct HistoryEntry {
    pub(super) doc: Node,
    pub(super) selection: Selection,
}

// ── DocState ───────────────────────────────────────────────────────────────

/// Document state for the WYSIWYG editor.
///
/// Holds the current document tree, selection, and edit history. Provides
/// the editing API that the WYSIWYG component calls for keyboard events
/// and toolbar actions.
pub struct DocState {
    /// The current document tree.
    pub(super) doc: Node,
    /// Cursor/selection state.
    pub(super) selection: Selection,
    /// Undo stack (previous doc + selection states).
    pub(super) undo_stack: Vec<HistoryEntry>,
    /// Redo stack.
    pub(super) redo_stack: Vec<HistoryEntry>,
}

impl DocState {
    // ── Constructors ───────────────────────────────────────────────────

    /// Create a new `DocState` from a markdown string.
    pub fn from_markdown(markdown: &str) -> Self {
        let doc = parse_markdown(markdown);
        Self::from_doc(doc)
    }

    /// Create from an existing document tree.
    ///
    /// If the document has no children, a single empty `Paragraph` is
    /// inserted so the editor always has a valid cursor home. The cursor
    /// is placed at position 1 (inside the first textblock).
    pub fn from_doc(doc: Node) -> Self {
        let doc = ensure_min_content(doc);
        DocState {
            doc,
            selection: Selection::cursor(1),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    // ── Accessors ──────────────────────────────────────────────────────

    /// Get the current document.
    pub fn doc(&self) -> &Node {
        &self.doc
    }

    /// Get the current selection.
    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// Set the selection/cursor position.
    ///
    /// Automatically adjusts both anchor and head into the nearest textblock
    /// if they land on structural positions (between blocks, on closing tokens).
    pub fn set_selection(&mut self, selection: Selection) {
        let anchor = self.adjust_into_textblock(selection.anchor);
        let head = self.adjust_into_textblock(selection.head);
        self.selection = Selection { anchor, head };
    }

    /// Serialize the document to markdown.
    pub fn to_markdown(&self) -> String {
        serialize_markdown(&self.doc)
    }

    /// Replace the document from a markdown string (used when switching from
    /// Source mode).
    pub fn set_from_markdown(&mut self, markdown: &str) {
        self.push_undo();
        self.doc = ensure_min_content(parse_markdown(markdown));
        self.selection = Selection::cursor(1);
        self.redo_stack.clear();
    }
}
