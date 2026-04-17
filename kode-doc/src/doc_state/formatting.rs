//! Block and inline formatting: marks, block types, lists, blockquotes, links, and rules.

use crate::attrs::Attrs;
use crate::fragment::Fragment;
use crate::mark::{Mark, MarkType};
use crate::node::Node;
use crate::node_type::NodeType;
use crate::slice::Slice;
use crate::transform::Transform;

use super::{DocState, Selection};

impl DocState {
    /// Toggle a mark on the selection range.
    /// If the selection is collapsed (cursor), this is a no-op.
    pub fn toggle_mark(&mut self, mark_type: MarkType) {
        let from = self.selection.from();
        let to = self.selection.to();

        if from == to {
            return; // No range to apply mark to.
        }

        let mark = Mark::new(mark_type);

        // Check if the entire range already has this mark.
        let has_mark = self.range_has_mark(from, to, mark_type);

        let mut tr = Transform::new(self.doc.clone());
        let result = if has_mark {
            tr.remove_mark(from, to, mark)
        } else {
            tr.add_mark(from, to, mark)
        };

        if result.is_ok() {
            self.push_undo();
            self.doc = tr.doc;
            self.redo_stack.clear();
        }
    }

    /// Set the block type for the block at the cursor position.
    pub fn set_block_type(&mut self, node_type: NodeType, attrs: Attrs) {
        let from = self.selection.from();
        let to = self.selection.to();

        // Resolve the cursor to find the textblock range.
        let resolved = self.doc.resolve(from);
        let (block_from, block_to) = if resolved.parent().node_type.is_textblock() {
            let start = resolved.before(resolved.depth);
            let end = resolved.after(resolved.depth);
            (start, end)
        } else {
            (from, to)
        };

        let mut tr = Transform::new(self.doc.clone());
        if tr.set_block_type(block_from, block_to, node_type, attrs).is_ok() {
            self.push_undo();
            self.doc = tr.doc;
            // set_block_type only changes the node type/attrs, not content.
            // The cursor position within the block doesn't change, so keep
            // the selection as-is, clamped to the new doc size.
            let max = self.doc.content.size();
            let new_anchor = self.selection.anchor.min(max);
            let new_head = self.selection.head.min(max);
            self.selection = Selection::range(new_anchor, new_head);
            self.redo_stack.clear();
        }
    }

    // ── Block-level operations (wrap / lift) ─────────────────────────────

    /// Check if the cursor is inside a node of the given type by walking
    /// ancestors from the cursor position upward.
    pub fn is_in_node(&self, node_type: NodeType) -> bool {
        let resolved = self.doc.resolve(self.selection.head);
        for d in 0..=resolved.depth {
            if resolved.node(d).node_type == node_type {
                return true;
            }
        }
        false
    }

    /// Toggle blockquote: wrap in blockquote if not already in one,
    /// lift out of blockquote if already inside one.
    pub fn toggle_blockquote(&mut self) {
        let from = self.selection.from();
        let to = self.selection.to();

        if self.is_in_node(NodeType::Blockquote) {
            // Lift out of blockquote.
            self.push_undo();
            let mut tr = Transform::new(self.doc.clone());
            if tr.lift(from, NodeType::Blockquote).is_ok() {
                // Lifting removes the blockquote wrapper (1 open + 1 close token).
                // Positions inside the wrapper shift left by 1.
                let new_anchor = self.selection.anchor.saturating_sub(1);
                let new_head = self.selection.head.saturating_sub(1);
                self.doc = tr.doc;
                let max = self.doc.content.size();
                self.selection = Selection::range(new_anchor.min(max), new_head.min(max));
                self.redo_stack.clear();
            }
        } else {
            // Wrap in blockquote.
            self.push_undo();
            let mut tr = Transform::new(self.doc.clone());
            if tr.wrap(from, to, NodeType::Blockquote, Attrs::new()).is_ok() {
                // Wrapping adds a blockquote wrapper (1 open token before content).
                // Positions inside the wrapped range shift right by 1.
                let new_anchor = self.selection.anchor + 1;
                let new_head = self.selection.head + 1;
                self.doc = tr.doc;
                let max = self.doc.content.size();
                self.selection = Selection::range(new_anchor.min(max), new_head.min(max));
                self.redo_stack.clear();
            }
        }
    }

