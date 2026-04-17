//! Markdown string → kode-doc tree parser.
//!
//! Uses tree-sitter-markdown for block-level structure, then parses inline
//! content (bold, italic, code, links, etc.) into properly marked text nodes.

use std::cell::RefCell;

use arborium_tree_sitter::{Language, Parser};

use crate::attrs::{
    code_block_attrs, empty_attrs, heading_attrs, image_attrs, link_attrs, ordered_list_attrs,
};
use crate::fragment::Fragment;
use crate::mark::{Mark, MarkType};
use crate::node::Node;
use crate::node_type::NodeType;

thread_local! {
    /// Cached tree-sitter parser — avoids re-creating and re-configuring
    /// the parser on every `parse_markdown` call.
    static PARSER: RefCell<Parser> = RefCell::new({
        let language = Language::new(arborium_markdown::language());
        let mut parser = Parser::new();
        parser
            .set_language(&language)
            .expect("failed to set markdown language");
        parser
    });
}

/// Parse a markdown string into a kode-doc document tree.
///
/// Uses tree-sitter-markdown to parse the source, then walks the CST
/// to build a typed document tree with proper node types and marks.
pub fn parse_markdown(source: &str) -> Node {
    let tree = PARSER.with(|p| p.borrow_mut().parse(source, None));

    let Some(tree) = tree else {
        return Node::branch(NodeType::Doc, Fragment::empty());
    };

    let root = tree.root_node();
    let children = convert_block_children(&root, source);
    Node::branch(NodeType::Doc, Fragment::from_vec(children))
}

// ── Block-level conversion ──────────────────────────────────────────────

/// Convert all named children of a tree-sitter node into kode-doc nodes.
fn convert_block_children(node: &arborium_tree_sitter::Node, source: &str) -> Vec<Node> {
    let mut result = Vec::new();
    let mut cursor = node.walk();

    for child in node.named_children(&mut cursor) {
        let kind = child.kind();
        match kind {
            // Flatten containers: recurse into sections instead of wrapping
            "document" | "section" => {
                result.extend(convert_block_children(&child, source));
            }
            _ => {
                if let Some(doc_node) = convert_block_node(&child, source) {
                    result.push(doc_node);
                }
            }
        }
    }
    result
}

