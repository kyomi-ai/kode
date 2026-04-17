use arborium_tree_sitter::{InputEdit, Language, Parser, Point, Tree};

use crate::nodes::NodeKind;

/// Wraps a tree-sitter parser configured for markdown.
/// Supports incremental re-parsing after edits.
pub struct MarkdownTree {
    parser: Parser,
    tree: Option<Tree>,
    source: String,
}

impl MarkdownTree {
    /// Create a new markdown parser and parse the given source.
    pub fn new(source: &str) -> Self {
        let language = Language::new(arborium_markdown::language());
        let mut parser = Parser::new();
        parser
            .set_language(&language)
            .expect("failed to set markdown language");

        let tree = parser.parse(source, None);

        Self {
            parser,
            tree,
            source: source.to_string(),
        }
    }

    /// Get the current source text.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Get the current parse tree, if available.
    pub fn tree(&self) -> Option<&Tree> {
        self.tree.as_ref()
    }

    /// Replace the entire source and re-parse from scratch.
    pub fn set_source(&mut self, source: &str) {
        self.source = source.to_string();
        self.tree = self.parser.parse(source, None);
    }

    /// Apply an edit and incrementally re-parse.
    ///
    /// `start_byte` / `old_end_byte` / `new_end_byte` describe the edit in byte offsets.
    /// Points describe the same edit in row/column coordinates.
    ///
    /// # Panics
    /// Panics if `start_byte` or `old_end_byte` are not on UTF-8 char boundaries.
    pub fn edit(
        &mut self,
        start_byte: usize,
        old_end_byte: usize,
        new_text: &str,
        start_point: Point,
        old_end_point: Point,
    ) {
        // Apply the edit to our source string
        let new_end_byte = start_byte + new_text.len();
        self.source.replace_range(start_byte..old_end_byte, new_text);

        // Calculate new end point
        let new_end_point = byte_offset_to_point(&self.source, new_end_byte);

        // Tell tree-sitter about the edit
        if let Some(tree) = &mut self.tree {
            tree.edit(&InputEdit {
                start_byte,
                old_end_byte,
                new_end_byte,
                start_position: start_point,
                old_end_position: old_end_point,
                new_end_position: new_end_point,
            });
        }

        // Re-parse incrementally
        self.tree = self.parser.parse(&self.source, self.tree.as_ref());
    }

    /// Get the S-expression representation of the parse tree (for debugging).
    pub fn sexp(&self) -> Option<String> {
        self.tree.as_ref().map(|t| t.root_node().to_sexp())
    }

    /// Walk the top-level blocks of the document, calling the visitor for each.
    pub fn walk_blocks<F>(&self, mut visitor: F)
    where
        F: FnMut(BlockInfo),
    {
        let Some(tree) = &self.tree else { return };
        let root = tree.root_node();
        walk_blocks_recursive(&root, &self.source, &mut visitor);
    }

    /// Find the block node at the given byte offset.
    pub fn block_at_byte(&self, byte_offset: usize) -> Option<BlockInfo> {
        let tree = self.tree.as_ref()?;
        let root = tree.root_node();

        // Find the deepest named node at this offset
        let node = root.named_descendant_for_byte_range(byte_offset, byte_offset)?;

        // Walk up to find the nearest block-level node
        let mut current = node;
        loop {
            let kind = NodeKind::from_ts_kind(current.kind());
            if kind.is_block() {
                return Some(block_info_from_node(&current, &self.source));
            }
            match current.parent() {
                Some(parent) if parent.kind() != "document" => current = parent,
                _ => return Some(block_info_from_node(&current, &self.source)),
            }
        }
    }

    /// Find the innermost node at the given byte offset.
    pub fn node_at_byte(&self, byte_offset: usize) -> Option<NodeInfo> {
        let tree = self.tree.as_ref()?;
        let root = tree.root_node();
        let node = root.descendant_for_byte_range(byte_offset, byte_offset)?;
        let kind = refine_node_kind(&node);
        Some(NodeInfo {
            kind,
            start_byte: node.start_byte(),
            end_byte: node.end_byte(),
            start_point: node.start_position(),
            end_point: node.end_position(),
        })
    }
}

