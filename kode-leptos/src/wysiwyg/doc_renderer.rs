//! Render a `kode-doc::Node` tree into Leptos views for the WYSIWYG editor.
//!
//! This module is the replacement for `blocks.rs` (which walks a tree-sitter CST).
//! It walks the structured `kode_doc::Node` tree and produces the same HTML output
//! with the same CSS classes, so existing `wysiwyg.css` styles continue to apply.
//!
//! Every block-level element gets `data-pos-start` and `data-pos-end` attributes
//! containing token positions. These are used by the cursor positioning Effect to
//! map tree positions to DOM elements.

use std::sync::Arc;

use kode_doc::attrs::{get_attr, AttrValue};
use kode_doc::{Fragment, Mark, MarkType, Node, NodeType};
use leptos::prelude::*;

use crate::extension::Extension;
use crate::highlight::{self, html_escape, Language};

/// Render a kode-doc document tree into Leptos views for the WYSIWYG editor.
///
/// Each block node becomes a view with `data-pos-start` and `data-pos-end`
/// attributes containing token positions. These are used by the cursor
/// positioning Effect to map tree positions to DOM elements.
///
/// `parent_offset` is the absolute token position of `doc`'s opening token
/// in the overall document (0 for the root Doc node).
pub fn render_doc(
    doc: &Node,
    extensions: &[Arc<dyn Extension>],
    language_aliases: &[(String, String)],
) -> Vec<AnyView> {
    // Notify extensions that a new render pass is starting, so they can
    // reset counters and reuse cached state for unchanged blocks.
    for ext in extensions {
        ext.begin_render_pass();
    }

    // The resolve() function treats positions as offsets into doc.content
    // (position 0 = start of doc content, NOT doc's opening token).
    // So content_start for the root doc is 0.
    render_block_children(&doc.content, 0, extensions, language_aliases)
}

/// Render all block-level children of a fragment.
///
/// `content_start` is the absolute token position where this fragment's
/// content begins (i.e. the position after the parent's opening token).
fn render_block_children(
    content: &Fragment,
    content_start: usize,
    extensions: &[Arc<dyn Extension>],
    language_aliases: &[(String, String)],
) -> Vec<AnyView> {
    let mut views = Vec::new();
    let mut pos = content_start;

    for child in content.iter() {
        if let Some(v) = render_block_node(child, pos, extensions, language_aliases) {
            views.push(v);
        }
        pos += child.node_size();
    }

    views
}

