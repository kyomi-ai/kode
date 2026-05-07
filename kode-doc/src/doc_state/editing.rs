//! Editing operations: insert, delete, split, and join.

use crate::fragment::Fragment;
use crate::node::Node;
use crate::node_type::NodeType;
use crate::slice::Slice;
use crate::transform::Transform;

use super::{DocState, Selection};

impl DocState {
    // ── Editing operations ─────────────────────────────────────────────

    /// Insert text at the current cursor position.
    /// Replaces the selection if there is one.
    pub fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.push_undo();
        self.insert_text_inner(text);
        self.redo_stack.clear();
    }

    /// Insert text without pushing undo or clearing redo. Used by
    /// compound operations that manage their own undo entry.
    pub(super) fn insert_text_inner(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        let from = self.adjust_into_textblock(self.selection.from());
        let to = self.adjust_into_textblock(self.selection.to());

        // If the cursor is still not inside a textblock (e.g. empty document),
        // bootstrap an empty paragraph first so text has a valid home.
        let (from, to) = if !self.doc.resolve(from).parent().node_type.is_textblock() {
            let new_p = Node::branch(NodeType::Paragraph, Fragment::empty());
            let mut tr = Transform::new(self.doc.clone());
            if tr.insert(from, Fragment::from_node(new_p)).is_ok() {
                self.doc = tr.doc;
                let inside = from + 1; // position inside the new paragraph
                (inside, inside)
            } else {
                return;
            }
        } else {
            (from, to)
        };

        // Determine marks at the cursor position so inserted text inherits them.
        let marks = self.doc.resolve(from).marks();

        let text_node = Node::new_text_with_marks(text, marks);
        let content = Fragment::from_node(text_node);
        let slice = Slice::new(content, 0, 0);

        let mut tr = Transform::new(self.doc.clone());
        if tr.replace(from, to, slice).is_ok() {
            self.doc = tr.doc;
            let new_pos = from + text.chars().count();
            self.selection = Selection::cursor(new_pos);
        }
    }

    /// Delete the selection, or delete one character before the cursor (backspace).
    pub fn backspace(&mut self) {
        let from = self.adjust_into_textblock(self.selection.from());
        let to = self.adjust_into_textblock(self.selection.to());

        // If there is a range selection, delete it.
        if from != to {
            self.push_undo();
            let mut tr = Transform::new(self.doc.clone());
            if tr.delete(from, to).is_ok() {
                self.doc = tr.doc;
                self.selection = Selection::cursor(from);
            }
            self.redo_stack.clear();
            return;
        }

        // Cursor position — check if we're at the start of a textblock.
        if from == 0 {
            return; // At start of document, nothing to do.
        }

        let resolved = self.doc.resolve(from);

        // ── Atomic block handling ─────────────────────────────────────
        //
        // Case 1: Cursor at a gap position (parent is NOT a textblock).
        // If the node before the cursor is atomic, delete the entire
        // atomic block.
        if !resolved.parent().node_type.is_textblock() {
            if let Some(node_before) = resolved.node_before() {
                if node_before.is_atom() {
                    let atom_size = node_before.node_size();
                    let delete_from = from - atom_size;
                    self.push_undo();
                    let mut tr = Transform::new(self.doc.clone());
                    if tr.delete(delete_from, from).is_ok() {
                        self.doc = tr.doc;
                        self.selection = Selection::cursor(
                            self.adjust_into_textblock(
                                delete_from.min(self.doc.content.size())
                            )
                        );
                    }
                    self.redo_stack.clear();
                    return;
                }
            }
        }

        // Check if cursor is at start of a textblock content.
        if resolved.parent_offset == 0 && resolved.parent().node_type.is_textblock() {
            // If inside any non-Doc wrapper (ListItem, Blockquote, List,
            // CodeBlock, etc.), lift the content out or delete if empty.
            // Walk ancestors from innermost outward to find the first
            // non-Doc, non-textblock wrapper.
            let wrapper_depth = (1..resolved.depth).rev().find(|&d| {
                let nt = resolved.node(d).node_type;
                nt != NodeType::Doc && !nt.is_textblock()
            });

            if let Some(wd) = wrapper_depth {
                let wrapper_node = resolved.node(wd);

                // Inside a table structure: backspace at cell start is a no-op.
                // (Table itself is unreachable here — the walker always lands on
                // TableRow/TableHeader first — but included for defensive completeness.)
                if matches!(wrapper_node.node_type, NodeType::TableRow | NodeType::TableHeader | NodeType::Table) {
                    return;
                }

                let wrapper_start = resolved.before(wd);
                let wrapper_end = resolved.after(wd);
                let is_empty = wrapper_node.text_content().is_empty();

                self.push_undo();

                if is_empty {
                    // Empty container: delete the entire wrapper.
                    let mut tr = Transform::new(self.doc.clone());
                    if tr.delete(wrapper_start, wrapper_end).is_ok() {
                        self.doc = tr.doc;
                        let target = if wrapper_start > 0 {
                            self.adjust_into_textblock(
                                (wrapper_start - 1).min(self.doc.content.size())
                            )
                        } else {
                            self.adjust_into_textblock(
                                wrapper_start.min(self.doc.content.size())
                            )
                        };
                        self.selection = Selection::cursor(target);
                    }
                } else if wrapper_node.node_type == NodeType::ListItem
                    && wd > 0 && resolved.index(wd - 1) > 0
                {
                    // Non-first list item: merge with previous item by deleting
                    // all structure between the end of the previous item's last
                    // textblock content and the start of the current textblock
                    // content. This collapses the structural tokens between
                    // the two textblocks, merging them into one.
                    //
                    // We dynamically find the previous LI's last textblock
                    // instead of assuming a fixed token count, because the
                    // previous item's last child could be a nested list,
                    // blockquote, or other non-paragraph node.
                    let delete_from = {
                        let list_node = resolved.node(wd - 1);
                        let prev_li_idx = resolved.index(wd - 1) - 1;
                        let prev_li = list_node.child(prev_li_idx);
                        // Compute absolute position of previous LI's content start.
                        // Walk children of the list up to prev_li_idx.
                        let list_content_start = resolved.start(wd - 1);
                        let mut prev_li_start = list_content_start;
                        for i in 0..prev_li_idx {
                            prev_li_start += list_node.child(i).node_size();
                        }
                        // prev_li_start is the opening token of prev_li.
                        // Find the last textblock by walking down last children.
                        let mut node = prev_li;
                        let mut pos = prev_li_start + 1; // content start of prev_li
                        loop {
                            if node.node_type.is_textblock() {
                                // Found it — content end is pos + content.size()
                                break pos + node.content.size();
                            }
                            match node.last_child() {
                                Some(child) => {
                                    // Advance pos past all earlier siblings
                                    let count = node.child_count();
                                    for i in 0..count - 1 {
                                        pos += node.child(i).node_size();
                                    }
                                    pos += 1; // opening token of last child
                                    node = child;
                                }
                                None => {
                                    // No children — fallback to wrapper_start - 1
                                    break wrapper_start - 1;
                                }
                            }
                        }
                    };
                    let mut tr = Transform::new(self.doc.clone());
                    if tr.delete(delete_from, from).is_ok() {
                        self.doc = tr.doc;
                        let cursor = delete_from.min(self.doc.content.size());
                        self.selection = Selection::cursor(cursor);
                    }
                } else {
                    // Non-empty: replace the wrapper with its children,
                    // effectively lifting the content one level up.
                    let inner_content = wrapper_node.content.clone();
                    let slice = Slice::new(inner_content, 0, 0);
                    let mut tr = Transform::new(self.doc.clone());
                    if tr.replace(wrapper_start, wrapper_end, slice).is_ok() {
                        self.doc = tr.doc;
                        self.selection = Selection::cursor(
                            from.min(self.doc.content.size())
                        );
                    }
                }

                // Clean up any now-empty parent containers.
                let cursor = self.selection.from().min(self.doc.content.size());
                if cursor <= self.doc.content.size() {
                    let new_resolved = self.doc.resolve(cursor);
                    for d in (1..=new_resolved.depth).rev() {
                        let node = new_resolved.node(d);
                        let nt = node.node_type;
                        if nt != NodeType::Doc && !nt.is_textblock()
                            && node.child_count() == 0
                        {
                            let empty_start = new_resolved.before(d);
                            let empty_end = new_resolved.after(d);
                            let mut tr = Transform::new(self.doc.clone());
                            if tr.delete(empty_start, empty_end).is_ok() {
                                self.doc = tr.doc;
                                self.selection = Selection::cursor(
                                    self.adjust_into_textblock(
                                        empty_start.min(self.doc.content.size())
                                    )
                                );
                            }
                            break;
                        }
                    }
                }

                self.redo_stack.clear();
                return;
            }

            // No wrapper — if the textblock is anything other than Paragraph,
            // convert it to Paragraph first. This handles Headings, CodeBlocks,
            // and any other special textblock type. Standard editor behavior:
            // backspace at start of a heading/code block converts to paragraph.
            let parent_type = resolved.parent().node_type;
            if parent_type != NodeType::Paragraph && parent_type != NodeType::TableCell {
                self.push_undo();
                let block_start = resolved.before(resolved.depth);
                let block_end = resolved.after(resolved.depth);
                let mut tr = Transform::new(self.doc.clone());
                if tr.set_block_type(block_start, block_end, NodeType::Paragraph, Default::default()).is_ok() {
                    self.doc = tr.doc;
                    let max = self.doc.content.size();
                    self.selection = Selection::cursor(from.min(max));
                }
                self.redo_stack.clear();
                return;
            }

            // Empty paragraph: delete the paragraph itself rather than
            // joining backward (which would destroy an adjacent atomic block).
            let parent_content_size = resolved.parent().content.size();
            if parent_content_size == 0 && resolved.depth > 0 {
                let block_start = resolved.before(resolved.depth);
                let block_end = resolved.after(resolved.depth);
                self.push_undo();
                let mut tr = Transform::new(self.doc.clone());
                if tr.delete(block_start, block_end).is_ok() {
                    self.doc = tr.doc;
                    let target = if block_start > 0 {
                        self.adjust_into_textblock(
                            (block_start - 1).min(self.doc.content.size())
                        )
                    } else {
                        self.adjust_into_textblock(
                            block_start.min(self.doc.content.size())
                        )
                    };
                    self.selection = Selection::cursor(target);
                }
                self.redo_stack.clear();
                return;
            }

            // Non-empty paragraph at start — try to join with the previous block.
            self.join_backward();
            return;
        }

        // Delete one character before cursor.
        let delete_from = from - 1;
        let mut tr = Transform::new(self.doc.clone());
        if tr.delete(delete_from, from).is_ok() {
            self.push_undo();
            self.doc = tr.doc;
            self.selection = Selection::cursor(delete_from);
            self.redo_stack.clear();
        }
    }

    /// Delete the selection, or delete one character after the cursor.
    pub fn delete_forward(&mut self) {
        let from = self.adjust_into_textblock(self.selection.from());
        let to = self.adjust_into_textblock(self.selection.to());

        // If there is a range selection, delete it.
        if from != to {
            let mut tr = Transform::new(self.doc.clone());
            if tr.delete(from, to).is_ok() {
                self.push_undo();
                self.doc = tr.doc;
                self.selection = Selection::cursor(from);
                self.redo_stack.clear();
            }
            return;
        }

        // Cursor position — check if at end of a textblock.
        let doc_size = self.doc.content.size();
        if from >= doc_size {
            return; // At end of document, nothing to do.
        }

        let resolved = self.doc.resolve(from);

        // ── Atomic block handling ─────────────────────────────────────
        //
        // Case 1: Cursor at a gap position (parent is NOT a textblock).
        // If the node after the cursor is atomic, delete the entire
        // atomic block.
        if !resolved.parent().node_type.is_textblock() {
            if let Some(node_after) = resolved.node_after() {
                if node_after.is_atom() {
                    let atom_size = node_after.node_size();
                    let delete_to = from + atom_size;
                    self.push_undo();
                    let mut tr = Transform::new(self.doc.clone());
                    if tr.delete(from, delete_to).is_ok() {
                        self.doc = tr.doc;
                        self.selection = Selection::cursor(
                            self.adjust_into_textblock(
                                from.min(self.doc.content.size())
                            )
                        );
                    }
                    self.redo_stack.clear();
                    return;
                }
            }
        }

        // Check if cursor is at the end of a textblock's content.
        if resolved.parent().node_type.is_textblock()
            && resolved.parent_offset == resolved.parent().content.size()
        {
            let after_pos = resolved.after(resolved.depth);

            // Empty paragraph before an atomic block: remove the paragraph
            // rather than deleting the atom.
            if resolved.parent().content.size() == 0 && resolved.depth > 0 {
                let block_start = resolved.before(resolved.depth);
                let block_end = resolved.after(resolved.depth);
                self.push_undo();
                let mut tr = Transform::new(self.doc.clone());
                if tr.delete(block_start, block_end).is_ok() {
                    self.doc = tr.doc;
                    self.selection = Selection::cursor(
                        self.adjust_into_textblock(
                            block_start.min(self.doc.content.size())
                        )
                    );
                }
                self.redo_stack.clear();
                return;
            }

            // At the end of a non-empty textblock — if the next block is
            // atomic, delete it (single-character delete-forward).
            if after_pos < doc_size {
                let after_resolved = self.doc.resolve(after_pos);
                if let Some(next_node) = after_resolved.node_after() {
                    if next_node.is_atom() {
                        let atom_size = next_node.node_size();
                        let delete_to = after_pos + atom_size;
                        self.push_undo();
                        let mut tr = Transform::new(self.doc.clone());
                        if tr.delete(after_pos, delete_to).is_ok() {
                            self.doc = tr.doc;
                        }
                        self.redo_stack.clear();
                        return;
                    }
                }
            }

            // At the end of a textblock — try to join with the next block
            // (forward join: delete the closing token of current block and
            // opening token of next block).
            self.join_forward();
            return;
        }

        let delete_to = from + 1;
        let mut tr = Transform::new(self.doc.clone());
        if tr.delete(from, delete_to).is_ok() {
            self.push_undo();
            self.doc = tr.doc;
            // Cursor stays at same position.
            self.redo_stack.clear();
        }
    }

    /// Split the current block at the cursor (Enter key).
    ///
    /// If there is a range selection, the range is deleted first, then the
    /// block is split at the resulting cursor position.
    pub fn split_block(&mut self) {
        self.push_undo();
        self.split_block_inner();
        self.redo_stack.clear();
    }

    /// Split block without pushing undo or clearing redo. Used by
    /// compound operations that manage their own undo entry.
    pub(super) fn split_block_inner(&mut self) {
        let raw_from = self.selection.from();
        let raw_to = self.selection.to();

        // If cursor is between blocks (e.g., at a closing token), insert a
        // new empty paragraph at that position rather than trying to split.
        let resolved = self.doc.resolve(raw_from);
        if !resolved.parent().node_type.is_textblock() && raw_from == raw_to {
            let new_p = Node::branch(NodeType::Paragraph, Fragment::empty());
            let mut tr = Transform::new(self.doc.clone());
            if tr.insert(raw_from, Fragment::from_node(new_p)).is_ok() {
                self.doc = tr.doc;
                self.selection = Selection::cursor(raw_from + 1);
            }
            return;
        }

        let from = self.adjust_into_textblock(raw_from);
        let to = self.adjust_into_textblock(raw_to);

        // Inside a code block, Enter inserts a newline character.
        let resolved_from = self.doc.resolve(from);
        if resolved_from.parent().node_type == NodeType::CodeBlock {
            self.insert_text_inner("\n");
            return;
        }

        // Inside a table cell, Enter inserts a HardBreak (<br>) since
        // table cells use normal whitespace collapsing where \n is invisible.
        // No push_undo/redo_stack.clear here — the caller (split_block) handles that.
        if resolved_from.parent().node_type == NodeType::TableCell {
            if from != to {
                let mut tr = Transform::new(self.doc.clone());
                if tr.delete(from, to).is_ok() {
                    self.doc = tr.doc;
                } else {
                    return;
                }
            }
            let br = Node::leaf(NodeType::HardBreak);
            let mut tr = Transform::new(self.doc.clone());
            if tr.insert(from, Fragment::from_node(br)).is_ok() {
                self.doc = tr.doc;
                self.selection = Selection::cursor(from + 1);
            }
            return;
        }

        // Delete selection range first if present.
        if from != to {
            let mut tr = Transform::new(self.doc.clone());
            if tr.delete(from, to).is_ok() {
                self.doc = tr.doc;
            } else {
                return;
            }
        }

        // Resolve the cursor to find the textblock depth.
        let resolved = self.doc.resolve(from);

        // Find the textblock depth — we need to split at depth 1 relative to
        // the innermost textblock.
        if !resolved.parent().node_type.is_textblock() {
            let new_p = Node::branch(NodeType::Paragraph, Fragment::empty());
            let mut tr = Transform::new(self.doc.clone());
            if tr.insert(from, Fragment::from_node(new_p)).is_ok() {
                self.doc = tr.doc;
                self.selection = Selection::cursor(from + 1);
            }
            return;
        }

        // Split at the cursor position. Depth=1 splits the innermost textblock.
        // When inside a list item (Doc > List > ListItem > Paragraph), we need
        // depth=2 to split both the paragraph AND the list item, creating a new
        // sibling list item.
        let split_depth = if resolved.depth >= 2
            && resolved.node(resolved.depth - 1).node_type == NodeType::ListItem
        {
            2
        } else {
            1
        };
        let parent_type = resolved.parent().node_type;
        let at_end_of_block = resolved.parent_offset
            == resolved.parent().content.size();

        let mut tr = Transform::new(self.doc.clone());
        if tr.split(from, split_depth).is_ok() {
            self.doc = tr.doc;

            let new_pos = from + 2 * split_depth;
            self.selection = Selection::cursor(new_pos);

            // When splitting at the end of a heading, convert the new (empty)
            // block to a paragraph.
            if parent_type == NodeType::Heading && at_end_of_block {
                let mut tr2 = Transform::new(self.doc.clone());
                if tr2.set_block_type(new_pos, new_pos, NodeType::Paragraph, Default::default()).is_ok() {
                    self.doc = tr2.doc;
                }
            }
        }
    }

    /// Join with the previous block (Backspace at start of block).
    /// Returns `true` if a join was performed, `false` if at start of doc.
    pub fn join_backward(&mut self) -> bool {
        let from = self.selection.from();

        let resolved = self.doc.resolve(from);

        // Find the textblock boundary. We need to find the position between
        // the current textblock and the previous sibling block.
        // The textblock depth is resolved.depth (if parent is textblock).
        let tb_depth = if resolved.parent().node_type.is_textblock() {
            resolved.depth
        } else {
            // Not inside a textblock — can't join backward.
            return false;
        };

        // Table cells must not join across cell boundaries.
        if resolved.parent().node_type == NodeType::TableCell {
            return false;
        }

        // The position before the current textblock's opening token.
        let before_pos = resolved.before(tb_depth);
        if before_pos == 0 {
            return false; // First block in document.
        }

        // Check what's before us.
        let before_resolved = self.doc.resolve(before_pos);
        let prev_node = before_resolved.node_before();
        match prev_node {
            Some(n) if n.is_atom() => {
                // Atomic block — delete the entire atomic node.
                let atom_start = before_pos - n.node_size();
                self.push_undo();
                let mut tr = Transform::new(self.doc.clone());
                if tr.delete(atom_start, before_pos).is_ok() {
                    self.doc = tr.doc;
                    self.selection = Selection::cursor(
                        self.adjust_into_textblock(
                            atom_start.min(self.doc.content.size())
                        )
                    );
                }
                self.redo_stack.clear();
                return true;
            }
            Some(n) if n.node_type.is_textblock() => {
                // Textblock — will join below.
            }
            Some(n) if n.node_type.is_leaf() => {
                // Leaf node (HR, HardBreak, Image) — delete it.
                let leaf_start = before_pos - n.node_size();
                self.push_undo();
                let mut tr = Transform::new(self.doc.clone());
                if tr.delete(leaf_start, before_pos).is_ok() {
                    self.doc = tr.doc;
                    self.selection = Selection::cursor(
                        leaf_start.min(self.doc.content.size())
                    );
                }
                self.redo_stack.clear();
                return true;
            }
            _ => return false,
        }

        self.push_undo();

        // The join position is at `before_pos` — between the previous block's
        // closing token and the current block's opening token.
        let mut tr = Transform::new(self.doc.clone());
        if tr.join(before_pos).is_ok() {
            self.doc = tr.doc;
            // Cursor moves to the join point — the end of the previous block's
            // content, which is now at `before_pos - 1` (the join deleted two
            // tokens: closing of prev and opening of current).
            self.selection = Selection::cursor(before_pos - 1);
        }

        self.redo_stack.clear();
        true
    }

    /// Join with the next block (Delete at end of block).
    /// Returns `true` if a join was performed, `false` otherwise.
    pub fn join_forward(&mut self) -> bool {
        let from = self.selection.from();

        let resolved = self.doc.resolve(from);

        let tb_depth = if resolved.parent().node_type.is_textblock() {
            resolved.depth
        } else {
            return false;
        };

        // Table cells must not join across cell boundaries.
        if resolved.parent().node_type == NodeType::TableCell {
            return false;
        }

        // The position after the current textblock's closing token.
        let after_pos = resolved.after(tb_depth);
        let doc_size = self.doc.content.size();
        if after_pos >= doc_size {
            return false; // Last block in document.
        }

        // Check that there's a block after us to join with.
        let after_resolved = self.doc.resolve(after_pos);
        let next_node = after_resolved.node_after();
        match next_node {
            Some(n) if n.node_type.is_textblock() => {}
            _ => return false, // Next sibling is not a textblock.
        }

        self.push_undo();

        // The join position is at `after_pos` — between the current block's
        // closing token and the next block's opening token.
        let mut tr = Transform::new(self.doc.clone());
        if tr.join(after_pos).is_ok() {
            self.doc = tr.doc;
            // Cursor stays at same position (end of current block's text,
            // which is now in the middle of the joined block).
        }

        self.redo_stack.clear();
        true
    }

    /// Move the cursor to the next table cell (Tab in a table).
    ///
    /// Walks ancestors to find a `TableCell`, then moves to the next cell in
    /// the row. If at the last cell in a row, jumps to the first cell of the
    /// next row. Returns `false` if the cursor is not in a table cell or is
    /// already at the last cell of the last row.
    pub fn move_to_next_cell(&mut self) -> bool {
        let resolved = self.doc.resolve(self.selection.head);

        // Walk ancestors to find a TableCell depth.
        let cell_depth = match (0..=resolved.depth)
            .rev()
            .find(|&d| resolved.node(d).node_type == NodeType::TableCell)
        {
            Some(d) => d,
            None => return false,
        };

        let row_depth = cell_depth - 1;
        let table_depth = cell_depth - 2;

        let cell_index = resolved.index(row_depth);
        let row = resolved.node(row_depth);

        if cell_index + 1 < row.child_count() {
            // Move to the next cell in this row.
            let mut pos = resolved.start(row_depth);
            for i in 0..=cell_index {
                pos += row.child(i).node_size();
            }
            // pos is now the opening token of the next cell; +1 for content start.
            self.selection = Selection::cursor(pos + 1);
            true
        } else {
            // Last cell in row — check for a next row.
            let table = resolved.node(table_depth);
            let row_index = resolved.index(table_depth);

            if row_index + 1 < table.child_count() {
                // Move to the first cell of the next row.
                let mut pos = resolved.start(table_depth);
                for i in 0..=row_index {
                    pos += table.child(i).node_size();
                }
                // pos = next row's opening token; +1 row content, +1 cell opening.
                self.selection = Selection::cursor(pos + 2);
                true
            } else {
                false
            }
        }
    }

    /// Move the cursor to the previous table cell (Shift+Tab in a table).
    ///
    /// Walks ancestors to find a `TableCell`, then moves to the previous cell
    /// in the row. If at the first cell in a row, jumps to the last cell of
    /// the previous row. Returns `false` if the cursor is not in a table cell
    /// or is already at the first cell of the first row.
    pub fn move_to_prev_cell(&mut self) -> bool {
        let resolved = self.doc.resolve(self.selection.head);

        // Walk ancestors to find a TableCell depth.
        let cell_depth = match (0..=resolved.depth)
            .rev()
            .find(|&d| resolved.node(d).node_type == NodeType::TableCell)
        {
            Some(d) => d,
            None => return false,
        };

        let row_depth = cell_depth - 1;
        let table_depth = cell_depth - 2;

        let cell_index = resolved.index(row_depth);

        if cell_index > 0 {
            // Move to the previous cell in this row.
            let mut pos = resolved.start(row_depth);
            for i in 0..cell_index - 1 {
                pos += resolved.node(row_depth).child(i).node_size();
            }
            // pos is now the opening token of the previous cell; +1 for content start.
            self.selection = Selection::cursor(pos + 1);
            true
        } else {
            // First cell in row — check for a previous row.
            let table = resolved.node(table_depth);
            let row_index = resolved.index(table_depth);

            if row_index > 0 {
                // Move to the last cell of the previous row.
                let mut pos = resolved.start(table_depth);
                for i in 0..row_index - 1 {
                    pos += table.child(i).node_size();
                }
                // pos = previous row's opening token; +1 for row content start.
                pos += 1;

                let prev_row = table.child(row_index - 1);
                for i in 0..prev_row.child_count() - 1 {
                    pos += prev_row.child(i).node_size();
                }
                // pos = last cell's opening token; +1 for content start.
                self.selection = Selection::cursor(pos + 1);
                true
            } else {
                false
            }
        }
    }

    /// Move a block from `[block_start, block_end)` to `target_pos`.
    ///
    /// This is the engine behind drag-and-drop block reordering. The block
    /// identified by `block_start..block_end` is extracted, removed from its
    /// current position, and re-inserted at `target_pos` (which is adjusted
    /// through the deletion's step map so it stays correct after the removal).
    ///
    /// No-op if `target_pos` is within `block_start..=block_end`.
    ///
    /// Panics if `block_start > block_end`.
    pub fn move_block(&mut self, block_start: usize, block_end: usize, target_pos: usize) {
        assert!(block_start <= block_end, "block_start ({block_start}) > block_end ({block_end})");

        // No-op: moving to the same position (including block_end, which
        // maps back to block_start after deletion — producing an identical doc).
        if target_pos >= block_start && target_pos <= block_end {
            return;
        }

        self.push_undo();

        // Extract the block content before mutating.
        let extracted = self.doc.slice(block_start, block_end).content;

        // Delete the block, then insert at the mapped target position.
        let mut tr = Transform::new(self.doc.clone());
        if tr.delete(block_start, block_end).is_ok() {
            let adjusted_target = tr.map_pos(target_pos, 1);
            if tr.insert(adjusted_target, extracted).is_ok() {
                self.doc = tr.doc;
                self.selection = Selection::cursor(
                    self.adjust_into_textblock(adjusted_target + 1),
                );
            }
        }

        self.redo_stack.clear();
    }
}

