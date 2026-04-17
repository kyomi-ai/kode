//! Composable document transforms.
//!
//! A [`Transform`] accumulates a sequence of [`Step`]s that modify a document,
//! providing both low-level step application and high-level editing operations
//! like split, join, and mark manipulation.

use crate::attrs::Attrs;
use crate::fragment::Fragment;
use crate::mark::Mark;
use crate::node::Node;
use crate::node_type::NodeType;
use crate::slice::Slice;
use crate::step::{Step, StepMap};

/// A composable sequence of steps that transforms a document.
///
/// Provides high-level editing operations that build on [`Step::Replace`],
/// [`Step::AddMark`], and [`Step::RemoveMark`].
pub struct Transform {
    /// The current document (after all steps applied).
    pub doc: Node,
    /// Steps applied so far.
    pub steps: Vec<Step>,
    /// Document state before each step (for undo).
    pub docs: Vec<Node>,
    /// Step maps for position mapping.
    pub maps: Vec<StepMap>,
}

impl Transform {
    /// Create a new transform starting from the given document.
    pub fn new(doc: Node) -> Self {
        Transform {
            doc,
            steps: Vec::new(),
            docs: Vec::new(),
            maps: Vec::new(),
        }
    }

    /// Apply a step, updating the document, step list, and maps.
    pub fn step(&mut self, step: Step) -> Result<&mut Self, String> {
        let result = step.apply(&self.doc)?;
        self.docs.push(self.doc.clone());
        self.doc = result.doc;
        self.steps.push(step);
        self.maps.push(result.map);
        Ok(self)
    }

    /// Map a position through all steps applied so far.
    pub fn map_pos(&self, mut pos: usize, assoc: i8) -> usize {
        for map in &self.maps {
            pos = map.map(pos, assoc);
        }
        pos
    }

    // ── High-level operations ───────────────────────────────────────────

    /// Replace content between `from` and `to` with the given slice.
    pub fn replace(&mut self, from: usize, to: usize, slice: Slice) -> Result<&mut Self, String> {
        self.step(Step::Replace { from, to, slice })
    }

    /// Insert nodes at a position.
    pub fn insert(&mut self, pos: usize, content: Fragment) -> Result<&mut Self, String> {
        self.replace(pos, pos, Slice::new(content, 0, 0))
    }

    /// Delete a range.
    pub fn delete(&mut self, from: usize, to: usize) -> Result<&mut Self, String> {
        if from == to {
            return Ok(self);
        }
        self.replace(from, to, Slice::empty())
    }

    /// Replace a range with a fragment (open_start=0, open_end=0).
    pub fn replace_with(
        &mut self,
        from: usize,
        to: usize,
        content: Fragment,
    ) -> Result<&mut Self, String> {
        self.replace(from, to, Slice::new(content, 0, 0))
    }