/// Render a single block-level node.
///
/// `start` is the absolute token position of this node in the document.
/// For branch nodes, content starts at `start + 1` (after the opening token)
/// and ends at `start + 1 + content.size()` (before the closing token).
pub(crate) fn render_block_node(
    node: &Node,
    start: usize,
    extensions: &[Arc<dyn Extension>],
    language_aliases: &[(String, String)],
) -> Option<AnyView> {
    // For branch nodes: content occupies [start+1, start+1+content.size())
    let content_start = start + 1;
    let content_end = content_start + node.content.size();

    match node.node_type {
        // ── Paragraph ────────────────────────────────────────────────
        NodeType::Paragraph => {
            let inline_html = render_inline_content(&node.content);
            Some(
                view! {
                    <p class="wysiwyg-paragraph"
                        data-pos-start=content_start
                        data-pos-end=content_end
                        inner_html=inline_html />
                }
                .into_any(),
            )
        }

        // ── Heading ──────────────────────────────────────────────────
        NodeType::Heading => {
            let level = match get_attr(&node.attrs, "level") {
                Some(AttrValue::Int(n)) => *n as u8,
                _ => 1,
            };
            let inline_html = render_inline_content(&node.content);
            let class = format!("wysiwyg-heading wysiwyg-h{}", level);
            match level {
                1 => Some(
                    view! { <h1 class=class data-pos-start=content_start data-pos-end=content_end inner_html=inline_html /> }
                        .into_any(),
                ),
                2 => Some(
                    view! { <h2 class=class data-pos-start=content_start data-pos-end=content_end inner_html=inline_html /> }
                        .into_any(),
                ),
                3 => Some(
                    view! { <h3 class=class data-pos-start=content_start data-pos-end=content_end inner_html=inline_html /> }
                        .into_any(),
                ),
                4 => Some(
                    view! { <h4 class=class data-pos-start=content_start data-pos-end=content_end inner_html=inline_html /> }
                        .into_any(),
                ),
                5 => Some(
                    view! { <h5 class=class data-pos-start=content_start data-pos-end=content_end inner_html=inline_html /> }
                        .into_any(),
                ),
                _ => Some(
                    view! { <h6 class=class data-pos-start=content_start data-pos-end=content_end inner_html=inline_html /> }
                        .into_any(),
                ),
            }
        }

        // ── Blockquote ───────────────────────────────────────────────
        NodeType::Blockquote => {
            let children = render_block_children(&node.content, content_start, extensions, language_aliases);
            Some(
                view! {
                    <blockquote class="wysiwyg-blockquote"
                        data-pos-start=content_start
                        data-pos-end=content_end>
                        {children}
                    </blockquote>
                }
                .into_any(),
            )
        }

        // ── Bullet list ──────────────────────────────────────────────
        NodeType::BulletList => {
            let items = render_list_items(&node.content, content_start, extensions, language_aliases);
            Some(
                view! {
                    <ul class="wysiwyg-list wysiwyg-bullet-list"
                        data-pos-start=content_start
                        data-pos-end=content_end>
                        {items}
                    </ul>
                }
                .into_any(),
            )
        }

        // ── Ordered list ─────────────────────────────────────────────
        NodeType::OrderedList => {
            let start_num = match get_attr(&node.attrs, "start") {
                Some(AttrValue::Int(n)) => *n as i32,
                _ => 1,
            };
            let items = render_list_items(&node.content, content_start, extensions, language_aliases);
            Some(
                view! {
                    <ol class="wysiwyg-list wysiwyg-ordered-list"
                        start=start_num
                        data-pos-start=content_start
                        data-pos-end=content_end>
                        {items}
                    </ol>
                }
                .into_any(),
            )
        }

        // ── Code block ──────────────────────────────────────────────
        NodeType::CodeBlock => {
            let lang = match get_attr(&node.attrs, "language") {
                Some(AttrValue::String(s)) => s.as_str(),
                _ => "",
            };
            let content_text = node.text_content();

            // Check extensions first for custom rendering
            for ext in extensions {
                if ext.code_block_languages().contains(&lang) {
                    if let Some(ext_view) =
                        ext.render_code_block(lang, &content_text, start, start + node.node_size())
                    {
                        // Wrap in a marker div so the mousedown handler can
                        // detect clicks inside extension content and avoid
                        // stealing focus / repositioning the cursor.
                        return Some(
                            view! {
                                <div data-kode-extension=ext.name()
                                    data-pos-start=start
                                    data-pos-end={start + node.node_size()}>
                                    {ext_view}
                                </div>
                            }
                            .into_any(),
                        );
                    }
                }
            }

            // Default: syntax-highlighted code block
            let highlight_lang = match_language(lang, language_aliases);
            let highlighted_lines: Vec<String> = content_text
                .lines()
                .map(|line| highlight::highlight_line(line, highlight_lang))
                .collect();
            let mut code_html = highlighted_lines.join("\n");
            // A trailing \n in <pre> doesn't render a visible blank line
            // in HTML — the browser collapses it. Append an extra \n so
            // the last empty line is visible and the block expands.
            if content_text.ends_with('\n') {
                code_html.push('\n');
            }

            let lang_label = if lang.is_empty() {
                String::new()
            } else {
                lang.to_string()
            };

            Some(
                view! {
                    <div class="wysiwyg-code-block"
                        data-pos-start=content_start
                        data-pos-end=content_end>
                        {if !lang_label.is_empty() {
                            Some(view! { <div class="wysiwyg-code-lang">{lang_label}</div> })
                        } else {
                            None
                        }}
                        // NOTE: highlight_line() HTML-escapes source text before wrapping in <span> tags
                        <pre class="kode-content"><code data-pos-start=content_start data-pos-end=content_end inner_html=code_html /></pre>
                    </div>
                }
                .into_any(),
            )
        }

        // ── Horizontal rule ──────────────────────────────────────────
        // Leaf node: uses different position semantics than branch nodes.
        // A leaf occupies a single token (node_size() == 1), so there is no
        // open/close pair — we use `start` and `start + 1` directly.
        NodeType::HorizontalRule => Some(
            view! {
                <hr class="wysiwyg-hr"
                    data-pos-start=start
                    data-pos-end={start + 1} />
            }
            .into_any(),
        ),

        // ── Table ────────────────────────────────────────────────────
        NodeType::Table => {
            let rows = render_table_rows(&node.content, content_start);
            Some(
                view! {
                    <table class="wysiwyg-table"
                        data-pos-start=content_start
                        data-pos-end=content_end>
                        {rows}
                    </table>
                }
                .into_any(),
            )
        }

        // Table sub-nodes at top level (shouldn't happen outside a table)
        NodeType::TableRow | NodeType::TableHeader | NodeType::TableCell => None,

        // ── Inline-only types at block level (shouldn't happen, but be safe)
        NodeType::Text | NodeType::HardBreak | NodeType::Image => None,

        // ── Doc (nested doc — shouldn't happen, but recurse if it does)
        NodeType::Doc => {
            let children = render_block_children(&node.content, content_start, extensions, language_aliases);
            if children.is_empty() {
                None
            } else {
                Some(view! { <div class="wysiwyg-doc">{children}</div> }.into_any())
            }
        }

        // ── ListItem at top level (shouldn't happen outside a list)
        NodeType::ListItem => {
            let children = render_list_item_content(node, content_start, extensions, language_aliases);
            Some(
                view! {
                    <li class="wysiwyg-list-item"
                        data-pos-start=content_start
                        data-pos-end=content_end>
                        {children}
                    </li>
                }
                .into_any(),
            )
        }
    }
}