/// Convert a single tree-sitter block node into a kode-doc Node.
fn convert_block_node(node: &arborium_tree_sitter::Node, source: &str) -> Option<Node> {
    match node.kind() {
        "paragraph" => {
            let text = node_text(node, source);
            let inlines = parse_inline(text);
            if inlines.is_empty() {
                // Empty paragraph: still produce a node with no children
                Some(Node::branch(NodeType::Paragraph, Fragment::empty()))
            } else {
                Some(Node::branch(
                    NodeType::Paragraph,
                    Fragment::from_vec(inlines),
                ))
            }
        }

        "atx_heading" => {
            let level = detect_heading_level(node);
            let content = heading_inline_text(node, source);
            let inlines = parse_inline(&content);
            Some(Node::branch_with_attrs(
                NodeType::Heading,
                heading_attrs(level),
                Fragment::from_vec(inlines),
            ))
        }

        "setext_heading" => {
            let text = node
                .named_children(&mut node.walk())
                .find(|c| c.kind() == "paragraph")
                .map(|c| node_text(&c, source))
                .unwrap_or_default();
            let has_h1 = node
                .children(&mut node.walk())
                .any(|c| c.kind() == "setext_h1_underline");
            let level: u8 = if has_h1 { 1 } else { 2 };
            let inlines = parse_inline(text);
            Some(Node::branch_with_attrs(
                NodeType::Heading,
                heading_attrs(level),
                Fragment::from_vec(inlines),
            ))
        }

        "list" => {
            let is_ordered = node
                .children(&mut node.walk())
                .find(|c| c.kind() == "list_item")
                .map(|item| {
                    item.children(&mut item.walk()).any(|c| {
                        c.kind() == "list_marker_dot" || c.kind() == "list_marker_parenthesis"
                    })
                })
                .unwrap_or(false);

            let items = convert_list_items(node, source);

            if is_ordered {
                let start = detect_ordered_list_start(node, source);
                Some(Node::branch_with_attrs(
                    NodeType::OrderedList,
                    ordered_list_attrs(start),
                    Fragment::from_vec(items),
                ))
            } else {
                Some(Node::branch(
                    NodeType::BulletList,
                    Fragment::from_vec(items),
                ))
            }
        }

        "block_quote" => {
            let children = convert_block_children(node, source);
            Some(Node::branch(
                NodeType::Blockquote,
                Fragment::from_vec(children),
            ))
        }

        "fenced_code_block" => {
            let lang = code_block_language(node, source).unwrap_or("");
            let content = code_block_content(node, source).unwrap_or("");
            let attrs = if lang.is_empty() {
                empty_attrs()
            } else {
                code_block_attrs(lang)
            };
            let children = if content.is_empty() {
                vec![]
            } else {
                vec![Node::new_text(content)]
            };
            Some(Node::branch_with_attrs(
                NodeType::CodeBlock,
                attrs,
                Fragment::from_vec(children),
            ))
        }

        "indented_code_block" => {
            let text = node_text(node, source);
            // Strip 4 spaces or 1 tab of indent from each line
            let content: String = text
                .lines()
                .map(|l| {
                    if let Some(stripped) = l.strip_prefix("    ") {
                        stripped
                    } else if let Some(stripped) = l.strip_prefix('\t') {
                        stripped
                    } else {
                        l
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            let children = if content.is_empty() {
                vec![]
            } else {
                vec![Node::new_text(&content)]
            };
            Some(Node::branch(
                NodeType::CodeBlock,
                Fragment::from_vec(children),
            ))
        }

        "thematic_break" => Some(Node::leaf(NodeType::HorizontalRule)),

        "html_block" => {
            // Treat HTML blocks as paragraphs with raw content
            let text = node_text(node, source);
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(Node::branch(
                    NodeType::Paragraph,
                    Fragment::from_node(Node::new_text(trimmed)),
                ))
            }
        }

        "pipe_table" => {
            let rows = convert_table_children(node, source);
            Some(Node::branch(NodeType::Table, Fragment::from_vec(rows)))
        }

        // Skip structural nodes that are handled by their parents
        "list_item" | "block_continuation" | "block_quote_marker"
        | "list_marker_minus" | "list_marker_plus" | "list_marker_star"
        | "list_marker_dot" | "list_marker_parenthesis" => None,

        // Front matter: skip
        "minus_metadata" | "plus_metadata" => None,

        // Link reference definitions: skip
        "link_reference_definition" => None,

        // Unknown: treat as paragraph with raw text
        _ => {
            let text = node_text(node, source);
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(Node::branch(
                    NodeType::Paragraph,
                    Fragment::from_node(Node::new_text(trimmed)),
                ))
            }
        }
    }
}

/// Convert list_item children of a list node.
fn convert_list_items(list_node: &arborium_tree_sitter::Node, source: &str) -> Vec<Node> {
    let mut items = Vec::new();
    let mut cursor = list_node.walk();

    for child in list_node.named_children(&mut cursor) {
        if child.kind() == "list_item" {
            let block_children = convert_block_children(&child, source);
            items.push(Node::branch(
                NodeType::ListItem,
                Fragment::from_vec(block_children),
            ));
        }
    }
    items
}

/// Convert children of a pipe_table node into Table{Header,Row} nodes.
///
/// A pipe_table has children: pipe_table_header, pipe_table_delimiter_row,
/// and one or more pipe_table_row. We skip the delimiter row (it's just
/// visual formatting) and convert header/data rows into structured nodes.
fn convert_table_children(
    table_node: &arborium_tree_sitter::Node,
    source: &str,
) -> Vec<Node> {
    let mut rows = Vec::new();
    let mut cursor = table_node.walk();

    for child in table_node.named_children(&mut cursor) {
        match child.kind() {
            "pipe_table_header" => {
                let cells = convert_table_cells(&child, source);
                rows.push(Node::branch(
                    NodeType::TableHeader,
                    Fragment::from_vec(cells),
                ));
            }
            "pipe_table_row" => {
                let cells = convert_table_cells(&child, source);
                rows.push(Node::branch(
                    NodeType::TableRow,
                    Fragment::from_vec(cells),
                ));
            }
            // Skip delimiter row (| --- | --- |)
            _ => {}
        }
    }

    rows
}

/// Extract cells from a pipe_table_header or pipe_table_row.
///
/// The raw text looks like `| col1 | col2 | col3 |`. We split on `|`,
/// trim each cell, and parse inline markdown (bold, italic, code, links).
fn convert_table_cells(
    row_node: &arborium_tree_sitter::Node,
    source: &str,
) -> Vec<Node> {
    let text = node_text(row_node, source);
    // Strip leading/trailing pipe and split on `|`
    let stripped = text.trim().trim_start_matches('|').trim_end_matches('|');
    stripped
        .split('|')
        .map(|cell_text| {
            let trimmed = cell_text.trim();
            let inlines = parse_inline(trimmed);
            Node::branch(NodeType::TableCell, Fragment::from_vec(inlines))
        })
        .collect()
}

// ── Tree-sitter helpers ─────────────────────────────────────────────────

/// Extract the raw text of a tree-sitter node.
fn node_text<'a>(node: &arborium_tree_sitter::Node, source: &'a str) -> &'a str {
    &source[node.start_byte()..node.end_byte()]
}

/// Detect heading level from atx_heading marker children.
fn detect_heading_level(node: &arborium_tree_sitter::Node) -> u8 {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "atx_h1_marker" => return 1,
            "atx_h2_marker" => return 2,
            "atx_h3_marker" => return 3,
            "atx_h4_marker" => return 4,
            "atx_h5_marker" => return 5,
            "atx_h6_marker" => return 6,
            _ => {}
        }
    }
    1
}

/// Extract inline text from a heading, skipping the marker.
fn heading_inline_text(node: &arborium_tree_sitter::Node, source: &str) -> String {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "inline" {
            return node_text(&child, source).to_string();
        }
    }
    // Fallback: extract text after the marker
    let full = node_text(node, source);
    let trimmed = full.trim_start_matches('#').trim_start();
    trimmed.to_string()
}

/// Extract the info string (language) from a fenced code block node.
fn code_block_language<'a>(
    node: &arborium_tree_sitter::Node,
    source: &'a str,
) -> Option<&'a str> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32)
            && child.kind() == "info_string"
        {
            let text = &source[child.start_byte()..child.end_byte()];
            let lang = text.trim();
            if !lang.is_empty() {
                return Some(lang);
            }
        }
    }
    None
}