    /// Split the node at `pos` at the given `depth`.
    ///
    /// This is the Enter key operation. At depth=1 it splits the innermost
    /// textblock into two sibling nodes. The left sibling gets content before
    /// `pos`, the right sibling gets content after `pos`.
    ///
    /// The algorithm directly constructs a replacement that closes the current
    /// node at `pos`, then opens a new sibling with the remaining content.
    pub fn split(&mut self, pos: usize, depth: usize) -> Result<&mut Self, String> {
        if depth == 0 {
            return Err("split depth must be >= 1".to_string());
        }

        let resolved = self.doc.resolve(pos);

        if depth > resolved.depth {
            return Err(format!(
                "split depth {} exceeds node depth {} at position {}",
                depth, resolved.depth, pos
            ));
        }

        // Calculate the range to replace: from the position to itself, but
        // we need to express the split as a replacement of the content around
        // the split point.
        //
        // Strategy: replace from `before(split_depth)` to `after(split_depth)`
        // with two complete nodes — one with content before pos, one with
        // content after pos.
        let split_depth = resolved.depth - depth + 1;

        // Build the left and right halves at the innermost split level.
        let inner_parent = resolved.node(resolved.depth);
        let inner_start = resolved.start(resolved.depth);
        let inner_offset = pos - inner_start;

        let left_content = inner_parent.content.cut(0, inner_offset);
        let right_content = inner_parent
            .content
            .cut(inner_offset, inner_parent.content.size());

        // Wrap with ancestor nodes from inner to split_depth.
        let mut left_node = Node::branch_with_attrs(
            inner_parent.node_type,
            inner_parent.attrs.clone(),
            left_content,
        );
        let mut right_node = Node::branch_with_attrs(
            inner_parent.node_type,
            inner_parent.attrs.clone(),
            right_content,
        );

        // Wrap outward to split_depth.
        for d in (split_depth..resolved.depth).rev() {
            let ancestor = resolved.node(d);
            let idx = resolved.index(d);

            // Left: take children before idx, then left_node.
            let mut left_children = Vec::new();
            for i in 0..idx {
                left_children.push(ancestor.child(i).clone());
            }
            left_children.push(left_node);
            left_node = Node::branch_with_attrs(
                ancestor.node_type,
                ancestor.attrs.clone(),
                Fragment::from_vec(left_children),
            );

            // Right: right_node, then children after idx.
            let mut right_children = vec![right_node];
            for i in (idx + 1)..ancestor.child_count() {
                right_children.push(ancestor.child(i).clone());
            }
            right_node = Node::branch_with_attrs(
                ancestor.node_type,
                ancestor.attrs.clone(),
                Fragment::from_vec(right_children),
            );
        }

        // The replacement range at split_depth's parent.
        let replace_start = resolved.before(split_depth);
        let replace_end = resolved.after(split_depth);

        let replacement = Fragment::from_vec(vec![left_node, right_node]);
        let slice = Slice::new(replacement, 0, 0);

        self.replace(replace_start, replace_end, slice)
    }

    /// Join two adjacent blocks at the given position.
    ///
    /// This is the Backspace-at-block-boundary operation. The position should
    /// be between two adjacent block nodes. This deletes the closing token of
    /// the first block and the opening token of the second, merging their content.
    pub fn join(&mut self, pos: usize) -> Result<&mut Self, String> {
        // Delete from pos-1 (closing token of first block) to pos+1 (opening
        // token of second block). This merges the two blocks.
        if pos == 0 {
            return Err("cannot join at position 0".to_string());
        }
        let doc_size = self.doc.content.size();
        if pos + 1 > doc_size {
            return Err(format!(
                "join position {pos} out of range for document of size {doc_size}"
            ));
        }

        let rp = self.doc.resolve(pos);
        match (rp.node_before(), rp.node_after()) {
            (Some(nb), Some(na)) if nb.node_type.is_block() && na.node_type.is_block() => {}
            _ => {
                return Err(format!(
                    "join: position {} is not between two block nodes",
                    pos
                ))
            }
        }

        self.delete(pos - 1, pos + 1)
    }

    /// Change the block type of nodes in a range.
    ///
    /// For each textblock in the range, replaces its type and attributes while
    /// keeping its content. For example, changing a paragraph to a heading.
    pub fn set_block_type(
        &mut self,
        from: usize,
        to: usize,
        node_type: NodeType,
        attrs: Attrs,
    ) -> Result<&mut Self, String> {
        // Walk the document to find textblocks in the range. For each one,
        // replace it with a node of the new type containing the same content.
        let doc = self.doc.clone();

        let mut targets: Vec<(usize, usize)> = Vec::new();

        doc.nodes_between(from, to, &mut |node, pos, _parent, _idx| {
            if node.node_type.is_textblock() {
                let node_end = pos + node.node_size();
                targets.push((pos, node_end));
                return false; // don't descend
            }
            true // descend into non-textblock branches
        });

        // Apply from right to left so earlier positions stay valid.
        for (block_start, block_end) in targets.into_iter().rev() {
            // Extract the content of this block.
            let resolved = self.doc.resolve(block_start + 1);
            let parent = resolved.parent();
            let content = parent.content.clone();

            // Build a new node with the target type and same content.
            let new_node = Node::branch_with_attrs(node_type, attrs.clone(), content);
            let replacement = Fragment::from_node(new_node);

            // Replace the entire block node.
            self.replace(
                block_start,
                block_end,
                Slice::new(replacement, 0, 0),
            )?;
        }

        Ok(self)
    }

