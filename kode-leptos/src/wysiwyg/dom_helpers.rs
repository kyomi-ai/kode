//! DOM utility functions for the tree-based WYSIWYG editor.
//!
//! Provides helpers for applying markdown commands via roundtrip.

use kode_doc::Selection;

/// Apply a MarkdownCommands operation to a DocState via markdown roundtrip.
/// Serializes to markdown, creates a temporary MarkdownEditor, runs the command,
/// then parses the result back into DocState if changed.
/// Returns `true` if the document was modified.
pub(super) fn apply_md_command(
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