/// Extract the content of a fenced code block (without fences).
fn code_block_content<'a>(
    node: &arborium_tree_sitter::Node,
    source: &'a str,
) -> Option<&'a str> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32)
            && child.kind() == "code_fence_content"
        {
            return Some(&source[child.start_byte()..child.end_byte()]);
        }
    }
    None
}

/// Detect the start number of an ordered list by parsing the first marker.
fn detect_ordered_list_start(
    list_node: &arborium_tree_sitter::Node,
    source: &str,
) -> i64 {
    let mut cursor = list_node.walk();
    for child in list_node.children(&mut cursor) {
        if child.kind() == "list_item" {
            let mut item_cursor = child.walk();
            for item_child in child.children(&mut item_cursor) {
                if item_child.kind() == "list_marker_dot"
                    || item_child.kind() == "list_marker_parenthesis"
                {
                    let marker_text = node_text(&item_child, source);
                    // Parse "1." or "1)" → 1
                    let num_str: String =
                        marker_text.chars().take_while(|c| c.is_ascii_digit()).collect();
                    if let Ok(n) = num_str.parse::<i64>() {
                        return n;
                    }
                }
            }
        }
    }
    1
}

// ── Inline parsing ──────────────────────────────────────────────────────
//
// Since arborium-markdown-inline conflicts with arborium-tree-sitter due to
// native library linking, we parse inline markdown manually. This handles:
// **bold**, *italic*, `code`, ~~strikethrough~~, [link](url), ![image](url),
// hard line breaks (two trailing spaces + newline), and backslash escapes.

/// Parse inline markdown text into a sequence of kode-doc nodes.
fn parse_inline(text: &str) -> Vec<Node> {
    let text = text.trim_end_matches('\n').trim_end_matches('\r');
    if text.is_empty() {
        return vec![];
    }
    let chars: Vec<char> = text.chars().collect();
    let mut nodes = Vec::new();
    parse_inline_recursive(&chars, &[], &mut nodes);
    nodes
}

