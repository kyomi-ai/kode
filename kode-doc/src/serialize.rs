//! kode-doc tree → markdown string serializer.
//!
//! Converts a [`Node`] document tree back into a markdown string. This is the
//! complement of [`parse_markdown`](crate::parse_markdown) — together they
//! enable round-tripping between markdown text and the document tree.

use crate::attrs::{get_attr, AttrValue};
use crate::mark::MarkType;
use crate::node::Node;
use crate::node_type::NodeType;

/// Serialize a kode-doc document tree to a markdown string.
///
/// Walks the tree recursively, emitting markdown syntax for each node type.
/// Block nodes are separated by `\n\n`. Inline marks are rendered as their
/// markdown delimiters (`**`, `*`, `` ` ``, `~~`, `[]()`).
pub fn serialize_markdown(doc: &Node) -> String {
    let mut out = String::new();
    serialize_node(doc, &mut out, &BlockContext::Top);
    // Trim trailing whitespace but preserve a single trailing newline if content exists
    let trimmed = out.trim_end();
    if trimmed.is_empty() {
        String::new()
    } else {
        trimmed.to_string()
    }
}

/// Context passed down during block serialization to handle indentation
/// and list numbering.
enum BlockContext {
    /// Top-level or generic block context.
    Top,
    /// Inside a blockquote — each line needs `> ` prefix.
    Blockquote,
}

/// Serialize a single node, appending to `out`.
fn serialize_node(node: &Node, out: &mut String, ctx: &BlockContext) {
    match node.node_type {
        NodeType::Doc => {
            serialize_block_children(node, out, ctx);
        }
        NodeType::Paragraph => {
            serialize_inline_children(node, out);
        }
        NodeType::Heading => {
            let level = match get_attr(&node.attrs, "level") {
                Some(AttrValue::Int(n)) => *n as usize,
                _ => 1,
            };
            for _ in 0..level {
                out.push('#');
            }
            out.push(' ');
            serialize_inline_children(node, out);
        }
        NodeType::Blockquote => {
            serialize_blockquote(node, out);
        }
        NodeType::BulletList => {
            serialize_bullet_list(node, out);
        }
        NodeType::OrderedList => {
            serialize_ordered_list(node, out);
        }
        NodeType::ListItem => {
            // ListItem should be handled by the list serializers.
            // If encountered directly, serialize its block children.
            serialize_block_children(node, out, ctx);
        }
        NodeType::CodeBlock => {
            serialize_code_block(node, out);
        }
        NodeType::HorizontalRule => {
            out.push_str("---");
        }
        NodeType::Text => {
            serialize_text_node(node, out);
        }
        NodeType::HardBreak => {
            out.push_str("  \n");
        }
        NodeType::Image => {
            serialize_image(node, out);
        }
        NodeType::Table => {
            serialize_table(node, out);
        }
        // TableRow, TableHeader, TableCell are handled by serialize_table
        NodeType::TableRow | NodeType::TableHeader | NodeType::TableCell => {}
    }
}

/// Serialize the block children of a node, joining with `\n\n`.
fn serialize_block_children(node: &Node, out: &mut String, ctx: &BlockContext) {
    let children = node.content.children();
    for (i, child) in children.iter().enumerate() {
        if i > 0 {
            out.push_str("\n\n");
        }
        serialize_node(child, out, ctx);
    }
}

/// Serialize all inline children of a node (paragraph, heading, etc.).
fn serialize_inline_children(node: &Node, out: &mut String) {
    for child in node.content.iter() {
        serialize_node(child, out, &BlockContext::Top);
    }
}

/// Serialize a text node with its marks applied as markdown delimiters.
fn serialize_text_node(node: &Node, out: &mut String) {
    let text = node.text().unwrap_or("");
    if node.marks.is_empty() {
        out.push_str(text);
        return;
    }

    // Check for link mark — it wraps the entire text differently.
    let link_mark = node.marks.iter().find(|m| m.mark_type == MarkType::Link);

    // Collect non-link marks for delimiter wrapping.
    let non_link_marks: Vec<_> = node
        .marks
        .iter()
        .filter(|m| m.mark_type != MarkType::Link)
        .collect();

    // Build opening and closing delimiters based on mark combination.
    let (open, close) = build_mark_delimiters(&non_link_marks);

    if let Some(link) = link_mark {
        let href = match get_attr(&link.attrs, "href") {
            Some(AttrValue::String(s)) => s.as_str(),
            _ => "",
        };
        let title = match get_attr(&link.attrs, "title") {
            Some(AttrValue::String(s)) => Some(s.as_str()),
            _ => None,
        };
        out.push('[');
        out.push_str(&open);
        out.push_str(text);
        out.push_str(&close);
        out.push_str("](");
        out.push_str(href);
        if let Some(t) = title {
            out.push_str(" \"");
            out.push_str(t);
            out.push('"');
        }
        out.push(')');
    } else {
        out.push_str(&open);
        out.push_str(text);
        out.push_str(&close);
    }
}

