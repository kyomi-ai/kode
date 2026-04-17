//! Document tree nodes.
//!
//! A [`Node`] is the fundamental unit of the document tree. Every node has a
//! [`NodeType`], optional attributes, a content [`Fragment`], and optional marks.
//! Text nodes additionally carry a string payload.
//!
//! Nodes use a token-based sizing system where:
//! - Text nodes: size = number of characters
//! - Leaf nodes (hr, hard_break, image): size = 1
//! - Branch nodes: size = 1 (open) + content size + 1 (close)

use crate::attrs::{empty_attrs, Attrs};
use crate::fragment::Fragment;
use crate::mark::Mark;
use crate::node_type::NodeType;
use crate::position::ResolvedPos;
use crate::slice::Slice;

/// A node in the document tree.
///
/// Nodes are the building blocks of the document model. Each node has a type
/// that determines its structural role, along with optional attributes, child
/// content, marks (for inline formatting), and text content (for text nodes).
#[derive(Clone, Debug)]
pub struct Node {
    /// The structural type of this node.
    pub node_type: NodeType,
    /// Key-value attributes (e.g. heading level, link href).
    pub attrs: Attrs,
    /// Child nodes wrapped in a [`Fragment`].
    pub content: Fragment,
    /// Inline formatting marks (only meaningful on text nodes).
    pub marks: Vec<Mark>,
    /// Text payload (only present on text nodes).
    text: Option<String>,
}

impl Node {
    // ── Constructors ────────────────────────────────────────────────

    /// Creates a text node with the given content and no marks.
    ///
    /// # Panics
    ///
    /// Panics if `text` is empty — text nodes must have content.
    pub fn new_text(text: &str) -> Self {
        assert!(!text.is_empty(), "text nodes must have non-empty content");
        Node {
            node_type: NodeType::Text,
            attrs: empty_attrs(),
            content: Fragment::empty(),
            marks: Vec::new(),
            text: Some(text.to_string()),
        }
    }

    /// Creates a text node with the given content and marks.
    ///
    /// # Panics
    ///
    /// Panics if `text` is empty.
    pub fn new_text_with_marks(text: &str, marks: Vec<Mark>) -> Self {
        assert!(!text.is_empty(), "text nodes must have non-empty content");
        Node {
            node_type: NodeType::Text,
            attrs: empty_attrs(),
            content: Fragment::empty(),
            marks,
            text: Some(text.to_string()),
        }
    }

    /// Creates a leaf node (e.g. horizontal rule, hard break) with no attributes.
    pub fn leaf(node_type: NodeType) -> Self {
        Node {
            node_type,
            attrs: empty_attrs(),
            content: Fragment::empty(),
            marks: Vec::new(),
            text: None,
        }
    }

    /// Creates a leaf node with the given attributes (e.g. image with src/alt).
    pub fn leaf_with_attrs(node_type: NodeType, attrs: Attrs) -> Self {
        Node {
            node_type,
            attrs,
            content: Fragment::empty(),
            marks: Vec::new(),
            text: None,
        }
    }

    /// Creates a branch node with the given content and no attributes.
    pub fn branch(node_type: NodeType, content: Fragment) -> Self {
        Node {
            node_type,
            attrs: empty_attrs(),
            content,
            marks: Vec::new(),
            text: None,
        }
    }

    /// Creates a branch node with the given attributes and content.
    pub fn branch_with_attrs(node_type: NodeType, attrs: Attrs, content: Fragment) -> Self {
        Node {
            node_type,
            attrs,
            content,
            marks: Vec::new(),
            text: None,
        }
    }

    // ── Position resolution ────────────────────────────────────────

    /// Resolve a position in this node's content to a [`ResolvedPos`].
    ///
    /// The position is in the range `[0, self.content.size()]` — it refers to
    /// a location within this node's content, not including this node's own
    /// opening/closing tokens.
    ///
    /// # Panics
    ///
    /// Panics if `pos > self.content.size()`.
    pub fn resolve(&self, pos: usize) -> ResolvedPos {
        ResolvedPos::resolve(self, pos)
    }

    // ── Size and content ────────────────────────────────────────────

    /// Returns the token size of this node.
    ///
    /// - Text nodes: number of characters in the text
    /// - Non-text leaf nodes (hr, hard_break, image): 1
    /// - Branch nodes: 1 (open) + content size + 1 (close)
    pub fn node_size(&self) -> usize {
        if self.node_type.is_text() {
            self.text.as_ref().map_or(0, |t| t.chars().count())
        } else if self.node_type.is_leaf() {
            1
        } else {
            1 + self.content.size() + 1
        }
    }

    /// Concatenates the text content of all descendants.
    ///
    /// For text nodes, returns the text directly. For branch nodes, recursively
    /// collects text from all descendant text nodes.
    pub fn text_content(&self) -> String {
        if let Some(ref t) = self.text {
            return t.clone();
        }
        let mut out = String::new();
        for child in self.content.iter() {
            out.push_str(&child.text_content());
        }
        out
    }

    /// Returns `true` if this is a text node.
    pub fn is_text(&self) -> bool {
        self.node_type.is_text()
    }

    /// Returns the text content of this node, if it is a text node.
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    // ── Child access (delegates to Fragment) ────────────────────────

    /// Returns a reference to the child at the given index.
    ///
    /// # Panics
    ///
    /// Panics if `index` is out of bounds.
    pub fn child(&self, index: usize) -> &Node {
        self.content.child(index)
    }

    /// Returns the number of direct children.
    pub fn child_count(&self) -> usize {
        self.content.child_count()
    }

    /// Returns the first child node, if any.
    pub fn first_child(&self) -> Option<&Node> {
        if self.content.child_count() > 0 {
            Some(self.content.child(0))
        } else {
            None
        }
    }

    /// Returns the last child node, if any.
    pub fn last_child(&self) -> Option<&Node> {
        let count = self.content.child_count();
        if count > 0 {
            Some(self.content.child(count - 1))
        } else {
            None
        }
    }

    // ── Traversal ───────────────────────────────────────────────────

