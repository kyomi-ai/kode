//! Atomic edit operations on the document tree.
//!
//! [`Step`] is the building block of transforms. Each step can be applied to a
//! document to produce a new document, inverted to produce an undo step, and
//! mapped through a [`StepMap`] to adjust positions after other edits.

use crate::fragment::Fragment;
use crate::mark::Mark;
use crate::node::Node;
use crate::slice::Slice;

/// The result of applying a step to a document.
#[derive(Clone, Debug)]
pub struct StepResult {
    /// The new document after the step was applied.
    pub doc: Node,
    /// Position mapping produced by this step.
    pub map: StepMap,
}

/// An atomic edit operation on the document tree.
///
/// Steps are the building blocks of transforms. Each step:
/// - Can be applied to a document to produce a new document
/// - Can be inverted to produce an undo step
/// - Produces a [`StepMap`] for position mapping
#[derive(Clone, Debug)]
pub enum Step {
    /// Replace content between two positions with a slice.
    Replace {
        from: usize,
        to: usize,
        slice: Slice,
    },
    /// Add a mark to a range of text.
    AddMark {
        from: usize,
        to: usize,
        mark: Mark,
    },
    /// Remove a mark from a range of text.
    RemoveMark {
        from: usize,
        to: usize,
        mark: Mark,
    },
}

impl Step {
    /// Apply this step to a document, producing a new document and a step map.
    pub fn apply(&self, doc: &Node) -> Result<StepResult, String> {
        match self {
            Step::Replace { from, to, slice } => apply_replace(doc, *from, *to, slice),
            Step::AddMark { from, to, mark } => apply_add_mark(doc, *from, *to, mark),
            Step::RemoveMark { from, to, mark } => apply_remove_mark(doc, *from, *to, mark),
        }
    }

    /// Create the inverse of this step (for undo).
    ///
    /// The inverse step, when applied to the result of this step, restores
    /// the original document.
    pub fn invert(&self, doc: &Node) -> Step {
        match self {
            Step::Replace { from, to, slice } => {
                // The inverse replaces the inserted content with the original content.
                let original_slice = doc.slice(*from, *to);
                let new_to = *from + slice.size();
                Step::Replace {
                    from: *from,
                    to: new_to,
                    slice: original_slice,
                }
            }
            Step::AddMark { from, to, mark } => {
                // Inverse of adding a mark is removing it.
                Step::RemoveMark {
                    from: *from,
                    to: *to,
                    mark: mark.clone(),
                }
            }
            Step::RemoveMark { from, to, mark } => {
                // Inverse of removing a mark is adding it.
                Step::AddMark {
                    from: *from,
                    to: *to,
                    mark: mark.clone(),
                }
            }
        }
    }

    /// Adjust this step's positions through a mapping.
    ///
    /// Returns `None` if the step's range was entirely deleted by the mapping.
    pub fn map(&self, mapping: &StepMap) -> Option<Step> {
        match self {
            Step::Replace { from, to, slice } => {
                let new_from = mapping.map(*from, 1);
                let new_to = mapping.map(*to, -1);
                if new_from > new_to {
                    return None;
                }
                Some(Step::Replace {
                    from: new_from,
                    to: new_to,
                    slice: slice.clone(),
                })
            }
            Step::AddMark { from, to, mark } => {
                let new_from = mapping.map(*from, 1);
                let new_to = mapping.map(*to, -1);
                if new_from > new_to {
                    return None;
                }
                Some(Step::AddMark {
                    from: new_from,
                    to: new_to,
                    mark: mark.clone(),
                })
            }
            Step::RemoveMark { from, to, mark } => {
                let new_from = mapping.map(*from, 1);
                let new_to = mapping.map(*to, -1);
                if new_from > new_to {
                    return None;
                }
                Some(Step::RemoveMark {
                    from: new_from,
                    to: new_to,
                    mark: mark.clone(),
                })
            }
        }
    }
}