// ── Table row/column operations ──────────────────────────────────────────

/// Context about the cursor's position within a table structure.
struct TableContext {
    /// Depth of the Table node in the resolved path.
    table_depth: usize,
    /// Depth of the TableRow or TableHeader node.
    row_depth: usize,
    /// Which row in the table (0-based).
    row_index: usize,
    /// Which cell in the row (0-based).
    col_index: usize,
}

impl DocState {
    /// Find the table context for the current cursor position.
    ///
    /// Walks ancestors from innermost to outermost looking for a `TableCell`.
    /// Returns `None` if the cursor is not inside a table cell.
    fn find_table_context(&self) -> Option<TableContext> {
        let resolved = self.doc.resolve(self.selection.head);

        // Walk ancestors to find a TableCell depth.
        let cell_depth = (0..=resolved.depth)
            .rev()
            .find(|&d| resolved.node(d).node_type == NodeType::TableCell)?;

        // TableCell must be at depth >= 2 (Table > Row > Cell).
        if cell_depth < 2 {
            return None;
        }

        let row_depth = cell_depth - 1;
        let table_depth = cell_depth - 2;

        // Verify the ancestor types are correct.
        let row_type = resolved.node(row_depth).node_type;
        if !matches!(row_type, NodeType::TableRow | NodeType::TableHeader) {
            return None;
        }
        if resolved.node(table_depth).node_type != NodeType::Table {
            return None;
        }

        let col_index = resolved.index(row_depth);
        let row_index = resolved.index(table_depth);

        Some(TableContext {
            table_depth,
            row_depth,
            row_index,
            col_index,
        })
    }