    /// Wrap the block(s) containing the range `[from, to]` in a new parent node.
    ///
    /// For example, wrapping a paragraph in a blockquote:
    /// Before: `<doc><p>Text</p></doc>`
    /// After:  `<doc><blockquote><p>Text</p></blockquote></doc>`
    ///
    /// The algorithm finds the blocks that contain `from` and `to` at the
    /// textblock level, extracts them from their parent, wraps them in a new
    /// node of `wrapper_type`, and replaces the original range.
    ///
    /// **Assumption:** `from` and `to` must resolve to the same depth. This
    /// holds when both positions are inside sibling blocks under the same parent.
    pub fn wrap(
        &mut self,
        from: usize,
        to: usize,
        wrapper_type: NodeType,
        attrs: Attrs,
    ) -> Result<&mut Self, String> {
        let res_from = self.doc.resolve(from);
        let res_to = self.doc.resolve(to);

        if res_from.depth != res_to.depth {
            return Err(format!(
                "wrap: from depth {} != to depth {}; from and to must resolve to the same depth",
                res_from.depth, res_to.depth
            ));
        }

        // Find the depth of the blocks we want to wrap. We wrap at the
        // innermost block level that contains the from/to range.
        let wrap_depth = res_from.depth;

        // Get the position range that covers the block(s) to wrap.
        let block_start = res_from.before(wrap_depth);
        let block_end = res_to.after(wrap_depth);

        // Extract the content between block_start and block_end.
        let content_slice = self.doc.slice(block_start, block_end);

        // Create wrapper node containing the extracted content.
        let wrapper = Node::branch_with_attrs(wrapper_type, attrs, content_slice.content);

        // Create a slice containing just the wrapper.
        let wrapper_slice = Slice::new(Fragment::from_node(wrapper), 0, 0);

        // Replace the block range with the wrapper.
        self.replace(block_start, block_end, wrapper_slice)
    }

    /// Lift content out of a wrapper node, removing the wrapper.
    ///
    /// Before: `<doc><blockquote><p>Text</p></blockquote></doc>`
    /// After:  `<doc><p>Text</p></doc>`
    ///
    /// Walks ancestors from the cursor position upward to find a node matching
    /// `wrapper_type`, then replaces the wrapper with its children.
    pub fn lift(
        &mut self,
        pos: usize,
        wrapper_type: NodeType,
    ) -> Result<&mut Self, String> {
        let resolved = self.doc.resolve(pos);

        // Find the wrapper in ancestors.
        let mut wrapper_depth = None;
        for d in (1..=resolved.depth).rev() {
            if resolved.node(d).node_type == wrapper_type {
                wrapper_depth = Some(d);
                break;
            }
        }
        let wrapper_depth =
            wrapper_depth.ok_or_else(|| format!("no {wrapper_type:?} ancestor found"))?;

        let wrapper_start = resolved.before(wrapper_depth);
        let wrapper_end = resolved.after(wrapper_depth);

        // Get the wrapper's children as a fragment.
        let wrapper_node = resolved.node(wrapper_depth);
        let children = wrapper_node.content.clone();

        // Replace the wrapper with its children.
        let slice = Slice::new(children, 0, 0);
        self.replace(wrapper_start, wrapper_end, slice)
    }