// ── List rendering ───────────────────────────────────────────────────────

/// Render list item children from a list's content fragment.
fn render_list_items(
    content: &Fragment,
    content_start: usize,
    extensions: &[Arc<dyn Extension>],
    language_aliases: &[(String, String)],
) -> Vec<AnyView> {
    let mut items = Vec::new();
    let mut pos = content_start;

    for child in content.iter() {
        // Schema invariant: lists contain only ListItem children.
        debug_assert!(
            child.node_type == NodeType::ListItem,
            "unexpected non-ListItem child in list: {:?}",
            child.node_type
        );
        if child.node_type == NodeType::ListItem {
            let item_content_start = pos + 1;
            let item_content_end = item_content_start + child.content.size();
            let children = render_list_item_content(child, item_content_start, extensions, language_aliases);
            items.push(
                view! {
                    <li class="wysiwyg-list-item"
                        data-pos-start=item_content_start
                        data-pos-end=item_content_end>
                        {children}
                    </li>
                }
                .into_any(),
            );
        }
        pos += child.node_size();
    }

    items
}

/// Render the content inside a list item.
///
/// List items contain block nodes (paragraphs, nested lists, etc.).
/// For paragraphs, we render their inline content directly as a `<span>`
/// rather than wrapping in `<p>` to match the existing blocks.rs behavior.
fn render_list_item_content(
    list_item: &Node,
    content_start: usize,
    extensions: &[Arc<dyn Extension>],
    language_aliases: &[(String, String)],
) -> Vec<AnyView> {
    let mut parts = Vec::new();
    let mut pos = content_start;

    for child in list_item.content.iter() {
        match child.node_type {
            NodeType::Paragraph => {
                let child_content_start = pos + 1;
                let child_content_end = child_content_start + child.content.size();
                // Render paragraph content inline (as <span>) inside list items
                let inline_html = render_inline_content(&child.content);
                parts.push(
                    view! {
                        <span data-pos-start=child_content_start
                              data-pos-end=child_content_end
                              inner_html=inline_html />
                    }
                    .into_any(),
                );
            }
            _ => {
                // Nested lists, blockquotes, code blocks, etc. — render as blocks
                if let Some(v) = render_block_node(child, pos, extensions, language_aliases) {
                    parts.push(v);
                }
            }
        }
        pos += child.node_size();
    }

    parts
}