/// Apply a Replace step.
fn apply_replace(doc: &Node, from: usize, to: usize, slice: &Slice) -> Result<StepResult, String> {
    let new_doc = doc.replace(from, to, slice.clone())?;
    let old_size = to - from;
    let new_size = slice.size();
    let map = StepMap {
        ranges: vec![(from, old_size, new_size)],
    };
    Ok(StepResult { doc: new_doc, map })
}

/// Apply an AddMark step by walking text nodes in [from, to) and adding the mark.
fn apply_add_mark(doc: &Node, from: usize, to: usize, mark: &Mark) -> Result<StepResult, String> {
    // Collect text node ranges that need the mark added.
    let mut replacements: Vec<(usize, usize, Vec<Mark>)> = Vec::new();

    doc.nodes_between(from, to, &mut |node, pos, _parent, _idx| {
        if !node.is_text() {
            return true; // descend into branches
        }

        // Calculate the overlap between [from, to) and this text node.
        let node_end = pos + node.node_size();
        let start = from.max(pos);
        let end = to.min(node_end);

        if start >= end {
            return false;
        }

        // Check if the mark already exists on this text node.
        let has_mark = node.marks.iter().any(|m| m.mark_type == mark.mark_type);
        if !has_mark {
            let new_marks = mark.add_to_set(&node.marks);
            replacements.push((start, end, new_marks));
        }

        false
    });

    if replacements.is_empty() {
        return Ok(StepResult {
            doc: doc.clone(),
            map: StepMap::empty(),
        });
    }

    // Apply replacements from right to left so positions stay valid.
    let mut current = doc.clone();
    for (start, end, new_marks) in replacements.into_iter().rev() {
        let original_slice = current.slice(start, end);
        // Rebuild the slice content with new marks.
        let new_content = remark_fragment(&original_slice.content, &new_marks);
        let new_slice = Slice::new(new_content, original_slice.open_start, original_slice.open_end);
        current = current.replace(start, end, new_slice)?;
    }

    // Mark steps don't change positions — the text is the same length.
    Ok(StepResult {
        doc: current,
        map: StepMap::empty(),
    })
}

/// Apply a RemoveMark step by walking text nodes in [from, to) and removing the mark.
fn apply_remove_mark(
    doc: &Node,
    from: usize,
    to: usize,
    mark: &Mark,
) -> Result<StepResult, String> {
    let mut replacements: Vec<(usize, usize, Vec<Mark>)> = Vec::new();

    doc.nodes_between(from, to, &mut |node, pos, _parent, _idx| {
        if !node.is_text() {
            return true;
        }

        let node_end = pos + node.node_size();
        let start = from.max(pos);
        let end = to.min(node_end);

        if start >= end {
            return false;
        }

        let has_mark = node.marks.iter().any(|m| m.mark_type == mark.mark_type);
        if has_mark {
            let new_marks = mark.remove_from_set(&node.marks);
            replacements.push((start, end, new_marks));
        }

        false
    });

    if replacements.is_empty() {
        return Ok(StepResult {
            doc: doc.clone(),
            map: StepMap::empty(),
        });
    }

    let mut current = doc.clone();
    for (start, end, new_marks) in replacements.into_iter().rev() {
        let original_slice = current.slice(start, end);
        let new_content = remark_fragment(&original_slice.content, &new_marks);
        let new_slice = Slice::new(new_content, original_slice.open_start, original_slice.open_end);
        current = current.replace(start, end, new_slice)?;
    }

    Ok(StepResult {
        doc: current,
        map: StepMap::empty(),
    })
}

/// Replace the marks on all text nodes in a fragment.
///
/// **Note:** This overwrites ALL marks on every text node in the fragment with
/// the provided `marks` slice. It does not merge with existing marks. Callers
/// are responsible for computing the desired final mark set before calling.
fn remark_fragment(fragment: &Fragment, marks: &[Mark]) -> Fragment {
    let mut nodes = Vec::new();
    for child in fragment.iter() {
        if child.is_text() {
            let text = child.text().unwrap();
            nodes.push(Node::new_text_with_marks(text, marks.to_vec()));
        } else {
            let new_content = remark_fragment(&child.content, marks);
            nodes.push(Node::branch_with_attrs(
                child.node_type,
                child.attrs.clone(),
                new_content,
            ));
        }
    }
    Fragment::from_vec(nodes)
}

