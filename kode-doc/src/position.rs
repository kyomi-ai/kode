//! Token-based position resolution.
//!
//! A [`ResolvedPos`] maps a single integer position to its location in the
//! document tree — which nodes contain it, at what depth, and what child index.
//! Created by [`Node::resolve(pos)`](crate::node::Node::resolve).
//!
//! Positions are relative to a node's *content* — the opening and closing
//! tokens of the root node are not counted. For a document with content size
//! `N`, valid positions are `0..=N`.

use crate::mark::Mark;
use crate::node::Node;

/// An entry in the path from the root to the resolved position.
///
/// Each entry records the ancestor node, the child index within that ancestor
/// that contains the position, and the absolute start position of that
/// ancestor's content.
#[derive(Clone, Debug)]
struct PathEntry {
    /// The ancestor node at this depth.
    node: Node,
    /// Which child of this ancestor contains (or borders) the position.
    index: usize,
    /// Absolute start position of this ancestor's content.
    start: usize,
}

/// A resolved position in the document tree.
///
/// Created by [`Node::resolve(pos)`](crate::node::Node::resolve). Provides
/// tree context for a position: which nodes contain it, at what depth, what
/// child index, etc.
///
/// # Position model
///
/// Positions are integers in the range `[0, doc.content.size()]`. Position 0 is
/// the start of the document's content (before the first child). Each branch
/// node contributes an opening token (+1) and a closing token (+1) to the
/// position space; each text character is one token; each non-text leaf is one
/// token.
#[derive(Clone, Debug)]
pub struct ResolvedPos {
    /// Absolute position in the document.
    pub pos: usize,
    /// Nesting depth (0 = doc level).
    pub depth: usize,
    /// Offset within the innermost parent node's content.
    pub parent_offset: usize,
    /// Path from root to innermost parent: one entry per depth level.
    path: Vec<PathEntry>,
}

impl ResolvedPos {
    /// The ancestor node at the given depth.
    ///
    /// `depth=0` is the document root, `depth=self.depth` is the innermost
    /// parent.
    ///
    /// # Panics
    ///
    /// Panics if `depth > self.depth`.
    pub fn node(&self, depth: usize) -> &Node {
        assert!(
            depth <= self.depth,
            "depth {depth} exceeds resolved depth {}",
            self.depth
        );
        &self.path[depth].node
    }

    /// The innermost parent node (shortcut for `node(self.depth)`).
    pub fn parent(&self) -> &Node {
        self.node(self.depth)
    }

    /// Child index at the given depth — which child of the ancestor at `depth`
    /// contains (or borders) this position.
    ///
    /// # Panics
    ///
    /// Panics if `depth > self.depth`.
    pub fn index(&self, depth: usize) -> usize {
        assert!(
            depth <= self.depth,
            "depth {depth} exceeds resolved depth {}",
            self.depth
        );
        self.path[depth].index
    }

    /// Absolute start position of the content of the node at the given depth.
    ///
    /// For `depth=0` (doc), this is 0.
    ///
    /// # Panics
    ///
    /// Panics if `depth > self.depth`.
    pub fn start(&self, depth: usize) -> usize {
        assert!(
            depth <= self.depth,
            "depth {depth} exceeds resolved depth {}",
            self.depth
        );
        self.path[depth].start
    }

    /// Absolute end position of the content of the node at the given depth.
    ///
    /// `end = start + node.content.size`
    ///
    /// # Panics
    ///
    /// Panics if `depth > self.depth`.
    pub fn end(&self, depth: usize) -> usize {
        self.start(depth) + self.node(depth).content.size()
    }

    /// Absolute position just before the node at the given depth — the opening
    /// token of that node.
    ///
    /// `before = start - 1`
    ///
    /// # Panics
    ///
    /// Panics if `depth == 0` (the document has no opening token in the
    /// position space) or if `depth > self.depth`.
    pub fn before(&self, depth: usize) -> usize {
        assert!(depth > 0, "before() is not defined for depth 0 (the document root)");
        self.start(depth) - 1
    }

    /// Absolute position just after the node at the given depth — the closing
    /// token of that node.
    ///
    /// `after = end + 1`
    ///
    /// # Panics
    ///
    /// Panics if `depth == 0` or if `depth > self.depth`.
    pub fn after(&self, depth: usize) -> usize {
        assert!(depth > 0, "after() is not defined for depth 0 (the document root)");
        self.end(depth) + 1
    }

