//! Selection management: position adjustment, word/line selection, and cursor queries.

use crate::attrs::{get_attr, AttrValue};
use crate::mark::MarkType;
use crate::position::ResolvedPos;

use super::{DocState, FormattingState, Selection};

impl DocState {
    // ── Position adjustment ─────────────────────────────────────────────

    /// If `pos` is between blocks (parent is not a textblock), nudge it into
    /// the nearest textblock.  When the position is right after a textblock's
    /// closing token, move it to the end of that textblock's content.  When
    /// it's right before a textblock's opening token, move it to the start
    /// of that textblock's content.  Returns the adjusted position.
    pub fn adjust_into_textblock(&self, pos: usize) -> usize {
        let resolved = self.doc.resolve(pos);
        if resolved.parent().node_type.is_textblock() {
            return pos; // Already inside a textblock.
        }
        // Try the node before this position — if it's a textblock, move
        // to the end of its content.
        if resolved.node_before().is_some_and(|n| n.node_type.is_textblock()) {
            // pos is right after the closing token of the previous textblock.
            // The end of its content = pos - 1.
            return pos - 1;
        }
        // Try the node after — if it's a textblock, move to the start
        // of its content.
        if resolved.node_after().is_some_and(|n| n.node_type.is_textblock()) {
            // pos is right before the opening token of the next textblock.
            // The start of its content = pos + 1.
            return pos + 1;
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
                crate::node_type::NodeType::Blockquote => state.blockquote = true,
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