    /// Toggle bullet list: wrap in list if not in one, lift from list if
    /// already in a bullet list, or change list type if in an ordered list.
    pub fn toggle_bullet_list(&mut self) {
        self.toggle_list(NodeType::BulletList, Attrs::new());
    }

    /// Toggle ordered list: same pattern as bullet list but for ordered.
    pub fn toggle_ordered_list(&mut self) {
        use crate::attrs::ordered_list_attrs;
        self.toggle_list(NodeType::OrderedList, ordered_list_attrs(1));
    }

    /// Shared logic for toggling between list types.
    fn toggle_list(&mut self, target_list_type: NodeType, attrs: Attrs) {
        let from = self.selection.from();
        let to = self.selection.to();
        let resolved = self.doc.resolve(from);

        // Check if we're inside any list.
        let mut current_list_depth = None;
        let mut current_list_type = None;
        for d in (1..=resolved.depth).rev() {
            let nt = resolved.node(d).node_type;
            if nt == NodeType::BulletList || nt == NodeType::OrderedList {
                current_list_depth = Some(d);
                current_list_type = Some(nt);
                break;
            }
        }

        match (current_list_depth, current_list_type) {
            (Some(_depth), Some(lt)) if lt == target_list_type => {
                // Already in the target list type — lift out of list.
                // Strategy: lift the list wrapper first, then lift the ListItem wrapper.
                self.push_undo();
                let mut tr = Transform::new(self.doc.clone());
                if tr.lift(from, target_list_type).is_ok() {
                    // After lifting the list wrapper, the ListItem is now directly
                    // in the doc. The list's opening token has been removed, so
                    // every position inside shifts left by 1. `from.saturating_sub(1)`
                    // reliably points inside the (now unwrapped) ListItem because
                    // the original `from` was: list_open + list_item_open + para_open + …,
                    // and removing the list_open token makes it land inside the ListItem.
                    let inner_pos = from.saturating_sub(1);
                    if tr.lift(inner_pos, NodeType::ListItem).is_ok() {
                        // Removed 2 wrappers total (list + list item = 2 open tokens).
                        let new_anchor = self.selection.anchor.saturating_sub(2);
                        let new_head = self.selection.head.saturating_sub(2);
                        self.doc = tr.doc;
                        let max = self.doc.content.size();
                        self.selection =
                            Selection::range(new_anchor.min(max), new_head.min(max));
                        self.redo_stack.clear();
                    }
                }
            }
            (Some(depth), Some(_other_list_type)) => {
                // In a different list type — change the list type.
                // Replace the list node with one of the target type, keeping children.
                self.push_undo();
                let list_node = resolved.node(depth);
                let list_start = resolved.before(depth);
                let list_end = resolved.after(depth);
                let new_list = Node::branch_with_attrs(
                    target_list_type,
                    attrs,
                    list_node.content.clone(),
                );
                let mut tr = Transform::new(self.doc.clone());
                if tr
                    .replace(
                        list_start,
                        list_end,
                        Slice::new(Fragment::from_node(new_list), 0, 0),
                    )
                    .is_ok()
                {
                    // Same structure, just type changed — positions unchanged.
                    self.doc = tr.doc;
                    self.redo_stack.clear();
                }
            }
            _ => {
                // Not in any list — wrap in the target list type.
                self.push_undo();
                let mut tr = Transform::new(self.doc.clone());
                if tr.wrap_in_list(from, to, target_list_type, attrs).is_ok() {
                    // wrap_in_list adds 2 wrapper tokens (list open + list item open).
                    let new_anchor = self.selection.anchor + 2;
                    let new_head = self.selection.head + 2;
                    self.doc = tr.doc;
                    let max = self.doc.content.size();
                    self.selection =
                        Selection::range(new_anchor.min(max), new_head.min(max));
                    self.redo_stack.clear();
                }
            }
        }
    }