    /// Offset into a text node at this position.
    ///
    /// If the position is inside a text node, returns the character offset
    /// within that text node. If the position is between nodes (not inside
    /// text), returns 0.
    pub fn text_offset(&self) -> usize {
        let parent = self.parent();
        let idx = self.index(self.depth);
        // Check if position is inside a text child.
        // parent_offset tells us where we are in the parent's content.
        // Walk children to find if we're inside a text node.
        let mut offset = 0;
        for i in 0..idx {
            offset += parent.child(i).node_size();
        }
        // If we're past all children or at a boundary, text_offset is 0.
        if idx >= parent.child_count() {
            return 0;
        }
        let child = parent.child(idx);
        if child.is_text() {
            // We're inside this text node. The offset into it is parent_offset - offset.
            self.parent_offset - offset
        } else {
            0
        }
    }

    /// The node immediately after this position, if any.
    ///
    /// Returns the child node that starts at this position, or `None` if the
    /// position is at the end of the parent's content or inside a text node.
    pub fn node_after(&self) -> Option<&Node> {
        let parent = self.parent();
        let idx = self.index(self.depth);
        if idx >= parent.child_count() {
            return None;
        }
        let child = parent.child(idx);
        // If the offset within the parent equals the start of this child,
        // the position is right before this child.
        let mut child_start = 0;
        for i in 0..idx {
            child_start += parent.child(i).node_size();
        }
        if self.parent_offset == child_start {
            Some(child)
        } else if child.is_text() {
            // We're inside a text node — no node_after.
            None
        } else {
            // We're inside a branch child (shouldn't happen at this depth),
            // but for safety return None.
            None
        }
    }

    /// The node immediately before this position, if any.
    ///
    /// Returns the child node that ends at this position, or `None` if the
    /// position is at the start of the parent's content or inside a text node.
    pub fn node_before(&self) -> Option<&Node> {
        let parent = self.parent();
        let idx = self.index(self.depth);

        // Check if we're at the exact start of the child at `idx`.
        let mut child_start = 0;
        for i in 0..idx {
            child_start += parent.child(i).node_size();
        }

        if self.parent_offset == child_start && idx > 0 {
            // Position is exactly at the boundary before child[idx],
            // so child[idx-1] is the node before.
            Some(parent.child(idx - 1))
        } else if idx < parent.child_count() && parent.child(idx).is_text() {
            // Inside a text node — no discrete node before.
            None
        } else {
            None
        }
    }

    /// The marks active at this position.
    ///
    /// If inside a text node, returns that node's marks. If at a boundary
    /// between nodes, returns the marks of the adjacent text node, preferring
    /// `node_after` — this matches ProseMirror's convention that typing at a
    /// boundary inherits marks from the node the cursor is entering, not the
    /// one it is leaving.
    ///
    /// Returns empty if no adjacent text node exists.
    pub fn marks(&self) -> Vec<Mark> {
        let parent = self.parent();
        let idx = self.index(self.depth);

        // If inside a text node (not at a child boundary), return its marks.
        if idx < parent.child_count() {
            let child = parent.child(idx);
            if child.is_text() {
                let mut child_start = 0;
                for i in 0..idx {
                    child_start += parent.child(i).node_size();
                }
                if self.parent_offset > child_start {
                    return child.marks.clone();
                }
            }
        }

        // At a boundary: prefer node_after, then node_before.
        // ProseMirror convention — at boundaries, marks come from the node
        // the cursor would enter (node_after) rather than the one it leaves.
        if idx < parent.child_count() {
            let after = parent.child(idx);
            if after.is_text() {
                return after.marks.clone();
            }
        }

        if idx > 0 {
            let before = parent.child(idx - 1);
            if before.is_text() {
                return before.marks.clone();
            }
        }

        Vec::new()
    }

    /// Find the deepest common ancestor depth with another position.
    ///
    /// Returns the largest `depth` such that `self.start(depth) <= other_pos`
    /// and `self.end(depth) >= other_pos`, i.e. both positions are contained
    /// within the node at that depth.
    pub fn shared_depth(&self, other_pos: usize) -> usize {
        for d in (0..=self.depth).rev() {
            // Uses `>=` (not `>`) because a position at `end` is still
            // within the node — it sits right before the closing token.
            // For example, in <p>Hello</p> with end=6, position 6 is the
            // valid "end of content" position inside <p>.
            if self.start(d) <= other_pos && self.end(d) >= other_pos {
                return d;
            }
        }
        0
    }

    /// Whether this position and another resolved position are in the same
    /// parent node at the same depth.
    pub fn same_parent(&self, other: &ResolvedPos) -> bool {
        self.depth == other.depth && self.start(self.depth) == other.start(other.depth)
    }

    // ── Internal construction ──────────────────────────────────────────

