//! Fragment — an ordered sequence of child nodes.
//!
//! A [`Fragment`] wraps a `Vec<Node>` with a cached total token size and
//! enforces the invariant that adjacent text nodes with identical marks are
//! always merged. This normalization is applied during construction and
//! concatenation.

use crate::node::Node;

/// An ordered, normalized sequence of child nodes.
///
/// Fragments maintain two invariants:
/// 1. The cached `size` field always equals the sum of `node_size()` for all children.
/// 2. No two adjacent children are text nodes with identical mark sets.
#[derive(Clone, Debug)]
pub struct Fragment {
    children: Vec<Node>,
    size: usize,
}

impl Fragment {
    /// Creates an empty fragment with no children and size 0.
    pub fn empty() -> Self {
        Fragment {
            children: Vec::new(),
            size: 0,
        }
    }

    /// Creates a fragment containing a single node.
    pub fn from_node(node: Node) -> Self {
        let size = node.node_size();
        Fragment {
            children: vec![node],
            size,
        }
    }

    /// Creates a fragment from a vector of nodes, normalizing adjacent text
    /// nodes with identical marks by merging them.
    pub fn from_vec(nodes: Vec<Node>) -> Self {
        let merged = normalize_nodes(nodes);
        let size = merged.iter().map(|n| n.node_size()).sum();
        Fragment {
            children: merged,
            size,
        }
    }

    /// Returns a reference to the child at `index`.
    ///
    /// # Panics
    ///
    /// Panics if `index >= child_count()`.
    pub fn child(&self, index: usize) -> &Node {
        &self.children[index]
    }

    /// Returns a slice of all child nodes.
    pub fn children(&self) -> &[Node] {
        &self.children
    }

    /// Returns the number of direct children.
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Returns the total token size of all children.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Finds the child index, intra-child offset, and child start position for
    /// a token position.
    ///
    /// Given a position within this fragment's token space (0..size), returns
    /// `(child_index, offset_within_child, child_start_pos)` where
    /// `child_start_pos` is the absolute offset of the matched child within
    /// this fragment. If `pos` equals the size, returns
    /// `(child_count, 0, size)`.
    ///
    /// # Panics
    ///
    /// Panics if `pos > self.size()`.
    pub fn find_index(&self, pos: usize) -> (usize, usize, usize) {
        assert!(
            pos <= self.size,
            "position {pos} out of bounds for fragment of size {}",
            self.size
        );

        let mut cur = 0;
        for (i, child) in self.children.iter().enumerate() {
            let end = cur + child.node_size();
            if pos < end {
                return (i, pos - cur, cur);
            }
            cur = end;
        }

        (self.children.len(), 0, cur)
    }

    /// Extracts a sub-fragment by token position range `[from, to)`.
    ///
    /// Cuts through text nodes as needed, preserving marks.
    ///
    /// # Panics
    ///
    /// Panics if `from > to` or `to > self.size()`.
    pub fn cut(&self, from: usize, to: usize) -> Fragment {
        assert!(from <= to, "cut: from ({from}) > to ({to})");
        assert!(
            to <= self.size,
            "cut: to ({to}) > size ({})",
            self.size
        );

        if from == to {
            return Fragment::empty();
        }

        if from == 0 && to == self.size {
            return self.clone();
        }

        let (from_idx, from_offset, _) = self.find_index(from);
        let (to_idx, to_offset, _) = self.find_index(to);

        if from_idx == to_idx {
            // Same child — delegate to the child.
            let child = &self.children[from_idx];
            return Self::cut_single_child(child, from_offset, to_offset);
        }

        let mut result = Vec::new();

        // First child (may be partial if from_offset > 0).
        if from_idx < self.children.len() {
            let first = &self.children[from_idx];
            if from_offset == 0 {
                result.push(first.clone());
            } else {
                Self::push_partial_child_from(&mut result, first, from_offset);
            }
        }

        // Full children in between.
        for i in (from_idx + 1)..to_idx {
            result.push(self.children[i].clone());
        }

        // Last child (may be partial if to_offset > 0).
        // to_offset == 0 means the cut ends exactly at this child's start,
        // so the child is NOT included.
        if to_idx > from_idx && to_idx < self.children.len() && to_offset > 0 {
            let last = &self.children[to_idx];
            if to_offset == last.node_size() {
                result.push(last.clone());
            } else {
                Self::push_partial_child_to(&mut result, last, to_offset);
            }
        }

        Fragment::from_vec(result)
    }