    /// Insert a link mark on the selection. If the selection is collapsed,
    /// inserts the URL as link text with the link mark applied.
    pub fn insert_link(&mut self, url: &str) {
        let from = self.selection.from();
        let to = self.selection.to();

        let link_mark = Mark::with_attrs(
            MarkType::Link,
            crate::attrs::link_attrs(url, None),
        );

        if from == to {
            // No selection — insert the URL as linked text at the cursor.
            self.push_undo();
            let text_node = Node::new_text_with_marks(url, vec![link_mark]);
            let content = Fragment::from_node(text_node);
            let mut tr = Transform::new(self.doc.clone());
            if tr.replace(from, to, Slice::new(content, 0, 0)).is_ok() {
                let new_pos = from + url.chars().count();
                self.doc = tr.doc;
                self.selection = Selection::cursor(new_pos);
                self.redo_stack.clear();
            }
        } else {
            // Selection exists — apply the link mark to the selection.
            self.push_undo();
            let mut tr = Transform::new(self.doc.clone());
            if tr.add_mark(from, to, link_mark).is_ok() {
                self.doc = tr.doc;
                self.redo_stack.clear();
            }
        }
    }

    /// Insert a link with custom display text at the cursor position.
    /// The displayed text is `text` and the link URL is `url`.
    pub fn insert_link_with_text(&mut self, text: &str, url: &str) {
        self.push_undo();
        let link_mark = Mark::with_attrs(
            MarkType::Link,
            crate::attrs::link_attrs(url, None),
        );
        let text_node = Node::new_text_with_marks(text, vec![link_mark]);
        let content = Fragment::from_node(text_node);
        let from = self.selection.from();
        let to = self.selection.to();
        let mut tr = Transform::new(self.doc.clone());
        if tr.replace(from, to, Slice::new(content, 0, 0)).is_ok() {
            let new_pos = from + text.chars().count();
            self.doc = tr.doc;
            self.selection = Selection::cursor(new_pos);
            self.redo_stack.clear();
        }
    }

    /// Insert a horizontal rule at the cursor position.
    ///
    /// If the cursor is inside a textblock, splits the block first, then
    /// inserts the horizontal rule between the two halves.
    pub fn insert_horizontal_rule(&mut self) {
        let from = self.selection.from();
        let to = self.selection.to();

        // Delete selection if any.
        let mut tr = Transform::new(self.doc.clone());
        let pos = if from != to {
            if tr.delete(from, to).is_err() {
                return;
            }
            from
        } else {
            from
        };

        // Resolve position to check context.
        let resolved = tr.doc.resolve(pos);

        if resolved.parent().node_type.is_textblock() {
            // Inside a textblock: split the block, insert HR between halves.
            if tr.split(pos, 1).is_err() {
                return;
            }
            // After split, the cursor is at the boundary between two blocks.
            // The split point is at the closing token of the left block.
            let rp = tr.doc.resolve(pos);
            let insert_pos = rp.after(rp.depth);

            let hr = Node::leaf(NodeType::HorizontalRule);
            if tr.insert(insert_pos, Fragment::from_node(hr)).is_ok() {
                self.push_undo();
                self.doc = tr.doc;
                // Place cursor at start of the block after the HR.
                self.selection = Selection::cursor(insert_pos + 2);
                self.redo_stack.clear();
            }
        } else {
            // Between blocks — just insert the HR.
            let hr = Node::leaf(NodeType::HorizontalRule);
            if tr.insert(pos, Fragment::from_node(hr)).is_ok() {
                self.push_undo();
                self.doc = tr.doc;
                self.selection = Selection::cursor(pos + 1);
                self.redo_stack.clear();
            }
        }
    }
}