    /// Iterates over all descendant nodes within the given token position range.
    ///
    /// The callback receives `(node, absolute_pos, parent, child_index)` for
    /// each node whose span overlaps `[from, to)`. The `from` and `to`
    /// parameters are relative to this node's content (i.e. excluding the
    /// opening token of `self`).
    ///
    /// The callback returns `true` to descend into the node's children, or
    /// `false` to skip them.
    pub fn nodes_between<F>(&self, from: usize, to: usize, f: &mut F)
    where
        F: FnMut(&Node, usize, Option<&Node>, usize) -> bool,
    {
        self.nodes_between_inner(from, to, 0, f);
    }

    fn nodes_between_inner<F>(&self, from: usize, to: usize, start_pos: usize, f: &mut F)
    where
        F: FnMut(&Node, usize, Option<&Node>, usize) -> bool,
    {
        let mut pos = start_pos;
        for (i, child) in self.content.iter().enumerate() {
            let child_size = child.node_size();
            let end = pos + child_size;

            if end > from && pos < to {
                let descend = f(child, pos, Some(self), i);
                if descend && child.content.child_count() > 0 {
                    // For branch children, content starts at pos + 1 (after the opening token).
                    child.nodes_between_inner(from, to, pos + 1, f);
                }
            }

            pos = end;
        }
    }

    // ── Internal helpers for Fragment ────────────────────────────────

    /// Returns `true` if this text node can be merged with `other`.
    ///
    /// Two text nodes can merge if they are both text nodes with identical marks.
    pub(crate) fn can_merge_with(&self, other: &Node) -> bool {
        self.is_text() && other.is_text() && Mark::same_set(&self.marks, &other.marks)
    }

    /// Merges this text node with `other` by appending their text content.
    ///
    /// # Panics
    ///
    /// Panics if either node is not a text node.
    pub(crate) fn merge_text(mut self, other: &Node) -> Node {
        let self_text = self.text.take().expect("merge_text called on non-text node");
        let other_text = other.text.as_deref().expect("merge_text called with non-text node");
        self.text = Some(format!("{}{}", self_text, other_text));
        self
    }

    /// Creates a text node that is a substring of this text node.
    ///
    /// # Panics
    ///
    /// Panics if this is not a text node.
    pub(crate) fn cut_text(&self, from: usize, to: usize) -> Node {
        let text = self.text.as_deref().expect("cut_text called on non-text node");
        let sliced: String = text.chars().skip(from).take(to - from).collect();
        Node::new_text_with_marks(&sliced, self.marks.clone())
    }

    // ── Slice and replace ──────────────────────────────────────────

    /// Extract a slice from this node between two content positions.
    ///
    /// The positions are relative to this node's content (`0` to
    /// `self.content.size()`). The returned slice records how many
    /// node levels were cut through at the start and end
    /// (`open_start`, `open_end`).
    ///
    /// # Panics
    ///
    /// Panics if `from > to` or `to > self.content.size()`.
    pub fn slice(&self, from: usize, to: usize) -> Slice {
        assert!(from <= to, "slice: from ({from}) > to ({to})");
        assert!(
            to <= self.content.size(),
            "slice: to ({to}) > content size ({})",
            self.content.size()
        );

        if from == to {
            return Slice::empty();
        }

        let res_from = self.resolve(from);
        let res_to = self.resolve(to);
        let shared = res_from.shared_depth(to);

        // open_start and open_end are the number of node levels cut through
        // beyond the shared ancestor.
        let open_start = res_from.depth - shared;
        let open_end = res_to.depth - shared;

        // Extract the content between from and to within the shared ancestor.
        let shared_start = res_from.start(shared);
        let content = self.content_between(&res_from, &res_to, shared, shared_start);

        Slice::new(content, open_start, open_end)
    }

    /// Extract content between two resolved positions at the shared depth.
    ///
    /// Walks from the `from` position up to the shared depth collecting
    /// right-side content, then from the `to` position collecting left-side
    /// content, and assembles them into a single fragment.
    fn content_between(
        &self,
        from: &ResolvedPos,
        to: &ResolvedPos,
        shared_depth: usize,
        shared_start: usize,
    ) -> Fragment {
        // Simple approach: use Fragment::cut on the shared ancestor's content.
        // The from/to positions relative to the shared ancestor's content are:
        let cut_from = from.pos - shared_start;
        let cut_to = to.pos - shared_start;
        let shared_node = from.node(shared_depth);
        shared_node.content.cut(cut_from, cut_to)
    }

    /// Replace content between two positions with a slice.
    ///
    /// This is the fundamental edit operation. All other transforms
    /// (split, join, insert, delete) are built on top of replace.
    ///
    /// The algorithm handles open slices by joining open sides with the
    /// surrounding document structure. When a slice is "open" on one side,
    /// its content at that side gets merged into the existing node rather
    /// than creating a new wrapper.
    ///
    /// For example, deleting across `<p>He|llo</p><p>Wo|rld</p>` produces
    /// `<p>Horld</p>` — the two partial paragraphs are merged.
    ///
    /// # Panics
    ///
    /// Panics if `from > to` or `to > self.content.size()`.
    pub fn replace(&self, from: usize, to: usize, slice: Slice) -> Result<Node, String> {
        assert!(from <= to, "replace: from ({from}) > to ({to})");
        assert!(
            to <= self.content.size(),
            "replace: to ({to}) > content size ({})",
            self.content.size()
        );

        if from == to && slice.is_empty() {
            return Ok(self.clone());
        }

        let res_from = self.resolve(from);
        let res_to = self.resolve(to);

        replace_doc(&res_from, &res_to, &slice)
    }
}

/// Top-level replace: find the shared depth, build new content, rebuild ancestors.
fn replace_doc(
    from: &ResolvedPos,
    to: &ResolvedPos,
    slice: &Slice,
) -> Result<Node, String> {
    let mut shared = from.shared_depth(to.pos);

    // When the slice has open sides that need to split nodes, cap the shared
    // depth so the replace algorithm works at a level that can accommodate
    // the split. Without this, an open slice inserted at from==to inside a
    // textblock would be peeled flat and lose its node-splitting structure.
    //
    // The open sides of the slice indicate how many levels of nesting are
    // "open" and need to join with surrounding content. The replace must
    // operate at a depth where both from and to have enough depth remaining
    // to descend into the open sides.
    if slice.open_start > 0 || slice.open_end > 0 {
        let max_from = from.depth.saturating_sub(slice.open_start);
        let max_to = to.depth.saturating_sub(slice.open_end);
        shared = shared.min(max_from).min(max_to);
    }

    // Build new content at the shared ancestor level.
    let new_content = replace_at_depth(from, to, slice, shared);

    // Rebuild from the shared ancestor back up to the root.
    rebuild_from(from, to, shared, new_content)
}