    /// Resolve a position within a document node.
    ///
    /// Walks from the root into the tree, descending into children until the
    /// position is at a boundary or inside a text/leaf node.
    pub(crate) fn resolve(doc: &Node, pos: usize) -> ResolvedPos {
        assert!(
            pos <= doc.content.size(),
            "position {pos} out of range for document of content size {}",
            doc.content.size()
        );

        let mut path = Vec::new();
        let mut current_node = doc;
        let mut remaining = pos;
        let mut abs_start = 0;

        // Push the document (depth 0).
        // We'll fill in the index after we know which child it falls into.
        loop {
            let (child_idx, child_offset, child_start) =
                current_node.content.find_index(remaining);

            // Push this level to the path.
            path.push(PathEntry {
                node: current_node.clone(),
                index: child_idx,
                start: abs_start,
            });

            // If we're at a boundary (child_offset == 0 at a child boundary,
            // or past all children), or the child is a text/leaf node, stop.
            if child_idx >= current_node.child_count() {
                // Past all children — position is at the end of content.
                break;
            }

            let child = current_node.child(child_idx);

            if child.is_text() {
                // Inside a text node — stop descending.
                break;
            }

            if child.node_type.is_leaf() {
                // Non-text leaf (hr, hard_break, image). Position is at this leaf.
                break;
            }

            // child_offset is the offset into the child's node_size range.
            // 0 means we're at the child's opening token — position is between
            // nodes at this level, not inside the child.
            if child_offset == 0 {
                break;
            }

            // Descend into the branch child.
            // child_offset=1 means we're just past the opening token (start of content).
            // The child's content starts at abs_start + child_start + 1 (opening token).
            let child_abs_start = abs_start + child_start + 1;

            remaining = child_offset - 1; // offset within the child's content
            abs_start = child_abs_start;
            current_node = child;
        }

        let depth = path.len() - 1;
        let parent_offset = remaining;

        ResolvedPos {
            pos,
            depth,
            parent_offset,
            path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attrs::heading_attrs;
    use crate::fragment::Fragment;
    use crate::mark::{Mark, MarkType};
    use crate::node_type::NodeType;

    // ── Helper builders ────────────────────────────────────────────────

    /// Build: <doc><p>Hello</p></doc>
    /// Content size: 1 + 5 + 1 = 7
    /// Positions (within doc content): 0..=7
    ///   pos 0: before <p>
    ///   pos 1: start of p content (before 'H')
    ///   pos 2: after 'H', before 'e'
    ///   pos 6: end of p content (after 'o')
    ///   pos 7: after </p>
    fn simple_doc() -> Node {
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hello")),
        );
        Node::branch(NodeType::Doc, Fragment::from_node(p))
    }

    /// Build: <doc><p>Hi</p><hr/><p>World</p></doc>
    /// Content size: (1+2+1) + 1 + (1+5+1) = 4 + 1 + 7 = 12
    fn multi_block_doc() -> Node {
        let p1 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hi")),
        );
        let hr = Node::leaf(NodeType::HorizontalRule);
        let p2 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("World")),
        );
        Node::branch(NodeType::Doc, Fragment::from_vec(vec![p1, hr, p2]))
    }

    /// Build: <doc><blockquote><p>Text</p></blockquote></doc>
    /// Content size: 1 + (1 + (1+4+1) + 1) + 1 = bq_size = 1+6+1 = 8, doc content = 8
    fn nested_doc() -> Node {
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Text")),
        );
        let bq = Node::branch(NodeType::Blockquote, Fragment::from_node(p));
        Node::branch(NodeType::Doc, Fragment::from_node(bq))
    }

    /// Build: <doc><p><strong>Bold</strong>Normal</p></doc>
    fn marked_doc() -> Node {
        let bold = Node::new_text_with_marks("Bold", vec![Mark::new(MarkType::Strong)]);
        let normal = Node::new_text("Normal");
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_vec(vec![bold, normal]),
        );
        Node::branch(NodeType::Doc, Fragment::from_node(p))
    }

    // ── Basic resolution tests ─────────────────────────────────────────

    #[test]
    fn resolve_pos_0_simple_doc() {
        // pos 0: before <p>, at doc level
        let doc = simple_doc();
        let rp = doc.resolve(0);
        assert_eq!(rp.pos, 0);
        assert_eq!(rp.depth, 0);
        assert_eq!(rp.parent_offset, 0);
        assert_eq!(rp.parent().node_type, NodeType::Doc);
    }

    #[test]
    fn resolve_pos_1_inside_paragraph() {
        // pos 1: start of p content (just past opening token of p)
        let doc = simple_doc();
        let rp = doc.resolve(1);
        assert_eq!(rp.pos, 1);
        assert_eq!(rp.depth, 1);
        assert_eq!(rp.parent_offset, 0);
        assert_eq!(rp.parent().node_type, NodeType::Paragraph);
    }

    #[test]
    fn resolve_pos_mid_text() {
        // pos 3: inside "Hello" at character index 2 (after "He")
        let doc = simple_doc();
        let rp = doc.resolve(3);
        assert_eq!(rp.pos, 3);
        assert_eq!(rp.depth, 1);
        assert_eq!(rp.parent_offset, 2);
        assert_eq!(rp.parent().node_type, NodeType::Paragraph);
    }

    #[test]
    fn resolve_pos_end_of_paragraph_content() {
        // pos 6: end of p content (after 'o' in "Hello")
        let doc = simple_doc();
        let rp = doc.resolve(6);
        assert_eq!(rp.pos, 6);
        assert_eq!(rp.depth, 1);
        assert_eq!(rp.parent_offset, 5);
        assert_eq!(rp.parent().node_type, NodeType::Paragraph);
    }

    #[test]
    fn resolve_pos_after_paragraph() {
        // pos 7: after </p>, at doc level
        let doc = simple_doc();
        let rp = doc.resolve(7);
        assert_eq!(rp.pos, 7);
        assert_eq!(rp.depth, 0);
        assert_eq!(rp.parent_offset, 7);
        assert_eq!(rp.parent().node_type, NodeType::Doc);
    }

    // ── Multi-block document ───────────────────────────────────────────

    #[test]
    fn resolve_multi_block_pos_0() {
        // Before first <p>
        let doc = multi_block_doc();
        let rp = doc.resolve(0);
        assert_eq!(rp.depth, 0);
        assert_eq!(rp.parent_offset, 0);
        assert_eq!(rp.index(0), 0);
    }

    #[test]
    fn resolve_multi_block_inside_first_p() {
        // pos 1: start of first p content
        let doc = multi_block_doc();
        let rp = doc.resolve(1);
        assert_eq!(rp.depth, 1);
        assert_eq!(rp.parent_offset, 0);
        assert_eq!(rp.parent().node_type, NodeType::Paragraph);
        assert_eq!(rp.parent().text_content(), "Hi");
    }

    #[test]
    fn resolve_multi_block_between_p_and_hr() {
        // pos 4: after first </p>, before <hr>
        let doc = multi_block_doc();
        let rp = doc.resolve(4);
        assert_eq!(rp.depth, 0);
        assert_eq!(rp.parent_offset, 4);
        assert_eq!(rp.index(0), 1); // hr is child index 1
    }

    #[test]
    fn resolve_multi_block_after_hr() {
        // pos 5: after <hr>, before second <p>
        let doc = multi_block_doc();
        let rp = doc.resolve(5);
        assert_eq!(rp.depth, 0);
        assert_eq!(rp.parent_offset, 5);
        assert_eq!(rp.index(0), 2); // second p is child index 2
    }

    #[test]
    fn resolve_multi_block_inside_second_p() {
        // pos 6: start of second p content
        let doc = multi_block_doc();
        let rp = doc.resolve(6);
        assert_eq!(rp.depth, 1);
        assert_eq!(rp.parent_offset, 0);
        assert_eq!(rp.parent().node_type, NodeType::Paragraph);
        assert_eq!(rp.parent().text_content(), "World");
    }

    #[test]
    fn resolve_multi_block_end() {
        // pos 12: end of doc content
        let doc = multi_block_doc();
        let rp = doc.resolve(12);
        assert_eq!(rp.depth, 0);
        assert_eq!(rp.parent_offset, 12);
    }

    // ── Nested document (blockquote) ───────────────────────────────────

    #[test]
    fn resolve_nested_pos_0() {
        // pos 0: before <blockquote>
        let doc = nested_doc();
        let rp = doc.resolve(0);
        assert_eq!(rp.depth, 0);
        assert_eq!(rp.parent().node_type, NodeType::Doc);
    }

    #[test]
    fn resolve_nested_inside_blockquote() {
        // pos 1: inside blockquote, before <p>
        let doc = nested_doc();
        let rp = doc.resolve(1);
        assert_eq!(rp.depth, 1);
        assert_eq!(rp.parent().node_type, NodeType::Blockquote);
        assert_eq!(rp.parent_offset, 0);
    }

    #[test]
    fn resolve_nested_inside_paragraph() {
        // pos 2: inside paragraph, start of content
        let doc = nested_doc();
        let rp = doc.resolve(2);
        assert_eq!(rp.depth, 2);
        assert_eq!(rp.parent().node_type, NodeType::Paragraph);
        assert_eq!(rp.parent_offset, 0);
    }

    #[test]
    fn resolve_nested_inside_text() {
        // pos 4: inside "Text" at character 2 ("Te|xt")
        let doc = nested_doc();
        let rp = doc.resolve(4);
        assert_eq!(rp.depth, 2);
        assert_eq!(rp.parent().node_type, NodeType::Paragraph);
        assert_eq!(rp.parent_offset, 2);
    }

    // ── node(), parent(), index(), start(), end() ──────────────────────

    #[test]
    fn node_at_depth() {
        let doc = nested_doc();
        let rp = doc.resolve(3); // inside paragraph text
        assert_eq!(rp.node(0).node_type, NodeType::Doc);
        assert_eq!(rp.node(1).node_type, NodeType::Blockquote);
        assert_eq!(rp.node(2).node_type, NodeType::Paragraph);
    }

    #[test]
    fn index_at_depth() {
        // <doc><p>Hi</p><hr/><p>World</p></doc>
        // Resolve pos 6 (start of second p content)
        let doc = multi_block_doc();
        let rp = doc.resolve(6);
        assert_eq!(rp.index(0), 2); // second p is child[2] of doc
        assert_eq!(rp.index(1), 0); // "World" is child[0] of p
    }

    #[test]
    fn start_and_end() {
        // <doc><blockquote><p>Text</p></blockquote></doc>
        // doc content start: 0, doc content end: 8
        // bq content start: 1, bq content end: 7
        // p content start: 2, p content end: 6
        let doc = nested_doc();
        let rp = doc.resolve(3);
        assert_eq!(rp.start(0), 0);
        assert_eq!(rp.end(0), 8);
        assert_eq!(rp.start(1), 1);
        assert_eq!(rp.end(1), 7);
        assert_eq!(rp.start(2), 2);
        assert_eq!(rp.end(2), 6);
    }

    #[test]
    fn before_and_after() {
        let doc = nested_doc();
        let rp = doc.resolve(3);
        // blockquote: before=0, after=8
        assert_eq!(rp.before(1), 0);
        assert_eq!(rp.after(1), 8);
        // paragraph: before=1, after=7
        assert_eq!(rp.before(2), 1);
        assert_eq!(rp.after(2), 7);
    }

    #[test]
    #[should_panic(expected = "before() is not defined for depth 0")]
    fn before_depth_0_panics() {
        let doc = simple_doc();
        let rp = doc.resolve(0);
        rp.before(0);
    }

    #[test]
    #[should_panic(expected = "after() is not defined for depth 0")]
    fn after_depth_0_panics() {
        let doc = simple_doc();
        let rp = doc.resolve(0);
        rp.after(0);
    }

    // ── text_offset ────────────────────────────────────────────────────

    #[test]
    fn text_offset_inside_text() {
        // pos 3 in simple_doc = "Hello" at char index 2
        let doc = simple_doc();
        let rp = doc.resolve(3);
        assert_eq!(rp.text_offset(), 2);
    }

    #[test]
    fn text_offset_at_start_of_text() {
        // pos 1 in simple_doc = start of "Hello", char index 0
        let doc = simple_doc();
        let rp = doc.resolve(1);
        assert_eq!(rp.text_offset(), 0);
    }

    #[test]
    fn text_offset_between_nodes() {
        // pos 0 in simple_doc = between doc start and <p> — not in text
        let doc = simple_doc();
        let rp = doc.resolve(0);
        assert_eq!(rp.text_offset(), 0);
    }

    #[test]
    fn text_offset_at_end_of_parent() {
        // pos 6 in simple_doc = end of p content (past "Hello")
        let doc = simple_doc();
        let rp = doc.resolve(6);
        // Index points past all children, so text_offset = 0.
        assert_eq!(rp.text_offset(), 0);
    }

    // ── node_after / node_before ───────────────────────────────────────

    #[test]
    fn node_after_at_start_of_doc() {
        // pos 0: before first <p> — node_after is the paragraph
        let doc = simple_doc();
        let rp = doc.resolve(0);
        let after = rp.node_after().unwrap();
        assert_eq!(after.node_type, NodeType::Paragraph);
    }

    #[test]
    fn node_after_inside_text_is_none() {
        // pos 3: inside "Hello" — no discrete node after
        let doc = simple_doc();
        let rp = doc.resolve(3);
        assert!(rp.node_after().is_none());
    }

    #[test]
    fn node_after_at_end_of_content_is_none() {
        // pos 7: end of doc content — no node after
        let doc = simple_doc();
        let rp = doc.resolve(7);
        assert!(rp.node_after().is_none());
    }

    #[test]
    fn node_before_at_start_is_none() {
        // pos 0: before everything — no node before
        let doc = simple_doc();
        let rp = doc.resolve(0);
        assert!(rp.node_before().is_none());
    }

    #[test]
    fn node_before_after_paragraph() {
        // pos 7: after </p> in simple_doc — node_before is the paragraph
        let doc = simple_doc();
        let rp = doc.resolve(7);
        let before = rp.node_before().unwrap();
        assert_eq!(before.node_type, NodeType::Paragraph);
    }

    #[test]
    fn node_after_hr() {
        // pos 4: between first </p> and <hr>
        let doc = multi_block_doc();
        let rp = doc.resolve(4);
        let after = rp.node_after().unwrap();
        assert_eq!(after.node_type, NodeType::HorizontalRule);
    }

    #[test]
    fn node_before_hr() {
        // pos 5: after <hr>, before second <p>
        let doc = multi_block_doc();
        let rp = doc.resolve(5);
        let before = rp.node_before().unwrap();
        assert_eq!(before.node_type, NodeType::HorizontalRule);
    }

    // ── node_after at start of text content ────────────────────────────

    #[test]
    fn node_after_at_start_of_paragraph_content() {
        // pos 1: start of p content in simple_doc
        // The child at this position is the text node "Hello".
        let doc = simple_doc();
        let rp = doc.resolve(1);
        let after = rp.node_after().unwrap();
        assert_eq!(after.node_type, NodeType::Text);
        assert_eq!(after.text(), Some("Hello"));
    }

    // ── marks ──────────────────────────────────────────────────────────

    #[test]
    fn marks_inside_bold_text() {
        // <doc><p><strong>Bold</strong>Normal</p></doc>
        // p content: "Bold" (strong, 4 chars) + "Normal" (no marks, 6 chars)
        // pos 1 = start of p content
        // pos 2 = inside "Bold" at char 1
        let doc = marked_doc();
        let rp = doc.resolve(2);
        let marks = rp.marks();
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].mark_type, MarkType::Strong);
    }

    #[test]
    fn marks_inside_normal_text() {
        // pos 5 = start of p content + 4 = inside "Normal" at char 0
        let doc = marked_doc();
        let rp = doc.resolve(5);
        let marks = rp.marks();
        assert!(marks.is_empty());
    }

    #[test]
    fn marks_at_boundary_prefers_after() {
        // pos 1: start of p content. "Bold" (strong) starts here.
        // node_after is "Bold" — should return strong marks.
        let doc = marked_doc();
        let rp = doc.resolve(1);
        let marks = rp.marks();
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].mark_type, MarkType::Strong);
    }

    #[test]
    fn marks_between_nodes_no_text() {
        // pos 0: before <p> at doc level — no text adjacent
        let doc = simple_doc();
        let rp = doc.resolve(0);
        let marks = rp.marks();
        assert!(marks.is_empty());
    }

    // ── shared_depth ───────────────────────────────────────────────────

    #[test]
    fn shared_depth_same_paragraph() {
        // Two positions inside the same paragraph should share depth 1.
        let doc = simple_doc();
        let rp = doc.resolve(2);
        assert_eq!(rp.shared_depth(4), 1);
    }

    #[test]
    fn shared_depth_different_paragraphs() {
        // pos 2 (inside first p) and pos 8 (inside second p) share depth 0 (doc).
        let doc = multi_block_doc();
        let rp = doc.resolve(2);
        assert_eq!(rp.shared_depth(8), 0);
    }

    #[test]
    fn shared_depth_same_position() {
        let doc = simple_doc();
        let rp = doc.resolve(3);
        assert_eq!(rp.shared_depth(3), 1); // same paragraph
    }

    // ── same_parent ────────────────────────────────────────────────────

    #[test]
    fn same_parent_within_paragraph() {
        let doc = simple_doc();
        let a = doc.resolve(2);
        let b = doc.resolve(4);
        assert!(a.same_parent(&b));
    }

    #[test]
    fn same_parent_different_paragraphs() {
        let doc = multi_block_doc();
        let a = doc.resolve(1); // inside first p
        let b = doc.resolve(6); // inside second p
        assert!(!a.same_parent(&b));
    }

    #[test]
    fn same_parent_both_at_doc_level() {
        let doc = multi_block_doc();
        let a = doc.resolve(0); // before first p (doc level)
        let b = doc.resolve(4); // between first p and hr (doc level)
        assert!(a.same_parent(&b));
    }

    // ── Edge cases ─────────────────────────────────────────────────────

    #[test]
    fn resolve_empty_paragraph() {
        // <doc><p></p></doc> — content size of doc = 2, p content = 0
        let p = Node::branch(NodeType::Paragraph, Fragment::empty());
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(p));
        // pos 0: before <p> (doc level)
        let rp = doc.resolve(0);
        assert_eq!(rp.depth, 0);
        // pos 1: inside empty <p> (paragraph level)
        let rp = doc.resolve(1);
        assert_eq!(rp.depth, 1);
        assert_eq!(rp.parent_offset, 0);
        assert_eq!(rp.parent().node_type, NodeType::Paragraph);
        // pos 2: after </p> (doc level)
        let rp = doc.resolve(2);
        assert_eq!(rp.depth, 0);
        assert_eq!(rp.parent_offset, 2);
    }

    #[test]
    fn resolve_doc_end_equals_content_size() {
        let doc = simple_doc();
        let size = doc.content.size();
        let rp = doc.resolve(size);
        assert_eq!(rp.pos, size);
        assert_eq!(rp.depth, 0);
        assert_eq!(rp.parent_offset, size);
    }

    #[test]
    #[should_panic(expected = "position 100 out of range")]
    fn resolve_out_of_range_panics() {
        let doc = simple_doc();
        doc.resolve(100);
    }

    #[test]
    fn resolve_at_hr_leaf() {
        // In multi_block_doc, hr is at doc content offset 4.
        // pos 4: at the hr position (doc level).
        let doc = multi_block_doc();
        let rp = doc.resolve(4);
        assert_eq!(rp.depth, 0);
        let after = rp.node_after().unwrap();
        assert_eq!(after.node_type, NodeType::HorizontalRule);
    }

    // ── Deep nesting ───────────────────────────────────────────────────

    #[test]
    fn resolve_deep_nesting() {
        // <doc><blockquote><p>AB</p></blockquote></doc>
        // doc content size: 1 + (1 + 2 + 1) + 1 = 6  => wait
        // bq node_size: 1 + (1+2+1) + 1 = 6
        // doc content size: 6
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("AB")),
        );
        let bq = Node::branch(NodeType::Blockquote, Fragment::from_node(p));
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(bq));

        // pos 0: doc level, before bq
        let rp = doc.resolve(0);
        assert_eq!(rp.depth, 0);

        // pos 1: inside bq, before p
        let rp = doc.resolve(1);
        assert_eq!(rp.depth, 1);
        assert_eq!(rp.parent().node_type, NodeType::Blockquote);

        // pos 2: inside p, before 'A'
        let rp = doc.resolve(2);
        assert_eq!(rp.depth, 2);
        assert_eq!(rp.parent().node_type, NodeType::Paragraph);
        assert_eq!(rp.parent_offset, 0);

        // pos 3: inside p, between 'A' and 'B'
        let rp = doc.resolve(3);
        assert_eq!(rp.depth, 2);
        assert_eq!(rp.parent_offset, 1);
        assert_eq!(rp.text_offset(), 1);

        // pos 4: inside p, after 'B'
        let rp = doc.resolve(4);
        assert_eq!(rp.depth, 2);
        assert_eq!(rp.parent_offset, 2);

        // pos 5: inside bq, after p
        let rp = doc.resolve(5);
        assert_eq!(rp.depth, 1);
        assert_eq!(rp.parent().node_type, NodeType::Blockquote);

        // pos 6: doc level, after bq
        let rp = doc.resolve(6);
        assert_eq!(rp.depth, 0);
    }

    // ── Marks at boundary between bold and normal ──────────────────────

    #[test]
    fn marks_at_bold_normal_boundary() {
        // <doc><p><strong>Bold</strong>Normal</p></doc>
        // p content: "Bold" (4 chars, strong) + "Normal" (6 chars, no marks)
        // The boundary is at parent_offset=4 within p content (pos 5).
        // At the boundary, node_after is "Normal" (no marks), so marks should be empty.
        let doc = marked_doc();
        let rp = doc.resolve(5); // offset 4 in p content
        let marks = rp.marks();
        // "Normal" has no marks, and it's the node_after at this boundary.
        assert!(marks.is_empty());
    }

    // ── Two paragraphs shared_depth ────────────────────────────────────

    #[test]
    fn shared_depth_nested() {
        // <doc><blockquote><p>Text</p></blockquote></doc>
        let doc = nested_doc();
        // pos 3 and pos 5 are both inside the paragraph
        let rp = doc.resolve(3);
        assert_eq!(rp.shared_depth(5), 2); // same paragraph
        // pos 3 (inside p) and pos 1 (before p, inside bq)
        assert_eq!(rp.shared_depth(1), 1); // blockquote level
    }

    // ── Heading with attrs ─────────────────────────────────────────────

    #[test]
    fn resolve_heading() {
        // <doc><h1>Title</h1></doc>
        let h = Node::branch_with_attrs(
            NodeType::Heading,
            heading_attrs(1),
            Fragment::from_node(Node::new_text("Title")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(h));
        let rp = doc.resolve(3); // inside "Title" at char 2
        assert_eq!(rp.depth, 1);
        assert_eq!(rp.parent().node_type, NodeType::Heading);
        assert_eq!(rp.parent_offset, 2);
    }

    // ── Paragraph with hard break ──────────────────────────────────────

    #[test]
    fn resolve_with_hard_break() {
        // <doc><p>A<br/>B</p></doc>
        // p content: "A" (1) + <br> (1) + "B" (1) = 3
        // doc content: 1 + 3 + 1 = 5
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_vec(vec![
                Node::new_text("A"),
                Node::leaf(NodeType::HardBreak),
                Node::new_text("B"),
            ]),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(p));

        // pos 1: start of p content, before "A"
        let rp = doc.resolve(1);
        assert_eq!(rp.depth, 1);
        assert_eq!(rp.parent_offset, 0);
        let after = rp.node_after().unwrap();
        assert_eq!(after.node_type, NodeType::Text);
        assert_eq!(after.text(), Some("A"));

        // pos 2: after "A", before <br>
        let rp = doc.resolve(2);
        assert_eq!(rp.parent_offset, 1);
        let after = rp.node_after().unwrap();
        assert_eq!(after.node_type, NodeType::HardBreak);
        let before = rp.node_before().unwrap();
        assert_eq!(before.node_type, NodeType::Text);
        assert_eq!(before.text(), Some("A"));

        // pos 3: after <br>, before "B"
        let rp = doc.resolve(3);
        assert_eq!(rp.parent_offset, 2);
        let after = rp.node_after().unwrap();
        assert_eq!(after.node_type, NodeType::Text);
        assert_eq!(after.text(), Some("B"));
    }

    // ── Empty doc ──────────────────────────────────────────────────────

    #[test]
    fn resolve_empty_doc() {
        let doc = Node::branch(NodeType::Doc, Fragment::empty());
        let rp = doc.resolve(0);
        assert_eq!(rp.depth, 0);
        assert_eq!(rp.parent_offset, 0);
        assert_eq!(rp.pos, 0);
        assert!(rp.node_after().is_none());
        assert!(rp.node_before().is_none());
    }

    // ── Position reference from the task description ───────────────────

    #[test]
    fn position_reference_multi_block() {
        // <doc><p>Hi</p><hr/><p>World</p></doc>
        // Content size: 4 + 1 + 7 = 12
        let doc = multi_block_doc();
        assert_eq!(doc.content.size(), 12);

        // pos 0: before first p
        let rp = doc.resolve(0);
        assert_eq!(rp.depth, 0);

        // pos 1: inside first p, at start of content (before "H")
        let rp = doc.resolve(1);
        assert_eq!(rp.depth, 1);
        assert_eq!(rp.parent_offset, 0);

        // pos 3: inside first p, at end of content (after "i")
        let rp = doc.resolve(3);
        assert_eq!(rp.depth, 1);
        assert_eq!(rp.parent_offset, 2);

        // pos 4: after first p, before hr
        let rp = doc.resolve(4);
        assert_eq!(rp.depth, 0);
        assert_eq!(rp.parent_offset, 4);

        // pos 5: after hr, before second p
        let rp = doc.resolve(5);
        assert_eq!(rp.depth, 0);
        assert_eq!(rp.parent_offset, 5);

        // pos 6: inside second p, at start of content
        let rp = doc.resolve(6);
        assert_eq!(rp.depth, 1);
        assert_eq!(rp.parent_offset, 0);

        // pos 11: inside second p, at end of content
        let rp = doc.resolve(11);
        assert_eq!(rp.depth, 1);
        assert_eq!(rp.parent_offset, 5);

        // pos 12: after second p (end of doc content)
        let rp = doc.resolve(12);
        assert_eq!(rp.depth, 0);
        assert_eq!(rp.parent_offset, 12);
    }

    // ── Additional edge-case tests ────────────────────────────────────

    #[test]
    fn node_before_at_end_of_text_within_paragraph() {
        // <doc><p>Hello</p></doc>
        // pos 6 = end of p content (parent_offset=5, past all children)
        // node_before should be None because we're past all children in the
        // paragraph — parent_offset != child_start for any boundary.
        let doc = simple_doc();
        let rp = doc.resolve(6);
        assert_eq!(rp.depth, 1);
        assert_eq!(rp.parent_offset, 5);
        // idx points past all children (sentinel), so node_before checks
        // if we're at a boundary. parent_offset=5 == child_start=5 (sum of
        // all children), and idx=1 > 0, so node_before returns child[0].
        let before = rp.node_before().unwrap();
        assert_eq!(before.node_type, NodeType::Text);
        assert_eq!(before.text(), Some("Hello"));
    }

    #[test]
    fn node_after_returns_next_block_after_leaf() {
        // <doc><p>Hi</p><hr/><p>World</p></doc>
        // pos 5 = after hr, before second p
        // node_after should return the second paragraph.
        let doc = multi_block_doc();
        let rp = doc.resolve(5);
        assert_eq!(rp.depth, 0);
        let after = rp.node_after().unwrap();
        assert_eq!(after.node_type, NodeType::Paragraph);
        assert_eq!(after.text_content(), "World");
    }

    #[test]
    fn shared_depth_at_exact_end_position() {
        // <doc><p>Hello</p></doc>
        // Resolve pos 3 (inside "Hello"), then check shared_depth with
        // pos 6, which is the `end` of the paragraph (end(1) = 6).
        // The `>=` in shared_depth means pos 6 is still "within" the
        // paragraph node at depth 1, because `end` is the last valid
        // content position (before the closing token).
        let doc = simple_doc();
        let rp = doc.resolve(3);
        assert_eq!(rp.end(1), 6);
        // pos 6 == end(1), so shared_depth should include depth 1.
        assert_eq!(rp.shared_depth(6), 1);
    }
}