// ── Table rendering ─────────────────────────────────────────────────────

/// Render the rows of a table node.
///
/// TableHeader children become `<thead><tr>...</tr></thead>`.
/// TableRow children become `<tr>...</tr>` inside an implicit `<tbody>`.
fn render_table_rows(content: &Fragment, content_start: usize) -> Vec<AnyView> {
    let mut views = Vec::new();
    let mut pos = content_start;

    for child in content.iter() {
        let row_content_start = pos + 1;
        let row_content_end = row_content_start + child.content.size();

        match child.node_type {
            NodeType::TableHeader => {
                let cells = render_table_cells(child, row_content_start, "th");
                views.push(
                    view! {
                        <thead data-pos-start=row_content_start data-pos-end=row_content_end>
                            <tr class="wysiwyg-table-row">{cells}</tr>
                        </thead>
                    }
                    .into_any(),
                );
            }
            NodeType::TableRow => {
                let cells = render_table_cells(child, row_content_start, "td");
                views.push(
                    view! {
                        <tr class="wysiwyg-table-row"
                            data-pos-start=row_content_start
                            data-pos-end=row_content_end>
                            {cells}
                        </tr>
                    }
                    .into_any(),
                );
            }
            _ => {}
        }
        pos += child.node_size();
    }

    views
}

/// Render the cells of a table row as `<th>` or `<td>` elements.
fn render_table_cells(row: &Node, content_start: usize, tag: &str) -> Vec<AnyView> {
    let mut cells = Vec::new();
    let mut pos = content_start;

    for cell in row.content.iter() {
        let cell_content_start = pos + 1;
        let cell_content_end = cell_content_start + cell.content.size();
        let inline_html = render_inline_content(&cell.content);

        let cell_view = if tag == "th" {
            view! {
                <th class="wysiwyg-table-cell"
                    data-pos-start=cell_content_start
                    data-pos-end=cell_content_end
                    inner_html=inline_html />
            }
            .into_any()
        } else {
            view! {
                <td class="wysiwyg-table-cell"
                    data-pos-start=cell_content_start
                    data-pos-end=cell_content_end
                    inner_html=inline_html />
            }
            .into_any()
        };
        cells.push(cell_view);
        pos += cell.node_size();
    }

    cells
}

// ── Inline content rendering ─────────────────────────────────────────────

/// Render a fragment of inline content (text nodes with marks, hard breaks,
/// images) into an HTML string.
///
/// This produces the same HTML as the character-scanning `render_inline_markdown`
/// in blocks.rs, but works from the structured Node tree where marks are already
/// parsed. No need to re-parse markdown syntax.
fn render_inline_content(content: &Fragment) -> String {
    let mut html = String::new();

    for child in content.iter() {
        match child.node_type {
            NodeType::Text => {
                let text = child.text().unwrap_or("");
                if child.marks.is_empty() {
                    html.push_str(&html_escape(text));
                } else {
                    render_marked_text(&mut html, text, &child.marks);
                }
            }
            NodeType::HardBreak => {
                html.push_str("<br>");
            }
            NodeType::Image => {
                let src = match get_attr(&child.attrs, "src") {
                    Some(AttrValue::String(s)) => html_escape(&sanitize_url(s)),
                    _ => String::new(),
                };
                let alt = match get_attr(&child.attrs, "alt") {
                    Some(AttrValue::String(s)) => html_escape(s),
                    _ => String::new(),
                };
                html.push_str(&format!(
                    "<img class=\"wysiwyg-image\" src=\"{}\" alt=\"{}\" />",
                    src, alt
                ));
            }
            _ => {
                // Unexpected inline node — render text content as escaped text
                html.push_str(&html_escape(&child.text_content()));
            }
        }
    }

    html
}