/// Build replacement content at depth `d` within the shared ancestor.
///
/// This is the core recursive algorithm adapted from ProseMirror's `replace`.
/// It handles three cases:
///
/// 1. **Same child**: Both from and to are inside the same child at depth `d`.
///    Recurse into that child at depth `d+1`.
///
/// 2. **Different children, same parent**: from and to are in different children.
///    Close the left branch (content before from + open-start slice content),
///    close the right branch (open-end slice content + content after to),
///    merge them into a single node, and replace the child range.
///
/// 3. **At deepest level**: from or to is at this depth (not deeper).
///    Do a direct cut-and-splice.
fn replace_at_depth(
    from: &ResolvedPos,
    to: &ResolvedPos,
    slice: &Slice,
    d: usize,
) -> Fragment {
    let node = from.node(d);
    let from_idx = from.index(d);
    let to_idx = to.index(d);

    // Case 1: Both positions in the same child — recurse deeper.
    //
    // However, if the slice has open sides that would require splitting
    // at a depth deeper than the flat splice can handle, we must NOT
    // recurse and instead handle the split at this level (Case 3).
    let slice_needs_split = slice.content.child_count() >= 2
        && slice.open_start > 0
        && slice.open_end > 0;
    let can_recurse = from_idx == to_idx
        && from.depth > d
        && to.depth > d
        && !slice_needs_split;

    if can_recurse {
        let child = node.child(from_idx);
        let inner = replace_at_depth(from, to, slice, d + 1);
        let new_child = Node::branch_with_attrs(
            child.node_type,
            child.attrs.clone(),
            inner,
        );
        return node.content.replace_child(from_idx, new_child);
    }

    // Case 2: At least one position is at depth d (not inside a child),
    // or they are in different children but with no open slice sides to
    // descend further. Do a flat splice at this level.
    if from.depth == d
        || to.depth == d
        || (slice.open_start == 0 && slice.open_end == 0 && from_idx == to_idx)
    {
        // Direct splice: cut left, insert slice content, cut right.
        let node_start = from.start(d);
        let left = node.content.cut(0, from.pos - node_start);
        let right = node.content.cut(to.pos - node_start, node.content.size());
        // For open slices: peel as much as we can, but since we're at the
        // flat level, use the raw content.
        let insert = if slice.open_start == 0 && slice.open_end == 0 {
            slice.content.clone()
        } else {
            peel_open(&slice.content, slice.open_start, slice.open_end)
        };
        return left.append(insert).append(right);
    }

    // Case 3: Different children at depth d, and from/to are both deeper.
    //
    // Strategy depends on slice structure:
    // - 0 or 1 children (fully consumed by open sides): merge left + right into ONE node
    // - 2+ children: first child → left join, last child → right join, middle stays

    let left_depth = (d + 1 + slice.open_start).min(from.depth);
    let right_depth = (d + 1 + slice.open_end).min(to.depth);

    // Content before from at the placement depth.
    let left_parent = from.node(left_depth);
    let left_offset = from.pos - from.start(left_depth);
    let left_content = left_parent.content.cut(0, left_offset);

    // Content after to at the placement depth.
    let right_parent = to.node(right_depth);
    let right_offset = to.pos - to.start(right_depth);
    let right_content = right_parent.content.cut(right_offset, right_parent.content.size());

    let middle = slice_middle(slice);
    let has_separate_sides = slice.content.child_count() >= 2
        && slice.open_start > 0
        && slice.open_end > 0;

    let replacement = if has_separate_sides {
        // Multi-child slice: left + middle + right as separate nodes.
        let open_start_content = peel_slice_start(slice);
        let open_end_content = peel_slice_end(slice);

        let left_merged = left_content.append(open_start_content);
        let left_node = close_node_left(from, d, left_depth, left_merged);

        let right_merged = open_end_content.append(right_content);
        let right_node = close_node_right(to, d, right_depth, right_merged);

        let mut nodes = vec![left_node];
        nodes.extend(middle);
        nodes.push(right_node);
        nodes
    } else {
        // Empty slice or single-child: merge everything into one node.
        let inner = peel_open(&slice.content, slice.open_start, slice.open_end);
        let merged = left_content.append(inner).append(right_content);
        let merged_node = close_node_left(from, d, left_depth, merged);
        let mut nodes = vec![merged_node];
        nodes.extend(middle);
        nodes
    };

    let to_end = to_idx + 1;
    node.content.replace_range(from_idx, to_end, replacement)
}

/// Build the left branch: at each ancestor level from inner_depth up to stop_depth,
/// copy children 0..index (before pos), then append the current merged node.
fn close_node_left(
    pos: &ResolvedPos,
    stop_depth: usize,
    inner_depth: usize,
    content: Fragment,
) -> Node {
    let mut current = Node::branch_with_attrs(
        pos.node(inner_depth).node_type,
        pos.node(inner_depth).attrs.clone(),
        content,
    );

    for dd in (stop_depth + 1..inner_depth).rev() {
        let ancestor = pos.node(dd);
        let idx = pos.index(dd);
        let mut children: Vec<Node> = Vec::new();
        for i in 0..idx {
            children.push(ancestor.child(i).clone());
        }
        children.push(current);
        current = Node::branch_with_attrs(
            ancestor.node_type,
            ancestor.attrs.clone(),
            Fragment::from_vec(children),
        );
    }

    current
}

/// Build the right branch: at each ancestor level from inner_depth up to stop_depth,
/// prepend the current merged node, then copy children (index+1)..end (after pos).
fn close_node_right(
    pos: &ResolvedPos,
    stop_depth: usize,
    inner_depth: usize,
    content: Fragment,
) -> Node {
    let mut current = Node::branch_with_attrs(
        pos.node(inner_depth).node_type,
        pos.node(inner_depth).attrs.clone(),
        content,
    );

    for dd in (stop_depth + 1..inner_depth).rev() {
        let ancestor = pos.node(dd);
        let idx = pos.index(dd);
        let mut children = vec![current];
        for i in (idx + 1)..ancestor.child_count() {
            children.push(ancestor.child(i).clone());
        }
        current = Node::branch_with_attrs(
            ancestor.node_type,
            ancestor.attrs.clone(),
            Fragment::from_vec(children),
        );
    }

    current
}

