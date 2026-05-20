//! Selection management: position adjustment, word/line selection, and cursor queries.

use crate::attrs::{get_attr, AttrValue};
use crate::mark::MarkType;
use crate::position::ResolvedPos;

use super::{DocState, FormattingState, GapSide, Selection};

impl DocState {
    // ── Position adjustment ─────────────────────────────────────────────

    /// If `pos` is between blocks (parent is not a textblock), nudge it into
    /// the nearest textblock.  When the position is right after a textblock's
    /// closing token, move it to the end of that textblock's content.  When
    /// it's right before a textblock's opening token, move it to the start
    /// of that textblock's content.  Returns the adjusted position.
    ///
    /// Atomic blocks are never entered — if a neighboring node is atomic,
    /// the position is a valid **gap cursor** position and is returned as-is.
    /// If one neighbor is atomic and the other is a non-atomic textblock,
    /// the position is adjusted into the non-atomic textblock.
    pub fn adjust_into_textblock(&self, pos: usize) -> usize {
        let resolved = self.doc.resolve(pos);
        if resolved.parent().node_type.is_textblock() {
            return pos; // Already inside a textblock.
        }

        let before = resolved.node_before();
        let after = resolved.node_after();

        let before_is_textblock = before.is_some_and(|n| n.node_type.is_textblock() && !n.is_atom());
        let after_is_textblock = after.is_some_and(|n| n.node_type.is_textblock() && !n.is_atom());

        // Try the node before — if it's a non-atomic textblock, move
        // to the end of its content.
        if before_is_textblock {
            // pos is right after the closing token of the previous textblock.
            // The end of its content = pos - 1.
            return pos - 1;
        }
        // Try the node after — if it's a non-atomic textblock, move to the
        // start of its content.
        if after_is_textblock {
            // pos is right before the opening token of the next textblock.
            // The start of its content = pos + 1.
            return pos + 1;
        }
        // Neither neighbor is a non-atomic textblock. This position is either
        // a gap cursor next to an atomic block, between two non-textblocks,
        // or at a doc boundary — return as-is.
        pos
    }

    // ── Selection expansion for atomic blocks ────────────────────────

    /// Expand a selection so that partially-selected atomic blocks are
    /// fully included.
    ///
    /// If `from` or `to` falls inside an atomic block's position range
    /// `[block_start, block_end]`, the boundary is expanded to include the
    /// entire block.  Selections that do not intersect any atomic block
    /// are returned unchanged.
    ///
    /// This should be called when setting range selections (e.g. after
    /// click-drag or shift+arrow) so that atomic blocks are always
    /// selected as indivisible units.
    pub fn expand_selection_around_atoms(&self, sel: &Selection) -> Selection {
        let from = sel.from();
        let to = sel.to();

        let expanded_from = self.expand_pos_out_of_atom(from, true);
        let expanded_to = self.expand_pos_out_of_atom(to, false);

        if expanded_from == from && expanded_to == to {
            return sel.clone();
        }

        // Preserve anchor/head directionality.
        if sel.anchor <= sel.head {
            Selection::range(expanded_from, expanded_to)
        } else {
            Selection::range(expanded_to, expanded_from)
        }
    }

    /// If `pos` is inside an atomic node, return the position just before
    /// (if `toward_start` is true) or just after (if false) that node.
    /// Otherwise return `pos` unchanged.
    fn expand_pos_out_of_atom(&self, pos: usize, toward_start: bool) -> usize {
        let resolved = self.doc.resolve(pos);
        // Walk ancestors from innermost to outermost, looking for an atomic
        // node. The first atomic ancestor we find is the one to expand to.
        for d in (1..=resolved.depth).rev() {
            let ancestor = resolved.node(d);
            if ancestor.is_atom() {
                if toward_start {
                    return resolved.before(d);
                }
                return resolved.after(d);
            }
        }
        pos
    }

    // ── Query ──────────────────────────────────────────────────────────