    /// Wrap block(s) in a list, creating `ListItem` nodes around each block.
    ///
    /// For example, wrapping a paragraph in a bullet list:
    /// Before: `<doc><p>Text</p></doc>`
    /// After:  `<doc><ul><li><p>Text</p></li></ul></doc>`
    ///
    /// Each block in the range `[from, to]` gets its own `ListItem` wrapper,
    /// and all `ListItem`s are then wrapped in the `list_type` node.
    ///
    /// **Assumption:** `from` and `to` must resolve to the same depth. This
    /// holds when both positions are inside sibling blocks under the same parent.
    pub fn wrap_in_list(
        &mut self,
        from: usize,
        to: usize,
        list_type: NodeType,
        attrs: Attrs,
    ) -> Result<&mut Self, String> {
        let res_from = self.doc.resolve(from);
        let res_to = self.doc.resolve(to);

        if res_from.depth != res_to.depth {
            return Err(format!(
                "wrap_in_list: from depth {} != to depth {}; from and to must resolve to the same depth",
                res_from.depth, res_to.depth
            ));
        }

        // Find the depth of the blocks we want to wrap.
        let wrap_depth = res_from.depth;

        let block_start = res_from.before(wrap_depth);
        let block_end = res_to.after(wrap_depth);

        // Extract the blocks.
        let content_slice = self.doc.slice(block_start, block_end);

        // Wrap each top-level child in a ListItem.
        let mut list_items = Vec::new();
        for child in content_slice.content.iter() {
            let li = Node::branch(
                NodeType::ListItem,
                Fragment::from_node(child.clone()),
            );
            list_items.push(li);
        }

        // Wrap all ListItems in the list node.
        let list_node = Node::branch_with_attrs(
            list_type,
            attrs,
            Fragment::from_vec(list_items),
        );

        let wrapper_slice = Slice::new(Fragment::from_node(list_node), 0, 0);
        self.replace(block_start, block_end, wrapper_slice)
    }

    /// Apply a mark to a text range.
    pub fn add_mark(
        &mut self,
        from: usize,
        to: usize,
        mark: Mark,
    ) -> Result<&mut Self, String> {
        self.step(Step::AddMark { from, to, mark })
    }

    /// Remove a mark from a text range.
    pub fn remove_mark(
        &mut self,
        from: usize,
        to: usize,
        mark: Mark,
    ) -> Result<&mut Self, String> {
        self.step(Step::RemoveMark { from, to, mark })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attrs::heading_attrs;
    use crate::mark::MarkType;

    // ── Helpers ─────────────────────────────────────────────────────────

    /// <doc><p>Hello</p></doc>
    fn simple_doc() -> Node {
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hello")),
        );
        Node::branch(NodeType::Doc, Fragment::from_node(p))
    }

