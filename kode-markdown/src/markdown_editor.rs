use kode_core::{Editor, Position, Selection, Transaction};

use crate::parse::MarkdownTree;

/// Coordinated markdown editor that owns both the text `Editor` and the
/// `MarkdownTree`. After every mutation that changes text content, the tree
/// is automatically synced via `set_source()`. This is the single
/// coordination point — callers should never need to manually sync.
pub struct MarkdownEditor {
    editor: Editor,
    tree: MarkdownTree,
}

impl MarkdownEditor {
    /// Create a new markdown editor with the given text.
    pub fn new(text: &str) -> Self {
        Self {
            editor: Editor::new(text),
            tree: MarkdownTree::new(text),
        }
    }

    /// Create an empty markdown editor.
    pub fn empty() -> Self {
        Self::new("")
    }

    // ── Accessors ────────────────────────────────────────────────────────

    /// Immutable access to the inner `Editor`.
    pub fn editor(&self) -> &Editor {
        &self.editor
    }

    /// Mutable access to the inner `Editor`.
    ///
    /// **Important**: If you mutate the editor text through this reference
    /// (e.g., via `MarkdownCommands`, `InputRules`, or direct calls to
    /// `insert()`, `backspace()`, `undo()`, `redo()`, etc.), you must call
    /// `sync_tree()` afterward to keep the tree in sync.
    pub fn editor_mut(&mut self) -> &mut Editor {
        &mut self.editor
    }

    /// Immutable access to the inner `Buffer`.
    pub fn buffer(&self) -> &kode_core::Buffer {
        self.editor.buffer()
    }

    /// Document version, incremented on every edit.
    pub fn version(&self) -> u64 {
        self.editor.version()
    }

    /// Immutable access to the `MarkdownTree`.
    pub fn tree(&self) -> &MarkdownTree {
        &self.tree
    }

    /// Full reparse of the tree from the editor's current text.
    /// Call this after using `editor_mut()` to make direct mutations.
    pub fn sync_tree(&mut self) {
        self.tree.set_source(&self.editor.text());
    }

    // ── Text-mutating wrappers (auto-sync tree) ─────────────────────────

    /// Insert text at the cursor. If there's a selection, replace it.
    pub fn insert(&mut self, text: &str) {
        self.editor.insert(text);
        self.sync_tree();
    }

    /// Delete the character before the cursor (backspace).
    pub fn backspace(&mut self) {
        self.editor.backspace();
        self.sync_tree();
    }

    /// Delete the character after the cursor (forward delete).
    pub fn delete_forward(&mut self) {
        self.editor.delete_forward();
        self.sync_tree();
    }

    /// Insert a newline at the cursor.
    pub fn insert_newline(&mut self) {
        self.editor.insert_newline();
        self.sync_tree();
    }

    /// Undo the last transaction.
    pub fn undo(&mut self) {
        self.editor.undo();
        self.sync_tree();
    }

    /// Redo the last undone transaction.
    pub fn redo(&mut self) {
        self.editor.redo();
        self.sync_tree();
    }

    /// Delete the current selection. No-op if cursor only.
    pub fn delete_selection(&mut self) {
        self.editor.delete_selection();
        self.sync_tree();
    }

    /// Apply a pre-built transaction atomically.
    pub fn apply_transaction(&mut self, tx: Transaction) {
        self.editor.apply_transaction(tx);
        self.sync_tree();
    }

    /// Insert a tab (2 spaces) at the cursor, or indent all selected lines.
    pub fn indent(&mut self) {
        self.editor.indent();
        self.sync_tree();
    }

    /// Remove one level of indentation from the current or selected lines.
    pub fn outdent(&mut self) {
        self.editor.outdent();
        self.sync_tree();
    }

    /// Duplicate the current line or all selected lines.
    pub fn duplicate_lines(&mut self) {
        self.editor.duplicate_lines();
        self.sync_tree();
    }

    /// Delete from cursor to start of previous word (Ctrl+Backspace).
    pub fn delete_word_back(&mut self) {
        self.editor.delete_word_back();
        self.sync_tree();
    }

    /// Delete from cursor to end of next word (Ctrl+Delete).
    pub fn delete_word_forward(&mut self) {
        self.editor.delete_word_forward();
        self.sync_tree();
    }

