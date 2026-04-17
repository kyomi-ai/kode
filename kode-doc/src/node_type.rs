//! Node type enumeration and property queries.
//!
//! Every node in the document tree has a [`NodeType`] that determines its
//! structural role: whether it is a block or inline element, whether it can
//! contain children, and how it participates in the token-based position system.

/// The type of a document tree node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeType {
    /// The root document node.
    Doc,
    /// A paragraph block.
    Paragraph,
    /// A heading block (level stored in attrs).
    Heading,
    /// A blockquote wrapper.
    Blockquote,
    /// An unordered list.
    BulletList,
    /// An ordered list (start number in attrs).
    OrderedList,
    /// A single list item.
    ListItem,
    /// A fenced code block (language in attrs).
    CodeBlock,
    /// A horizontal rule (leaf block).
    HorizontalRule,
    /// A hard line break (inline leaf).
    HardBreak,
    /// An image (inline leaf, attrs: src, alt, title).
    Image,
    /// A table block.
    Table,
    /// A table row (inside Table).
    TableRow,
    /// A table header row (inside Table, rendered as `<thead>`).
    TableHeader,
    /// A single table cell (inside TableRow or TableHeader).
    TableCell,
    /// A text node (inline leaf, content in `Node::text`).
    Text,
}

impl NodeType {
    /// Returns `true` if this is a block-level node type.
    ///
    /// Block nodes occupy their own vertical space in the document layout.
    pub fn is_block(self) -> bool {
        matches!(
            self,
            NodeType::Doc
                | NodeType::Paragraph
                | NodeType::Heading
                | NodeType::Blockquote
                | NodeType::BulletList
                | NodeType::OrderedList
                | NodeType::ListItem
                | NodeType::CodeBlock
                | NodeType::HorizontalRule
                | NodeType::Table
                | NodeType::TableRow
                | NodeType::TableHeader
                | NodeType::TableCell
        )
    }

    /// Returns `true` if this is an inline node type.
    ///
    /// Inline nodes appear within text flow inside textblock nodes.
    pub fn is_inline(self) -> bool {
        matches!(self, NodeType::Text | NodeType::HardBreak | NodeType::Image)
    }

    /// Returns `true` if this is a textblock — a block that contains inline content.
    ///
    /// Textblocks are the innermost block-level containers that hold text and
    /// inline nodes directly.
    pub fn is_textblock(self) -> bool {
        matches!(
            self,
            NodeType::Paragraph
                | NodeType::Heading
                | NodeType::CodeBlock
                | NodeType::TableCell
        )
    }

    /// Returns `true` if this is a leaf node (no children in the tree).
    ///
    /// Leaf nodes have a `node_size()` of 1 (for non-text leaves) or
    /// `text.len()` (for text nodes).
    pub fn is_leaf(self) -> bool {
        matches!(
            self,
            NodeType::HorizontalRule | NodeType::HardBreak | NodeType::Image | NodeType::Text
        )
    }