    /// Insert a new empty row below the current row.
    ///
    /// No-op if the cursor is not inside a table cell.
    pub fn insert_row_below(&mut self) {
        let ctx = match self.find_table_context() {
            Some(c) => c,
            None => return,
        };

        let resolved = self.doc.resolve(self.selection.head);
        let row = resolved.node(ctx.row_depth);
        let col_count = row.child_count();

        // Build a new TableRow with empty cells.
        let cells: Vec<Node> = (0..col_count)
            .map(|_| Node::branch(NodeType::TableCell, Fragment::empty()))
            .collect();
        let new_row = Node::branch(NodeType::TableRow, Fragment::from_vec(cells));

        // Insert position: right after the current row's closing token.
        let insert_pos = resolved.after(ctx.row_depth);

        self.push_undo();
        let mut tr = Transform::new(self.doc.clone());
        if tr.insert(insert_pos, Fragment::from_node(new_row)).is_ok() {
            self.doc = tr.doc;
            // Cursor into the first cell of the new row: +1 row open + 1 cell open.
            self.selection = Selection::cursor(insert_pos + 2);
        }
        self.redo_stack.clear();
    }

    /// Insert a new empty row above the current row.
    ///
    /// If the current row is the header row, inserts below it instead (cannot
    /// insert above the header). No-op if the cursor is not inside a table cell.
    pub fn insert_row_above(&mut self) {
        let ctx = match self.find_table_context() {
            Some(c) => c,
            None => return,
        };

        let resolved = self.doc.resolve(self.selection.head);
        let row = resolved.node(ctx.row_depth);

        // Cannot insert above the header row — insert below instead.
        if row.node_type == NodeType::TableHeader {
            self.insert_row_below();
            return;
        }

        let col_count = row.child_count();

        // Build a new TableRow with empty cells.
        let cells: Vec<Node> = (0..col_count)
            .map(|_| Node::branch(NodeType::TableCell, Fragment::empty()))
            .collect();
        let new_row = Node::branch(NodeType::TableRow, Fragment::from_vec(cells));

        // Insert position: before the current row's opening token.
        let insert_pos = resolved.before(ctx.row_depth);

        self.push_undo();
        let mut tr = Transform::new(self.doc.clone());
        if tr.insert(insert_pos, Fragment::from_node(new_row)).is_ok() {
            self.doc = tr.doc;
            // Cursor into the first cell of the new row: +1 row open + 1 cell open.
            self.selection = Selection::cursor(insert_pos + 2);
        }
        self.redo_stack.clear();
    }