// ── StepMap ─────────────────────────────────────────────────────────────

/// Position mapping after a step.
///
/// Records ranges that changed: `(position, old_size, new_size)`.
/// Used to adjust cursor positions and other positions after edits.
#[derive(Clone, Debug)]
pub struct StepMap {
    /// Changed ranges: `(start_pos, old_size, new_size)`.
    pub ranges: Vec<(usize, usize, usize)>,
}

impl StepMap {
    /// An empty step map (no position changes).
    pub fn empty() -> Self {
        StepMap {
            ranges: Vec::new(),
        }
    }

    /// Map a position through this step map.
    ///
    /// `assoc` controls behavior at boundaries:
    /// - `assoc < 0`: stick left (position stays before inserted content)
    /// - `assoc > 0`: stick right (position moves after inserted content)
    pub fn map(&self, mut pos: usize, assoc: i8) -> usize {
        for &(start, old_size, new_size) in &self.ranges {
            let end = start + old_size;

            if pos < start {
                // Position is before this range — unaffected.
                continue;
            }

            if pos > end {
                // Position is after this range — shift by the size difference.
                if new_size >= old_size {
                    pos += new_size - old_size;
                } else {
                    pos -= old_size - new_size;
                }
                continue;
            }

            // Position is within the replaced range.
            if assoc < 0 {
                // Stick left: collapse to the start of the replacement.
                pos = start;
            } else {
                // Stick right: move to the end of the replacement.
                pos = start + new_size;
            }
        }

        pos
    }