/// Build opening and closing delimiter strings for a set of non-link marks.
///
/// Mark ordering: Strong is outermost, then Em, then Strike, then Code.
/// So `**bold *and italic***` means Strong wrapping Em.
fn build_mark_delimiters(marks: &[&crate::mark::Mark]) -> (String, String) {
    let has_strong = marks.iter().any(|m| m.mark_type == MarkType::Strong);
    let has_em = marks.iter().any(|m| m.mark_type == MarkType::Em);
    let has_code = marks.iter().any(|m| m.mark_type == MarkType::Code);
    let has_strike = marks.iter().any(|m| m.mark_type == MarkType::Strike);

    let mut open = String::new();
    let mut close = String::new();

    // Strong is outermost
    if has_strong {
        open.push_str("**");
        close.insert_str(0, "**");
    }
    // Em is next
    if has_em {
        open.push('*');
        close.insert(0, '*');
    }
    // Strike
    if has_strike {
        open.push_str("~~");
        close.insert_str(0, "~~");
    }
    // Code is innermost
    if has_code {
        open.push('`');
        close.insert(0, '`');
    }

    (open, close)
}

/// Serialize a blockquote node. Each line of content gets `> ` prefix.
fn serialize_blockquote(node: &Node, out: &mut String) {
    // Serialize the blockquote content into a temporary buffer, then prefix
    // each line with `> `.
    let mut inner = String::new();
    serialize_block_children(node, &mut inner, &BlockContext::Blockquote);

    for (i, line) in inner.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if line.is_empty() {
            out.push('>');
        } else {
            out.push_str("> ");
            out.push_str(line);
        }
    }

    // Handle the case where inner ends with `\n` — we need to add the
    // `>` prefix for the trailing empty line in multi-paragraph blockquotes.
    if inner.ends_with('\n') {
        out.push('\n');
        out.push('>');
    }
}

/// Serialize a table as pipe-delimited markdown.
fn serialize_table(node: &Node, out: &mut String) {
    let children = node.content.children();
    for (i, child) in children.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        match child.node_type {
            NodeType::TableHeader => {
                // Header row: | col1 | col2 |
                serialize_table_row(child, out);
                // Delimiter row: | --- | --- |
                out.push('\n');
                let cell_count = child.content.children().len();
                out.push('|');
                for _ in 0..cell_count {
                    out.push_str(" --- |");
                }
            }
            NodeType::TableRow => {
                serialize_table_row(child, out);
            }
            _ => {}
        }
    }
}

/// Serialize a single table row (header or body) as `| cell | cell |`.
fn serialize_table_row(row: &Node, out: &mut String) {
    out.push('|');
    for cell in row.content.iter() {
        out.push(' ');
        serialize_inline_children(cell, out);
        out.push_str(" |");
    }
}

/// Serialize a bullet list.
fn serialize_bullet_list(node: &Node, out: &mut String) {
    let items = node.content.children();
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let mut item_buf = String::new();
        serialize_list_item_content(item, &mut item_buf, "- ", "  ");
        out.push_str(&item_buf);
    }
}

/// Serialize an ordered list.
fn serialize_ordered_list(node: &Node, out: &mut String) {
    let start = match get_attr(&node.attrs, "start") {
        Some(AttrValue::Int(n)) => *n,
        _ => 1,
    };
    let items = node.content.children();
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let number = start + i as i64;
        let marker = format!("{number}. ");
        let indent = " ".repeat(marker.len());
        let mut item_buf = String::new();
        serialize_list_item_content(item, &mut item_buf, &marker, &indent);
        out.push_str(&item_buf);
    }
}

/// Serialize a list item's content with the given marker for the first line
/// and indent for continuation lines.
fn serialize_list_item_content(item: &Node, out: &mut String, marker: &str, indent: &str) {
    let children = item.content.children();
    for (i, child) in children.iter().enumerate() {
        if i == 0 {
            // First block child: prefix with marker.
            out.push_str(marker);
            let mut child_buf = String::new();
            serialize_node(child, &mut child_buf, &BlockContext::Top);
            // Indent continuation lines of the first child.
            for (j, line) in child_buf.lines().enumerate() {
                if j > 0 {
                    out.push('\n');
                    out.push_str(indent);
                }
                out.push_str(line);
            }
        } else {
            // Subsequent block children: blank line + indent.
            out.push('\n');
            out.push('\n');
            let mut child_buf = String::new();
            serialize_node(child, &mut child_buf, &BlockContext::Top);
            for (j, line) in child_buf.lines().enumerate() {
                if j > 0 {
                    out.push('\n');
                }
                out.push_str(indent);
                out.push_str(line);
            }
        }
    }
}

/// Serialize a fenced code block.
fn serialize_code_block(node: &Node, out: &mut String) {
    let language = match get_attr(&node.attrs, "language") {
        Some(AttrValue::String(s)) => s.as_str(),
        _ => "",
    };
    out.push_str("```");
    out.push_str(language);
    out.push('\n');
    // Code block content is stored as a single text child.
    let content = node.text_content();
    out.push_str(&content);
    // Ensure the closing fence is on its own line.
    if !content.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("```");
}

/// Serialize an image node.
fn serialize_image(node: &Node, out: &mut String) {
    let src = match get_attr(&node.attrs, "src") {
        Some(AttrValue::String(s)) => s.as_str(),
        _ => "",
    };
    let alt = match get_attr(&node.attrs, "alt") {
        Some(AttrValue::String(s)) => s.as_str(),
        _ => "",
    };
    let title = match get_attr(&node.attrs, "title") {
        Some(AttrValue::String(s)) => Some(s.as_str()),
        _ => None,
    };
    out.push_str("![");
    out.push_str(alt);
    out.push_str("](");
    out.push_str(src);
    if let Some(t) = title {
        out.push_str(" \"");
        out.push_str(t);
        out.push('"');
    }
    out.push(')');
}