    /// Delete the current row.
    ///
    /// No-op if the cursor is not inside a table, if the current row is the
    /// header row (index 0), or if there is only one data row remaining.
    pub fn delete_row(&mut self) {
        let ctx = match self.find_table_context() {
            Some(c) => c,
            None => return,
        };

        let resolved = self.doc.resolve(self.selection.head);
        let row = resolved.node(ctx.row_depth);

        // Don't delete the header row.
        if row.node_type == NodeType::TableHeader {
            return;
        }
        let table = resolved.node(ctx.table_depth);

        // Don't delete if it's the only data row (header + 1 row = child_count 2).
        if table.child_count() <= 2 {
            return;
        }

        let delete_from = resolved.before(ctx.row_depth);
        let delete_to = resolved.after(ctx.row_depth);

        self.push_undo();
        let mut tr = Transform::new(self.doc.clone());
        if tr.delete(delete_from, delete_to).is_ok() {
            self.doc = tr.doc;
            let target = delete_from.min(self.doc.content.size());
            self.selection = Selection::cursor(self.adjust_into_textblock(target));
        }
        self.redo_stack.clear();
    }

    /// Insert a new column to the right of the current column.
    ///
    /// Rebuilds the entire table with an additional cell in every row at
    /// position `col_index + 1`. No-op if the cursor is not inside a table.
    pub fn insert_column_right(&mut self) {
        self.insert_column_at_offset(1);
    }

