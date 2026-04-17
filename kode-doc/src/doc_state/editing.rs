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
            if parent_type != NodeType::Paragraph {
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

            // Paragraph at start — try to join with the previous block.
            if !self.join_backward() {
                // Join failed (e.g., previous node is not a textblock, or
                // we're at the first block). If the paragraph is empty,
                // delete it entirely so backspace doesn't get stuck.
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
                }
            }
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

        // Check if cursor is at the end of a textblock's content.
        if resolved.parent().node_type.is_textblock()
            && resolved.parent_offset == resolved.parent().content.size()
        {
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

        // Inside a code block, Enter inserts a newline character instead
        // of splitting the block.
        let resolved_from = self.doc.resolve(from);
        if resolved_from.parent().node_type == NodeType::CodeBlock {
            self.insert_text_inner("\n");
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

        // The position before the current textblock's opening token.
        let before_pos = resolved.before(tb_depth);
        if before_pos == 0 {
            return false; // First block in document.
        }

        // Check what's before us.
        let before_resolved = self.doc.resolve(before_pos);
        let prev_node = before_resolved.node_before();
        match prev_node {
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
}