/// Rebuild the tree from depth `at` upward to the root.
///
/// At `at`, creates a new node with `new_content`. Then replaces the
/// corresponding child in each ancestor going up, handling the case
/// where `from` and `to` are in different children (range replacement).
fn rebuild_from(
    from: &ResolvedPos,
    to: &ResolvedPos,
    at: usize,
    new_content: Fragment,
) -> Result<Node, String> {
    let mut current = Node::branch_with_attrs(
        from.node(at).node_type,
        from.node(at).attrs.clone(),
        new_content,
    );

    for d in (0..at).rev() {
        let ancestor = from.node(d);
        let from_idx = from.index(d);
        debug_assert_eq!(
            from_idx,
            to.index(d),
            "rebuild_from: from/to must pass through same child above shared depth"
        );

        let updated = ancestor.content.replace_child(from_idx, current);
        current = Node::branch_with_attrs(
            ancestor.node_type,
            ancestor.attrs.clone(),
            updated,
        );
    }

    Ok(current)
}

/// Peel off open layers from a fragment.
///
/// Given a fragment with `open_start` open layers on the left and `open_end`
/// on the right, descend into the first/last children to extract the inner
/// content. The open layers are node wrappers that should be joined with
/// the surrounding document rather than kept as separate nodes.
///
/// For a single-child fragment, peels from both sides simultaneously.
/// For multi-child fragments, peels left from the first child and right
/// from the last child independently.
fn peel_open(content: &Fragment, open_start: usize, open_end: usize) -> Fragment {
    if open_start == 0 && open_end == 0 {
        return content.clone();
    }

    if content.child_count() == 0 {
        return Fragment::empty();
    }

    if content.child_count() == 1 {
        let child = content.child(0);
        if child.node_type.is_leaf() {
            return content.clone();
        }
        let new_open_start = open_start.saturating_sub(1);
        let new_open_end = open_end.saturating_sub(1);
        return peel_open(&child.content, new_open_start, new_open_end);
    }

    // Multiple children: peel left from first child, right from last child.
    let first = content.child(0);
    let last = content.child(content.child_count() - 1);

    let left = if open_start > 0 && !first.node_type.is_leaf() {
        peel_open(&first.content, open_start - 1, 0)
    } else {
        Fragment::from_node(first.clone())
    };

    let right = if open_end > 0 && !last.node_type.is_leaf() {
        peel_open(&last.content, 0, open_end - 1)
    } else {
        Fragment::from_node(last.clone())
    };

    // Middle children (between first and last) stay as-is.
    let mut middle = Fragment::empty();
    for i in 1..content.child_count() - 1 {
        middle = middle.append(Fragment::from_node(content.child(i).clone()));
    }

    left.append(middle).append(right)
}

/// Peel the open-start side of a slice to get the innermost content
/// that should be appended to the left side during a replace.
///
/// Descends into the FIRST child `open_start` times, returning its content.
fn peel_slice_start(slice: &Slice) -> Fragment {
    if slice.open_start == 0 || slice.content.child_count() == 0 {
        return Fragment::empty();
    }

    let mut content = &slice.content;
    for _ in 0..slice.open_start {
        if content.child_count() == 0 {
            return Fragment::empty();
        }
        let first = content.child(0);
        if first.node_type.is_leaf() {
            return content.clone();
        }
        content = &first.content;
    }
    content.clone()
}

/// Peel the open-end side of a slice to get the innermost content
/// that should be prepended to the right side during a replace.
///
/// Descends into the LAST child `open_end` times, returning its content.
fn peel_slice_end(slice: &Slice) -> Fragment {
    if slice.open_end == 0 || slice.content.child_count() == 0 {
        return Fragment::empty();
    }

    let mut content = &slice.content;
    for _ in 0..slice.open_end {
        if content.child_count() == 0 {
            return Fragment::empty();
        }
        let last = content.child(content.child_count() - 1);
        if last.node_type.is_leaf() {
            return content.clone();
        }
        content = &last.content;
    }
    content.clone()
}

/// Extract the middle (non-open) children from a slice.
///
/// If `open_start > 0`, the first child is consumed by the left join.
/// If `open_end > 0`, the last child is consumed by the right join.
/// Everything between is "middle" content that gets inserted as-is.
fn slice_middle(slice: &Slice) -> Vec<Node> {
    if slice.content.child_count() == 0 {
        return Vec::new();
    }

    let start = if slice.open_start > 0 { 1 } else { 0 };
    let end = if slice.open_end > 0 {
        slice.content.child_count().saturating_sub(1)
    } else {
        slice.content.child_count()
    };

    if start >= end {
        return Vec::new();
    }

    let mut result = Vec::new();
    for i in start..end {
        result.push(slice.content.child(i).clone());
    }
    result
}

impl PartialEq for Node {
    fn eq(&self, other: &Node) -> bool {
        if self.node_type != other.node_type {
            return false;
        }
        if self.attrs != other.attrs {
            return false;
        }
        if !Mark::same_set(&self.marks, &other.marks) {
            return false;
        }
        if self.text != other.text {
            return false;
        }
        if self.content.child_count() != other.content.child_count() {
            return false;
        }
        self.content
            .iter()
            .zip(other.content.iter())
            .all(|(a, b)| a == b)
    }
}

impl Eq for Node {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attrs::{heading_attrs, image_attrs};
    use crate::mark::MarkType;

    #[test]
    fn text_node_size() {
        let n = Node::new_text("Hello");
        assert_eq!(n.node_size(), 5);
        assert!(n.is_text());
        assert_eq!(n.text(), Some("Hello"));
    }

    #[test]
    fn text_node_unicode_size() {
        // node_size counts characters, not bytes.
        let n = Node::new_text("héllo");
        assert_eq!(n.node_size(), 5);
    }

    #[test]
    fn text_node_emoji_size() {
        let n = Node::new_text("👍🏻");
        // Two Unicode code points.
        assert_eq!(n.node_size(), 2);
    }