    /// Insert a new column to the left of the current column.
    ///
    /// Rebuilds the entire table with an additional cell in every row at
    /// position `col_index`. No-op if the cursor is not inside a table.
    pub fn insert_column_left(&mut self) {
        self.insert_column_at_offset(0);
    }

    /// Shared implementation for insert_column_left and insert_column_right.
    ///
    /// `offset` is 0 for left (insert at col_index) or 1 for right (insert
    /// at col_index + 1).
    fn insert_column_at_offset(&mut self, offset: usize) {
        let ctx = match self.find_table_context() {
            Some(c) => c,
            None => return,
        };

        let resolved = self.doc.resolve(self.selection.head);
        let table = resolved.node(ctx.table_depth);
        let insert_col = ctx.col_index + offset;

        // Rebuild the table with the new column.
        let mut new_rows = Vec::new();
        for row_idx in 0..table.child_count() {
            let row = table.child(row_idx);
            let mut new_cells: Vec<Node> = Vec::new();

            for cell_idx in 0..row.child_count() {
                if cell_idx == insert_col {
                    new_cells.push(Node::branch(NodeType::TableCell, Fragment::empty()));
                }
                new_cells.push(row.child(cell_idx).clone());
            }
            // If inserting after the last cell.
            if insert_col >= row.child_count() {
                new_cells.push(Node::branch(NodeType::TableCell, Fragment::empty()));
            }

            new_rows.push(Node::branch(row.node_type, Fragment::from_vec(new_cells)));
        }

        let new_table = Node::branch(NodeType::Table, Fragment::from_vec(new_rows));
        let table_start = resolved.before(ctx.table_depth);
        let table_end = resolved.after(ctx.table_depth);

        self.push_undo();
        let mut tr = Transform::new(self.doc.clone());
        if tr
            .replace(table_start, table_end, Slice::new(Fragment::from_node(new_table), 0, 0))
            .is_ok()
        {
            self.doc = tr.doc;

            // Place cursor in the new cell of the current row.
            // Walk the rebuilt table to find the absolute position of the new cell.
            let new_resolved = self.doc.resolve(table_start + 1);
            let new_table_node = new_resolved.node(ctx.table_depth);
            let mut row_pos = table_start + 1; // table content start
            for r in 0..ctx.row_index {
                row_pos += new_table_node.child(r).node_size();
            }
            // row_pos is at the row's opening token.
            let current_row = new_table_node.child(ctx.row_index);
            let mut cell_pos = row_pos + 1; // row content start
            for c in 0..insert_col {
                cell_pos += current_row.child(c).node_size();
            }
            // cell_pos is at the new cell's opening token. +1 for content start.
            self.selection = Selection::cursor(cell_pos + 1);
        }
        self.redo_stack.clear();
    }