/// Recursively parse inline content, accumulating text with the given marks.
fn parse_inline_recursive(chars: &[char], marks: &[Mark], out: &mut Vec<Node>) {
    let mut i = 0;
    let mut buf = String::new();

    while i < chars.len() {
        match chars[i] {
            // Backslash escape
            '\\' if i + 1 < chars.len() && is_punctuation(chars[i + 1]) => {
                buf.push(chars[i + 1]);
                i += 2;
            }

            // Hard line break: two+ spaces before newline
            ' ' if has_hard_break_at(chars, i) => {
                flush_text(&mut buf, marks, out);
                out.push(Node::leaf(NodeType::HardBreak));
                // Skip trailing spaces and the newline
                while i < chars.len() && chars[i] == ' ' {
                    i += 1;
                }
                if i < chars.len() && chars[i] == '\n' {
                    i += 1;
                }
            }

            // Soft line break: single newline → space
            '\n' => {
                buf.push(' ');
                i += 1;
            }

            // Image: ![alt](src "title")
            '!' if i + 1 < chars.len() && chars[i + 1] == '[' => {
                if let Some((alt, src, title, end)) = parse_image_or_link(chars, i + 1) {
                    flush_text(&mut buf, marks, out);
                    out.push(Node::leaf_with_attrs(
                        NodeType::Image,
                        image_attrs(&src, &alt, title.as_deref()),
                    ));
                    i = end;
                } else {
                    buf.push('!');
                    i += 1;
                }
            }

            // Link: [text](url "title")
            '[' => {
                if let Some((link_text, href, title, end)) = parse_image_or_link(chars, i) {
                    flush_text(&mut buf, marks, out);
                    let link_mark =
                        Mark::with_attrs(MarkType::Link, link_attrs(&href, title.as_deref()));
                    let mut link_marks = marks.to_vec();
                    link_marks = link_mark.add_to_set(&link_marks);
                    // Parse the link text for nested inline formatting
                    let link_chars: Vec<char> = link_text.chars().collect();
                    parse_inline_recursive(&link_chars, &link_marks, out);
                    i = end;
                } else {
                    buf.push('[');
                    i += 1;
                }
            }

            // Inline code: `code` or ``code``
            '`' => {
                let backtick_count = count_char(chars, i, '`');
                if let Some((content, end)) =
                    find_backtick_content(chars, i + backtick_count, backtick_count)
                {
                    flush_text(&mut buf, marks, out);
                    let code_mark = Mark::new(MarkType::Code);
                    let code_marks = code_mark.add_to_set(marks);
                    if !content.is_empty() {
                        out.push(Node::new_text_with_marks(&content, code_marks));
                    }
                    i = end;
                } else {
                    buf.push('`');
                    i += 1;
                }
            }

            // Strikethrough: ~~text~~
            '~' if i + 1 < chars.len() && chars[i + 1] == '~' => {
                if let Some((content, end)) = find_delimited(chars, i + 2, "~~") {
                    flush_text(&mut buf, marks, out);
                    let strike_mark = Mark::new(MarkType::Strike);
                    let mut inner_marks = marks.to_vec();
                    inner_marks = strike_mark.add_to_set(&inner_marks);
                    let inner_chars: Vec<char> = content.chars().collect();
                    parse_inline_recursive(&inner_chars, &inner_marks, out);
                    i = end;
                } else {
                    buf.push('~');
                    i += 1;
                }
            }

            // Bold+italic: ***text*** (3 asterisks)
            '*' if i + 2 < chars.len()
                && chars[i + 1] == '*'
                && chars[i + 2] == '*'
                && (i + 3 >= chars.len() || chars[i + 3] != '*') =>
            {
                if let Some((content, end)) = find_delimited(chars, i + 3, "***") {
                    flush_text(&mut buf, marks, out);
                    let strong_mark = Mark::new(MarkType::Strong);
                    let em_mark = Mark::new(MarkType::Em);
                    let mut inner_marks = marks.to_vec();
                    inner_marks = strong_mark.add_to_set(&inner_marks);
                    inner_marks = em_mark.add_to_set(&inner_marks);
                    let inner_chars: Vec<char> = content.chars().collect();
                    parse_inline_recursive(&inner_chars, &inner_marks, out);
                    i = end;
                } else {
                    buf.push('*');
                    i += 1;
                }
            }

            // Bold: **text**
            '*' if i + 1 < chars.len() && chars[i + 1] == '*' => {
                if let Some((content, end)) = find_delimited(chars, i + 2, "**") {
                    flush_text(&mut buf, marks, out);
                    let strong_mark = Mark::new(MarkType::Strong);
                    let mut inner_marks = marks.to_vec();
                    inner_marks = strong_mark.add_to_set(&inner_marks);
                    let inner_chars: Vec<char> = content.chars().collect();
                    parse_inline_recursive(&inner_chars, &inner_marks, out);
                    i = end;
                } else {
                    buf.push('*');
                    i += 1;
                }
            }

            // Italic: *text*
            '*' => {
                if let Some((content, end)) = find_delimited(chars, i + 1, "*") {
                    // Ensure we didn't match the start of **
                    if !content.is_empty() {
                        flush_text(&mut buf, marks, out);
                        let em_mark = Mark::new(MarkType::Em);
                        let mut inner_marks = marks.to_vec();
                        inner_marks = em_mark.add_to_set(&inner_marks);
                        let inner_chars: Vec<char> = content.chars().collect();
                        parse_inline_recursive(&inner_chars, &inner_marks, out);
                        i = end;
                    } else {
                        buf.push('*');
                        i += 1;
                    }
                } else {
                    buf.push('*');
                    i += 1;
                }
            }

            // Bold with underscores: __text__
            // Only treat as delimiter when preceded by a word boundary
            '_' if i + 1 < chars.len()
                && chars[i + 1] == '_'
                && is_delimiter_boundary(if i > 0 { Some(chars[i - 1]) } else { None }) =>
            {
                if let Some((content, end)) = find_delimited(chars, i + 2, "__") {
                    // The character after the closing __ must also be a boundary
                    if is_delimiter_boundary(chars.get(end).copied()) {
                        flush_text(&mut buf, marks, out);
                        let strong_mark = Mark::new(MarkType::Strong);
                        let mut inner_marks = marks.to_vec();
                        inner_marks = strong_mark.add_to_set(&inner_marks);
                        let inner_chars: Vec<char> = content.chars().collect();
                        parse_inline_recursive(&inner_chars, &inner_marks, out);
                        i = end;
                    } else {
                        buf.push('_');
                        i += 1;
                    }
                } else {
                    buf.push('_');
                    i += 1;
                }
            }

            // Italic with underscore: _text_
            // Only treat as delimiter when preceded by a word boundary
            '_' if is_delimiter_boundary(if i > 0 { Some(chars[i - 1]) } else { None }) => {
                if let Some((content, end)) = find_delimited(chars, i + 1, "_") {
                    // The character after the closing _ must also be a boundary
                    if !content.is_empty()
                        && is_delimiter_boundary(chars.get(end).copied())
                    {
                        flush_text(&mut buf, marks, out);
                        let em_mark = Mark::new(MarkType::Em);
                        let mut inner_marks = marks.to_vec();
                        inner_marks = em_mark.add_to_set(&inner_marks);
                        let inner_chars: Vec<char> = content.chars().collect();
                        parse_inline_recursive(&inner_chars, &inner_marks, out);
                        i = end;
                    } else {
                        buf.push('_');
                        i += 1;
                    }
                } else {
                    buf.push('_');
                    i += 1;
                }
            }

            // Regular character
            c => {
                buf.push(c);
                i += 1;
            }
        }
    }

    flush_text(&mut buf, marks, out);
}

/// Flush accumulated plain text as a Text node with the given marks.
fn flush_text(buf: &mut String, marks: &[Mark], out: &mut Vec<Node>) {
    if buf.is_empty() {
        return;
    }
    let text = std::mem::take(buf);
    if marks.is_empty() {
        out.push(Node::new_text(&text));
    } else {
        out.push(Node::new_text_with_marks(&text, marks.to_vec()));
    }
}

/// Check if a character is a word boundary for underscore-based delimiters.
///
/// Underscores inside words (e.g., `foo_bar_baz`) must NOT trigger emphasis.
/// A boundary is whitespace, punctuation, or an implicit start/end of string.
fn is_delimiter_boundary(c: Option<char>) -> bool {
    match c {
        None => true,
        Some(ch) => ch.is_whitespace() || is_punctuation(ch),
    }
}

