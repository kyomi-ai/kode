/// Classification of markdown node types from the tree-sitter CST.
///
/// These represent the semantic meaning of each node, useful for
/// rendering decisions and command dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NodeKind {
    // Block-level
    Document,
    Section,
    Paragraph,
    Heading { level: u8 },
    FencedCodeBlock,
    IndentedCodeBlock,
    BlockQuote,
    BulletList,
    OrderedList,
    ListItem,
    ThematicBreak,
    HtmlBlock,
    Table,
    TableRow,
    TableHeader,
    TableDelimiterRow,
    LinkReferenceDefinition,
    FrontMatter,

    // Inline
    Emphasis,       // *italic* or _italic_
    StrongEmphasis, // **bold** or __bold__
    InlineCode,     // `code`
    Link,           // [text](url)
    Image,          // ![alt](url)
    Strikethrough,  // ~~text~~
    HardLineBreak,
    SoftLineBreak,

    // Code block parts
    InfoString,       // language identifier in fenced code block
    CodeFenceContent, // the code inside a fenced block

    // Other
    Inline, // inline container
    Text,   // plain text
    Unknown,
}

impl NodeKind {
    /// Classify a tree-sitter node kind string.
    pub fn from_ts_kind(kind: &str) -> Self {
        match kind {
            "document" => NodeKind::Document,
            "section" => NodeKind::Section,
            "paragraph" => NodeKind::Paragraph,
            "atx_heading" | "setext_heading" => NodeKind::Heading { level: 0 }, // level set separately
            "fenced_code_block" => NodeKind::FencedCodeBlock,
            "indented_code_block" => NodeKind::IndentedCodeBlock,
            "block_quote" => NodeKind::BlockQuote,
            "thematic_break" => NodeKind::ThematicBreak,
            "html_block" => NodeKind::HtmlBlock,
            "list" => NodeKind::BulletList, // caller refines to ordered/bullet
            "list_item" => NodeKind::ListItem,
            "pipe_table" => NodeKind::Table,
            "pipe_table_row" => NodeKind::TableRow,
            "pipe_table_header" => NodeKind::TableHeader,
            "pipe_table_delimiter_row" => NodeKind::TableDelimiterRow,
            "link_reference_definition" => NodeKind::LinkReferenceDefinition,
            "minus_metadata" | "plus_metadata" => NodeKind::FrontMatter,

            "emphasis" => NodeKind::Emphasis,
            "strong_emphasis" => NodeKind::StrongEmphasis,
            "code_span" => NodeKind::InlineCode,
            "link" | "full_reference_link" | "collapsed_reference_link"
            | "shortcut_link" | "autolink" | "uri_autolink" => NodeKind::Link,
            "image" => NodeKind::Image,
            "strikethrough" => NodeKind::Strikethrough,
            "hard_line_break" => NodeKind::HardLineBreak,
            "soft_line_break" => NodeKind::SoftLineBreak,

            "info_string" => NodeKind::InfoString,
            "code_fence_content" => NodeKind::CodeFenceContent,

            "inline" => NodeKind::Inline,

            _ => NodeKind::Unknown,
        }
    }

    /// True if this is a block-level element.
    pub fn is_block(&self) -> bool {
        matches!(
            self,
            NodeKind::Document
                | NodeKind::Section
                | NodeKind::Paragraph
                | NodeKind::Heading { .. }
                | NodeKind::FencedCodeBlock
                | NodeKind::IndentedCodeBlock
                | NodeKind::BlockQuote
                | NodeKind::BulletList
                | NodeKind::OrderedList
                | NodeKind::ListItem
                | NodeKind::ThematicBreak
                | NodeKind::HtmlBlock
                | NodeKind::Table
                | NodeKind::LinkReferenceDefinition
                | NodeKind::FrontMatter
        )
    }

    /// True if this is an inline element.
    pub fn is_inline(&self) -> bool {
        matches!(
            self,
            NodeKind::Emphasis
                | NodeKind::StrongEmphasis
                | NodeKind::InlineCode
                | NodeKind::Link
                | NodeKind::Image
                | NodeKind::Strikethrough
                | NodeKind::HardLineBreak
                | NodeKind::SoftLineBreak
                | NodeKind::Inline
                | NodeKind::Text
        )
    }

    /// True if this node can contain other blocks (is a container).
    pub fn is_container(&self) -> bool {
        matches!(
            self,
            NodeKind::Document
                | NodeKind::Section
                | NodeKind::BlockQuote
                | NodeKind::BulletList
                | NodeKind::OrderedList
                | NodeKind::ListItem
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_block_nodes() {
        assert!(NodeKind::from_ts_kind("paragraph").is_block());
        assert!(NodeKind::from_ts_kind("atx_heading").is_block());
        assert!(NodeKind::from_ts_kind("fenced_code_block").is_block());
        assert!(NodeKind::from_ts_kind("list").is_block());
    }

    #[test]
    fn classify_inline_nodes() {
        assert!(NodeKind::from_ts_kind("emphasis").is_inline());
        assert!(NodeKind::from_ts_kind("strong_emphasis").is_inline());
        assert!(NodeKind::from_ts_kind("code_span").is_inline());
        assert!(NodeKind::from_ts_kind("link").is_inline());
    }

    #[test]
    fn containers() {
        assert!(NodeKind::from_ts_kind("block_quote").is_container());
        assert!(NodeKind::from_ts_kind("list").is_container());
        assert!(!NodeKind::from_ts_kind("paragraph").is_container());
    }
}