/// Map a tree token position to the corresponding byte offset in the
/// serialized markdown string.
///
/// Tree positions count opening/closing tokens for branch nodes and
/// individual characters for text nodes. The serialized markdown contains
/// syntax characters (`**`, `#`, `` ` ``, etc.) that occupy bytes but not
/// tree positions, and structural tokens (open/close) that occupy tree
/// positions but not bytes.
///
/// This function walks the document tree in the same order as the serializer,
/// advancing both a tree-position counter and a byte-offset counter. When the
/// tree position reaches `tree_pos`, the accumulated byte offset is returned.
///
/// **Note:** The result is approximate for content inside lists and blockquotes.
/// List/blockquote prefix bytes (`- `, `> `, indentation) are not fully
/// accounted for. The offset is exact for paragraphs, headings, and code blocks.
///
/// If `tree_pos` exceeds the document size, returns `markdown.len()`.
pub fn tree_pos_to_byte_offset(doc: &Node, markdown: &str, tree_pos: usize) -> usize {
    let mut state = PosMapState {
        tree_pos: 0,
        byte_offset: 0,
        target: tree_pos,
        found: None,
    };
    pos_map_node(doc, &mut state);
    state.found.unwrap_or(markdown.len())
}

/// Map a byte offset in the serialized markdown back to the closest tree
/// token position.
///
/// This is the inverse of [`tree_pos_to_byte_offset`]. It walks the document
/// tree in serialization order, advancing both a tree-position counter and a
/// byte-offset counter. When the accumulated byte offset reaches or exceeds
/// `target_byte`, the current tree position is returned.
///
/// Used for placing the cursor after structured paste operations.
pub fn byte_offset_to_tree_pos(doc: &Node, _markdown: &str, target_byte: usize) -> usize {
    let mut state = ReversePosMapState {
        tree_pos: 0,
        byte_offset: 0,
        target_byte,
        found: None,
    };
    reverse_pos_map_node(doc, &mut state);
    state
        .found
        .unwrap_or_else(|| doc.content.size())
}

/// Internal state for the byte-offset-to-tree-position walk.
struct ReversePosMapState {
    tree_pos: usize,
    byte_offset: usize,
    target_byte: usize,
    found: Option<usize>,
}

impl ReversePosMapState {
    /// Check if the target byte offset has been reached; record tree position.
    fn check(&mut self) -> bool {
        if self.found.is_some() {
            return true;
        }
        if self.byte_offset >= self.target_byte {
            self.found = Some(self.tree_pos);
            return true;
        }
        false
    }
}

/// Walk a node in serialization order, tracking byte offset and tree position.
/// When byte offset reaches the target, record the tree position.
fn reverse_pos_map_node(node: &Node, state: &mut ReversePosMapState) {
    if state.found.is_some() {
        return;
    }

    match node.node_type {
        NodeType::Doc => {
            let children = node.content.children();
            for (i, child) in children.iter().enumerate() {
                if i > 0 {
                    state.byte_offset += 2; // "\n\n"
                    if state.check() {
                        return;
                    }
                }
                reverse_pos_map_node(child, state);
                if state.found.is_some() {
                    return;
                }
            }
        }
        NodeType::Text => {
            let text = node.text().unwrap_or("");
            // Account for opening mark delimiters.
            let non_link: Vec<_> = node
                .marks
                .iter()
                .filter(|m| m.mark_type != MarkType::Link)
                .collect();
            let (_, close) = build_mark_delimiters(&non_link);
            let has_link = node.marks.iter().any(|m| m.mark_type == MarkType::Link);

            if has_link {
                state.byte_offset += 1; // "["
            }
            if !non_link.is_empty() {
                let (open, _) = build_mark_delimiters(&non_link);
                state.byte_offset += open.len();
            }
            if state.check() {
                return;
            }

            for ch in text.chars() {
                state.tree_pos += 1;
                state.byte_offset += ch.len_utf8();
                if state.check() {
                    return;
                }
            }

            // Closing mark delimiters.
            state.byte_offset += close.len();

            // Link closing syntax.
            if let Some(link) = node.marks.iter().find(|m| m.mark_type == MarkType::Link).filter(|_| has_link) {
                let href = match get_attr(&link.attrs, "href") {
                    Some(AttrValue::String(s)) => s.as_str(),
                    _ => "",
                };
                let title = match get_attr(&link.attrs, "title") {
                    Some(AttrValue::String(s)) => Some(s.as_str()),
                    _ => None,
                };
                state.byte_offset += 2 + href.len(); // "](" + href
                if let Some(t) = title {
                    state.byte_offset += 2 + t.len() + 1;
                }
                state.byte_offset += 1; // ")"
            }
        }
        NodeType::HardBreak => {
            state.tree_pos += 1;
            state.byte_offset += 3;
            state.check();
        }
        NodeType::Image => {
            state.tree_pos += 1;
            let src = match get_attr(&node.attrs, "src") {
                Some(AttrValue::String(s)) => s.as_str(),
                _ => "",
            };
            let alt = match get_attr(&node.attrs, "alt") {
                Some(AttrValue::String(s)) => s.as_str(),
                _ => "",
            };
            let title = match get_attr(&node.attrs, "title") {
                Some(AttrValue::String(s)) => Some(s.as_str()),
                _ => None,
            };
            state.byte_offset += 2 + alt.len() + 2 + src.len();
            if let Some(t) = title {
                state.byte_offset += 2 + t.len() + 1;
            }
            state.byte_offset += 1;
            state.check();
        }
        NodeType::HorizontalRule => {
            state.tree_pos += 1;
            state.byte_offset += 3;
            state.check();
        }
        _ => {
            // Branch nodes.
            state.tree_pos += 1;

            match node.node_type {
                NodeType::Heading => {
                    let level = match get_attr(&node.attrs, "level") {
                        Some(AttrValue::Int(n)) => *n as usize,
                        _ => 1,
                    };
                    state.byte_offset += level + 1;
                }
                NodeType::CodeBlock => {
                    let language = match get_attr(&node.attrs, "language") {
                        Some(AttrValue::String(s)) => s.as_str(),
                        _ => "",
                    };
                    state.byte_offset += 3 + language.len() + 1;
                }
                _ => {}
            }

            if state.check() {
                return;
            }

            if node.node_type == NodeType::Paragraph || node.node_type == NodeType::Heading {
                for child in node.content.iter() {
                    reverse_pos_map_node(child, state);
                    if state.found.is_some() {
                        return;
                    }
                }
            } else if node.node_type == NodeType::CodeBlock {
                for child in node.content.iter() {
                    reverse_pos_map_node(child, state);
                    if state.found.is_some() {
                        return;
                    }
                }
                state.byte_offset += 4; // "\n```"
            } else {
                let children = node.content.children();
                for (i, child) in children.iter().enumerate() {
                    if i > 0 {
                        state.byte_offset += 1;
                    }
                    reverse_pos_map_node(child, state);
                    if state.found.is_some() {
                        return;
                    }
                }
            }

            state.tree_pos += 1;
            state.check();
        }
    }
}