/// Information about a block-level element.
#[derive(Debug, Clone)]
pub struct BlockInfo {
    pub kind: NodeKind,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_point: Point,
    pub end_point: Point,
    /// The raw text of this block.
    pub text: String,
}

/// Information about any node in the tree.
#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub kind: NodeKind,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_point: Point,
    pub end_point: Point,
}

fn is_block_node(kind: &str) -> bool {
    NodeKind::from_ts_kind(kind).is_block()
}

/// Refine a NodeKind from tree-sitter, resolving heading levels and list types.
fn refine_node_kind(node: &arborium_tree_sitter::Node) -> NodeKind {
    let mut kind = NodeKind::from_ts_kind(node.kind());

    // Refine heading level
    if matches!(kind, NodeKind::Heading { .. }) {
        let level = detect_heading_level(node);
        kind = NodeKind::Heading { level };
    }

    // Refine list type (bullet vs ordered)
    if matches!(kind, NodeKind::BulletList) {
        let ordered = node
            .children(&mut node.walk())
            .find(|c| c.kind() == "list_item")
            .map(|item| {
                item.children(&mut item.walk())
                    .any(|c| c.kind() == "list_marker_dot" || c.kind() == "list_marker_parenthesis")
            })
            .unwrap_or(false);
        kind = if ordered {
            NodeKind::OrderedList
        } else {
            NodeKind::BulletList
        };
    }

    kind
}

fn block_info_from_node(node: &arborium_tree_sitter::Node, source: &str) -> BlockInfo {
    let kind = refine_node_kind(node);

    let start_byte = node.start_byte();
    let end_byte = node.end_byte();
    let text = source[start_byte..end_byte].to_string();

    BlockInfo {
        kind,
        start_byte,
        end_byte,
        start_point: node.start_position(),
        end_point: node.end_position(),
        text,
    }
}

fn detect_heading_level(node: &arborium_tree_sitter::Node) -> u8 {
    if node.kind() == "setext_heading" {
        let has_h1 = node
            .children(&mut node.walk())
            .any(|c| c.kind() == "setext_h1_underline");
        return if has_h1 { 1 } else { 2 };
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
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
    }
    1
}

fn walk_blocks_recursive<F>(
    node: &arborium_tree_sitter::Node,
    source: &str,
    visitor: &mut F,
) where
    F: FnMut(BlockInfo),
{
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i as u32) {
            let kind_str = child.kind();
            if is_block_node(kind_str) {
                visitor(block_info_from_node(&child, source));
                // Recurse into containers
                let kind = NodeKind::from_ts_kind(kind_str);
                if kind.is_container() {
                    walk_blocks_recursive(&child, source, visitor);
                }
            }
        }
    }
}

/// Convert a byte offset in a string to a tree-sitter Point (row, column in bytes).
fn byte_offset_to_point(source: &str, byte_offset: usize) -> Point {
    let offset = byte_offset.min(source.len());
    let slice = &source[..offset];
    let row = slice.matches('\n').count();
    let last_newline = slice.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let column = offset - last_newline;
    Point { row, column }
}

/// Extract the info string (language) from a fenced code block node.
pub fn code_block_language<'a>(
    node: &arborium_tree_sitter::Node,
    source: &'a str,
) -> Option<&'a str> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == "info_string" {
                let text = &source[child.start_byte()..child.end_byte()];
                let lang = text.trim();
                if !lang.is_empty() {
                    return Some(lang);
                }
            }
        }
    }
    None
}

