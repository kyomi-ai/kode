//! Clipboard operations: copy/paste support for markdown content.

use crate::fragment::Fragment;
use crate::node::Node;
use crate::node_type::NodeType;
use crate::parse::parse_markdown;
use crate::serialize::serialize_markdown;
use crate::slice::Slice;
use crate::transform::Transform;

use super::{DocState, Selection};

impl DocState {
    /// Serialize the selected content to markdown.
    ///
    /// Extracts the fragment between the selection's from/to positions,
    /// wraps it in a temporary Doc node, and serializes to markdown.
    /// Returns an empty string if the selection is collapsed (cursor).
    pub fn selected_markdown(&self) -> String {
        let from = self.selection.from();
        let to = self.selection.to();
        if from == to {
            return String::new();
        }
        let fragment = self.doc.content.cut(from, to);
        let temp_doc = Node::branch(NodeType::Doc, fragment);
        serialize_markdown(&temp_doc)
    }

    /// Insert structured content from a markdown string at the current cursor.
    ///
    /// Parses the markdown into a document tree, then splices it into the
    /// current document using a markdown-level splice: the current doc is
    /// serialized, the pasted markdown is inserted at the cursor's byte
    /// offset, and the result is re-parsed.
    ///
    /// This works reliably for headings, code blocks, blockquotes, and
    /// paragraphs. For list items (where byte offset mapping is approximate),
    /// the paste handler routes through [`insert_text_multiline`] instead.
    pub fn insert_from_markdown(&mut self, markdown: &str) {
        if markdown.is_empty() {
            return;
        }

        self.push_undo();

        // Delete selection if any.
        let delete_from = self.adjust_into_textblock(self.selection.from());
        let delete_to = self.adjust_into_textblock(self.selection.to());
        let cursor_pos = if delete_from != delete_to {
            let mut tr = Transform::new(self.doc.clone());
            if tr.delete(delete_from, delete_to).is_ok() {
                self.doc = tr.doc;
                let pos = delete_from.min(self.doc.content.size());
                let adjusted = self.adjust_into_textblock(pos);
                self.selection = Selection::cursor(adjusted);
                adjusted
            } else {
                delete_from
            }
        } else {
            delete_from
        };

        // Parse the pasted markdown into a document tree.
        let pasted_doc = parse_markdown(markdown);
        if pasted_doc.child_count() == 0 {
            return;
        }

        // If the pasted content is a single paragraph, insert its inline
        // content at the cursor position. Only paragraphs are inlined — other
        // textblock types (headings, code blocks) carry semantic block-level
        // meaning and must be inserted as blocks.
        if pasted_doc.child_count() == 1
            && pasted_doc.child(0).node_type == NodeType::Paragraph
        {
            let para = pasted_doc.child(0);
            let content = para.content.clone();
            let slice = Slice::new(content, 0, 0);
            let mut tr = Transform::new(self.doc.clone());
            if tr.replace(cursor_pos, cursor_pos, slice).is_ok() {
                let new_pos = cursor_pos + para.content.size();
                self.doc = tr.doc;
                self.selection = Selection::cursor(new_pos.min(self.doc.content.size()));
            }
            self.redo_stack.clear();
            return;
        }

        // Split the current block at the cursor so we can insert
        // the pasted blocks between the two halves.
        let resolved = self.doc.resolve(cursor_pos);
        let in_textblock = resolved.parent().node_type.is_textblock();
        let in_code = resolved.parent().node_type == NodeType::CodeBlock;

        // For code blocks, just insert as plain text.
        if in_code {
            let plain = pasted_doc.text_content();
            self.insert_text_inner(&plain);
            self.redo_stack.clear();
            return;
        }

        // Find the position between blocks to insert at.
        // If inside a textblock, split it first.
        let insert_pos = if in_textblock && resolved.parent_offset > 0
            && resolved.parent_offset < resolved.parent().content.size()
        {
            // Mid-block: split, then insert between halves.
            let mut tr = Transform::new(self.doc.clone());
            if tr.split(cursor_pos, 1).is_ok() {
                self.doc = tr.doc;
                cursor_pos + 1
            } else {
                cursor_pos
            }
        } else if in_textblock && resolved.parent_offset == resolved.parent().content.size() {
            resolved.after(resolved.depth)
        } else if in_textblock && resolved.parent_offset == 0 {
            resolved.before(resolved.depth)
        } else {
            cursor_pos
        };

        // Insert each block from the pasted document.
        let mut pos = insert_pos;
        for child in pasted_doc.content.iter() {
            let child_size = child.node_size();
            let fragment = Fragment::from_node(child.clone());
            let mut tr = Transform::new(self.doc.clone());
            if tr.insert(pos, fragment).is_ok() {
                self.doc = tr.doc;
                pos += child_size;
            }
        }

        // Place cursor at the end of inserted content.
        let max_pos = self.doc.content.size();
        let adjusted = self.adjust_into_textblock(pos.min(max_pos));
        self.selection = Selection::cursor(adjusted);
        self.redo_stack.clear();
    }

    /// Insert plain text with context-aware line handling.
    ///
    /// Unlike `insert_text` which treats the entire string as inline text,
    /// this method handles newlines based on the cursor context:
    /// - In a code block: inserts as-is (newlines are literal)
    /// - Elsewhere: each `\n` triggers a block split (new paragraph/list item)
    pub fn insert_text_multiline(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        self.push_undo();

        let from = self.adjust_into_textblock(self.selection.from());
        let resolved = self.doc.resolve(from);
        let parent_type = resolved.parent().node_type;

        // Code blocks: insert as-is, newlines are literal content.
        if parent_type == NodeType::CodeBlock {
            self.insert_text_inner(text);
            self.redo_stack.clear();
            return;
        }

        // Split on newlines — each line becomes a new block.
        let lines: Vec<&str> = text.split('\n').collect();
        for (i, line) in lines.iter().enumerate() {
            if !line.is_empty() {
                self.insert_text_inner(line);
            }
            if i < lines.len() - 1 {
                self.split_block_inner();
            }
        }

        self.redo_stack.clear();
    }

    /// Extract plain text between two positions in the document.
    pub fn text_between(&self, from: usize, to: usize) -> String {
        if from >= to {
            return String::new();
        }
        // Use the slice mechanism to extract content, then collect text
        let slice = self.doc.content.cut(from, to);
        fn collect_text(fragment: &Fragment, out: &mut String) {
            for child in fragment.iter() {
                if child.is_text() {
                    out.push_str(&child.text_content());
                } else {
                    collect_text(&child.content, out);
                    // Add newline between blocks
                    if child.node_type.is_block() && !out.is_empty() && !out.ends_with('\n') {
                        out.push('\n');
                    }
                }
            }
        }
        let mut result = String::new();
        collect_text(&slice, &mut result);
        result
    }
}