/// Check if a character is ASCII punctuation (for backslash escapes).
fn is_punctuation(c: char) -> bool {
    matches!(
        c,
        '!' | '"'
            | '#'
            | '$'
            | '%'
            | '&'
            | '\''
            | '('
            | ')'
            | '*'
            | '+'
            | ','
            | '-'
            | '.'
            | '/'
            | ':'
            | ';'
            | '<'
            | '='
            | '>'
            | '?'
            | '@'
            | '['
            | '\\'
            | ']'
            | '^'
            | '_'
            | '`'
            | '{'
            | '|'
            | '}'
            | '~'
    )
}

/// Check if position `i` starts a hard line break (2+ spaces followed by \n).
fn has_hard_break_at(chars: &[char], i: usize) -> bool {
    // Need at least 2 spaces then a newline
    if i + 2 >= chars.len() {
        return false;
    }
    let mut j = i;
    while j < chars.len() && chars[j] == ' ' {
        j += 1;
    }
    let space_count = j - i;
    space_count >= 2 && j < chars.len() && chars[j] == '\n'
}

/// Count consecutive occurrences of `c` starting at position `i`.
fn count_char(chars: &[char], i: usize, c: char) -> usize {
    let mut count = 0;
    let mut j = i;
    while j < chars.len() && chars[j] == c {
        count += 1;
        j += 1;
    }
    count
}

/// Find content enclosed by `count` backticks, handling inline code spans.
fn find_backtick_content(
    chars: &[char],
    start: usize,
    count: usize,
) -> Option<(String, usize)> {
    let mut i = start;
    while i + count <= chars.len() {
        if count_char(chars, i, '`') == count {
            let raw: String = chars[start..i].iter().collect();
            // CommonMark §6.1: If the resulting string both begins AND ends
            // with a space character, and does not consist entirely of space
            // characters, a single space is stripped from the front and back.
            // We compare `chars().count()` (not `len()`) so multi-byte
            // codepoints don't break the "only spaces" guard.
            let char_count = raw.chars().count();
            let content = if raw.starts_with(' ')
                && raw.ends_with(' ')
                && char_count > 1
                && raw.chars().any(|c| c != ' ')
            {
                raw[1..raw.len() - 1].to_string()
            } else {
                raw
            };
            return Some((content, i + count));
        }
        i += 1;
    }
    None
}

/// Find content delimited by the given string (e.g., "**", "*", "~~").
fn find_delimited(chars: &[char], start: usize, delim: &str) -> Option<(String, usize)> {
    let delim_chars: Vec<char> = delim.chars().collect();
    let dlen = delim_chars.len();

    if start >= chars.len() {
        return None;
    }

    let mut i = start;
    while i + dlen <= chars.len() {
        // Check for backslash escape
        if chars[i] == '\\' && i + 1 < chars.len() {
            i += 2;
            continue;
        }
        if chars[i..i + dlen] == delim_chars[..] {
            let content: String = chars[start..i].iter().collect();
            return Some((content, i + dlen));
        }
        i += 1;
    }
    None
}