/// Extract the content of a fenced code block (without fences).
pub fn code_block_content<'a>(
    node: &arborium_tree_sitter::Node,
    source: &'a str,
) -> Option<&'a str> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == "code_fence_content" {
                return Some(&source[child.start_byte()..child.end_byte()]);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_markdown() {
        let md = "# Hello\n\nThis is a paragraph.\n";
        let tree = MarkdownTree::new(md);
        assert!(tree.tree().is_some());

        let sexp = tree.sexp().unwrap();
        assert!(sexp.contains("atx_heading"));
        assert!(sexp.contains("paragraph"));
    }

    #[test]
    fn walk_blocks_finds_all() {
        let md = "# Title\n\nParagraph text.\n\n- item 1\n- item 2\n\n```rust\nfn main() {}\n```\n";
        let tree = MarkdownTree::new(md);

        let mut blocks = Vec::new();
        tree.walk_blocks(|info| blocks.push(info));

        let kinds: Vec<_> = blocks.iter().map(|b| b.kind).collect();
        assert!(kinds.contains(&NodeKind::Heading { level: 1 }));
        assert!(kinds.contains(&NodeKind::Paragraph));
        assert!(kinds.contains(&NodeKind::BulletList));
        assert!(kinds.contains(&NodeKind::FencedCodeBlock));
    }

    #[test]
    fn heading_levels() {
        let md = "# H1\n\n## H2\n\n### H3\n";
        let tree = MarkdownTree::new(md);

        let mut headings = Vec::new();
        tree.walk_blocks(|info| {
            if let NodeKind::Heading { level } = info.kind {
                headings.push(level);
            }
        });
        assert_eq!(headings, vec![1, 2, 3]);
    }

    #[test]
    fn ordered_vs_unordered_list() {
        let md = "- bullet\n- list\n\n1. ordered\n2. list\n";
        let tree = MarkdownTree::new(md);

        let mut lists = Vec::new();
        tree.walk_blocks(|info| {
            match info.kind {
                NodeKind::BulletList => lists.push(false),
                NodeKind::OrderedList => lists.push(true),
                _ => {}
            }
        });
        assert_eq!(lists, vec![false, true]);
    }

    #[test]
    fn fenced_code_block_language() {
        let md = "```rust\nfn main() {}\n```\n";
        let tree = MarkdownTree::new(md);

        let t = tree.tree().unwrap();
        let root = t.root_node();

        let mut found_lang = None;
        for i in 0..root.named_child_count() {
            let child = root.named_child(i as u32).unwrap();
            let code_node = if child.kind() == "fenced_code_block" {
                Some(child)
            } else {
                find_child_by_kind(&child, "fenced_code_block")
            };
            if let Some(code) = code_node {
                found_lang = code_block_language(&code, md).map(|s| s.to_string());
            }
        }
        assert_eq!(found_lang.as_deref(), Some("rust"));
    }

    #[test]
    fn incremental_edit() {
        let mut tree = MarkdownTree::new("# Hello\n\nWorld\n");

        tree.edit(
            9,
            14,
            "Rust",
            Point { row: 2, column: 0 },
            Point { row: 2, column: 5 },
        );

        assert_eq!(tree.source(), "# Hello\n\nRust\n");
        assert!(tree.tree().is_some());
        let sexp = tree.sexp().unwrap();
        assert!(sexp.contains("atx_heading"));
        assert!(sexp.contains("paragraph"));
    }

    #[test]
    fn block_at_byte_offset() {
        let md = "# Title\n\nSome paragraph.\n";
        let tree = MarkdownTree::new(md);

        let block = tree.block_at_byte(0).unwrap();
        assert!(matches!(block.kind, NodeKind::Heading { level: 1 }));

        let block = tree.block_at_byte(10).unwrap();
        assert_eq!(block.kind, NodeKind::Paragraph);
    }

    #[test]
    fn empty_document() {
        let tree = MarkdownTree::new("");
        assert!(tree.tree().is_some());
        let mut blocks = Vec::new();
        tree.walk_blocks(|info| blocks.push(info));
        assert!(blocks.is_empty());
    }

    #[test]
    fn node_at_byte_uses_node_kind() {
        let md = "# Hello\n";
        let tree = MarkdownTree::new(md);
        let node = tree.node_at_byte(2).unwrap();
        // Should return a typed NodeKind, not a raw string
        assert!(!matches!(node.kind, NodeKind::Unknown));
    }

    fn find_child_by_kind<'a>(
        node: &arborium_tree_sitter::Node<'a>,
        kind: &str,
    ) -> Option<arborium_tree_sitter::Node<'a>> {
        for i in 0..node.named_child_count() {
            if let Some(child) = node.named_child(i as u32) {
                if child.kind() == kind {
                    return Some(child);
                }
                if let Some(found) = find_child_by_kind(&child, kind) {
                    return Some(found);
                }
            }
        }
        None
    }
}