/// Render text with marks by nesting HTML tags.
///
/// Mark ordering follows MarkType sort order: Strong > Em > Code > Link > Strike.
/// We sort before rendering to ensure consistent nesting regardless of input order.
/// We open tags from the first mark outward and close in reverse.
fn render_marked_text(html: &mut String, text: &str, marks: &[Mark]) {
    let mut sorted_marks = marks.to_vec();
    sorted_marks.sort_by_key(|m| m.mark_type);

    // Open marks in order
    for mark in &sorted_marks {
        match mark.mark_type {
            MarkType::Strong => html.push_str("<strong>"),
            MarkType::Em => html.push_str("<em>"),
            MarkType::Code => html.push_str("<code class=\"wysiwyg-inline-code\">"),
            MarkType::Link => {
                let href = match get_attr(&mark.attrs, "href") {
                    Some(AttrValue::String(s)) => html_escape(&sanitize_url(s)),
                    _ => String::new(),
                };
                html.push_str(&format!("<a class=\"wysiwyg-link\" href=\"{}\">", href));
            }
            MarkType::Strike => html.push_str("<del>"),
        }
    }

    // The text content
    html.push_str(&html_escape(text));

    // Close marks in reverse order
    for mark in sorted_marks.iter().rev() {
        match mark.mark_type {
            MarkType::Strong => html.push_str("</strong>"),
            MarkType::Em => html.push_str("</em>"),
            MarkType::Code => html.push_str("</code>"),
            MarkType::Link => html.push_str("</a>"),
            MarkType::Strike => html.push_str("</del>"),
        }
    }
}

// ── Utility ──────────────────────────────────────────────────────────────

/// Sanitize a URL, allowing only safe schemes.
///
/// Permits `http://`, `https://`, `/` (root-relative), `#` (fragment),
/// and `mailto:`. Strips anything else (e.g. `javascript:`) to prevent XSS.
fn sanitize_url(url: &str) -> String {
    let trimmed = url.trim();
    let lower = trimmed.to_lowercase();

    // Allow explicitly safe absolute schemes
    if lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
    {
        return trimmed.to_string();
    }

    // Block protocol-relative URLs (//host/path) — browsers resolve as absolute URLs
    if trimmed.starts_with("//") {
        return String::new();
    }

    // Block any other scheme (e.g. javascript:, data:, vbscript:).
    // A scheme is present when ':' appears before the first '/'.
    let colon_pos = trimmed.find(':');
    let slash_pos = trimmed.find('/');
    let has_scheme = match (colon_pos, slash_pos) {
        (Some(c), Some(s)) => c < s,
        (Some(_), None) => true,
        _ => false,
    };

    if has_scheme {
        // Unknown scheme — reject
        String::new()
    } else {
        // Relative path, /, #fragment — allow
        trimmed.to_string()
    }
}