    /// Get the formatting state at the current cursor position.
    /// Used by the toolbar to show active buttons.
    ///
    /// Uses `selection.head` (not `selection.from()`) so that the toolbar
    /// reflects formatting at the cursor's active end, which matches user
    /// expectation when extending a selection in either direction.
    pub fn formatting_at_cursor(&self) -> FormattingState {
        let resolved = self.doc.resolve(self.selection.head);
        let marks = {
            let m = resolved.marks();
            if m.is_empty() {
                // At a boundary, node_after may be non-marked while node_before
                // is marked. Check node_before so that the toolbar still shows
                // the mark as active (e.g. cursor right after a bold range).
                if let Some(nb) = resolved.node_before() {
                    if nb.is_text() {
                        nb.marks.clone()
                    } else {
                        m
                    }
                } else {
                    m
                }
            } else {
                m
            }
        };

        let mut state = FormattingState::default();

        // Check inline marks.
        for mark in &marks {
            match mark.mark_type {
                MarkType::Strong => state.bold = true,
                MarkType::Em => state.italic = true,
                MarkType::Code => state.code = true,
                MarkType::Strike => state.strikethrough = true,
                MarkType::Link => {} // Links don't show as a toggle state.
            }
        }

        // Check block type by walking ancestor nodes.
        for d in 0..=resolved.depth {
            let node = resolved.node(d);
            match node.node_type {
                crate::node_type::NodeType::Heading => {
                    if let Some(AttrValue::Int(level)) = get_attr(&node.attrs, "level") {
                        state.heading_level = *level as u8;
                    }
                }
                crate::node_type::NodeType::BulletList => state.bullet_list = true,
                crate::node_type::NodeType::OrderedList => state.ordered_list = true,
                crate::node_type::NodeType::TaskList => state.task_list = true,
                crate::node_type::NodeType::Blockquote => state.blockquote = true,
                crate::node_type::NodeType::Table => state.in_table = true,
                _ => {}
            }
        }

        state
    }

    /// Get the resolved position for the cursor head.
    pub fn resolve_cursor(&self) -> ResolvedPos {
        self.doc.resolve(self.selection.head)
    }

    // ── Word/line selection ──────────────────────────────────────────

    /// Select the word at the cursor position (double-click).
    ///
    /// A "word" is a contiguous run of non-whitespace characters within the
    /// parent textblock's visible text. If the cursor is not inside a
    /// textblock, this is a no-op.
    pub fn select_word(&mut self) {
        let resolved = self.doc.resolve(self.selection.head);
        let parent = resolved.parent();
        if !parent.node_type.is_textblock() {
            return;
        }

        let parent_start = resolved.start(resolved.depth);
        let text = parent.text_content();
        let text_chars: Vec<char> = text.chars().collect();
        // Convert parent_offset (token offset) to a text-character index,
        // accounting for non-text inline leaves (each = 1 token, 0 chars).
        let mut char_offset = 0usize;
        let mut tokens_seen = 0usize;
        for child in parent.content.iter() {
            if tokens_seen >= resolved.parent_offset {
                break;
            }
            if child.node_type.is_text() {
                let n = child.text().unwrap_or("").chars().count()
                    .min(resolved.parent_offset - tokens_seen);
                char_offset += n;
                tokens_seen += n;
            } else {
                tokens_seen += 1;
            }
        }
        let offset = char_offset.min(text_chars.len());

        // If the cursor is on a whitespace character, do nothing — there is no
        // word to select.
        if offset < text_chars.len() && text_chars[offset].is_whitespace() {
            return;
        }

        // Find word boundaries by scanning for whitespace.
        let start = text_chars[..offset]
            .iter()
            .rposition(|c| c.is_whitespace())
            .map(|i| i + 1)
            .unwrap_or(0);
        let end = text_chars[offset..]
            .iter()
            .position(|c| c.is_whitespace())
            .map(|i| offset + i)
            .unwrap_or(text_chars.len());

        // Only update if we found a non-empty word.
        if start < end {
            self.selection = Selection::range(parent_start + start, parent_start + end);
        }
    }

    /// Select the entire line (textblock content) at the cursor position
    /// (triple-click).
    ///
    /// Selects from the start to the end of the parent textblock's content.
    /// If the cursor is not inside a textblock, this is a no-op.
    pub fn select_line(&mut self) {
        let resolved = self.doc.resolve(self.selection.head);
        let parent = resolved.parent();
        if !parent.node_type.is_textblock() {
            return;
        }

        let start = resolved.start(resolved.depth);
        let end = resolved.end(resolved.depth);
        self.selection = Selection::range(start, end);
    }

    // ── Gap cursor ────────────────────────────────────────────────────

    /// Check if `pos` is a valid gap cursor position: between blocks where
    /// at least one neighbor is an atomic block.
    pub fn is_gap_cursor(&self, pos: usize) -> bool {
        let resolved = self.doc.resolve(pos);
        if resolved.parent().node_type.is_textblock() {
            return false;
        }
        let before = resolved.node_before();
        let after = resolved.node_after();
        before.is_some_and(|n| n.is_atom()) || after.is_some_and(|n| n.is_atom())
    }