    /// Delete the current column from the table.
    ///
    /// Rebuilds the entire table without the cell at `col_index` in each row.
    /// No-op if the cursor is not inside a table or if any row has only one cell.
    pub fn delete_column(&mut self) {
        let ctx = match self.find_table_context() {
            Some(c) => c,
            None => return,
        };

        let resolved = self.doc.resolve(self.selection.head);
        let table = resolved.node(ctx.table_depth);

        // Don't delete if any row has only one column.
        for row_idx in 0..table.child_count() {
            if table.child(row_idx).child_count() <= 1 {
                return;
            }
        }

        // Rebuild the table without the cell at col_index.
        let mut new_rows = Vec::new();
        for row_idx in 0..table.child_count() {
            let row = table.child(row_idx);
            let mut new_cells: Vec<Node> = Vec::new();
            for cell_idx in 0..row.child_count() {
                if cell_idx != ctx.col_index {
                    new_cells.push(row.child(cell_idx).clone());
                }
            }
            new_rows.push(Node::branch(row.node_type, Fragment::from_vec(new_cells)));
        }

        let new_col_count = new_rows.first().map_or(0, |r| r.child_count());
        let clamped_col = ctx.col_index.min(new_col_count.saturating_sub(1));

        let new_table = Node::branch(NodeType::Table, Fragment::from_vec(new_rows));
        let table_start = resolved.before(ctx.table_depth);
        let table_end = resolved.after(ctx.table_depth);

        self.push_undo();
        let mut tr = Transform::new(self.doc.clone());
        if tr
            .replace(table_start, table_end, Slice::new(Fragment::from_node(new_table), 0, 0))
            .is_ok()
        {
            self.doc = tr.doc;
            // Walk the rebuilt table to find the target cell.
            let rp = self.doc.resolve(table_start + 1);
            let rebuilt_table = rp.node(ctx.table_depth);
            let mut cursor_pos = table_start + 1; // table content start
            for ri in 0..rebuilt_table.child_count() {
                if ri == ctx.row_index {
                    let row = rebuilt_table.child(ri);
                    cursor_pos += 1; // row opening token
                    for ci in 0..row.child_count() {
                        if ci == clamped_col {
                            cursor_pos += 1; // cell opening token
                            break;
                        }
                        cursor_pos += row.child(ci).node_size();
                    }
                    break;
                }
                cursor_pos += rebuilt_table.child(ri).node_size();
            }
            self.selection = Selection::cursor(cursor_pos.min(self.doc.content.size()));
        }
        self.redo_stack.clear();
    }
}