    /// Create the inverse map (swap old and new sizes).
    pub fn invert(&self) -> StepMap {
        StepMap {
            ranges: self
                .ranges
                .iter()
                .map(|&(pos, old_size, new_size)| (pos, new_size, old_size))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attrs::heading_attrs;
    use crate::fragment::Fragment;
    use crate::mark::MarkType;
    use crate::node_type::NodeType;

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

    // ── Replace step: apply ─────────────────────────────────────────────

    #[test]
    fn replace_step_delete_text() {
        // <doc><p>Hello</p></doc> — delete "ell" (pos 2..5)
        let doc = simple_doc();
        let step = Step::Replace {
            from: 2,
            to: 5,
            slice: Slice::empty(),
        };
        let result = step.apply(&doc).unwrap();
        assert_eq!(result.doc.child(0).text_content(), "Ho");
        assert_eq!(result.map.ranges, vec![(2, 3, 0)]);
    }

    #[test]
    fn replace_step_insert_text() {
        // Insert "XY" at position 3 (inside "Hello" after "He")
        let doc = simple_doc();
        let slice = Slice::new(Fragment::from_node(Node::new_text("XY")), 0, 0);
        let step = Step::Replace {
            from: 3,
            to: 3,
            slice,
        };
        let result = step.apply(&doc).unwrap();
        assert_eq!(result.doc.child(0).text_content(), "HeXYllo");
        assert_eq!(result.map.ranges, vec![(3, 0, 2)]);
    }

    // ── Replace step: invert ────────────────────────────────────────────

    #[test]
    fn replace_step_invert_restores_doc() {
        let doc = simple_doc();
        let step = Step::Replace {
            from: 2,
            to: 5,
            slice: Slice::empty(),
        };
        let result = step.apply(&doc).unwrap();
        let inv = step.invert(&doc);
        let restored = inv.apply(&result.doc).unwrap();
        assert_eq!(restored.doc, doc);
    }

    #[test]
    fn replace_step_invert_insert() {
        let doc = simple_doc();
        let slice = Slice::new(Fragment::from_node(Node::new_text("XY")), 0, 0);
        let step = Step::Replace {
            from: 3,
            to: 3,
            slice,
        };
        let result = step.apply(&doc).unwrap();
        let inv = step.invert(&doc);
        let restored = inv.apply(&result.doc).unwrap();
        assert_eq!(restored.doc, doc);
    }

    // ── AddMark step ────────────────────────────────────────────────────

    #[test]
    fn add_mark_to_text_range() {
        // <doc><p>Hello</p></doc>
        // Bold "ell" (pos 2..5)
        let doc = simple_doc();
        let step = Step::AddMark {
            from: 2,
            to: 5,
            mark: Mark::new(MarkType::Strong),
        };
        let result = step.apply(&doc).unwrap();

        // p should now have 3 children: "H", bold "ell", "o"
        let p = &result.doc.child(0);
        assert_eq!(p.child_count(), 3);
        assert_eq!(p.child(0).text(), Some("H"));
        assert!(p.child(0).marks.is_empty());
        assert_eq!(p.child(1).text(), Some("ell"));
        assert_eq!(p.child(1).marks.len(), 1);
        assert_eq!(p.child(1).marks[0].mark_type, MarkType::Strong);
        assert_eq!(p.child(2).text(), Some("o"));
        assert!(p.child(2).marks.is_empty());
    }

    #[test]
    fn add_mark_already_present_is_noop() {
        let bold = Node::new_text_with_marks("Hello", vec![Mark::new(MarkType::Strong)]);
        let p = Node::branch(NodeType::Paragraph, Fragment::from_node(bold));
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(p));

        let step = Step::AddMark {
            from: 1,
            to: 6,
            mark: Mark::new(MarkType::Strong),
        };
        let result = step.apply(&doc).unwrap();
        assert_eq!(result.doc, doc);
    }

    // ── RemoveMark step ─────────────────────────────────────────────────

    #[test]
    fn remove_mark_from_text_range() {
        // <doc><p><strong>Hello</strong></p></doc>
        let bold = Node::new_text_with_marks("Hello", vec![Mark::new(MarkType::Strong)]);
        let p = Node::branch(NodeType::Paragraph, Fragment::from_node(bold));
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(p));

        let step = Step::RemoveMark {
            from: 1,
            to: 6,
            mark: Mark::new(MarkType::Strong),
        };
        let result = step.apply(&doc).unwrap();

        let p = &result.doc.child(0);
        assert_eq!(p.child_count(), 1);
        assert_eq!(p.child(0).text(), Some("Hello"));
        assert!(p.child(0).marks.is_empty());
    }

    #[test]
    fn remove_mark_not_present_is_noop() {
        let doc = simple_doc();
        let step = Step::RemoveMark {
            from: 1,
            to: 6,
            mark: Mark::new(MarkType::Strong),
        };
        let result = step.apply(&doc).unwrap();
        assert_eq!(result.doc, doc);
    }

    // ── StepMap ─────────────────────────────────────────────────────────

    #[test]
    fn step_map_position_before_range() {
        // Replace at position 5..8 with 2 chars.
        let map = StepMap {
            ranges: vec![(5, 3, 2)],
        };
        // Position 3 is before the range — unaffected.
        assert_eq!(map.map(3, 1), 3);
    }

    #[test]
    fn step_map_position_after_range() {
        let map = StepMap {
            ranges: vec![(5, 3, 2)],
        };
        // Position 10 is after range (5..8), shifted by -1 (2-3).
        assert_eq!(map.map(10, 1), 9);
    }

    #[test]
    fn step_map_position_inside_range_stick_right() {
        let map = StepMap {
            ranges: vec![(5, 3, 2)],
        };
        // Position 6 is inside the replaced range. Stick right → 5 + 2 = 7.
        assert_eq!(map.map(6, 1), 7);
    }

    #[test]
    fn step_map_position_inside_range_stick_left() {
        let map = StepMap {
            ranges: vec![(5, 3, 2)],
        };
        // Position 6 is inside the replaced range. Stick left → 5.
        assert_eq!(map.map(6, -1), 5);
    }