/// Map a code block language string to a highlight Language.
///
/// Checks `aliases` first — each entry maps a custom name to a built-in
/// language (e.g. `("chartml", "yaml")`).  If no alias matches, falls back
/// to the hardcoded table.
fn match_language(info: &str, aliases: &[(String, String)]) -> Language {
    let lang = info.split_whitespace().next().unwrap_or("");
    let lower = lang.to_lowercase();

    // Check aliases first (lowercase both sides for case-insensitive matching)
    let resolved = aliases
        .iter()
        .find(|(from, _)| from == &lower)
        .map(|(_, to)| to.to_lowercase())
        .unwrap_or(lower);

    // Delegate to the shared language resolution in highlight.rs
    highlight::language_from_info_string(&resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kode_doc::attrs::{code_block_attrs, heading_attrs, image_attrs, link_attrs, ordered_list_attrs};
    use kode_doc::{Fragment, Mark, MarkType, Node, NodeType};

    // ── Inline rendering tests ──────────────────────────────────────

    #[test]
    fn inline_plain_text() {
        let frag = Fragment::from_node(Node::new_text("hello world"));
        let html = render_inline_content(&frag);
        assert_eq!(html, "hello world");
    }

    #[test]
    fn inline_bold_text() {
        let frag = Fragment::from_vec(vec![
            Node::new_text("hello "),
            Node::new_text_with_marks("bold", vec![Mark::new(MarkType::Strong)]),
            Node::new_text(" text"),
        ]);
        let html = render_inline_content(&frag);
        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("hello "));
        assert!(html.contains(" text"));
    }

    #[test]
    fn inline_italic_text() {
        let frag = Fragment::from_node(Node::new_text_with_marks(
            "italic",
            vec![Mark::new(MarkType::Em)],
        ));
        let html = render_inline_content(&frag);
        assert_eq!(html, "<em>italic</em>");
    }

    #[test]
    fn inline_code_text() {
        let frag = Fragment::from_node(Node::new_text_with_marks(
            "foo()",
            vec![Mark::new(MarkType::Code)],
        ));
        let html = render_inline_content(&frag);
        assert!(html.contains("<code class=\"wysiwyg-inline-code\">"));
        assert!(html.contains("foo()"));
    }

    #[test]
    fn inline_link() {
        let frag = Fragment::from_node(Node::new_text_with_marks(
            "click here",
            vec![Mark::with_attrs(
                MarkType::Link,
                link_attrs("https://example.com", None),
            )],
        ));
        let html = render_inline_content(&frag);
        assert!(html.contains("href=\"https://example.com\""));
        assert!(html.contains("click here</a>"));
    }

    #[test]
    fn inline_strikethrough() {
        let frag = Fragment::from_node(Node::new_text_with_marks(
            "deleted",
            vec![Mark::new(MarkType::Strike)],
        ));
        let html = render_inline_content(&frag);
        assert_eq!(html, "<del>deleted</del>");
    }

    #[test]
    fn inline_nested_marks() {
        // Strong + Em (Strong comes first in sort order)
        let frag = Fragment::from_node(Node::new_text_with_marks(
            "both",
            vec![Mark::new(MarkType::Strong), Mark::new(MarkType::Em)],
        ));
        let html = render_inline_content(&frag);
        assert_eq!(html, "<strong><em>both</em></strong>");
    }

    #[test]
    fn inline_hard_break() {
        let frag = Fragment::from_vec(vec![
            Node::new_text("line1"),
            Node::leaf(NodeType::HardBreak),
            Node::new_text("line2"),
        ]);
        let html = render_inline_content(&frag);
        assert_eq!(html, "line1<br>line2");
    }

    #[test]
    fn inline_image() {
        let frag = Fragment::from_node(Node::leaf_with_attrs(
            NodeType::Image,
            image_attrs("img.png", "An image", None),
        ));
        let html = render_inline_content(&frag);
        assert!(html.contains("src=\"img.png\""));
        assert!(html.contains("alt=\"An image\""));
    }

    #[test]
    fn inline_html_escape() {
        let frag = Fragment::from_node(Node::new_text("<script>alert('xss')</script>"));
        let html = render_inline_content(&frag);
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }

    // ── Block rendering tests ────────────────────────────────────────

    #[test]
    fn render_empty_doc() {
        let doc = Node::branch(NodeType::Doc, Fragment::empty());
        let views = render_doc(&doc, &[], &[]);
        assert!(views.is_empty());
    }

    #[test]
    fn render_doc_with_paragraph() {
        let para = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hello world")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(para));
        let views = render_doc(&doc, &[], &[]);
        assert_eq!(views.len(), 1);
    }

    #[test]
    fn render_doc_with_heading() {
        let heading = Node::branch_with_attrs(
            NodeType::Heading,
            heading_attrs(2),
            Fragment::from_node(Node::new_text("Title")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(heading));
        let views = render_doc(&doc, &[], &[]);
        assert_eq!(views.len(), 1);
    }

    #[test]
    fn render_doc_with_bullet_list() {
        let item1 = Node::branch(
            NodeType::ListItem,
            Fragment::from_node(Node::branch(
                NodeType::Paragraph,
                Fragment::from_node(Node::new_text("item 1")),
            )),
        );
        let item2 = Node::branch(
            NodeType::ListItem,
            Fragment::from_node(Node::branch(
                NodeType::Paragraph,
                Fragment::from_node(Node::new_text("item 2")),
            )),
        );
        let list = Node::branch(
            NodeType::BulletList,
            Fragment::from_vec(vec![item1, item2]),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(list));
        let views = render_doc(&doc, &[], &[]);
        assert_eq!(views.len(), 1);
    }

    #[test]
    fn render_doc_with_code_block() {
        let code = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("sql"),
            Fragment::from_node(Node::new_text("SELECT 1")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(code));
        let views = render_doc(&doc, &[], &[]);
        assert_eq!(views.len(), 1);
    }

    #[test]
    fn render_doc_with_hr() {
        let hr = Node::leaf(NodeType::HorizontalRule);
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(hr));
        let views = render_doc(&doc, &[], &[]);
        assert_eq!(views.len(), 1);
    }

    #[test]
    fn render_doc_with_blockquote() {
        let bq = Node::branch(
            NodeType::Blockquote,
            Fragment::from_node(Node::branch(
                NodeType::Paragraph,
                Fragment::from_node(Node::new_text("quoted")),
            )),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(bq));
        let views = render_doc(&doc, &[], &[]);
        assert_eq!(views.len(), 1);
    }

    #[test]
    fn render_doc_with_ordered_list() {
        let item = Node::branch(
            NodeType::ListItem,
            Fragment::from_node(Node::branch(
                NodeType::Paragraph,
                Fragment::from_node(Node::new_text("first")),
            )),
        );
        let list = Node::branch_with_attrs(
            NodeType::OrderedList,
            ordered_list_attrs(3),
            Fragment::from_node(item),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(list));
        let views = render_doc(&doc, &[], &[]);
        assert_eq!(views.len(), 1);
    }

    #[test]
    fn render_doc_multiple_blocks() {
        let heading = Node::branch_with_attrs(
            NodeType::Heading,
            heading_attrs(1),
            Fragment::from_node(Node::new_text("Title")),
        );
        let para = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Body text")),
        );
        let hr = Node::leaf(NodeType::HorizontalRule);
        let doc = Node::branch(
            NodeType::Doc,
            Fragment::from_vec(vec![heading, para, hr]),
        );
        let views = render_doc(&doc, &[], &[]);
        assert_eq!(views.len(), 3);
    }

    // ── Mark ordering tests ─────────────────────────────────────────

    #[test]
    fn marks_inverse_order_produces_same_output() {
        // Pass marks in [Em, Strong] order — should still render Strong outside Em
        let frag = Fragment::from_node(Node::new_text_with_marks(
            "text",
            vec![Mark::new(MarkType::Em), Mark::new(MarkType::Strong)],
        ));
        let html = render_inline_content(&frag);
        assert_eq!(html, "<strong><em>text</em></strong>");
    }

    // ── sanitize_url tests ──────────────────────────────────────────

    #[test]
    fn sanitize_url_allows_https() {
        assert_eq!(sanitize_url("https://example.com"), "https://example.com");
    }

    #[test]
    fn sanitize_url_allows_http() {
        assert_eq!(sanitize_url("http://example.com"), "http://example.com");
    }

    #[test]
    fn sanitize_url_allows_mailto() {
        assert_eq!(sanitize_url("mailto:user@example.com"), "mailto:user@example.com");
    }

    #[test]
    fn sanitize_url_allows_relative_path() {
        assert_eq!(sanitize_url("img.png"), "img.png");
        assert_eq!(sanitize_url("../images/photo.jpg"), "../images/photo.jpg");
    }

    #[test]
    fn sanitize_url_allows_root_relative() {
        assert_eq!(sanitize_url("/path/to/file"), "/path/to/file");
    }

    #[test]
    fn sanitize_url_allows_fragment() {
        assert_eq!(sanitize_url("#section"), "#section");
    }

    #[test]
    fn sanitize_url_blocks_javascript() {
        assert_eq!(sanitize_url("javascript:alert(1)"), "");
    }

    #[test]
    fn sanitize_url_blocks_javascript_uppercase() {
        assert_eq!(sanitize_url("JAVASCRIPT:alert(1)"), "");
    }

    #[test]
    fn sanitize_url_blocks_data_uri() {
        assert_eq!(sanitize_url("data:text/html,<script>alert(1)</script>"), "");
    }

    #[test]
    fn sanitize_url_blocks_vbscript() {
        assert_eq!(sanitize_url("vbscript:foo"), "");
    }

    #[test]
    fn sanitize_url_blocks_protocol_relative() {
        assert_eq!(sanitize_url("//evil.com/script.js"), "");
    }

    #[test]
    fn sanitize_url_then_html_escape_order_is_correct() {
        // sanitize_url must run BEFORE html_escape so that the scheme check
        // sees the raw URL, not HTML-entity-encoded text.
        let raw = "javascript:alert(1)";
        assert_eq!(sanitize_url(raw), "");
        assert_eq!(html_escape(&sanitize_url(raw)), "");
    }

    #[test]
    fn sanitize_url_empty_string() {
        assert_eq!(sanitize_url(""), "");
    }

    // ── Language alias tests ────────────────────────────────────────

    #[test]
    fn match_language_builtin() {
        assert_eq!(match_language("sql", &[]), Language::Sql);
        assert_eq!(match_language("yaml", &[]), Language::Yaml);
        assert_eq!(match_language("yml", &[]), Language::Yaml);
        assert_eq!(match_language("chartml", &[]), Language::Yaml);
        assert_eq!(match_language("markdown", &[]), Language::Markdown);
        assert_eq!(match_language("unknown", &[]), Language::Plain);
    }

    #[test]
    fn match_language_alias_resolves() {
        let aliases = vec![
            ("chartml".to_string(), "yaml".to_string()),
            ("hcl".to_string(), "sql".to_string()),
        ];
        assert_eq!(match_language("chartml", &aliases), Language::Yaml);
        assert_eq!(match_language("hcl", &aliases), Language::Sql);
        // Built-in still works
        assert_eq!(match_language("sql", &aliases), Language::Sql);
        // Known language resolves directly
        assert_eq!(match_language("python", &aliases), Language::Python);
        // Unknown stays plain
        assert_eq!(match_language("brainfuck", &aliases), Language::Plain);
    }

    #[test]
    fn match_language_alias_case_insensitive_key() {
        let aliases = vec![("chartml".to_string(), "yaml".to_string())];
        assert_eq!(match_language("ChartML", &aliases), Language::Yaml);
        assert_eq!(match_language("CHARTML", &aliases), Language::Yaml);
    }

    #[test]
    fn match_language_alias_case_insensitive_target() {
        let aliases = vec![("chartml".to_string(), "YAML".to_string())];
        assert_eq!(match_language("chartml", &aliases), Language::Yaml);
    }
}
