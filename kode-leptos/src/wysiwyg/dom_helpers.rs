//! DOM utility functions for the tree-based WYSIWYG editor.
//!
//! Provides helpers for navigating the DOM tree to find elements with
//! `data-pos-start`/`data-pos-end` attributes, parsing data attributes,
//! and applying markdown commands via roundtrip.

use kode_doc::Selection;

/// Apply a MarkdownCommands operation to a DocState via markdown roundtrip.
/// Serializes to markdown, creates a temporary MarkdownEditor, runs the command,
/// then parses the result back into DocState if changed.
/// Returns `true` if the document was modified.
pub(crate) fn apply_md_command(
    ds: &mut kode_doc::DocState,
    f: impl FnOnce(&mut kode_markdown::MarkdownEditor),
) -> bool {
    let saved_selection = ds.selection().clone();
    let md = ds.to_markdown();
    let mut temp = kode_markdown::MarkdownEditor::new(&md);
    f(&mut temp);
    temp.sync_tree();
    let new_md = temp.text();
    if new_md != md {
        ds.set_from_markdown(&new_md);
        // Restore cursor, clamped to valid range. The tree positions may not
        // match exactly after a structural change, but this is much better
        // than jumping to position 1 (the set_from_markdown default).
        let max_pos = ds.doc().content.size();
        let anchor = saved_selection.anchor.min(max_pos);
        let head = saved_selection.head.min(max_pos);
        ds.set_selection(Selection::range(anchor, head));
        true
    } else {
        false
    }
}

/// Walk up the DOM from an element to find the nearest ancestor (or self)
/// that has both `data-pos-start` and `data-pos-end` attributes.
pub(crate) fn find_ancestor_with_pos_attrs(el: &web_sys::Element) -> Option<web_sys::Element> {
    let mut current: Option<web_sys::Element> = Some(el.clone());
    while let Some(ref elem) = current {
        if elem.has_attribute("data-pos-start") && elem.has_attribute("data-pos-end") {
            return Some(elem.clone());
        }
        if elem.class_list().contains("wysiwyg-container") {
            return None;
        }
        current = elem.parent_element();
    }
    None
}

/// Walk up the DOM from an element to find the nearest ancestor (or self)
/// that has the given attribute.
pub(crate) fn find_ancestor_with_attr(el: &web_sys::Element, attr: &str) -> Option<web_sys::Element> {
    let mut current: Option<web_sys::Element> = Some(el.clone());
    while let Some(ref elem) = current {
        if elem.has_attribute(attr) {
            return Some(elem.clone());
        }
        if elem.class_list().contains("wysiwyg-container") {
            return None;
        }
        current = elem.parent_element();
    }
    None
}

/// Parse an integer data attribute from an element.
pub(crate) fn parse_data_attr(el: &web_sys::Element, attr: &str) -> Option<usize> {
    el.get_attribute(attr)?.parse::<usize>().ok()
}