    #[test]
    fn step_map_insertion_at_same_pos() {
        // Insert 3 chars at position 5 (old_size=0, new_size=3).
        let map = StepMap {
            ranges: vec![(5, 0, 3)],
        };
        // Position 5 at the insertion point: stick right → 5+3=8
        assert_eq!(map.map(5, 1), 8);
        // Stick left → stays at 5
        assert_eq!(map.map(5, -1), 5);
    }

    #[test]
    fn step_map_invert() {
        let map = StepMap {
            ranges: vec![(5, 3, 2)],
        };
        let inv = map.invert();
        assert_eq!(inv.ranges, vec![(5, 2, 3)]);
    }

    #[test]
    fn step_map_empty() {
        let map = StepMap::empty();
        assert_eq!(map.map(42, 1), 42);
    }

    // ── Step::map (position mapping) ────────────────────────────────────

    #[test]
    fn step_map_through_mapping_adjusts_positions() {
        let step = Step::Replace {
            from: 10,
            to: 15,
            slice: Slice::empty(),
        };
        // A prior edit inserted 3 chars at position 5.
        let mapping = StepMap {
            ranges: vec![(5, 0, 3)],
        };
        let mapped = step.map(&mapping).unwrap();
        match mapped {
            Step::Replace { from, to, .. } => {
                assert_eq!(from, 13); // 10 + 3
                assert_eq!(to, 18); // 15 + 3
            }
            _ => panic!("expected Replace"),
        }
    }

    #[test]
    fn step_map_keeps_collapsed_range_as_some() {
        let step = Step::Replace {
            from: 5,
            to: 8,
            slice: Slice::empty(),
        };
        // A prior edit deleted the range 3..10.
        let mapping = StepMap {
            ranges: vec![(3, 7, 0)],
        };
        let mapped = step.map(&mapping);
        // from=5 maps to 3 (stick right → 3+0=3), to=8 maps to 3 (stick left → 3).
        // from=3, to=3 — not collapsed (equal is ok).
        // Actually: from with assoc=1 → 3, to with assoc=-1 → 3. from <= to, so it's valid.
        // Let me re-examine: from=5 inside [3..10], assoc=1 → 3+0=3.
        // to=8 inside [3..10], assoc=-1 → 3.
        // from(3) <= to(3), so it returns Some.
        assert!(mapped.is_some());
    }

    // ── Cross-paragraph replace invert ──────────────────────────────────

    #[test]
    fn replace_across_paragraphs_invert() {
        // <doc><p>Hello</p><p>World</p></doc>
        // Delete from pos 3 (inside "Hello") to pos 10 (inside "World")
        // This merges the two paragraphs.
        let doc = two_para_doc();
        let step = Step::Replace {
            from: 3,
            to: 10,
            slice: Slice::empty(),
        };
        let result = step.apply(&doc).unwrap();
        // Should have one paragraph: "Herld"
        assert_eq!(result.doc.child_count(), 1);
        assert_eq!(result.doc.child(0).text_content(), "Herld");

        // Invert should restore original.
        let inv = step.invert(&doc);
        let restored = inv.apply(&result.doc).unwrap();
        assert_eq!(restored.doc, doc);
    }

    // ── Heading replace step ────────────────────────────────────────────

    #[test]
    fn replace_preserves_heading_attrs() {
        // <doc><h1>Title</h1></doc>
        let h = Node::branch_with_attrs(
            NodeType::Heading,
            heading_attrs(1),
            Fragment::from_node(Node::new_text("Title")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(h));

        // Delete "itl" (pos 2..5)
        let step = Step::Replace {
            from: 2,
            to: 5,
            slice: Slice::empty(),
        };
        let result = step.apply(&doc).unwrap();
        assert_eq!(result.doc.child(0).node_type, NodeType::Heading);
        assert_eq!(result.doc.child(0).attrs, heading_attrs(1));
        assert_eq!(result.doc.child(0).text_content(), "Te");
    }
}
