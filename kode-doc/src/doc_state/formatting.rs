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
            (Some(depth), Some(lt)) if lt == target_list_type => {
                // Already in the target list type — extract only the current
                // item from the list, leaving other items intact.
                self.push_undo();
                let list_node = resolved.node(depth);
                let list_start = resolved.before(depth);
                let list_end = resolved.after(depth);

                let item_index = resolved.index(depth);
                let item_count = list_node.child_count();

                // Extract inner blocks of just the target item.
                let target_item = list_node.child(item_index);
                let mut inner_blocks: Vec<Node> = target_item.content.iter().cloned().collect();
                if inner_blocks.is_empty() {
                    inner_blocks.push(Node::branch(NodeType::Paragraph, Fragment::empty()));
                }

                let replacement = if item_count == 1 {
                    // Only item — just extract inner blocks.
                    Fragment::from_vec(inner_blocks)
                } else if item_index == 0 {
                    // First item — inner blocks + remaining list.
                    let remaining: Vec<Node> = (1..item_count)
                        .map(|i| list_node.child(i).clone())
                        .collect();
                    let new_list = Node::branch_with_attrs(
                        lt,
                        list_node.attrs.clone(),
                        Fragment::from_vec(remaining),
                    );
                    let mut nodes = inner_blocks;
                    nodes.push(new_list);
                    Fragment::from_vec(nodes)
                } else if item_index == item_count - 1 {
                    // Last item — preceding list + inner blocks.
                    let preceding: Vec<Node> = (0..item_index)
                        .map(|i| list_node.child(i).clone())
                        .collect();
                    let new_list = Node::branch_with_attrs(
                        lt,
                        list_node.attrs.clone(),
                        Fragment::from_vec(preceding),
                    );
                    let mut nodes = vec![new_list];
                    nodes.extend(inner_blocks);
                    Fragment::from_vec(nodes)
                } else {
                    // Middle item — split into two lists with inner blocks between.
                    let before_items: Vec<Node> = (0..item_index)
                        .map(|i| list_node.child(i).clone())
                        .collect();
                    let after_items: Vec<Node> = ((item_index + 1)..item_count)
                        .map(|i| list_node.child(i).clone())
                        .collect();
                    let list_before = Node::branch_with_attrs(
                        lt,
                        list_node.attrs.clone(),
                        Fragment::from_vec(before_items),
                    );
                    let list_after = Node::branch_with_attrs(
                        lt,
                        list_node.attrs.clone(),
                        Fragment::from_vec(after_items),
                    );
                    let mut nodes = vec![list_before];
                    nodes.extend(inner_blocks);
                    nodes.push(list_after);
                    Fragment::from_vec(nodes)
                };

                let slice = Slice::new(replacement, 0, 0);
                let mut tr = Transform::new(self.doc.clone());
                if tr.replace(list_start, list_end, slice).is_ok() {
                    // Cursor position adjustment:
                    // - First/only item (index 0): cursor shifts -2
                    //   (list_open + li_open removed before content)
                    // - Middle/last item: cursor stays same
                    //   (li_open replaced by list_close at the same position)
                    let shift: isize = if item_index == 0 { -2 } else { 0 };
                    let new_anchor = (self.selection.anchor as isize + shift).max(0) as usize;
                    let new_head = (self.selection.head as isize + shift).max(0) as usize;
                    self.doc = tr.doc;
                    let max = self.doc.content.size();
                    self.selection = Selection::range(new_anchor.min(max), new_head.min(max));
                    self.redo_stack.clear();
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

    /// Insert a table with the given dimensions at the cursor position.
    ///
    /// `cols` is the number of columns. `rows` is the total number of rows
    /// including the header row (so `rows = 2` means 1 header + 1 body row).
    ///
    /// If the cursor is inside a textblock, splits the block first, then
    /// inserts the table between the two halves.
    pub fn insert_table(&mut self, cols: usize, rows: usize) {
        let cols = cols.max(1);
        let rows = rows.max(1);
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

        fn make_empty_cell() -> Node {
            Node::branch(NodeType::TableCell, Fragment::empty())
        }

        fn make_row(cols: usize, row_type: NodeType) -> Node {
            let cells: Vec<Node> = (0..cols).map(|_| make_empty_cell()).collect();
            Node::branch(row_type, Fragment::from_vec(cells))
        }

        let mut table_rows = vec![make_row(cols, NodeType::TableHeader)];
        for _ in 1..rows {
            table_rows.push(make_row(cols, NodeType::TableRow));
        }
        let table = Node::branch(
            NodeType::Table,
            Fragment::from_vec(table_rows),
        );

        if resolved.parent().node_type.is_textblock() {
            // Inside a textblock: split the block, insert table between halves.
            if tr.split(pos, 1).is_err() {
                return;
            }
            // After split, the cursor is at the boundary between two blocks.
            // The split point is at the closing token of the left block.
            let rp = tr.doc.resolve(pos);
            let insert_pos = rp.after(rp.depth);

            if tr.insert(insert_pos, Fragment::from_node(table)).is_ok() {
                self.push_undo();
                self.doc = tr.doc;
                // Place cursor inside the first header cell.
                // insert_pos + 3: table open + header open + first cell open = 3 tokens deep.
                self.selection = Selection::cursor(insert_pos + 3);
                self.redo_stack.clear();
            }
        } else {
            // Between blocks — just insert the table.
            if tr.insert(pos, Fragment::from_node(table)).is_ok() {
                self.push_undo();
                self.doc = tr.doc;
                // Place cursor inside the first header cell.
                // pos + 3: table open + header open + first cell open = 3 tokens deep.
                self.selection = Selection::cursor(pos + 3);
                self.redo_stack.clear();
            }
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

    /// Insert a block-level leaf node (e.g., upload placeholder) at the given
    /// position. If the position is inside a textblock, inserts after the
    /// containing block. If between blocks, inserts directly.
    pub fn insert_block_node(&mut self, pos: usize, node: Node) {
        self.push_undo();

        let resolved = self.doc.resolve(pos);

        let insert_pos = if resolved.parent().node_type.is_textblock() {
            // Inside a textblock — insert after it.
            resolved.after(resolved.depth)
        } else {
            // At a gap between blocks — insert directly.
            pos
        };

        let mut tr = Transform::new(self.doc.clone());
        if tr.insert(insert_pos, Fragment::from_node(node)).is_ok() {
            self.doc = tr.doc;
            // Place cursor after the inserted node.
            self.selection = Selection::cursor(insert_pos + 1);
        }
        self.redo_stack.clear();
    }

    /// Atomically replace a block-level leaf node at `pos` with `replacement`.
    /// This is a single undo entry (delete + insert together).
    pub fn replace_block_node(&mut self, pos: usize, replacement: Node) {
        self.push_undo();
        let mut tr = Transform::new(self.doc.clone());
        if tr.delete(pos, pos + 1).is_ok()
            && tr.insert(pos, Fragment::from_node(replacement)).is_ok()
        {
            self.doc = tr.doc;
            self.selection = Selection::cursor(pos + 1);
        }
        self.redo_stack.clear();
    }

    // ── Link query / mutation ────────────────────────────────────────

    /// Returns `(href, link_from, link_to)` if the cursor is inside a link
    /// mark, or `None` if there is no link at the cursor position.
    pub fn link_at_cursor(&self) -> Option<(String, usize, usize)> {
        let resolved = self.doc.resolve(self.selection.head);

        // Get marks at cursor, with node_before fallback (same pattern as
        // formatting_at_cursor in selection.rs).
        let marks = {
            let m = resolved.marks();
            if m.is_empty() {
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

        // Find the Link mark and extract href.
        let link_mark = marks.iter().find(|m| m.mark_type == MarkType::Link)?;
        let href = match crate::attrs::get_attr(&link_mark.attrs, "href") {
            Some(crate::attrs::AttrValue::String(s)) => s.clone(),
            _ => return None,
        };

        // Walk text nodes in the parent textblock to find the contiguous
        // range that shares the same Link mark with matching href.
        let parent = resolved.parent();
        let parent_start = resolved.start(resolved.depth);
        let mut link_from = None;
        let mut link_to = None;
        let mut pos = parent_start;

        for child in parent.content.iter() {
            let child_end = pos + child.node_size();
            let has_matching_link = child.is_text()
                && child.marks.iter().any(|m| {
                    m.mark_type == MarkType::Link
                        && matches!(
                            crate::attrs::get_attr(&m.attrs, "href"),
                            Some(crate::attrs::AttrValue::String(s)) if *s == href
                        )
                });
            if has_matching_link {
                if link_from.is_none() {
                    link_from = Some(pos);
                }
                link_to = Some(child_end);
            } else if link_from.is_some() {
                // Past the contiguous link range; stop scanning.
                break;
            }
            pos = child_end;
        }

        match (link_from, link_to) {
            (Some(from), Some(to)) => Some((href, from, to)),
            _ => None,
        }
    }

    /// Updates the link mark's href in the range `[from, to)`.
    pub fn update_link(&mut self, from: usize, to: usize, new_url: &str) {
        let mut tr = Transform::new(self.doc.clone());
        if tr.remove_mark(from, to, Mark::new(MarkType::Link)).is_ok() {
            let new_mark =
                Mark::with_attrs(MarkType::Link, crate::attrs::link_attrs(new_url, None));
            if tr.add_mark(from, to, new_mark).is_ok() {
                self.push_undo();
                self.doc = tr.doc;
                self.redo_stack.clear();
            }
        }
    }

    /// Removes the link mark from the range `[from, to)`.
    pub fn remove_link(&mut self, from: usize, to: usize) {
        let mut tr = Transform::new(self.doc.clone());
        if tr.remove_mark(from, to, Mark::new(MarkType::Link)).is_ok() {
            self.push_undo();
            self.doc = tr.doc;
            self.redo_stack.clear();
        }
    }

    /// Check if typing a space at the current cursor would complete a markdown
    /// list marker at the start of the current text block, and if so convert
    /// the block into the corresponding list.
    ///
    /// Returns `true` if a conversion was performed (caller should
    /// `preventDefault` the space), `false` otherwise.
    ///
    /// Recognized markers:
    /// - `"-"` or `"*"` at position 0 of a textblock → bullet list
    /// - Digits followed by `"."` (e.g. `"1."`, `"12."`) → ordered list
    ///   with the parsed start number
    pub fn try_auto_convert_list_on_space(&mut self) -> bool {
        use crate::attrs::ordered_list_attrs;

        let head = self.selection.head;
        let resolved = self.doc.resolve(head);

        // Must be inside a textblock.
        let parent = resolved.parent();
        if !parent.node_type.is_textblock() {
            return false;
        }

        let block_start = resolved.start(resolved.depth);
        let cursor_in_block = head - block_start;

        // Upper bound: no valid marker is longer than 10 chars, and must be
        // at least 1 char.
        if cursor_in_block == 0 || cursor_in_block > 10 {
            return false;
        }

        // Get text content before the cursor within this block.
        let text_before: String = parent
            .text_content()
            .chars()
            .take(cursor_in_block)
            .collect();

        // Determine what list type the marker maps to.
        let (target_type, target_attrs) = if text_before == "-" || text_before == "*" {
            (NodeType::BulletList, Attrs::new())
        } else if text_before.ends_with('.') {
            let digits = &text_before[..text_before.len() - 1];
            if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                let start_num: i64 = digits.parse().unwrap_or(1);
                (NodeType::OrderedList, ordered_list_attrs(start_num))
            } else {
                return false;
            }
        } else {
            return false;
        };

        // Walk ancestors to see if we're already inside a list.
        let mut existing_list_depth = None;
        let mut existing_list_type = None;
        for d in (1..=resolved.depth).rev() {
            let nt = resolved.node(d).node_type;
            if nt == NodeType::BulletList || nt == NodeType::OrderedList {
                existing_list_depth = Some(d);
                existing_list_type = Some(nt);
                break;
            }
        }

        // If already inside the same list type, don't convert (no-op).
        if existing_list_type == Some(target_type) {
            return false;
        }

        // ── Perform both transforms on scratch docs, commit only on success ──

        // Step 1: Delete the marker text from the paragraph.
        let delete_from = block_start;
        let delete_to = block_start + cursor_in_block;
        let mut tr = Transform::new(self.doc.clone());
        if tr.delete(delete_from, delete_to).is_err() {
            return false;
        }
        let after_delete = tr.doc;
        let new_head = head - cursor_in_block;

        // Step 2: Wrap in list or change list type on the post-delete doc.
        let (final_doc, final_selection) = match existing_list_depth {
            Some(depth) => {
                let re_resolved = after_delete.resolve(new_head);
                let list_node = re_resolved.node(depth);
                let list_start = re_resolved.before(depth);
                let list_end = re_resolved.after(depth);
                let new_list = Node::branch_with_attrs(
                    target_type,
                    target_attrs,
                    list_node.content.clone(),
                );
                let mut tr2 = Transform::new(after_delete);
                if tr2
                    .replace(
                        list_start,
                        list_end,
                        Slice::new(Fragment::from_node(new_list), 0, 0),
                    )
                    .is_err()
                {
                    return false;
                }
                (tr2.doc, Selection::cursor(new_head))
            }
            None => {
                let mut tr2 = Transform::new(after_delete);
                if tr2.wrap_in_list(new_head, new_head, target_type, target_attrs).is_err() {
                    return false;
                }
                let wrapped_head = new_head + 2;
                let max = tr2.doc.content.size();
                (tr2.doc, Selection::cursor(wrapped_head.min(max)))
            }
        };

        // Both transforms succeeded — commit atomically.
        self.push_undo();
        self.doc = final_doc;
        self.selection = final_selection;
        self.redo_stack.clear();
        true
    }
}