/// Parse a link or image: `[text](url "title")`.
/// `start` should point at the opening `[`.
/// Returns `(text, url, optional_title, end_position)`.
fn parse_image_or_link(
    chars: &[char],
    start: usize,
) -> Option<(String, String, Option<String>, usize)> {
    if start >= chars.len() || chars[start] != '[' {
        return None;
    }

    // Find matching ]
    let mut depth = 0;
    let mut i = start;
    let bracket_end;
    loop {
        if i >= chars.len() {
            return None;
        }
        if chars[i] == '\\' && i + 1 < chars.len() {
            i += 2;
            continue;
        }
        if chars[i] == '[' {
            depth += 1;
        } else if chars[i] == ']' {
            depth -= 1;
            if depth == 0 {
                bracket_end = i;
                break;
            }
        }
        i += 1;
    }

    let link_text: String = chars[start + 1..bracket_end].iter().collect();

    // Expect ( immediately after ]
    let paren_start = bracket_end + 1;
    if paren_start >= chars.len() || chars[paren_start] != '(' {
        return None;
    }

    // Find matching )
    let mut j = paren_start + 1;
    // Skip whitespace
    while j < chars.len() && chars[j].is_whitespace() {
        j += 1;
    }

    // Parse URL (may be in angle brackets)
    let url;
    if j < chars.len() && chars[j] == '<' {
        // Angle-bracket URL
        j += 1;
        let url_start = j;
        while j < chars.len() && chars[j] != '>' {
            j += 1;
        }
        url = chars[url_start..j].iter().collect();
        if j < chars.len() {
            j += 1; // skip >
        }
    } else {
        // Bare URL: read until whitespace or )
        let url_start = j;
        let mut paren_depth = 0;
        while j < chars.len() && (chars[j] != ')' || paren_depth > 0) && !chars[j].is_whitespace()
        {
            if chars[j] == '(' {
                paren_depth += 1;
            } else if chars[j] == ')' {
                paren_depth -= 1;
            }
            j += 1;
        }
        url = chars[url_start..j].iter().collect();
    }

    // Skip whitespace
    while j < chars.len() && chars[j].is_whitespace() {
        j += 1;
    }

    // Optional title
    let title;
    if j < chars.len() && (chars[j] == '"' || chars[j] == '\'') {
        let quote = chars[j];
        j += 1;
        let title_start = j;
        while j < chars.len() && chars[j] != quote {
            j += 1;
        }
        title = Some(chars[title_start..j].iter().collect::<String>());
        if j < chars.len() {
            j += 1; // skip closing quote
        }
    } else {
        title = None;
    }

    // Skip whitespace and expect )
    while j < chars.len() && chars[j].is_whitespace() {
        j += 1;
    }
    if j >= chars.len() || chars[j] != ')' {
        return None;
    }

    Some((link_text, url, title, j + 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attrs::{get_attr, AttrValue};

    // ── Empty document ──────────────────────────────────────────────

    #[test]
    fn parse_empty_document() {
        let doc = parse_markdown("");
        assert_eq!(doc.node_type, NodeType::Doc);
        assert_eq!(doc.child_count(), 0);
    }

    // ── Single paragraph ────────────────────────────────────────────

    #[test]
    fn parse_single_paragraph() {
        let doc = parse_markdown("Hello world\n");
        assert_eq!(doc.node_type, NodeType::Doc);
        assert_eq!(doc.child_count(), 1);

        let para = doc.child(0);
        assert_eq!(para.node_type, NodeType::Paragraph);
        assert_eq!(para.child_count(), 1);

        let text = para.child(0);
        assert_eq!(text.node_type, NodeType::Text);
        assert_eq!(text.text(), Some("Hello world"));
    }

    // ── Heading ─────────────────────────────────────────────────────

    #[test]
    fn parse_heading() {
        let doc = parse_markdown("## My Heading\n");
        assert_eq!(doc.child_count(), 1);

        let heading = doc.child(0);
        assert_eq!(heading.node_type, NodeType::Heading);
        assert_eq!(
            get_attr(&heading.attrs, "level"),
            Some(&AttrValue::Int(2))
        );

        let text = heading.child(0);
        assert_eq!(text.node_type, NodeType::Text);
        assert_eq!(text.text(), Some("My Heading"));
    }

    // ── Bold text ───────────────────────────────────────────────────

    #[test]
    fn parse_bold_text() {
        let doc = parse_markdown("Hello **bold** world\n");
        let para = doc.child(0);
        assert_eq!(para.node_type, NodeType::Paragraph);

        // Should be: "Hello " (no marks), "bold" (Strong), " world" (no marks)
        assert_eq!(para.child_count(), 3);

        let hello = para.child(0);
        assert_eq!(hello.text(), Some("Hello "));
        assert!(hello.marks.is_empty());

        let bold = para.child(1);
        assert_eq!(bold.text(), Some("bold"));
        assert_eq!(bold.marks.len(), 1);
        assert_eq!(bold.marks[0].mark_type, MarkType::Strong);

        let world = para.child(2);
        assert_eq!(world.text(), Some(" world"));
        assert!(world.marks.is_empty());
    }

    // ── Italic text ─────────────────────────────────────────────────

    #[test]
    fn parse_italic_text() {
        let doc = parse_markdown("Hello *italic* world\n");
        let para = doc.child(0);

        assert_eq!(para.child_count(), 3);

        let italic = para.child(1);
        assert_eq!(italic.text(), Some("italic"));
        assert_eq!(italic.marks.len(), 1);
        assert_eq!(italic.marks[0].mark_type, MarkType::Em);
    }

    // ── Inline code ─────────────────────────────────────────────────

    #[test]
    fn parse_inline_code() {
        let doc = parse_markdown("Use `code` here\n");
        let para = doc.child(0);

        assert_eq!(para.child_count(), 3);

        let code = para.child(1);
        assert_eq!(code.text(), Some("code"));
        assert_eq!(code.marks.len(), 1);
        assert_eq!(code.marks[0].mark_type, MarkType::Code);
    }

    // ── Bullet list ─────────────────────────────────────────────────

    #[test]
    fn parse_bullet_list() {
        let doc = parse_markdown("- item 1\n- item 2\n");
        assert_eq!(doc.child_count(), 1);

        let list = doc.child(0);
        assert_eq!(list.node_type, NodeType::BulletList);
        assert_eq!(list.child_count(), 2);

        let item1 = list.child(0);
        assert_eq!(item1.node_type, NodeType::ListItem);
        assert_eq!(item1.child_count(), 1);

        let para = item1.child(0);
        assert_eq!(para.node_type, NodeType::Paragraph);
        assert_eq!(para.text_content(), "item 1");
    }

    // ── Ordered list ────────────────────────────────────────────────

    #[test]
    fn parse_ordered_list() {
        let doc = parse_markdown("1. first\n2. second\n");
        assert_eq!(doc.child_count(), 1);

        let list = doc.child(0);
        assert_eq!(list.node_type, NodeType::OrderedList);
        assert_eq!(list.child_count(), 2);
        assert_eq!(get_attr(&list.attrs, "start"), Some(&AttrValue::Int(1)));

        let item1 = list.child(0);
        assert_eq!(item1.node_type, NodeType::ListItem);
        assert_eq!(item1.child(0).text_content(), "first");
    }

    // ── Blockquote ──────────────────────────────────────────────────

    #[test]
    fn parse_blockquote() {
        let doc = parse_markdown("> quoted text\n");
        assert_eq!(doc.child_count(), 1);

        let bq = doc.child(0);
        assert_eq!(bq.node_type, NodeType::Blockquote);
        assert_eq!(bq.child_count(), 1);

        let para = bq.child(0);
        assert_eq!(para.node_type, NodeType::Paragraph);
        assert_eq!(para.text_content(), "quoted text");
    }

    // ── Code block ──────────────────────────────────────────────────

    #[test]
    fn parse_code_block() {
        let doc = parse_markdown("```rust\nfn main() {}\n```\n");
        assert_eq!(doc.child_count(), 1);

        let code = doc.child(0);
        assert_eq!(code.node_type, NodeType::CodeBlock);
        assert_eq!(
            get_attr(&code.attrs, "language"),
            Some(&AttrValue::String("rust".to_string()))
        );

        assert_eq!(code.child_count(), 1);
        let text = code.child(0);
        assert_eq!(text.node_type, NodeType::Text);
        assert_eq!(text.text(), Some("fn main() {}\n"));
    }

    // ── Horizontal rule ─────────────────────────────────────────────

    #[test]
    fn parse_horizontal_rule() {
        let doc = parse_markdown("---\n");
        assert_eq!(doc.child_count(), 1);

        let hr = doc.child(0);
        assert_eq!(hr.node_type, NodeType::HorizontalRule);
    }

    // ── Link ────────────────────────────────────────────────────────

    #[test]
    fn parse_link() {
        let doc = parse_markdown("Click [here](https://example.com) please\n");
        let para = doc.child(0);
        assert_eq!(para.node_type, NodeType::Paragraph);

        // "Click " (no marks), "here" (Link mark), " please" (no marks)
        assert_eq!(para.child_count(), 3);

        let link = para.child(1);
        assert_eq!(link.text(), Some("here"));
        assert_eq!(link.marks.len(), 1);
        assert_eq!(link.marks[0].mark_type, MarkType::Link);
        assert_eq!(
            get_attr(&link.marks[0].attrs, "href"),
            Some(&AttrValue::String("https://example.com".to_string()))
        );
    }

    // ── Mixed content ───────────────────────────────────────────────

    #[test]
    fn parse_mixed_content() {
        let md = "# Title\n\nA paragraph.\n\n- item 1\n- item 2\n\n```js\nconsole.log();\n```\n";
        let doc = parse_markdown(md);

        assert_eq!(doc.child_count(), 4);
        assert_eq!(doc.child(0).node_type, NodeType::Heading);
        assert_eq!(doc.child(1).node_type, NodeType::Paragraph);
        assert_eq!(doc.child(2).node_type, NodeType::BulletList);
        assert_eq!(doc.child(3).node_type, NodeType::CodeBlock);
    }

    // ── Bold + italic nested ────────────────────────────────────────

    #[test]
    fn parse_bold_italic_nested() {
        let doc = parse_markdown("***bold and italic***\n");
        let para = doc.child(0);

        // The outermost is **, inner is *
        // Should produce text with both Strong and Em marks
        assert_eq!(para.child_count(), 1);
        let text = para.child(0);
        assert_eq!(text.text(), Some("bold and italic"));
        assert!(text.marks.iter().any(|m| m.mark_type == MarkType::Strong));
        assert!(text.marks.iter().any(|m| m.mark_type == MarkType::Em));
    }

    // ── Hard break ──────────────────────────────────────────────────

    #[test]
    fn parse_hard_break() {
        let doc = parse_markdown("line one  \nline two\n");
        let para = doc.child(0);

        // Should be: "line one" (text), HardBreak, "line two" (text)
        assert_eq!(para.child_count(), 3);
        assert_eq!(para.child(0).text(), Some("line one"));
        assert_eq!(para.child(1).node_type, NodeType::HardBreak);
        assert_eq!(para.child(2).text(), Some("line two"));
    }

    // ── Round-trip text content ─────────────────────────────────────

    #[test]
    fn round_trip_text_content() {
        let sources = [
            "Hello world\n",
            "**bold** text\n",
            "- item one\n- item two\n",
        ];

        for src in sources {
            let doc = parse_markdown(src);
            let content = doc.text_content();
            // The visible text should be present (minus markdown syntax)
            // For "**bold** text" → "bold text"
            // For "- item one\n- item two\n" → "item oneitem two"
            assert!(
                !content.is_empty(),
                "text_content should not be empty for: {src:?}"
            );
        }
    }

    // ── Image ───────────────────────────────────────────────────────

    #[test]
    fn parse_image() {
        let doc = parse_markdown("![alt text](image.png)\n");
        let para = doc.child(0);

        assert_eq!(para.child_count(), 1);
        let img = para.child(0);
        assert_eq!(img.node_type, NodeType::Image);
        assert_eq!(
            get_attr(&img.attrs, "src"),
            Some(&AttrValue::String("image.png".to_string()))
        );
        assert_eq!(
            get_attr(&img.attrs, "alt"),
            Some(&AttrValue::String("alt text".to_string()))
        );
    }

    // ── Strikethrough ───────────────────────────────────────────────

    #[test]
    fn parse_strikethrough() {
        let doc = parse_markdown("~~deleted~~\n");
        let para = doc.child(0);

        assert_eq!(para.child_count(), 1);
        let text = para.child(0);
        assert_eq!(text.text(), Some("deleted"));
        assert_eq!(text.marks.len(), 1);
        assert_eq!(text.marks[0].mark_type, MarkType::Strike);
    }

    // ── Code block without language ─────────────────────────────────

    #[test]
    fn parse_code_block_no_language() {
        let doc = parse_markdown("```\nsome code\n```\n");
        let code = doc.child(0);
        assert_eq!(code.node_type, NodeType::CodeBlock);
        assert!(code.attrs.is_empty());
    }

    // ── Heading levels ──────────────────────────────────────────────

    #[test]
    fn parse_heading_levels() {
        for level in 1..=6 {
            let hashes = "#".repeat(level);
            let md = format!("{hashes} Level {level}\n");
            let doc = parse_markdown(&md);
            let heading = doc.child(0);
            assert_eq!(heading.node_type, NodeType::Heading);
            assert_eq!(
                get_attr(&heading.attrs, "level"),
                Some(&AttrValue::Int(level as i64))
            );
        }
    }

    // ── Link with title ─────────────────────────────────────────────

    #[test]
    fn parse_link_with_title() {
        let doc = parse_markdown("[link](http://example.com \"Example\")\n");
        let para = doc.child(0);
        let link_node = para.child(0);
        assert_eq!(link_node.marks.len(), 1);
        assert_eq!(
            get_attr(&link_node.marks[0].attrs, "title"),
            Some(&AttrValue::String("Example".to_string()))
        );
    }

    // ── Ordered list with non-1 start ───────────────────────────────

    #[test]
    fn parse_ordered_list_start_number() {
        let doc = parse_markdown("3. third\n4. fourth\n");
        let list = doc.child(0);
        assert_eq!(list.node_type, NodeType::OrderedList);
        assert_eq!(get_attr(&list.attrs, "start"), Some(&AttrValue::Int(3)));
    }

    // ── Underscore italic ───────────────────────────────────────────

    #[test]
    fn parse_underscore_italic() {
        let doc = parse_markdown("Hello _italic_ world\n");
        let para = doc.child(0);

        assert_eq!(para.child_count(), 3);

        let hello = para.child(0);
        assert_eq!(hello.text(), Some("Hello "));
        assert!(hello.marks.is_empty());

        let em = para.child(1);
        assert_eq!(em.text(), Some("italic"));
        assert_eq!(em.marks.len(), 1);
        assert_eq!(em.marks[0].mark_type, MarkType::Em);

        let world = para.child(2);
        assert_eq!(world.text(), Some(" world"));
        assert!(world.marks.is_empty());
    }

    // ── Underscore bold ─────────────────────────────────────────────

    #[test]
    fn parse_underscore_bold() {
        let doc = parse_markdown("Hello __bold__ world\n");
        let para = doc.child(0);

        assert_eq!(para.child_count(), 3);

        let hello = para.child(0);
        assert_eq!(hello.text(), Some("Hello "));
        assert!(hello.marks.is_empty());

        let strong = para.child(1);
        assert_eq!(strong.text(), Some("bold"));
        assert_eq!(strong.marks.len(), 1);
        assert_eq!(strong.marks[0].mark_type, MarkType::Strong);

        let world = para.child(2);
        assert_eq!(world.text(), Some(" world"));
        assert!(world.marks.is_empty());
    }

    // ── Underscores inside words should NOT trigger emphasis ────────

    #[test]
    fn underscores_inside_words_are_literal() {
        let doc = parse_markdown("foo_bar_baz\n");
        let para = doc.child(0);

        // Should be a single text node with no marks
        assert_eq!(para.child_count(), 1);
        let text = para.child(0);
        assert_eq!(text.text(), Some("foo_bar_baz"));
        assert!(text.marks.is_empty());
    }

    // ── Nested list items ───────────────────────────────────────────

    #[test]
    fn parse_nested_list() {
        let md = "- parent\n  - child\n";
        let doc = parse_markdown(md);
        assert_eq!(doc.child_count(), 1);

        let outer = doc.child(0);
        assert_eq!(outer.node_type, NodeType::BulletList);
        // tree-sitter may represent this as a single item with a nested list,
        // or as two items — assert that it parses without panicking and
        // produces at least one list item.
        assert!(outer.child_count() >= 1);
        assert_eq!(outer.child(0).node_type, NodeType::ListItem);
    }

    // ── Backtick edge case: only backticks ──────────────────────────

    #[test]
    fn backtick_only_content_no_panic() {
        // Should NOT panic even when the string is only backticks with
        // no closing delimiter.
        let doc = parse_markdown("```\n");
        // This is a fenced code block opening; tree-sitter handles it.
        // The key assertion is that we don't panic.
        assert!(doc.child_count() <= 1);

        // A single backtick with no match should be literal.
        let doc2 = parse_markdown("` only backtick\n");
        let para = doc2.child(0);
        let content = para.text_content();
        assert!(content.contains('`'));
    }

    // ── find_backtick_content does not panic on empty input ─────────

    #[test]
    fn find_backtick_content_empty_slice() {
        // Regression: `while i <= chars.len() - count` would underflow
        // when chars is empty and count > 0.
        let result = find_backtick_content(&[], 0, 1);
        assert!(result.is_none());
    }
}

#[cfg(test)]
mod dunder_tests {
    use super::*;

    #[test]
    fn double_underscores_at_word_boundary_is_bold() {
        // CommonMark: __init__ IS bold when at word boundaries
        let doc = parse_markdown("__init__\n");
        let para = doc.child(0);
        assert_eq!(para.node_type, NodeType::Paragraph);
        assert_eq!(para.child_count(), 1);
        let text = para.child(0);
        assert_eq!(text.text(), Some("init"));
        assert!(!text.marks.is_empty());
    }

    #[test]
    fn single_underscores_dunder_is_literal() {
        let doc = parse_markdown("_foo_bar_\n");
        let para = doc.child(0);
        // Should be literal text, not italic
        assert_eq!(para.text_content(), "_foo_bar_");
    }
}