    /// If the current cursor is at a gap position next to an atomic block,
    /// return the side and the block's `(pos_start, pos_end)` range.
    pub fn gap_cursor_info(&self) -> Option<(GapSide, usize, usize)> {
        if !self.selection.is_cursor() {
            return None;
        }
        let pos = self.selection.head;
        let resolved = self.doc.resolve(pos);
        if resolved.parent().node_type.is_textblock() {
            return None;
        }

        if let Some(after) = resolved.node_after() {
            if after.is_atom() {
                return Some((GapSide::Before, pos, pos + after.node_size()));
            }
        }
        if let Some(before) = resolved.node_before() {
            if before.is_atom() {
                return Some((GapSide::After, pos - before.node_size(), pos));
            }
        }
        None
    }

    /// If `pos` is inside an atomic block, snap it to the nearest boundary
    /// (before or after the block). Returns `pos` unchanged if not inside
    /// an atom.
    pub fn snap_out_of_atom(&self, pos: usize) -> usize {
        let resolved = self.doc.resolve(pos);
        for d in (1..=resolved.depth).rev() {
            let ancestor = resolved.node(d);
            if ancestor.is_atom() {
                let before = resolved.before(d);
                let after = resolved.after(d);
                if pos - before <= after - pos {
                    return before;
                }
                return after;
            }
        }
        pos
    }

    // ── Cursor movement ──────────────────────────────────────────────

    /// Move the cursor one position to the right, treating atomic blocks
    /// as single characters.
    pub fn move_right(&mut self) {
        let pos = self.selection.head;
        let doc_size = self.doc.content.size();
        if pos >= doc_size {
            return;
        }

        let resolved = self.doc.resolve(pos);

        if resolved.parent().node_type.is_textblock() {
            if resolved.parent_offset < resolved.parent().content.size() {
                self.set_selection_raw(Selection::cursor(pos + 1));
            } else {
                let gap_pos = resolved.after(resolved.depth);
                self.settle_at_gap(gap_pos, true);
            }
        } else if let Some(after) = resolved.node_after() {
            if after.is_atom() {
                let past_atom = pos + after.node_size();
                self.settle_at_gap(past_atom, true);
            } else {
                self.set_selection_raw(Selection::cursor(pos + 1));
            }
        }
    }

    /// Move the cursor one position to the left, treating atomic blocks
    /// as single characters.
    pub fn move_left(&mut self) {
        let pos = self.selection.head;
        if pos == 0 {
            return;
        }

        let resolved = self.doc.resolve(pos);

        if resolved.parent().node_type.is_textblock() {
            if resolved.parent_offset > 0 {
                self.set_selection_raw(Selection::cursor(pos - 1));
            } else {
                let gap_pos = resolved.before(resolved.depth);
                self.settle_at_gap(gap_pos, false);
            }
        } else if let Some(before) = resolved.node_before() {
            if before.is_atom() {
                let before_atom = pos - before.node_size();
                self.settle_at_gap(before_atom, false);
            } else {
                self.set_selection_raw(Selection::cursor(pos - 1));
            }
        }
    }

    /// Given a gap position, decide whether it's a valid gap cursor
    /// (adjacent to an atom) or should be entered into an adjacent textblock.
    fn settle_at_gap(&mut self, gap_pos: usize, forward: bool) {
        let doc_size = self.doc.content.size();
        let gap_pos = gap_pos.min(doc_size);

        let resolved = self.doc.resolve(gap_pos);
        let before = resolved.node_before();
        let after = resolved.node_after();

        let has_atom = before.is_some_and(|n| n.is_atom())
            || after.is_some_and(|n| n.is_atom());

        if has_atom {
            self.set_selection_raw(Selection::cursor(gap_pos));
        } else if forward {
            if after.is_some_and(|n| n.node_type.is_textblock()) {
                self.set_selection_raw(Selection::cursor(gap_pos + 1));
            } else {
                self.set_selection_raw(Selection::cursor(gap_pos));
            }
        } else if before.is_some_and(|n| n.node_type.is_textblock()) {
            self.set_selection_raw(Selection::cursor(gap_pos - 1));
        } else {
            self.set_selection_raw(Selection::cursor(gap_pos));
        }
    }

    // ── Internal helpers ───────────────────────────────────────────────

    /// Check if every text node in `[from, to)` has the given mark type.
    ///
    /// Returns `false` if no text nodes exist in the range (avoids false
    /// positives on text-free selections like empty paragraphs or between
    /// block boundaries).
    pub(super) fn range_has_mark(&self, from: usize, to: usize, mark_type: MarkType) -> bool {
        let mut found_text = false;
        let mut all_have = true;
        self.doc.nodes_between(from, to, &mut |node, _pos, _parent, _idx| {
            if node.node_type.is_text() {
                found_text = true;
                if !node.marks.iter().any(|m| m.mark_type == mark_type) {
                    all_have = false;
                }
                return false; // Don't descend (text has no children anyway).
            }
            true // Descend into branches.
        });
        found_text && all_have
    }
}