    // ── Read-only delegations (no sync needed) ──────────────────────────

    /// Get the full text content.
    pub fn text(&self) -> String {
        self.editor.text()
    }

    /// Get the current cursor position.
    pub fn cursor(&self) -> Position {
        self.editor.cursor()
    }

    /// Get the current selection.
    pub fn selection(&self) -> Selection {
        self.editor.selection()
    }

    /// Get the selected text, or empty string if cursor only.
    pub fn selected_text(&self) -> String {
        self.editor.selected_text()
    }

    /// Set the cursor to a position, collapsing any selection.
    pub fn set_cursor(&mut self, pos: Position) {
        self.editor.set_cursor(pos);
    }

    /// Set a selection range.
    pub fn set_selection(&mut self, anchor: Position, head: Position) {
        self.editor.set_selection(anchor, head);
    }

    /// Select all text.
    pub fn select_all(&mut self) {
        self.editor.select_all();
    }

    // ── Cursor movement (no sync needed) ────────────────────────────────

    /// Move cursor left by one character.
    pub fn move_left(&mut self) {
        self.editor.move_left();
    }

    /// Move cursor right by one character.
    pub fn move_right(&mut self) {
        self.editor.move_right();
    }

    /// Move cursor up by one line.
    pub fn move_up(&mut self) {
        self.editor.move_up();
    }

    /// Move cursor down by one line.
    pub fn move_down(&mut self) {
        self.editor.move_down();
    }

    /// Move cursor left to the start of the previous word.
    pub fn move_word_left(&mut self) {
        self.editor.move_word_left();
    }

    /// Move cursor right to the end of the next word.
    pub fn move_word_right(&mut self) {
        self.editor.move_word_right();
    }

    /// Move cursor to start of current line.
    pub fn move_to_line_start(&mut self) {
        self.editor.move_to_line_start();
    }

    /// Move cursor to end of current line.
    pub fn move_to_line_end(&mut self) {
        self.editor.move_to_line_end();
    }

    /// Move cursor to start of document.
    pub fn move_to_start(&mut self) {
        self.editor.move_to_start();
    }

    /// Move cursor to end of document.
    pub fn move_to_end(&mut self) {
        self.editor.move_to_end();
    }

    // ── Selection extension (no sync needed) ────────────────────────────

    /// Extend selection one character left (Shift+Left).
    pub fn extend_selection_left(&mut self) {
        self.editor.extend_selection_left();
    }

    /// Extend selection one character right (Shift+Right).
    pub fn extend_selection_right(&mut self) {
        self.editor.extend_selection_right();
    }

    /// Extend selection up one line (Shift+Up).
    pub fn extend_selection_up(&mut self) {
        self.editor.extend_selection_up();
    }

    /// Extend selection down one line (Shift+Down).
    pub fn extend_selection_down(&mut self) {
        self.editor.extend_selection_down();
    }

    /// Extend selection to a specific position.
    pub fn extend_selection(&mut self, head: Position) {
        self.editor.extend_selection(head);
    }

    /// Extend selection to word boundary left (Ctrl+Shift+Left).
    pub fn extend_selection_word_left(&mut self) {
        self.editor.extend_selection_word_left();
    }

    /// Extend selection to word boundary right (Ctrl+Shift+Right).
    pub fn extend_selection_word_right(&mut self) {
        self.editor.extend_selection_word_right();
    }

    /// Extend selection to line start (Shift+Home).
    pub fn extend_selection_to_line_start(&mut self) {
        self.editor.extend_selection_to_line_start();
    }

    /// Extend selection to line end (Shift+End).
    pub fn extend_selection_to_line_end(&mut self) {
        self.editor.extend_selection_to_line_end();
    }

    /// Extend selection to document start (Ctrl+Shift+Home).
    pub fn extend_selection_to_start(&mut self) {
        self.editor.extend_selection_to_start();
    }

    /// Extend selection to document end (Ctrl+Shift+End).
    pub fn extend_selection_to_end(&mut self) {
        self.editor.extend_selection_to_end();
    }

    // ── Smart selection (no sync needed) ────────────────────────────────

    /// Select the word at the cursor (double-click behavior).
    pub fn select_word(&mut self) {
        self.editor.select_word();
    }

