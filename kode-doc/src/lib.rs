//! `kode-doc` — A tree-based document model for structured text editing.
//!
//! This crate provides the core data structures for representing documents as
//! immutable trees with token-based positioning. It is the foundation for the
//! kode editor's WYSIWYG mode.
//!
//! # Architecture
//!
//! - [`Node`] — the fundamental tree node, with a type, attributes, content, and marks
//! - [`Fragment`] — an ordered, normalized sequence of child nodes
//! - [`Mark`] / [`MarkType`] — inline formatting metadata on text nodes
//! - [`NodeType`] — the structural role of a node (block, inline, text, leaf)
//! - [`Attrs`] / [`AttrValue`] — key-value attributes on nodes and marks

pub mod attrs;
pub mod doc_state;
pub mod fragment;
pub mod mark;
pub mod node;
pub mod node_type;
pub mod parse;
pub mod position;
pub mod serialize;
pub mod slice;
pub mod step;
pub mod transform;

pub use attrs::{AttrValue, Attrs};
pub use doc_state::{DocState, FormattingState, Selection};
pub use fragment::Fragment;
pub use mark::{Mark, MarkType};
pub use node::Node;
pub use node_type::{can_contain, NodeType};
pub use parse::parse_markdown;
pub use position::ResolvedPos;
pub use serialize::{byte_offset_to_tree_pos, serialize_markdown, tree_pos_to_byte_offset};
pub use slice::Slice;
pub use step::{Step, StepMap, StepResult};
pub use transform::Transform;