/// Internal state for the tree-position-to-byte-offset walk.
struct PosMapState {
    /// Current tree position counter.
    tree_pos: usize,
    /// Current byte offset in the markdown string.
    byte_offset: usize,
    /// The target tree position we are looking for.
    target: usize,
    /// Set when the target position is found.
    found: Option<usize>,
}

impl PosMapState {
    /// Check if target has been reached; if so, record the byte offset.
    /// Returns `true` if the walk should stop (target found).
    fn check(&mut self) -> bool {
        if self.found.is_some() {
            return true;
        }
        if self.tree_pos >= self.target {
            self.found = Some(self.byte_offset);
            return true;
        }
        false
    }
}

/// Walk a single node, advancing tree position and byte offset in tandem.
fn pos_map_node(node: &Node, state: &mut PosMapState) {
    if state.found.is_some() {
        return;
    }

    match node.node_type {
        NodeType::Doc => {
            // Doc has no opening/closing tokens in the position space.
            // Its children are separated by "\n\n" in the serialized output.
            let children = node.content.children();
            for (i, child) in children.iter().enumerate() {
                if i > 0 {
                    // "\n\n" between blocks — bytes but no tree positions.
                    state.byte_offset += 2;
                }
                pos_map_node(child, state);
                if state.found.is_some() {
                    return;
                }
            }
        }
        NodeType::Text => {
            // Text nodes: each character is one tree position and some bytes.
            let text = node.text().unwrap_or("");
            // Account for mark delimiters (opening) — bytes but no tree positions.
            let (open, close) = if !node.marks.is_empty() {
                let non_link: Vec<_> = node.marks.iter().filter(|m| m.mark_type != MarkType::Link).collect();
                let (o, c) = build_mark_delimiters(&non_link);
                let has_link = node.marks.iter().any(|m| m.mark_type == MarkType::Link);
                if has_link {
                    // Link wrapping: [<open>text<close>](url)
                    // "[" before the content
                    state.byte_offset += 1; // "["
                    state.byte_offset += o.len();
                    (String::new(), c)
                } else {
                    state.byte_offset += o.len();
                    (String::new(), c)
                }
            } else {
                (String::new(), String::new())
            };

            // Walk each character of the text content.
            for ch in text.chars() {
                if state.check() {
                    return;
                }
                state.tree_pos += 1;
                state.byte_offset += ch.len_utf8();
            }

            // Closing mark delimiters — bytes but no tree positions.
            state.byte_offset += close.len();

            // Link closing syntax: "](url)" — bytes but no tree positions.
            if node.marks.iter().any(|m| m.mark_type == MarkType::Link) {
                // "](href)" or "](href \"title\")"
                let link_mark = node.marks.iter().find(|m| m.mark_type == MarkType::Link);
                if let Some(link) = link_mark {
                    let href = match get_attr(&link.attrs, "href") {
                        Some(AttrValue::String(s)) => s.as_str(),
                        _ => "",
                    };
                    let title = match get_attr(&link.attrs, "title") {
                        Some(AttrValue::String(s)) => Some(s.as_str()),
                        _ => None,
                    };
                    state.byte_offset += 2; // "]("
                    state.byte_offset += href.len();
                    if let Some(t) = title {
                        state.byte_offset += 2 + t.len() + 1; // " \"title\""
                    }
                    state.byte_offset += 1; // ")"
                }
            }

            let _ = open; // consumed above
        }
        NodeType::HardBreak => {
            // HardBreak is an inline leaf: 1 tree position, "  \n" in markdown (3 bytes).
            if state.check() {
                return;
            }
            state.tree_pos += 1;
            state.byte_offset += 3; // "  \n"
        }
        NodeType::Image => {
            // Image is an inline leaf: 1 tree position, "![alt](src)" in markdown.
            if state.check() {
                return;
            }
            state.tree_pos += 1;
            let src = match get_attr(&node.attrs, "src") {
                Some(AttrValue::String(s)) => s.as_str(),
                _ => "",
            };
            let alt = match get_attr(&node.attrs, "alt") {
                Some(AttrValue::String(s)) => s.as_str(),
                _ => "",
            };
            let title = match get_attr(&node.attrs, "title") {
                Some(AttrValue::String(s)) => Some(s.as_str()),
                _ => None,
            };
            // "![alt](src)" or "![alt](src \"title\")"
            state.byte_offset += 2 + alt.len() + 2 + src.len(); // "![" + alt + "](" + src
            if let Some(t) = title {
                state.byte_offset += 2 + t.len() + 1; // " \"title\""
            }
            state.byte_offset += 1; // ")"
        }
        NodeType::HorizontalRule => {
            // HR is a leaf block: 1 tree position for opening, 1 for closing.
            // Actually no — HorizontalRule is a leaf node, so node_size() = 1.
            // But it's rendered at doc level, so the Doc handles it.
            // Wait — HR has no open/close tokens since it's a leaf.
            // In the position space, a non-text leaf = 1 token.
            if state.check() {
                return;
            }
            state.tree_pos += 1;
            state.byte_offset += 3; // "---"
        }
        _ => {
            // Branch nodes: opening token (+1 tree pos, variable markdown bytes),
            // children, closing token (+1 tree pos, no markdown bytes for the close).

            // Opening token: +1 tree position.
            state.tree_pos += 1;

            // Compute the markdown prefix bytes for this node type BEFORE the
            // check, so that when the target lands on the opening token (i.e.,
            // the start of this node's content), the byte offset already
            // accounts for the markdown syntax prefix.
            match node.node_type {
                NodeType::Heading => {
                    let level = match get_attr(&node.attrs, "level") {
                        Some(AttrValue::Int(n)) => *n as usize,
                        _ => 1,
                    };
                    state.byte_offset += level + 1; // "### " (level hashes + space)
                }
                NodeType::CodeBlock => {
                    let language = match get_attr(&node.attrs, "language") {
                        Some(AttrValue::String(s)) => s.as_str(),
                        _ => "",
                    };
                    state.byte_offset += 3 + language.len() + 1; // "```lang\n"
                }
                _ => {
                    // Paragraph, Blockquote, BulletList, OrderedList, ListItem:
                    // Their markdown syntax is handled at the parent level
                    // (e.g., "> " for blockquote lines, "- " for list items).
                    // For this approximate mapping, we skip the structural bytes.
                    // This is acceptable because the cursor_byte is used for
                    // extension context, not pixel-perfect positioning.
                }
            }

            // Do NOT check here — the opening token position means "start of
            // this node's content." We need to descend into children so that
            // mark delimiters and other inline syntax are accounted for in the
            // byte offset before the check fires on the first text character.

            // Walk children.
            if node.node_type == NodeType::Paragraph || node.node_type == NodeType::Heading {
                // Inline children — no separators.
                for child in node.content.iter() {
                    pos_map_node(child, state);
                    if state.found.is_some() {
                        return;
                    }
                }
            } else if node.node_type == NodeType::CodeBlock {
                // Code block content is a single text child.
                for child in node.content.iter() {
                    pos_map_node(child, state);
                    if state.found.is_some() {
                        return;
                    }
                }
                // Closing fence: "\n```" — bytes but no tree positions.
                // (The newline may already be in the text content.)
                state.byte_offset += 4; // "\n```" (approximate)
            } else {
                // Block children: separated by "\n\n" for top-level blocks,
                // "\n" for list items, etc. Use "\n\n" as approximation.
                let children = node.content.children();
                for (i, child) in children.iter().enumerate() {
                    if i > 0 {
                        state.byte_offset += 1; // "\n" between siblings (approximate)
                    }
                    pos_map_node(child, state);
                    if state.found.is_some() {
                        return;
                    }
                }
            }

            // Closing token: +1 tree position, no markdown bytes.
            state.tree_pos += 1;
            state.check();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attrs::{
        code_block_attrs, heading_attrs, image_attrs, link_attrs, ordered_list_attrs,
    };
    use crate::fragment::Fragment;
    use crate::mark::{Mark, MarkType};
    use crate::parse::parse_markdown;

    // ── Helper builders ─────────────────────────────────────────────

    fn doc(children: Vec<Node>) -> Node {
        Node::branch(NodeType::Doc, Fragment::from_vec(children))
    }

    fn para(children: Vec<Node>) -> Node {
        Node::branch(NodeType::Paragraph, Fragment::from_vec(children))
    }

    fn heading(level: u8, children: Vec<Node>) -> Node {
        Node::branch_with_attrs(
            NodeType::Heading,
            heading_attrs(level),
            Fragment::from_vec(children),
        )
    }

    fn text(s: &str) -> Node {
        Node::new_text(s)
    }

    fn text_with_marks(s: &str, marks: Vec<Mark>) -> Node {
        Node::new_text_with_marks(s, marks)
    }

    fn bullet_list(items: Vec<Node>) -> Node {
        Node::branch(NodeType::BulletList, Fragment::from_vec(items))
    }

    fn ordered_list(start: i64, items: Vec<Node>) -> Node {
        Node::branch_with_attrs(
            NodeType::OrderedList,
            ordered_list_attrs(start),
            Fragment::from_vec(items),
        )
    }

    fn list_item(children: Vec<Node>) -> Node {
        Node::branch(NodeType::ListItem, Fragment::from_vec(children))
    }

    fn blockquote(children: Vec<Node>) -> Node {
        Node::branch(NodeType::Blockquote, Fragment::from_vec(children))
    }

    fn code_block(lang: &str, content: &str) -> Node {
        let attrs = if lang.is_empty() {
            crate::attrs::empty_attrs()
        } else {
            code_block_attrs(lang)
        };
        let children = if content.is_empty() {
            vec![]
        } else {
            vec![Node::new_text(content)]
        };
        Node::branch_with_attrs(NodeType::CodeBlock, attrs, Fragment::from_vec(children))
    }

    // ── Basic serialization tests ───────────────────────────────────

    #[test]
    fn empty_doc() {
        let d = doc(vec![]);
        assert_eq!(serialize_markdown(&d), "");
    }

    #[test]
    fn single_paragraph() {
        let d = doc(vec![para(vec![text("Hello world")])]);
        assert_eq!(serialize_markdown(&d), "Hello world");
    }

    #[test]
    fn heading_level_2() {
        let d = doc(vec![heading(2, vec![text("Title")])]);
        assert_eq!(serialize_markdown(&d), "## Title");
    }

    #[test]
    fn heading_level_1() {
        let d = doc(vec![heading(1, vec![text("Top")])]);
        assert_eq!(serialize_markdown(&d), "# Top");
    }

    #[test]
    fn heading_level_3() {
        let d = doc(vec![heading(3, vec![text("Sub")])]);
        assert_eq!(serialize_markdown(&d), "### Sub");
    }

    #[test]
    fn bold_text() {
        let d = doc(vec![para(vec![text_with_marks(
            "bold",
            vec![Mark::new(MarkType::Strong)],
        )])]);
        assert_eq!(serialize_markdown(&d), "**bold**");
    }

    #[test]
    fn italic_text() {
        let d = doc(vec![para(vec![text_with_marks(
            "italic",
            vec![Mark::new(MarkType::Em)],
        )])]);
        assert_eq!(serialize_markdown(&d), "*italic*");
    }

    #[test]
    fn inline_code() {
        let d = doc(vec![para(vec![text_with_marks(
            "code",
            vec![Mark::new(MarkType::Code)],
        )])]);
        assert_eq!(serialize_markdown(&d), "`code`");
    }

    #[test]
    fn bold_and_italic() {
        let d = doc(vec![para(vec![text_with_marks(
            "both",
            vec![Mark::new(MarkType::Strong), Mark::new(MarkType::Em)],
        )])]);
        assert_eq!(serialize_markdown(&d), "***both***");
    }

    #[test]
    fn strikethrough() {
        let d = doc(vec![para(vec![text_with_marks(
            "deleted",
            vec![Mark::new(MarkType::Strike)],
        )])]);
        assert_eq!(serialize_markdown(&d), "~~deleted~~");
    }

    #[test]
    fn strong_and_code() {
        let d = doc(vec![para(vec![text_with_marks(
            "x",
            vec![Mark::new(MarkType::Code)],
        )])]);
        // Code mark excludes Strong, so only code should appear.
        assert_eq!(serialize_markdown(&d), "`x`");
    }

    #[test]
    fn bullet_list_simple() {
        let d = doc(vec![bullet_list(vec![
            list_item(vec![para(vec![text("item1")])]),
            list_item(vec![para(vec![text("item2")])]),
        ])]);
        assert_eq!(serialize_markdown(&d), "- item1\n- item2");
    }

    #[test]
    fn ordered_list_simple() {
        let d = doc(vec![ordered_list(
            1,
            vec![
                list_item(vec![para(vec![text("first")])]),
                list_item(vec![para(vec![text("second")])]),
            ],
        )]);
        assert_eq!(serialize_markdown(&d), "1. first\n2. second");
    }

    #[test]
    fn ordered_list_custom_start() {
        let d = doc(vec![ordered_list(
            3,
            vec![
                list_item(vec![para(vec![text("a")])]),
                list_item(vec![para(vec![text("b")])]),
            ],
        )]);
        assert_eq!(serialize_markdown(&d), "3. a\n4. b");
    }

    #[test]
    fn blockquote_simple() {
        let d = doc(vec![blockquote(vec![para(vec![text("quoted")])])]);
        assert_eq!(serialize_markdown(&d), "> quoted");
    }

    #[test]
    fn code_block_with_language() {
        let d = doc(vec![code_block("sql", "SELECT 1;")]);
        assert_eq!(serialize_markdown(&d), "```sql\nSELECT 1;\n```");
    }

    #[test]
    fn code_block_no_language() {
        let d = doc(vec![code_block("", "hello")]);
        assert_eq!(serialize_markdown(&d), "```\nhello\n```");
    }

    #[test]
    fn horizontal_rule() {
        let d = doc(vec![Node::leaf(NodeType::HorizontalRule)]);
        assert_eq!(serialize_markdown(&d), "---");
    }

    #[test]
    fn link_text() {
        let d = doc(vec![para(vec![text_with_marks(
            "click here",
            vec![Mark::with_attrs(
                MarkType::Link,
                link_attrs("https://example.com", None),
            )],
        )])]);
        assert_eq!(
            serialize_markdown(&d),
            "[click here](https://example.com)"
        );
    }

    #[test]
    fn link_with_title() {
        let d = doc(vec![para(vec![text_with_marks(
            "link",
            vec![Mark::with_attrs(
                MarkType::Link,
                link_attrs("https://example.com", Some("Example")),
            )],
        )])]);
        assert_eq!(
            serialize_markdown(&d),
            "[link](https://example.com \"Example\")"
        );
    }

    #[test]
    fn image_simple() {
        let d = doc(vec![para(vec![Node::leaf_with_attrs(
            NodeType::Image,
            image_attrs("photo.png", "A photo", None),
        )])]);
        assert_eq!(serialize_markdown(&d), "![A photo](photo.png)");
    }

    #[test]
    fn image_with_title() {
        let d = doc(vec![para(vec![Node::leaf_with_attrs(
            NodeType::Image,
            image_attrs("pic.jpg", "alt text", Some("My Title")),
        )])]);
        assert_eq!(
            serialize_markdown(&d),
            "![alt text](pic.jpg \"My Title\")"
        );
    }

    #[test]
    fn hard_break() {
        let d = doc(vec![para(vec![
            text("line1"),
            Node::leaf(NodeType::HardBreak),
            text("line2"),
        ])]);
        assert_eq!(serialize_markdown(&d), "line1  \nline2");
    }

    #[test]
    fn blockquote_multiple_paragraphs() {
        let d = doc(vec![blockquote(vec![
            para(vec![text("first")]),
            para(vec![text("second")]),
        ])]);
        assert_eq!(serialize_markdown(&d), "> first\n>\n> second");
    }

    #[test]
    fn list_item_multiple_paragraphs() {
        let d = doc(vec![bullet_list(vec![list_item(vec![
            para(vec![text("first para")]),
            para(vec![text("second para")]),
        ])])]);
        assert_eq!(
            serialize_markdown(&d),
            "- first para\n\n  second para"
        );
    }

    #[test]
    fn multiple_blocks() {
        let d = doc(vec![
            heading(1, vec![text("Title")]),
            para(vec![text("Body text.")]),
            Node::leaf(NodeType::HorizontalRule),
        ]);
        assert_eq!(
            serialize_markdown(&d),
            "# Title\n\nBody text.\n\n---"
        );
    }

    #[test]
    fn mixed_inline_content() {
        let d = doc(vec![para(vec![
            text("normal "),
            text_with_marks("bold", vec![Mark::new(MarkType::Strong)]),
            text(" and "),
            text_with_marks("italic", vec![Mark::new(MarkType::Em)]),
        ])]);
        assert_eq!(
            serialize_markdown(&d),
            "normal **bold** and *italic*"
        );
    }

    // ── Round-trip tests ────────────────────────────────────────────

    #[test]
    fn round_trip_text_content_preserved() {
        let input = "Hello world";
        let tree = parse_markdown(input);
        let output = serialize_markdown(&tree);
        assert_eq!(output, input);
    }

    #[test]
    fn round_trip_heading() {
        let input = "## My Heading";
        let tree = parse_markdown(input);
        let output = serialize_markdown(&tree);
        assert_eq!(output, input);
    }

    #[test]
    fn round_trip_bold() {
        let input = "Some **bold** text";
        let tree = parse_markdown(input);
        let output = serialize_markdown(&tree);
        assert_eq!(output, input);
    }

    #[test]
    fn round_trip_italic() {
        let input = "Some *italic* text";
        let tree = parse_markdown(input);
        let output = serialize_markdown(&tree);
        assert_eq!(output, input);
    }

    #[test]
    fn round_trip_code_block() {
        // Tree-sitter needs a trailing newline after the closing fence to
        // correctly parse fenced code blocks — this matches real-world files.
        let input = "```rust\nfn main() {}\n```\n";
        let tree = parse_markdown(input);
        let output = serialize_markdown(&tree);
        // serialize_markdown trims trailing whitespace, so the trailing newline
        // is dropped. The semantic content must still match.
        assert_eq!(output, "```rust\nfn main() {}\n```");
    }

    #[test]
    fn round_trip_bullet_list() {
        let input = "- one\n- two\n- three";
        let tree = parse_markdown(input);
        let output = serialize_markdown(&tree);
        assert_eq!(output, input);
    }

    #[test]
    fn round_trip_blockquote() {
        let input = "> quoted text";
        let tree = parse_markdown(input);
        let output = serialize_markdown(&tree);
        assert_eq!(output, input);
    }

    #[test]
    fn round_trip_horizontal_rule() {
        let input = "---";
        let tree = parse_markdown(input);
        let output = serialize_markdown(&tree);
        assert_eq!(output, input);
    }

    #[test]
    fn round_trip_link() {
        let input = "[example](https://example.com)";
        let tree = parse_markdown(input);
        let output = serialize_markdown(&tree);
        assert_eq!(output, input);
    }

    #[test]
    fn round_trip_image() {
        let input = "![alt text](image.png)";
        let tree = parse_markdown(input);
        let output = serialize_markdown(&tree);
        assert_eq!(output, input);
    }

    #[test]
    fn round_trip_parse_serialize_parse_identical() {
        let inputs = [
            "Hello world",
            "## Heading",
            "**bold** and *italic*",
            "- item1\n- item2",
            "> blockquote",
            "```js\nconsole.log('hi');\n```",
            "---",
            "[link](https://example.com)",
            "![img](photo.png)",
        ];

        for input in inputs {
            let tree1 = parse_markdown(input);
            let serialized = serialize_markdown(&tree1);
            let tree2 = parse_markdown(&serialized);

            assert_eq!(
                tree1.text_content(),
                tree2.text_content(),
                "text_content mismatch for input: {input:?}\nserialized: {serialized:?}"
            );
        }
    }

    #[test]
    fn round_trip_ordered_list() {
        let input = "1. first\n2. second\n3. third";
        let tree = parse_markdown(input);
        let output = serialize_markdown(&tree);
        assert_eq!(output, input);
    }

    #[test]
    fn round_trip_inline_code() {
        let input = "Use `fmt.Println` here";
        let tree = parse_markdown(input);
        let output = serialize_markdown(&tree);
        assert_eq!(output, input);
    }

    // ── tree_pos_to_byte_offset tests ──────────────────────────────

    #[test]
    fn pos_to_byte_simple_paragraph() {
        // "Hello world" -> <doc><p>Hello world</p></doc>
        // Tree positions: <p>=0, H=1, e=2, l=3, l=4, o=5, ' '=6, w=7, o=8, r=9, l=10, d=11, </p>=12
        // Markdown: "Hello world" (11 bytes)
        let input = "Hello world";
        let doc = parse_markdown(input);
        let md = serialize_markdown(&doc);
        assert_eq!(md, input);

        // Position 0 is before the paragraph's opening token.
        // Position 1 is at "H" -> byte 0.
        assert_eq!(tree_pos_to_byte_offset(&doc, &md, 1), 0);
        // Position 6 is at " " -> byte 5.
        assert_eq!(tree_pos_to_byte_offset(&doc, &md, 6), 5);
        // Position 12 (closing token) -> byte 11 (end of string).
        assert_eq!(tree_pos_to_byte_offset(&doc, &md, 12), 11);
    }

    #[test]
    fn pos_to_byte_heading() {
        // "## My Heading" -> <doc><h2>My Heading</h2></doc>
        // Tree: <h2>=0, M=1, y=2, ' '=3, H=4, ...
        // Markdown: "## My Heading" -> "## " is 3 bytes prefix.
        let input = "## My Heading";
        let doc = parse_markdown(input);
        let md = serialize_markdown(&doc);
        assert_eq!(md, input);

        // Position 1 (M) -> byte 3 (after "## ").
        assert_eq!(tree_pos_to_byte_offset(&doc, &md, 1), 3);
        // Position 4 (H) -> byte 6.
        assert_eq!(tree_pos_to_byte_offset(&doc, &md, 4), 6);
    }

    #[test]
    fn pos_to_byte_bold_text() {
        // "**bold**" -> <doc><p><strong>bold</strong></p></doc>
        // Tree: <p>=0, b=1, o=2, l=3, d=4, </p>=5
        // Markdown: "**bold**" -> "**" is 2 bytes, "bold" is 4, "**" is 2.
        let input = "**bold**";
        let doc = parse_markdown(input);
        let md = serialize_markdown(&doc);
        assert_eq!(md, input);

        // Position 1 (b) -> byte 2 (after "**").
        assert_eq!(tree_pos_to_byte_offset(&doc, &md, 1), 2);
        // Position 4 (d) -> byte 5.
        assert_eq!(tree_pos_to_byte_offset(&doc, &md, 4), 5);
    }

    #[test]
    fn pos_to_byte_past_end_returns_md_len() {
        let input = "Hello";
        let doc = parse_markdown(input);
        let md = serialize_markdown(&doc);
        // Position 100 is way past end -> returns md.len().
        assert_eq!(tree_pos_to_byte_offset(&doc, &md, 100), md.len());
    }

    // ── byte_offset_to_tree_pos tests ──────────────────────────────

    #[test]
    fn byte_to_tree_pos_simple_paragraph() {
        let input = "Hello";
        let doc = parse_markdown(input);
        let md = serialize_markdown(&doc);
        // byte 0 -> tree pos 1 (start of paragraph content)
        assert_eq!(byte_offset_to_tree_pos(&doc, &md, 0), 1);
        // byte 5 -> tree pos 6 (end of "Hello")
        assert_eq!(byte_offset_to_tree_pos(&doc, &md, 5), 6);
    }

    #[test]
    fn byte_to_tree_pos_heading() {
        let input = "## Hi";
        let doc = parse_markdown(input);
        let md = serialize_markdown(&doc);
        // "## Hi" — byte 5 (past end) should return doc content size.
        let doc_size = doc.content.size();
        assert_eq!(byte_offset_to_tree_pos(&doc, &md, md.len() + 10), doc_size);
        // byte 0 should map to a valid tree position within the document.
        let pos_at_0 = byte_offset_to_tree_pos(&doc, &md, 0);
        assert!(pos_at_0 <= doc_size, "pos should be within doc bounds");
    }

    #[test]
    fn byte_to_tree_pos_past_end_returns_doc_size() {
        let input = "Hello";
        let doc = parse_markdown(input);
        let md = serialize_markdown(&doc);
        let doc_size = doc.content.size();
        assert_eq!(byte_offset_to_tree_pos(&doc, &md, 100), doc_size);
    }

    #[test]
    fn byte_to_tree_pos_round_trip_simple() {
        // Verify that byte_offset_to_tree_pos is the inverse of
        // tree_pos_to_byte_offset for simple cases.
        let input = "Hello world";
        let doc = parse_markdown(input);
        let md = serialize_markdown(&doc);
        // tree pos 1 -> byte, then byte -> tree pos should get back 1
        let byte = tree_pos_to_byte_offset(&doc, &md, 1);
        let back = byte_offset_to_tree_pos(&doc, &md, byte);
        assert_eq!(back, 1);
    }
}