    #[test]
    #[should_panic(expected = "text nodes must have non-empty content")]
    fn text_node_empty_panics() {
        Node::new_text("");
    }

    #[test]
    fn leaf_node_size() {
        let hr = Node::leaf(NodeType::HorizontalRule);
        assert_eq!(hr.node_size(), 1);

        let br = Node::leaf(NodeType::HardBreak);
        assert_eq!(br.node_size(), 1);

        let img = Node::leaf_with_attrs(
            NodeType::Image,
            image_attrs("img.png", "alt text", None),
        );
        assert_eq!(img.node_size(), 1);
    }

    #[test]
    fn branch_node_size() {
        // <p>Hello</p> = 1 + 5 + 1 = 7
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hello")),
        );
        assert_eq!(p.node_size(), 7);
    }

    #[test]
    fn empty_branch_node_size() {
        // <p></p> = 1 + 0 + 1 = 2
        let p = Node::branch(NodeType::Paragraph, Fragment::empty());
        assert_eq!(p.node_size(), 2);
    }

    #[test]
    fn nested_branch_node_size() {
        // <doc><p>Hi</p></doc> = 1 + (1 + 2 + 1) + 1 = 6
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hi")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(p));
        assert_eq!(doc.node_size(), 6);
    }

    #[test]
    fn text_content_of_text_node() {
        let n = Node::new_text("Hello");
        assert_eq!(n.text_content(), "Hello");
    }

    #[test]
    fn text_content_of_branch_collects_all_text() {
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_vec(vec![
                Node::new_text("Hello"),
                Node::new_text(" world"),
            ]),
        );
        assert_eq!(p.text_content(), "Hello world");
    }

    #[test]
    fn child_access() {
        let t1 = Node::new_text("Hello");
        let t2 = Node::new_text(" world");
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_vec(vec![t1, t2]),
        );
        // After merge, there's one child.
        assert_eq!(p.child_count(), 1);
        assert_eq!(p.child(0).text(), Some("Hello world"));
    }

    #[test]
    fn first_and_last_child() {
        let p1 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("First")),
        );
        let p2 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Last")),
        );
        let doc = Node::branch(
            NodeType::Doc,
            Fragment::from_vec(vec![p1, p2]),
        );
        assert_eq!(doc.first_child().unwrap().text_content(), "First");
        assert_eq!(doc.last_child().unwrap().text_content(), "Last");
    }

    #[test]
    fn first_and_last_child_empty() {
        let p = Node::branch(NodeType::Paragraph, Fragment::empty());
        assert!(p.first_child().is_none());
        assert!(p.last_child().is_none());
    }

    #[test]
    fn structural_equality() {
        let a = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hello")),
        );
        let b = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hello")),
        );
        assert_eq!(a, b);
    }

    #[test]
    fn structural_inequality_different_text() {
        let a = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hello")),
        );
        let b = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("World")),
        );
        assert_ne!(a, b);
    }

    #[test]
    fn structural_inequality_different_type() {
        let a = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hello")),
        );
        let b = Node::branch_with_attrs(
            NodeType::Heading,
            heading_attrs(1),
            Fragment::from_node(Node::new_text("Hello")),
        );
        assert_ne!(a, b);
    }

    #[test]
    fn structural_equality_with_marks() {
        let marks = vec![Mark::new(MarkType::Strong)];
        let a = Node::new_text_with_marks("Hello", marks.clone());
        let b = Node::new_text_with_marks("Hello", marks);
        assert_eq!(a, b);
    }

    #[test]
    fn nodes_between_visits_all_children() {
        // <doc><p>Hello</p><p>World</p></doc>
        let p1 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hello")),
        );
        let p2 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("World")),
        );
        let doc = Node::branch(
            NodeType::Doc,
            Fragment::from_vec(vec![p1, p2]),
        );

        let mut visited = Vec::new();
        doc.nodes_between(0, doc.content.size(), &mut |node, pos, _parent, _idx| {
            visited.push((node.node_type, pos));
            true
        });

        // Should visit: p1 at 0, "Hello" at 1, p2 at 7, "World" at 8
        assert_eq!(visited.len(), 4);
        assert_eq!(visited[0], (NodeType::Paragraph, 0));
        assert_eq!(visited[1], (NodeType::Text, 1));
        assert_eq!(visited[2], (NodeType::Paragraph, 7));
        assert_eq!(visited[3], (NodeType::Text, 8));
    }

    #[test]
    fn nodes_between_partial_range() {
        // <doc><p>Hello</p><p>World</p></doc>
        // p1 is at positions 0..7, p2 is at positions 7..14
        let p1 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hello")),
        );
        let p2 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("World")),
        );
        let doc = Node::branch(
            NodeType::Doc,
            Fragment::from_vec(vec![p1, p2]),
        );

        let mut visited = Vec::new();
        // Only visit the second paragraph's range.
        doc.nodes_between(7, 14, &mut |node, pos, _parent, _idx| {
            visited.push((node.node_type, pos));
            true
        });

        assert_eq!(visited.len(), 2);
        assert_eq!(visited[0], (NodeType::Paragraph, 7));
        assert_eq!(visited[1], (NodeType::Text, 8));
    }

    #[test]
    fn nodes_between_skip_children() {
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hello")),
        );
        let doc = Node::branch(
            NodeType::Doc,
            Fragment::from_node(p),
        );

        let mut visited = Vec::new();
        doc.nodes_between(0, doc.content.size(), &mut |node, pos, _parent, _idx| {
            visited.push((node.node_type, pos));
            false // don't descend
        });

        // Should only visit p, not the text inside.
        assert_eq!(visited.len(), 1);
        assert_eq!(visited[0], (NodeType::Paragraph, 0));
    }

    #[test]
    fn cut_text_produces_substring() {
        let n = Node::new_text_with_marks("Hello", vec![Mark::new(MarkType::Strong)]);
        let cut = n.cut_text(1, 4);
        assert_eq!(cut.text(), Some("ell"));
        assert_eq!(cut.marks.len(), 1);
        assert_eq!(cut.marks[0].mark_type, MarkType::Strong);
    }

    // ── Helper for slice/replace tests ──────────────────────────────

    /// Build: <doc><p>Hello</p><p>World</p></doc>
    /// Positions within doc content:
    ///   0: before <p>
    ///   1: start of p1 content (before 'H')
    ///   6: end of p1 content (after 'o')
    ///   7: after </p>, before <p>
    ///   8: start of p2 content (before 'W')
    ///  13: end of p2 content (after 'd')
    ///  14: after </p>
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

    // ── Slice tests ────────────────────────────────────────────────

    #[test]
    fn slice_within_single_paragraph() {
        let doc = two_para_doc();
        // Slice "ell" from positions 2..5 (inside first paragraph).
        // Both positions resolve to depth 1 (inside same paragraph).
        // shared_depth = 1, so open_start = 1-1 = 0, open_end = 1-1 = 0.
        // Content is the paragraph's content cut from offset 1..4 = text("ell").
        let s = doc.slice(2, 5);
        assert_eq!(s.open_start, 0);
        assert_eq!(s.open_end, 0);
        assert_eq!(s.content.child_count(), 1);
        assert_eq!(s.content.child(0).text(), Some("ell"));
    }

    #[test]
    fn slice_across_two_paragraphs() {
        let doc = two_para_doc();
        // Slice from pos 3 (inside "Hello" after "He") to pos 10 (inside "World" after "Wo")
        // This cuts through both paragraphs.
        let s = doc.slice(3, 10);
        assert_eq!(s.open_start, 1); // cut through p1
        assert_eq!(s.open_end, 1);   // cut through p2
        assert_eq!(s.content.child_count(), 2);
        assert_eq!(s.content.child(0).text_content(), "llo");
        assert_eq!(s.content.child(1).text_content(), "Wo");
    }

    #[test]
    fn slice_full_paragraph() {
        let doc = two_para_doc();
        // Slice the entire first paragraph: positions 0..7 (includes open/close tokens).
        let s = doc.slice(0, 7);
        assert_eq!(s.open_start, 0); // not cut through — starts at doc level
        assert_eq!(s.open_end, 0);   // not cut through — ends at doc level
        assert_eq!(s.content.child_count(), 1);
        assert_eq!(s.content.child(0).node_type, NodeType::Paragraph);
        assert_eq!(s.content.child(0).text_content(), "Hello");
    }

    #[test]
    fn slice_empty_range() {
        let doc = two_para_doc();
        let s = doc.slice(3, 3);
        assert!(s.is_empty());
        assert_eq!(s.size(), 0);
    }

    #[test]
    fn slice_entire_document() {
        let doc = two_para_doc();
        let s = doc.slice(0, doc.content.size());
        assert_eq!(s.open_start, 0);
        assert_eq!(s.open_end, 0);
        assert_eq!(s.content.child_count(), 2);
    }

    #[test]
    fn slice_at_paragraph_boundary() {
        let doc = two_para_doc();
        // Slice from end of p1 content to start of p2 content: positions 7..7
        // (between paragraphs).
        let s = doc.slice(7, 7);
        assert!(s.is_empty());
    }

    // ── Replace tests ──────────────────────────────────────────────

    #[test]
    fn replace_delete_text_within_paragraph() {
        let doc = two_para_doc();
        // Delete "ell" from positions 2..5 inside p1.
        let result = doc.replace(2, 5, Slice::empty()).unwrap();
        assert_eq!(result.child_count(), 2);
        assert_eq!(result.child(0).text_content(), "Ho");
        assert_eq!(result.child(1).text_content(), "World");
    }

    #[test]
    fn replace_delete_across_paragraphs() {
        let doc = two_para_doc();
        // Delete from pos 3 (after "He" in p1) to pos 10 (after "Wo" in p2).
        // Slice 3..10 covers "llo" + paragraph boundary + "Wo".
        // Remaining: "He" from p1 + "rld" from p2 → merged into <p>Herld</p>.
        let result = doc.replace(3, 10, Slice::empty()).unwrap();
        assert_eq!(result.child_count(), 1);
        assert_eq!(result.child(0).node_type, NodeType::Paragraph);
        assert_eq!(result.child(0).text_content(), "Herld");
    }

    #[test]
    fn replace_insert_text_within_paragraph() {
        let doc = two_para_doc();
        // Insert "XY" at position 3 (between "He" and "llo" in p1).
        // We create a slice with open_start=1, open_end=1 containing <p>XY</p>
        // so the text joins with the surrounding paragraph.
        let insert_content = Fragment::from_node(Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("XY")),
        ));
        let slice = Slice::new(insert_content, 1, 1);
        let result = doc.replace(3, 3, slice).unwrap();
        assert_eq!(result.child_count(), 2);
        assert_eq!(result.child(0).text_content(), "HeXYllo");
        assert_eq!(result.child(1).text_content(), "World");
    }

    #[test]
    fn replace_within_same_text_node() {
        let doc = two_para_doc();
        // Replace "ell" (pos 2..5) with "a" — "Hello" -> "Hao".
        let insert_content = Fragment::from_node(Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("a")),
        ));
        let slice = Slice::new(insert_content, 1, 1);
        let result = doc.replace(2, 5, slice).unwrap();
        assert_eq!(result.child(0).text_content(), "Hao");
        assert_eq!(result.child(1).text_content(), "World");
    }

    #[test]
    fn replace_with_empty_slice_pure_deletion() {
        let doc = two_para_doc();
        // Delete entire first paragraph (pos 0..7).
        let result = doc.replace(0, 7, Slice::empty()).unwrap();
        assert_eq!(result.child_count(), 1);
        assert_eq!(result.child(0).text_content(), "World");
    }

    #[test]
    fn replace_at_start_of_doc() {
        let doc = two_para_doc();
        // Insert a new paragraph at the start.
        let new_p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("First")),
        );
        let slice = Slice::new(Fragment::from_node(new_p), 0, 0);
        let result = doc.replace(0, 0, slice).unwrap();
        assert_eq!(result.child_count(), 3);
        assert_eq!(result.child(0).text_content(), "First");
        assert_eq!(result.child(1).text_content(), "Hello");
        assert_eq!(result.child(2).text_content(), "World");
    }

    #[test]
    fn replace_at_end_of_doc() {
        let doc = two_para_doc();
        // Insert a new paragraph at the end.
        let new_p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Last")),
        );
        let slice = Slice::new(Fragment::from_node(new_p), 0, 0);
        let result = doc.replace(14, 14, slice).unwrap();
        assert_eq!(result.child_count(), 3);
        assert_eq!(result.child(0).text_content(), "Hello");
        assert_eq!(result.child(1).text_content(), "World");
        assert_eq!(result.child(2).text_content(), "Last");
    }

    #[test]
    fn replace_entire_doc_content() {
        let doc = two_para_doc();
        // Replace all content with a single paragraph.
        let new_p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("New")),
        );
        let slice = Slice::new(Fragment::from_node(new_p), 0, 0);
        let result = doc.replace(0, 14, slice).unwrap();
        assert_eq!(result.child_count(), 1);
        assert_eq!(result.child(0).text_content(), "New");
    }

    #[test]
    fn replace_noop_empty_at_same_position() {
        let doc = two_para_doc();
        // Replace nothing with nothing — should be identity.
        let result = doc.replace(3, 3, Slice::empty()).unwrap();
        assert_eq!(result, doc);
    }

    // ── Cross-paragraph replace tests ────────────────────────────────

    #[test]
    fn replace_delete_all_of_first_paragraph_content() {
        let doc = two_para_doc();
        // Delete all of p1 content (pos 1..6) — leaves empty p1 + p2.
        let result = doc.replace(1, 6, Slice::empty()).unwrap();
        assert_eq!(result.child_count(), 2);
        assert_eq!(result.child(0).text_content(), "");
        assert_eq!(result.child(1).text_content(), "World");
    }

    #[test]
    fn replace_delete_from_start_of_p1_to_start_of_p2() {
        let doc = two_para_doc();
        // Delete from pos 1 (start of p1 content) to pos 8 (start of p2 content).
        // Keeps nothing from p1, nothing from p2 start → merged empty + "World".
        let result = doc.replace(1, 8, Slice::empty()).unwrap();
        assert_eq!(result.child_count(), 1);
        assert_eq!(result.child(0).text_content(), "World");
    }

    #[test]
    fn replace_delete_end_of_p1_to_end_of_p2() {
        let doc = two_para_doc();
        // Delete from pos 6 (end of p1 content) to pos 13 (end of p2 content).
        let result = doc.replace(6, 13, Slice::empty()).unwrap();
        assert_eq!(result.child_count(), 1);
        assert_eq!(result.child(0).text_content(), "Hello");
    }

    #[test]
    fn replace_across_paragraphs_with_open_slice() {
        let doc = two_para_doc();
        // Replace from pos 3 (after "He") to pos 10 (after "Wo") with "XY".
        // Slice: <p>XY</p> with open_start=1, open_end=1 → inline "XY".
        // Result: <p>HeXYrld</p>
        let insert = Fragment::from_node(Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("XY")),
        ));
        let slice = Slice::new(insert, 1, 1);
        let result = doc.replace(3, 10, slice).unwrap();
        assert_eq!(result.child_count(), 1);
        assert_eq!(result.child(0).text_content(), "HeXYrld");
    }

    #[test]
    fn replace_across_paragraphs_with_two_open_paragraphs() {
        let doc = two_para_doc();
        // Replace from pos 3 to pos 10 with two open paragraphs:
        // [<p>AB</p>, <p>CD</p>] open_start=1, open_end=1.
        // "AB" joins with "He" → <p>HeAB</p>
        // "CD" joins with "rld" → <p>CDrld</p>
        let p1 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("AB")),
        );
        let p2 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("CD")),
        );
        let insert = Fragment::from_vec(vec![p1, p2]);
        let slice = Slice::new(insert, 1, 1);
        let result = doc.replace(3, 10, slice).unwrap();
        assert_eq!(result.child_count(), 2);
        assert_eq!(result.child(0).text_content(), "HeAB");
        assert_eq!(result.child(1).text_content(), "CDrld");
    }

    /// Build: <doc><p>One</p><p>Two</p><p>Three</p></doc>
    fn three_para_doc() -> Node {
        let p1 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("One")),
        );
        let p2 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Two")),
        );
        let p3 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Three")),
        );
        Node::branch(NodeType::Doc, Fragment::from_vec(vec![p1, p2, p3]))
    }

    #[test]
    fn replace_delete_across_three_paragraphs() {
        let doc = three_para_doc();
        // Doc: <doc><p>One</p><p>Two</p><p>Three</p></doc>
        // p1: 0..5 (content 1..4), p2: 5..10 (content 6..9), p3: 10..17 (content 11..16)
        // Delete from pos 2 (after "O" in p1) to pos 13 (after "Th" in p3).
        // Remaining: "O" + "ree" → <p>Oree</p>
        let result = doc.replace(2, 13, Slice::empty()).unwrap();
        assert_eq!(result.child_count(), 1);
        assert_eq!(result.child(0).text_content(), "Oree");
    }

    // ── peel_open unit tests ─────────────────────────────────────────

    #[test]
    fn peel_open_no_open_returns_clone() {
        let frag = Fragment::from_node(Node::new_text("Hello"));
        let result = peel_open(&frag, 0, 0);
        assert_eq!(result.size(), 5);
        assert_eq!(result.child(0).text(), Some("Hello"));
    }

    #[test]
    fn peel_open_single_paragraph() {
        // <p>Hello</p> with open_start=1, open_end=1
        // Should peel to just text("Hello")
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hello")),
        );
        let frag = Fragment::from_node(p);
        let result = peel_open(&frag, 1, 1);
        assert_eq!(result.child_count(), 1);
        assert_eq!(result.child(0).text(), Some("Hello"));
        assert_eq!(result.size(), 5);
    }

    #[test]
    fn peel_open_two_paragraphs() {
        // [<p>AB</p>, <p>CD</p>] with open_start=1, open_end=1
        // Should peel first paragraph to "AB" and last to "CD"
        let p1 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("AB")),
        );
        let p2 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("CD")),
        );
        let frag = Fragment::from_vec(vec![p1, p2]);
        let result = peel_open(&frag, 1, 1);
        // After peeling: text("AB"), text("CD") → merged to "ABCD"
        assert_eq!(result.child_count(), 1);
        assert_eq!(result.child(0).text(), Some("ABCD"));
    }

    #[test]
    fn peel_open_empty_content() {
        let frag = Fragment::empty();
        let result = peel_open(&frag, 1, 1);
        assert_eq!(result.child_count(), 0);
        assert_eq!(result.size(), 0);
    }

    #[test]
    fn peel_open_leaf_not_peeled() {
        // A leaf node can't be peeled — returns content as-is.
        let frag = Fragment::from_node(Node::leaf(NodeType::HardBreak));
        let result = peel_open(&frag, 1, 0);
        assert_eq!(result.child_count(), 1);
        assert_eq!(result.child(0).node_type, NodeType::HardBreak);
    }

    #[test]
    fn peel_open_only_start() {
        // [<p>Hello</p>] with open_start=1, open_end=0
        // Should peel first child to text("Hello"), keep open_end as-is.
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hello")),
        );
        let frag = Fragment::from_node(p);
        let result = peel_open(&frag, 1, 0);
        // Single child, peel_open recurses with (child.content, 0, 0) → returns "Hello"
        assert_eq!(result.child_count(), 1);
        assert_eq!(result.child(0).text(), Some("Hello"));
    }

    // ── Tests exercising close_node_right (open_end >= 2) ───────────

    /// Build: <doc><blockquote><p>Hello</p><p>World</p></blockquote></doc>
    ///
    /// Positions within doc content:
    ///   0: before <blockquote>
    ///   1: before <p> (inside blockquote)
    ///   2: start of p1 content (before 'H')
    ///   7: end of p1 content (after 'o')
    ///   8: before <p> (second paragraph inside blockquote)
    ///   9: start of p2 content (before 'W')
    ///  14: end of p2 content (after 'd')
    ///  15: after </p> (end of blockquote content)
    ///  16: after </blockquote>
    fn blockquote_doc() -> Node {
        let p1 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hello")),
        );
        let p2 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("World")),
        );
        let bq = Node::branch(
            NodeType::Blockquote,
            Fragment::from_vec(vec![p1, p2]),
        );
        Node::branch(NodeType::Doc, Fragment::from_node(bq))
    }

    #[test]
    fn replace_across_blockquote_paragraphs_exercises_close_right() {
        let doc = blockquote_doc();
        // Delete from pos 4 (after "He" in p1) to pos 11 (after "Wo" in p2).
        // Both positions are at depth 2 (inside paragraphs inside blockquote).
        // shared depth is 1 (blockquote level).
        // Result: <doc><blockquote><p>Herld</p></blockquote></doc>
        let result = doc.replace(4, 11, Slice::empty()).unwrap();
        assert_eq!(result.child_count(), 1); // one blockquote
        let bq = result.child(0);
        assert_eq!(bq.node_type, NodeType::Blockquote);
        assert_eq!(bq.child_count(), 1); // merged into one paragraph
        assert_eq!(bq.child(0).text_content(), "Herld");
    }

    #[test]
    fn replace_with_open_end_2_slice_exercises_close_right() {
        // Build a slice with open_end=2 by slicing from a nested document.
        // Source: <doc><blockquote><p>Hello</p></blockquote><blockquote><p>World</p></blockquote></doc>
        let source = Node::branch(
            NodeType::Doc,
            Fragment::from_vec(vec![
                Node::branch(
                    NodeType::Blockquote,
                    Fragment::from_node(Node::branch(
                        NodeType::Paragraph,
                        Fragment::from_node(Node::new_text("Hello")),
                    )),
                ),
                Node::branch(
                    NodeType::Blockquote,
                    Fragment::from_node(Node::branch(
                        NodeType::Paragraph,
                        Fragment::from_node(Node::new_text("World")),
                    )),
                ),
            ]),
        );
        // Slice from pos 4 (after "He") to pos 13 (after "Wo").
        // shared_depth = 0, from.depth = 2, to.depth = 2
        // open_start = 2, open_end = 2
        let s = source.slice(4, 13);
        assert_eq!(s.open_start, 2);
        assert_eq!(s.open_end, 2);
        assert_eq!(s.content.child_count(), 2);

        // Target: <doc><blockquote><p>AB</p></blockquote><blockquote><p>CD</p></blockquote></doc>
        let target = Node::branch(
            NodeType::Doc,
            Fragment::from_vec(vec![
                Node::branch(
                    NodeType::Blockquote,
                    Fragment::from_node(Node::branch(
                        NodeType::Paragraph,
                        Fragment::from_node(Node::new_text("AB")),
                    )),
                ),
                Node::branch(
                    NodeType::Blockquote,
                    Fragment::from_node(Node::branch(
                        NodeType::Paragraph,
                        Fragment::from_node(Node::new_text("CD")),
                    )),
                ),
            ]),
        );
        // target positions: bq1 0..5 (p1 content at 2..4), bq2 5..10 (p2 content at 7..9)
        // Wait — bq size = 1 + (1 + 2 + 1) + 1 = 6. So bq1: 0..6, bq2: 6..12.
        // p1 content: positions 2..4 ("AB"), p2 content: positions 8..10 ("CD")
        //
        // Replace from pos 3 (after "A") to pos 9 (after "C") with the open_end=2 slice.
        // from resolves: depth 2 (doc > bq1 > p1), to resolves: depth 2 (doc > bq2 > p2)
        // shared_depth = 0 (different blockquotes at doc level)
        //
        // Case 3: left_depth = min(0+1+2, 2) = 2, right_depth = min(0+1+2, 2) = 2
        // has_separate_sides = true (2 children, open_start > 0, open_end > 0)
        //
        // Left side: "A" + peel_slice_start("llo") → "Allo"
        //   close_node_left wraps: <p>Allo</p> then <bq><p>Allo</p></bq>
        // Right side: peel_slice_end("Wo") + "D" → "WoD"
        //   close_node_right wraps: <p>WoD</p> then <bq><p>WoD</p></bq>
        //
        // Result: <doc><bq><p>Allo</p></bq><bq><p>WoD</p></bq></doc>
        let result = target.replace(3, 9, s).unwrap();
        assert_eq!(result.child_count(), 2);
        let bq1 = result.child(0);
        assert_eq!(bq1.node_type, NodeType::Blockquote);
        assert_eq!(bq1.child_count(), 1);
        assert_eq!(bq1.child(0).text_content(), "Allo");
        let bq2 = result.child(1);
        assert_eq!(bq2.node_type, NodeType::Blockquote);
        assert_eq!(bq2.child_count(), 1);
        assert_eq!(bq2.child(0).text_content(), "WoD");
    }
}