    /// Select the entire current line (triple-click behavior).
    pub fn select_line(&mut self) {
        self.editor.select_line();
    }

    /// Move cursor up by N lines (PageUp).
    pub fn page_up(&mut self, page_lines: usize) {
        self.editor.page_up(page_lines);
    }

    /// Move cursor down by N lines (PageDown).
    pub fn page_down(&mut self, page_lines: usize) {
        self.editor.page_down(page_lines);
    }

    // ── Dirty / undo state (no sync needed) ─────────────────────────────

    /// Check if the editor has unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.editor.is_dirty()
    }

    /// Mark the editor as clean (e.g., after saving).
    pub fn mark_clean(&mut self) {
        self.editor.mark_clean();
    }

    /// Check if undo is available.
    pub fn can_undo(&self) -> bool {
        self.editor.can_undo()
    }

    /// Check if redo is available.
    pub fn can_redo(&self) -> bool {
        self.editor.can_redo()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InputRules, MarkdownCommands, NodeKind};

    #[test]
    fn insert_text_syncs_editor_and_tree() {
        let mut md = MarkdownEditor::new("");
        md.insert("# Hello");
        assert_eq!(md.editor().text(), md.tree().source());
        assert_eq!(md.editor().text(), "# Hello");
    }

    #[test]
    fn undo_reverts_both_editor_and_tree() {
        let mut md = MarkdownEditor::new("");
        md.insert("# Hello");
        assert_eq!(md.text(), "# Hello");
        assert_eq!(md.tree().source(), "# Hello");

        md.undo();
        assert_eq!(md.editor().text(), "");
        assert_eq!(md.tree().source(), "");
        assert_eq!(md.editor().text(), md.tree().source());
    }

    #[test]
    fn walk_blocks_returns_correct_blocks_after_edits() {
        let mut md = MarkdownEditor::new("# Title");
        // Add a paragraph
        md.set_cursor(Position::new(0, 7));
        md.insert_newline();
        md.insert_newline();
        md.insert("Some paragraph text.");

        let mut blocks = Vec::new();
        md.tree().walk_blocks(|info| blocks.push(info.kind));

        assert!(
            blocks.contains(&NodeKind::Heading { level: 1 }),
            "Expected heading block, got: {:?}",
            blocks
        );
        assert!(
            blocks.contains(&NodeKind::Paragraph),
            "Expected paragraph block, got: {:?}",
            blocks
        );
    }

    #[test]
    fn toggle_bold_via_editor_mut_and_sync_tree() {
        let mut md = MarkdownEditor::new("hello world");
        // Select "world"
        md.set_selection(Position::new(0, 6), Position::new(0, 11));
        MarkdownCommands::toggle_bold(md.editor_mut());
        md.sync_tree();

        assert_eq!(md.editor().text(), "hello **world**");
        assert_eq!(md.tree().source(), "hello **world**");
        assert_eq!(md.editor().text(), md.tree().source());
    }

    #[test]
    fn input_rules_handle_enter_via_editor_mut_and_sync_tree() {
        let mut md = MarkdownEditor::new("- item 1");
        md.set_cursor(Position::new(0, 8));
        let handled = InputRules::handle_enter(md.editor_mut());
        md.sync_tree();

        assert!(handled);
        assert_eq!(md.editor().text(), "- item 1\n- ");
        assert_eq!(md.tree().source(), "- item 1\n- ");
        assert_eq!(md.editor().text(), md.tree().source());
    }

    #[test]
    fn backspace_syncs_tree() {
        let mut md = MarkdownEditor::new("abc");
        md.set_cursor(Position::new(0, 3));
        md.backspace();
        assert_eq!(md.editor().text(), "ab");
        assert_eq!(md.tree().source(), "ab");
    }

    #[test]
    fn delete_forward_syncs_tree() {
        let mut md = MarkdownEditor::new("abc");
        md.set_cursor(Position::new(0, 0));
        md.delete_forward();
        assert_eq!(md.editor().text(), "bc");
        assert_eq!(md.tree().source(), "bc");
    }

    #[test]
    fn redo_syncs_tree() {
        let mut md = MarkdownEditor::new("");
        md.insert("# Test");
        md.undo();
        assert_eq!(md.tree().source(), "");

        md.redo();
        assert_eq!(md.editor().text(), "# Test");
        assert_eq!(md.tree().source(), "# Test");
    }

    #[test]
    fn delete_selection_syncs_tree() {
        let mut md = MarkdownEditor::new("hello world");
        md.set_selection(Position::new(0, 5), Position::new(0, 11));
        md.delete_selection();
        assert_eq!(md.editor().text(), "hello");
        assert_eq!(md.tree().source(), "hello");
    }

    #[test]
    fn indent_outdent_sync_tree() {
        let mut md = MarkdownEditor::new("line1\nline2");
        md.set_selection(Position::new(0, 0), Position::new(1, 5));
        md.indent();
        assert_eq!(md.editor().text(), md.tree().source());
        assert_eq!(md.editor().text(), "  line1\n  line2");

        // Now outdent
        md.set_selection(Position::new(0, 0), Position::new(1, 7));
        md.outdent();
        assert_eq!(md.editor().text(), "line1\nline2");
        assert_eq!(md.tree().source(), "line1\nline2");
    }

    #[test]
    fn duplicate_lines_syncs_tree() {
        let mut md = MarkdownEditor::new("line1\nline2");
        md.set_cursor(Position::new(0, 0));
        md.duplicate_lines();
        assert_eq!(md.editor().text(), md.tree().source());
        assert_eq!(md.editor().text(), "line1\nline1\nline2");
    }

    #[test]
    fn apply_transaction_syncs_tree() {
        let mut md = MarkdownEditor::new("Hello\nWorld");
        let tx = Transaction::new(vec![
            kode_core::EditStep::replace(0, "Hello".to_string(), "> Hello".to_string()),
            kode_core::EditStep::replace(8, "World".to_string(), "> World".to_string()),
        ]);
        md.apply_transaction(tx);
        assert_eq!(md.editor().text(), "> Hello\n> World");
        assert_eq!(md.tree().source(), "> Hello\n> World");
    }

    #[test]
    fn delete_word_back_syncs_tree() {
        let mut md = MarkdownEditor::new("hello world");
        md.set_cursor(Position::new(0, 11));
        md.delete_word_back();
        assert_eq!(md.editor().text(), "hello ");
        assert_eq!(md.tree().source(), "hello ");
    }

    #[test]
    fn delete_word_forward_syncs_tree() {
        let mut md = MarkdownEditor::new("hello world");
        md.set_cursor(Position::new(0, 0));
        md.delete_word_forward();
        assert_eq!(md.editor().text(), "world");
        assert_eq!(md.tree().source(), "world");
    }

    #[test]
    fn read_only_methods_work() {
        let md = MarkdownEditor::new("# Hello");
        assert_eq!(md.text(), "# Hello");
        assert_eq!(md.cursor(), Position::zero());
        assert!(md.selection().is_cursor());
        assert_eq!(md.selected_text(), "");
        assert!(!md.is_dirty());
        assert!(!md.can_undo());
        assert!(!md.can_redo());
    }

    #[test]
    fn movement_methods_work() {
        let mut md = MarkdownEditor::new("hello world\nsecond line");
        md.move_right();
        assert_eq!(md.cursor(), Position::new(0, 1));
        md.move_to_line_end();
        assert_eq!(md.cursor(), Position::new(0, 11));
        md.move_down();
        assert_eq!(md.cursor(), Position::new(1, 11));
        md.move_to_line_start();
        assert_eq!(md.cursor(), Position::new(1, 0));
        md.move_left();
        // move_left from col 0 of line 1 wraps to end of line 0
        // Actually, Editor::move_left goes to prev char offset, which is end of line 0 (the newline char)
        // The offset of Position(1,0) is the char after '\n' on line 0. Going back one char
        // lands on the '\n' itself, which is Position(0, 11).
        assert_eq!(md.cursor(), Position::new(0, 11));
        md.move_to_start();
        assert_eq!(md.cursor(), Position::zero());
        md.move_to_end();
        assert_eq!(md.cursor(), Position::new(1, 11));
        md.move_to_start();
        md.move_up();
        // Already at line 0, move_up goes to (0, 0)
        assert_eq!(md.cursor(), Position::zero());
    }
}