    /// Returns `true` if this is a text node.
    pub fn is_text(self) -> bool {
        self == NodeType::Text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_types() {
        assert!(NodeType::Doc.is_block());
        assert!(NodeType::Paragraph.is_block());
        assert!(NodeType::Heading.is_block());
        assert!(NodeType::Blockquote.is_block());
        assert!(NodeType::BulletList.is_block());
        assert!(NodeType::OrderedList.is_block());
        assert!(NodeType::ListItem.is_block());
        assert!(NodeType::CodeBlock.is_block());
        assert!(NodeType::HorizontalRule.is_block());
        assert!(NodeType::Table.is_block());
        assert!(NodeType::TableRow.is_block());
        assert!(NodeType::TableHeader.is_block());
        assert!(NodeType::TableCell.is_block());

        assert!(!NodeType::Text.is_block());
        assert!(!NodeType::HardBreak.is_block());
        assert!(!NodeType::Image.is_block());
    }

    #[test]
    fn inline_types() {
        assert!(NodeType::Text.is_inline());
        assert!(NodeType::HardBreak.is_inline());
        assert!(NodeType::Image.is_inline());

        assert!(!NodeType::Doc.is_inline());
        assert!(!NodeType::Paragraph.is_inline());
    }

    #[test]
    fn textblock_types() {
        assert!(NodeType::Paragraph.is_textblock());
        assert!(NodeType::Heading.is_textblock());
        assert!(NodeType::CodeBlock.is_textblock());

        assert!(NodeType::TableCell.is_textblock());

        assert!(!NodeType::Doc.is_textblock());
        assert!(!NodeType::Blockquote.is_textblock());
        assert!(!NodeType::ListItem.is_textblock());
        assert!(!NodeType::Table.is_textblock());
        assert!(!NodeType::TableRow.is_textblock());
        assert!(!NodeType::TableHeader.is_textblock());
        assert!(!NodeType::Text.is_textblock());
    }

    #[test]
    fn leaf_types() {
        assert!(NodeType::HorizontalRule.is_leaf());
        assert!(NodeType::HardBreak.is_leaf());
        assert!(NodeType::Image.is_leaf());
        assert!(NodeType::Text.is_leaf());

        assert!(!NodeType::Doc.is_leaf());
        assert!(!NodeType::Paragraph.is_leaf());
        assert!(!NodeType::Heading.is_leaf());
    }

    #[test]
    fn text_type() {
        assert!(NodeType::Text.is_text());
        assert!(!NodeType::Paragraph.is_text());
        assert!(!NodeType::HardBreak.is_text());
    }

    #[test]
    fn block_and_inline_are_mutually_exclusive() {
        let all_types = [
            NodeType::Doc,
            NodeType::Paragraph,
            NodeType::Heading,
            NodeType::Blockquote,
            NodeType::BulletList,
            NodeType::OrderedList,
            NodeType::ListItem,
            NodeType::CodeBlock,
            NodeType::HorizontalRule,
            NodeType::Table,
            NodeType::TableRow,
            NodeType::TableHeader,
            NodeType::TableCell,
            NodeType::HardBreak,
            NodeType::Image,
            NodeType::Text,
        ];
        for nt in all_types {
            assert!(
                nt.is_block() || nt.is_inline(),
                "{nt:?} is neither block nor inline"
            );
            assert!(
                !(nt.is_block() && nt.is_inline()),
                "{nt:?} is both block and inline"
            );
        }
    }
}

/// Check if a parent node type can contain a child node type.
///
/// This enforces the document structure rules for markdown:
/// - Doc contains block nodes
/// - Paragraph/Heading/CodeBlock contain inline content
/// - Lists contain ListItems
/// - ListItem/Blockquote contain block nodes
/// - CodeBlock only contains Text (no marks)
/// - Leaf nodes (HorizontalRule, HardBreak, Image) contain nothing
pub fn can_contain(parent: NodeType, child: NodeType) -> bool {
    match parent {
        NodeType::Doc => child.is_block(),
        NodeType::Paragraph | NodeType::Heading => child.is_inline(),
        NodeType::Blockquote => child.is_block(),
        NodeType::BulletList | NodeType::OrderedList => child == NodeType::ListItem,
        NodeType::ListItem => child.is_block(),
        NodeType::CodeBlock => child == NodeType::Text,
        NodeType::Table => matches!(child, NodeType::TableHeader | NodeType::TableRow),
        NodeType::TableHeader | NodeType::TableRow => child == NodeType::TableCell,
        NodeType::TableCell => child.is_inline(),
        _ => false,
    }
}

#[cfg(test)]
mod validation_tests {
    use super::*;

    #[test]
    fn doc_contains_blocks() {
        assert!(can_contain(NodeType::Doc, NodeType::Paragraph));
        assert!(can_contain(NodeType::Doc, NodeType::Heading));
        assert!(can_contain(NodeType::Doc, NodeType::BulletList));
        assert!(!can_contain(NodeType::Doc, NodeType::Text));
        assert!(!can_contain(NodeType::Doc, NodeType::HardBreak));
    }

    #[test]
    fn paragraph_contains_inline() {
        assert!(can_contain(NodeType::Paragraph, NodeType::Text));
        assert!(can_contain(NodeType::Paragraph, NodeType::HardBreak));
        assert!(can_contain(NodeType::Paragraph, NodeType::Image));
        assert!(!can_contain(NodeType::Paragraph, NodeType::Paragraph));
    }

    #[test]
    fn lists_contain_items() {
        assert!(can_contain(NodeType::BulletList, NodeType::ListItem));
        assert!(can_contain(NodeType::OrderedList, NodeType::ListItem));
        assert!(!can_contain(NodeType::BulletList, NodeType::Paragraph));
    }

    #[test]
    fn list_item_contains_blocks() {
        assert!(can_contain(NodeType::ListItem, NodeType::Paragraph));
        assert!(!can_contain(NodeType::ListItem, NodeType::Text));
    }

    #[test]
    fn code_block_contains_only_text() {
        assert!(can_contain(NodeType::CodeBlock, NodeType::Text));
        assert!(!can_contain(NodeType::CodeBlock, NodeType::HardBreak));
    }

    #[test]
    fn leaves_contain_nothing() {
        assert!(!can_contain(NodeType::HorizontalRule, NodeType::Text));
        assert!(!can_contain(NodeType::Text, NodeType::Text));
    }
}