    /// Cuts a single child node from `from_offset` to `to_offset`.
    fn cut_single_child(child: &Node, from_offset: usize, to_offset: usize) -> Fragment {
        if child.is_text() {
            if from_offset == 0 && to_offset == child.node_size() {
                return Fragment::from_node(child.clone());
            }
            return Fragment::from_node(child.cut_text(from_offset, to_offset));
        }
        // Branch node: keep the wrapper, cut its content.
        let inner_from = from_offset.saturating_sub(1);
        let inner_to = to_offset.saturating_sub(1).min(child.content.size());
        let cut_content = child.content.cut(inner_from, inner_to);
        Fragment::from_node(Node::branch_with_attrs(
            child.node_type,
            child.attrs.clone(),
            cut_content,
        ))
    }

    /// Appends a partial slice of `child` starting at `from_offset` to `result`.
    fn push_partial_child_from(result: &mut Vec<Node>, child: &Node, from_offset: usize) {
        if child.is_text() {
            result.push(child.cut_text(from_offset, child.node_size()));
        } else {
            let inner_from = from_offset.saturating_sub(1);
            let cut_content = child.content.cut(inner_from, child.content.size());
            result.push(Node::branch_with_attrs(
                child.node_type,
                child.attrs.clone(),
                cut_content,
            ));
        }
    }

    /// Appends a partial slice of `child` ending at `to_offset` to `result`.
    fn push_partial_child_to(result: &mut Vec<Node>, child: &Node, to_offset: usize) {
        if child.is_text() {
            result.push(child.cut_text(0, to_offset));
        } else {
            let inner_to = to_offset.saturating_sub(1).min(child.content.size());
            let cut_content = child.content.cut(0, inner_to);
            result.push(Node::branch_with_attrs(
                child.node_type,
                child.attrs.clone(),
                cut_content,
            ));
        }
    }

    /// Concatenates two fragments, merging adjacent text nodes if possible.
    pub fn append(self, other: Fragment) -> Fragment {
        if other.child_count() == 0 {
            return self;
        }
        if self.child_count() == 0 {
            return other;
        }

        let mut nodes = self.children;
        let mut other_iter = other.children.into_iter();

        // Try to merge the last node of self with the first of other.
        if let Some(last) = nodes.last()
            && let Some(first_other) = other_iter.as_slice().first()
            && last.can_merge_with(first_other)
        {
            let first_other = other_iter.next().unwrap();
            let merged = nodes.pop().unwrap().merge_text(&first_other);
            nodes.push(merged);
        }

        nodes.extend(other_iter);
        let size = nodes.iter().map(|n| n.node_size()).sum();
        Fragment {
            children: nodes,
            size,
        }
    }

    /// Returns a new fragment with the child at `index` replaced by `node`.
    ///
    /// # Panics
    ///
    /// Panics if `index >= child_count()`.
    pub fn replace_child(&self, index: usize, node: Node) -> Fragment {
        let mut children = self.children.clone();
        children[index] = node;
        // Recompute size since the replacement may differ.
        let size = children.iter().map(|n| n.node_size()).sum();
        Fragment {
            children,
            size,
        }
    }

    /// Replaces children in the range `[from_index, to_index)` with the given nodes.
    ///
    /// Returns a new fragment with the specified range replaced. The replacement
    /// nodes are normalized (adjacent text nodes with identical marks are merged).
    ///
    /// # Panics
    ///
    /// Panics if `from_index > to_index` or `to_index > child_count()`.
    pub fn replace_range(&self, from_index: usize, to_index: usize, nodes: Vec<Node>) -> Fragment {
        assert!(
            from_index <= to_index,
            "replace_range: from_index ({from_index}) > to_index ({to_index})"
        );
        assert!(
            to_index <= self.children.len(),
            "replace_range: to_index ({to_index}) > child_count ({})",
            self.children.len()
        );

        let mut result = Vec::with_capacity(from_index + nodes.len() + (self.children.len() - to_index));
        result.extend_from_slice(&self.children[..from_index]);
        result.extend(nodes);
        result.extend_from_slice(&self.children[to_index..]);
        Fragment::from_vec(result)
    }