    /// <doc><p>Hello</p><p>World</p></doc>
    fn two_para_doc() -> Node {
        let p1 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hello")),
        );
        let p2 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("World")),
        );
        Node::branch(NodeType::Doc, Fragment::from_vec(vec![p1, p2]))
    }

    // ── split tests ─────────────────────────────────────────────────────

    #[test]
    fn split_middle_of_paragraph() {
        // <doc><p>Hello</p></doc>
        // Split at pos 3 (between "He" and "llo"), depth=1
        // Expected: <doc><p>He</p><p>llo</p></doc>
        let doc = simple_doc();
        let mut tr = Transform::new(doc);
        tr.split(3, 1).unwrap();

        let result = tr.doc;
        assert_eq!(result.child_count(), 2);
        assert_eq!(result.child(0).node_type, NodeType::Paragraph);
        assert_eq!(result.child(0).text_content(), "He");
        assert_eq!(result.child(1).node_type, NodeType::Paragraph);
        assert_eq!(result.child(1).text_content(), "llo");
    }

    #[test]
    fn split_end_of_paragraph() {
        // Split at pos 6 (end of "Hello" content), depth=1
        // Expected: <doc><p>Hello</p><p></p></doc>
        let doc = simple_doc();
        let mut tr = Transform::new(doc);
        tr.split(6, 1).unwrap();

        let result = tr.doc;
        assert_eq!(result.child_count(), 2);
        assert_eq!(result.child(0).text_content(), "Hello");
        assert_eq!(result.child(1).text_content(), "");
    }

    #[test]
    fn split_start_of_paragraph() {
        // Split at pos 1 (start of paragraph content), depth=1
        // Expected: <doc><p></p><p>Hello</p></doc>
        let doc = simple_doc();
        let mut tr = Transform::new(doc);
        tr.split(1, 1).unwrap();

        let result = tr.doc;
        assert_eq!(result.child_count(), 2);
        assert_eq!(result.child(0).text_content(), "");
        assert_eq!(result.child(1).text_content(), "Hello");
    }

    // ── join tests ──────────────────────────────────────────────────────

    #[test]
    fn join_two_paragraphs() {
        // <doc><p>Hello</p><p>World</p></doc>
        // Join at pos 7 (between the two paragraphs)
        // Expected: <doc><p>HelloWorld</p></doc>
        let doc = two_para_doc();
        // Position layout: <p> at 0, content 1-6, </p> at 7.
        // Wait — p1 node_size = 1+5+1 = 7. So p1 occupies positions 0..7.
        // p2 starts at 7, content at 8..13, </p> at 14.
        // The boundary between them is at position 7.
        // join(7) deletes 6..8 (closing of p1 and opening of p2).
        let mut tr = Transform::new(doc);
        tr.join(7).unwrap();

        let result = tr.doc;
        assert_eq!(result.child_count(), 1);
        assert_eq!(result.child(0).node_type, NodeType::Paragraph);
        assert_eq!(result.child(0).text_content(), "HelloWorld");
    }

    // ── delete tests ────────────────────────────────────────────────────

    #[test]
    fn delete_within_paragraph() {
        let doc = simple_doc();
        let mut tr = Transform::new(doc);
        tr.delete(2, 5).unwrap();

        assert_eq!(tr.doc.child(0).text_content(), "Ho");
    }

    #[test]
    fn delete_across_paragraphs() {
        // <doc><p>Hello</p><p>World</p></doc>
        // Delete from 3 (inside "Hello") to 10 (inside "World")
        let doc = two_para_doc();
        let mut tr = Transform::new(doc);
        tr.delete(3, 10).unwrap();

        assert_eq!(tr.doc.child_count(), 1);
        assert_eq!(tr.doc.child(0).text_content(), "Herld");
    }

    #[test]
    fn delete_noop_same_pos() {
        let doc = simple_doc();
        let mut tr = Transform::new(doc.clone());
        tr.delete(3, 3).unwrap();
        assert_eq!(&tr.doc, &doc);
        assert_eq!(tr.steps.len(), 0);
    }

    // ── insert tests ────────────────────────────────────────────────────

    #[test]
    fn insert_text_at_position() {
        let doc = simple_doc();
        let mut tr = Transform::new(doc);
        let content = Fragment::from_node(Node::new_text("XY"));
        tr.insert(3, content).unwrap();

        assert_eq!(tr.doc.child(0).text_content(), "HeXYllo");
    }

    #[test]
    fn insert_block_node() {
        // Insert a new paragraph between existing paragraphs.
        let doc = two_para_doc();
        let new_p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Middle")),
        );
        let mut tr = Transform::new(doc);
        // Position 7 is between the two paragraphs.
        tr.insert(7, Fragment::from_node(new_p)).unwrap();

        assert_eq!(tr.doc.child_count(), 3);
        assert_eq!(tr.doc.child(0).text_content(), "Hello");
        assert_eq!(tr.doc.child(1).text_content(), "Middle");
        assert_eq!(tr.doc.child(2).text_content(), "World");
    }

    // ── add_mark / remove_mark tests ────────────────────────────────────

    #[test]
    fn add_mark_to_range() {
        let doc = simple_doc();
        let mut tr = Transform::new(doc);
        tr.add_mark(2, 5, Mark::new(MarkType::Strong)).unwrap();

        let p = tr.doc.child(0);
        assert_eq!(p.child_count(), 3);
        assert_eq!(p.child(0).text(), Some("H"));
        assert!(p.child(0).marks.is_empty());
        assert_eq!(p.child(1).text(), Some("ell"));
        assert_eq!(p.child(1).marks[0].mark_type, MarkType::Strong);
        assert_eq!(p.child(2).text(), Some("o"));
        assert!(p.child(2).marks.is_empty());
    }

    #[test]
    fn remove_mark_from_range() {
        let bold = Node::new_text_with_marks("Hello", vec![Mark::new(MarkType::Strong)]);
        let p = Node::branch(NodeType::Paragraph, Fragment::from_node(bold));
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(p));

        let mut tr = Transform::new(doc);
        tr.remove_mark(1, 6, Mark::new(MarkType::Strong)).unwrap();

        let p = tr.doc.child(0);
        assert_eq!(p.child_count(), 1);
        assert_eq!(p.child(0).text(), Some("Hello"));
        assert!(p.child(0).marks.is_empty());
    }

    // ── set_block_type tests ────────────────────────────────────────────

    #[test]
    fn set_block_type_paragraph_to_heading() {
        let doc = simple_doc();
        let mut tr = Transform::new(doc);
        tr.set_block_type(0, 7, NodeType::Heading, heading_attrs(2))
            .unwrap();

        let h = tr.doc.child(0);
        assert_eq!(h.node_type, NodeType::Heading);
        assert_eq!(h.attrs, heading_attrs(2));
        assert_eq!(h.text_content(), "Hello");
    }

    #[test]
    fn set_block_type_heading_to_paragraph() {
        let h = Node::branch_with_attrs(
            NodeType::Heading,
            heading_attrs(1),
            Fragment::from_node(Node::new_text("Title")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(h));

        let mut tr = Transform::new(doc);
        tr.set_block_type(0, 7, NodeType::Paragraph, Attrs::new())
            .unwrap();

        let p = tr.doc.child(0);
        assert_eq!(p.node_type, NodeType::Paragraph);
        assert!(p.attrs.is_empty());
        assert_eq!(p.text_content(), "Title");
    }

    // ── Chain multiple operations ───────────────────────────────────────

    #[test]
    fn chain_split_then_set_type_then_add_mark() {
        // Start: <doc><p>Hello</p></doc>
        // 1. Split at pos 3 → <doc><p>He</p><p>llo</p></doc>
        // 2. Set second paragraph to heading
        // 3. Add bold to "ll" in the heading
        let doc = simple_doc();
        let mut tr = Transform::new(doc);

        // Split
        tr.split(3, 1).unwrap();
        assert_eq!(tr.doc.child_count(), 2);

        // Set block type on second paragraph (now at positions 5..10)
        // After split: <p>He</p> = size 4, <p>llo</p> = size 5.
        // Second p starts at position 4, ends at position 9.
        let second_p_start = 4;
        let second_p_end = 4 + 1 + 3 + 1; // = 9
        tr.set_block_type(second_p_start, second_p_end, NodeType::Heading, heading_attrs(1))
            .unwrap();
        assert_eq!(tr.doc.child(1).node_type, NodeType::Heading);
        assert_eq!(tr.doc.child(1).text_content(), "llo");

        // Add bold to "ll" (positions 5 and 6 inside the heading).
        tr.add_mark(5, 7, Mark::new(MarkType::Strong)).unwrap();
        let h = tr.doc.child(1);
        // Should have "ll" (strong) and "o" (no marks)
        assert_eq!(h.child_count(), 2);
        assert_eq!(h.child(0).text(), Some("ll"));
        assert_eq!(h.child(0).marks[0].mark_type, MarkType::Strong);
        assert_eq!(h.child(1).text(), Some("o"));
        assert!(h.child(1).marks.is_empty());
    }

    // ── map_pos tests ───────────────────────────────────────────────────

    #[test]
    fn map_pos_through_delete() {
        let doc = simple_doc();
        let mut tr = Transform::new(doc);
        // Delete "ell" (pos 2..5), removing 3 chars.
        tr.delete(2, 5).unwrap();

        // Position 1 (before deleted range) — unchanged.
        assert_eq!(tr.map_pos(1, 1), 1);
        // Position 6 (after deleted range) — shifted left by 3.
        assert_eq!(tr.map_pos(6, 1), 3);
    }

    #[test]
    fn map_pos_through_insert() {
        let doc = simple_doc();
        let mut tr = Transform::new(doc);
        let content = Fragment::from_node(Node::new_text("XY"));
        tr.insert(3, content).unwrap();

        // Position 1 — before insertion, unchanged.
        assert_eq!(tr.map_pos(1, 1), 1);
        // Position 3 — at insertion point, stick right → 3 + 2 = 5.
        assert_eq!(tr.map_pos(3, 1), 5);
        // Position 3 — stick left → stays at 3.
        assert_eq!(tr.map_pos(3, -1), 3);
        // Position 5 — after insertion, shifted right by 2.
        assert_eq!(tr.map_pos(5, 1), 7);
    }

    #[test]
    fn map_pos_through_multiple_steps() {
        let doc = two_para_doc();
        let mut tr = Transform::new(doc);

        // Step 1: delete "ell" from first paragraph (pos 2..5), -3 chars.
        tr.delete(2, 5).unwrap();
        // Step 2: insert "XY" at pos 2 in the now-shorter doc, +2 chars.
        let content = Fragment::from_node(Node::new_text("XY"));
        tr.insert(2, content).unwrap();

        // Original position 1 → through step1: 1, through step2: 1.
        assert_eq!(tr.map_pos(1, 1), 1);
        // Original position 6 (end of first paragraph content, "o") →
        // step1: 6 was after [2,5) delete, so 6-3=3.
        // step2: 3 at insertion point, stick right → 3+2=5.
        assert_eq!(tr.map_pos(6, 1), 5);
    }

    // ── replace_with test ───────────────────────────────────────────────

    #[test]
    fn replace_with_content() {
        let doc = simple_doc();
        let mut tr = Transform::new(doc);
        let content = Fragment::from_node(Node::new_text("XY"));
        tr.replace_with(2, 5, content).unwrap();

        assert_eq!(tr.doc.child(0).text_content(), "HXYo");
    }

    // ── split depth error ───────────────────────────────────────────────

    #[test]
    fn split_depth_zero_errors() {
        let doc = simple_doc();
        let mut tr = Transform::new(doc);
        let result = tr.split(3, 0);
        assert!(result.is_err());
    }

    // ── join error cases ────────────────────────────────────────────────

    #[test]
    fn join_at_zero_errors() {
        let doc = simple_doc();
        let mut tr = Transform::new(doc);
        assert!(tr.join(0).is_err());
    }

    #[test]
    fn join_out_of_range_errors() {
        let doc = simple_doc();
        let mut tr = Transform::new(doc);
        assert!(tr.join(100).is_err());
    }

    #[test]
    fn split_depth_exceeds_node_depth_errors() {
        // <doc><p>Hello</p></doc>
        // Position 3 resolves to depth 2 (doc > p > text).
        // Splitting with depth=5 should error, not underflow.
        let doc = simple_doc();
        let mut tr = Transform::new(doc);
        let result = tr.split(3, 5);
        let err = result.err().expect("expected an error");
        assert!(err.contains("split depth 5 exceeds node depth"), "got: {err}");
    }

    #[test]
    fn join_at_non_block_boundary_errors() {
        // <doc><p>Hello</p></doc>
        // Position 3 is inside the text "Hello", not between two block nodes.
        let doc = simple_doc();
        let mut tr = Transform::new(doc);
        let result = tr.join(3);
        let err = result.err().expect("expected an error");
        assert!(err.contains("not between two block nodes"), "got: {err}");
    }

    // ── wrap / lift / wrap_in_list tests ────────────────────────────────

    #[test]
    fn wrap_paragraph_in_blockquote() {
        // <doc><p>Hello</p></doc>
        // Wrap at pos 1 (inside paragraph) → <doc><blockquote><p>Hello</p></blockquote></doc>
        let doc = simple_doc();
        let mut tr = Transform::new(doc);
        tr.wrap(1, 1, NodeType::Blockquote, Attrs::new()).unwrap();

        assert_eq!(tr.doc.child_count(), 1);
        let bq = tr.doc.child(0);
        assert_eq!(bq.node_type, NodeType::Blockquote);
        assert_eq!(bq.child_count(), 1);
        assert_eq!(bq.child(0).node_type, NodeType::Paragraph);
        assert_eq!(bq.child(0).text_content(), "Hello");
    }

    #[test]
    fn lift_returns_err_when_no_matching_ancestor() {
        // <doc><p>Hello</p></doc> — no blockquote ancestor to lift from.
        let doc = simple_doc();
        let mut tr = Transform::new(doc);
        let result = tr.lift(1, NodeType::Blockquote);
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.contains("no"), "expected 'no ... ancestor found', got: {err}");
    }

    #[test]
    fn lift_blockquote_restores_paragraph() {
        // Wrap then lift should restore original structure.
        // Start: <doc><p>Hello</p></doc>
        // After wrap: <doc><blockquote><p>Hello</p></blockquote></doc>
        // After lift: <doc><p>Hello</p></doc>
        let doc = simple_doc();
        let mut tr = Transform::new(doc);
        tr.wrap(1, 1, NodeType::Blockquote, Attrs::new()).unwrap();

        // Lift from pos 2 (inside the blockquote > paragraph).
        tr.lift(2, NodeType::Blockquote).unwrap();

        assert_eq!(tr.doc.child_count(), 1);
        assert_eq!(tr.doc.child(0).node_type, NodeType::Paragraph);
        assert_eq!(tr.doc.child(0).text_content(), "Hello");
    }

    #[test]
    fn wrap_in_list_creates_list_items() {
        // <doc><p>Hello</p><p>World</p></doc>
        // Wrap both paragraphs in a bullet list.
        // Position 1 is inside first paragraph, position 10 is inside second.
        // Expected: <doc><ul><li><p>Hello</p></li><li><p>World</p></li></ul></doc>
        let doc = two_para_doc();
        let mut tr = Transform::new(doc);
        tr.wrap_in_list(1, 10, NodeType::BulletList, Attrs::new())
            .unwrap();

        assert_eq!(tr.doc.child_count(), 1);
        let ul = tr.doc.child(0);
        assert_eq!(ul.node_type, NodeType::BulletList);
        assert_eq!(ul.child_count(), 2);

        let li0 = ul.child(0);
        assert_eq!(li0.node_type, NodeType::ListItem);
        assert_eq!(li0.child_count(), 1);
        assert_eq!(li0.child(0).node_type, NodeType::Paragraph);
        assert_eq!(li0.child(0).text_content(), "Hello");

        let li1 = ul.child(1);
        assert_eq!(li1.node_type, NodeType::ListItem);
        assert_eq!(li1.child_count(), 1);
        assert_eq!(li1.child(0).node_type, NodeType::Paragraph);
        assert_eq!(li1.child(0).text_content(), "World");
    }
}