    /// Returns an iterator over the children.
    pub fn iter(&self) -> impl Iterator<Item = &Node> {
        self.children.iter()
    }
}

/// Merges adjacent text nodes with identical marks.
fn normalize_nodes(nodes: Vec<Node>) -> Vec<Node> {
    if nodes.is_empty() {
        return nodes;
    }

    let mut result: Vec<Node> = Vec::with_capacity(nodes.len());

    for node in nodes {
        if let Some(last) = result.last()
            && last.can_merge_with(&node)
        {
            let merged = result.pop().unwrap().merge_text(&node);
            result.push(merged);
            continue;
        }
        result.push(node);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mark::{Mark, MarkType};
    use crate::node_type::NodeType;

    #[test]
    fn empty_fragment() {
        let f = Fragment::empty();
        assert_eq!(f.child_count(), 0);
        assert_eq!(f.size(), 0);
    }

    #[test]
    fn from_single_node() {
        let f = Fragment::from_node(Node::new_text("Hello"));
        assert_eq!(f.child_count(), 1);
        assert_eq!(f.size(), 5);
    }

    #[test]
    fn from_vec_basic() {
        let f = Fragment::from_vec(vec![
            Node::new_text("Hello"),
            Node::leaf(NodeType::HardBreak),
            Node::new_text("World"),
        ]);
        assert_eq!(f.child_count(), 3);
        // 5 + 1 + 5 = 11
        assert_eq!(f.size(), 11);
    }

    // ── Text node merging ───────────────────────────────────────────

    #[test]
    fn adjacent_text_nodes_same_marks_are_merged() {
        let f = Fragment::from_vec(vec![
            Node::new_text("Hello"),
            Node::new_text(" World"),
        ]);
        assert_eq!(f.child_count(), 1);
        assert_eq!(f.child(0).text(), Some("Hello World"));
        assert_eq!(f.size(), 11);
    }

    #[test]
    fn adjacent_text_nodes_different_marks_not_merged() {
        let f = Fragment::from_vec(vec![
            Node::new_text("Hello"),
            Node::new_text_with_marks("World", vec![Mark::new(MarkType::Strong)]),
        ]);
        assert_eq!(f.child_count(), 2);
    }

    #[test]
    fn three_adjacent_text_nodes_merged() {
        let f = Fragment::from_vec(vec![
            Node::new_text("a"),
            Node::new_text("b"),
            Node::new_text("c"),
        ]);
        assert_eq!(f.child_count(), 1);
        assert_eq!(f.child(0).text(), Some("abc"));
    }

    #[test]
    fn merge_preserves_marks() {
        let marks = vec![Mark::new(MarkType::Em)];
        let f = Fragment::from_vec(vec![
            Node::new_text_with_marks("Hello", marks.clone()),
            Node::new_text_with_marks(" World", marks),
        ]);
        assert_eq!(f.child_count(), 1);
        assert_eq!(f.child(0).text(), Some("Hello World"));
        assert_eq!(f.child(0).marks.len(), 1);
        assert_eq!(f.child(0).marks[0].mark_type, MarkType::Em);
    }

    // ── find_index ──────────────────────────────────────────────────

    #[test]
    fn find_index_at_start() {
        let f = Fragment::from_node(Node::new_text("Hello"));
        assert_eq!(f.find_index(0), (0, 0, 0));
    }

    #[test]
    fn find_index_at_end_of_single_child() {
        // Single text child: pos == size resolves to sentinel (child_count, 0, size).
        let f = Fragment::from_node(Node::new_text("Hello"));
        assert_eq!(f.find_index(5), (1, 0, 5));
    }

    #[test]
    fn find_index_between_children() {
        // [text("Hi"), hr, text("Yo")]
        // Sizes: 2, 1, 2 — total 5
        let f = Fragment::from_vec(vec![
            Node::new_text("Hi"),
            Node::leaf(NodeType::HardBreak),
            Node::new_text("Yo"),
        ]);
        // Position 0: start of "Hi", child_start=0
        assert_eq!(f.find_index(0), (0, 0, 0));
        // Position 2: boundary — start of hr, child_start=2
        assert_eq!(f.find_index(2), (1, 0, 2));
        // Position 3: boundary — start of "Yo", child_start=3
        assert_eq!(f.find_index(3), (2, 0, 3));
        // Position 5: sentinel, child_start=5
        assert_eq!(f.find_index(5), (3, 0, 5));
    }

    #[test]
    fn find_index_with_branch_child() {
        // <p>Hi</p> has node_size 4 (1+2+1)
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hi")),
        );
        let f = Fragment::from_node(p);
        // Position 0: child_start=0
        assert_eq!(f.find_index(0), (0, 0, 0));
        // Position 2: inside the paragraph, child_start=0
        assert_eq!(f.find_index(2), (0, 2, 0));
        // Position 4: sentinel (end of only child), child_start=4
        assert_eq!(f.find_index(4), (1, 0, 4));
    }

    #[test]
    fn find_index_sentinel() {
        // find_index(fragment.size()) always returns (child_count, 0, size).
        let f = Fragment::from_vec(vec![
            Node::new_text("Hi"),
            Node::leaf(NodeType::HardBreak),
        ]);
        assert_eq!(f.find_index(f.size()), (2, 0, 3));

        let f2 = Fragment::from_node(Node::new_text("Hello"));
        assert_eq!(f2.find_index(f2.size()), (1, 0, 5));

        let f3 = Fragment::empty();
        assert_eq!(f3.find_index(0), (0, 0, 0));
    }

    #[test]
    #[should_panic(expected = "position 10 out of bounds")]
    fn find_index_out_of_bounds_panics() {
        let f = Fragment::from_node(Node::new_text("Hi"));
        f.find_index(10);
    }

    // ── cut ─────────────────────────────────────────────────────────

    #[test]
    fn cut_full_range_returns_clone() {
        let f = Fragment::from_node(Node::new_text("Hello"));
        let cut = f.cut(0, 5);
        assert_eq!(cut.size(), 5);
        assert_eq!(cut.child(0).text(), Some("Hello"));
    }

    #[test]
    fn cut_text_node_middle() {
        let f = Fragment::from_node(Node::new_text("Hello"));
        let cut = f.cut(1, 4);
        assert_eq!(cut.size(), 3);
        assert_eq!(cut.child(0).text(), Some("ell"));
    }

    #[test]
    fn cut_across_children() {
        // [text("Hello"), text("World")] — they have different marks so won't merge
        let f = Fragment::from_vec(vec![
            Node::new_text_with_marks("Hello", vec![Mark::new(MarkType::Strong)]),
            Node::new_text("World"),
        ]);
        // Cut "lloWo" — from pos 2 to pos 7
        let cut = f.cut(2, 7);
        assert_eq!(cut.size(), 5);
    }

    #[test]
    fn cut_empty_range() {
        let f = Fragment::from_node(Node::new_text("Hello"));
        let cut = f.cut(2, 2);
        assert_eq!(cut.size(), 0);
        assert_eq!(cut.child_count(), 0);
    }

    // ── append ──────────────────────────────────────────────────────

    #[test]
    fn append_empty_to_nonempty() {
        let f = Fragment::from_node(Node::new_text("Hello"));
        let result = f.append(Fragment::empty());
        assert_eq!(result.size(), 5);
    }

    #[test]
    fn append_nonempty_to_empty() {
        let f = Fragment::from_node(Node::new_text("Hello"));
        let result = Fragment::empty().append(f);
        assert_eq!(result.size(), 5);
    }

    #[test]
    fn append_merges_adjacent_text() {
        let a = Fragment::from_node(Node::new_text("Hello"));
        let b = Fragment::from_node(Node::new_text(" World"));
        let result = a.append(b);
        assert_eq!(result.child_count(), 1);
        assert_eq!(result.child(0).text(), Some("Hello World"));
        assert_eq!(result.size(), 11);
    }

    #[test]
    fn append_no_merge_different_marks() {
        let a = Fragment::from_node(Node::new_text_with_marks(
            "Hello",
            vec![Mark::new(MarkType::Strong)],
        ));
        let b = Fragment::from_node(Node::new_text(" World"));
        let result = a.append(b);
        assert_eq!(result.child_count(), 2);
    }

    // ── replace_child ───────────────────────────────────────────────

    #[test]
    fn replace_child_updates_size() {
        let f = Fragment::from_vec(vec![
            Node::new_text("Hi"),
            Node::new_text_with_marks("There", vec![Mark::new(MarkType::Em)]),
        ]);
        assert_eq!(f.size(), 7);

        let replaced = f.replace_child(0, Node::new_text("Hello"));
        assert_eq!(replaced.size(), 10); // 5 + 5
        assert_eq!(replaced.child(0).text(), Some("Hello"));
    }

    // ── iterator ────────────────────────────────────────────────────

    #[test]
    fn iter_visits_all_children() {
        let f = Fragment::from_vec(vec![
            Node::new_text("a"),
            Node::leaf(NodeType::HardBreak),
            Node::new_text("b"),
        ]);
        let texts: Vec<_> = f.iter().map(|n| n.node_type).collect();
        assert_eq!(texts, vec![NodeType::Text, NodeType::HardBreak, NodeType::Text]);
    }

    // ── cut across branch node boundaries ───────────────────────────

    #[test]
    fn cut_preserves_branch_wrapper_single_child() {
        // <p>Hello</p> = node_size 7 (1 + 5 + 1)
        // Fragment: [<p>Hello</p>]
        // Cutting from 1 to 4 should yield <p>Hel</p>, not raw "Hel".
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hello")),
        );
        let f = Fragment::from_node(p);
        let cut = f.cut(1, 4);
        assert_eq!(cut.child_count(), 1);
        assert_eq!(cut.child(0).node_type, NodeType::Paragraph);
        assert_eq!(cut.child(0).text_content(), "Hel");
        // 1 (open) + 3 (text) + 1 (close) = 5
        assert_eq!(cut.child(0).node_size(), 5);
    }

    #[test]
    fn cut_across_two_branch_nodes() {
        // [<p>Hello</p>, <p>World</p>]
        // p1: size 7 (1+5+1), p2: size 7 (1+5+1), total 14
        // Cut from 3 to 11: partial p1 from offset 3 + partial p2 to offset 4
        let p1 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hello")),
        );
        let p2 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("World")),
        );
        let f = Fragment::from_vec(vec![p1, p2]);
        assert_eq!(f.size(), 14);

        // from=3 -> find_index: pos 3 is inside p1 (size 7), returns (0, 3)
        // to=11 -> pos 11 is inside p2 (starts at 7, size 7, end 14), returns (1, 4)
        let cut = f.cut(3, 11);
        assert_eq!(cut.child_count(), 2);
        // First child: <p>llo</p> (inner from offset 2)
        assert_eq!(cut.child(0).node_type, NodeType::Paragraph);
        assert_eq!(cut.child(0).text_content(), "llo");
        // Second child: <p>Wor</p> (inner to offset 3)
        assert_eq!(cut.child(1).node_type, NodeType::Paragraph);
        assert_eq!(cut.child(1).text_content(), "Wor");
    }

    // ── children accessor ───────────────────────────────────────────

    #[test]
    fn children_accessor_returns_slice() {
        let f = Fragment::from_vec(vec![
            Node::new_text("Hi"),
            Node::leaf(NodeType::HardBreak),
        ]);
        let children = f.children();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].node_type, NodeType::Text);
        assert_eq!(children[1].node_type, NodeType::HardBreak);
    }
}
