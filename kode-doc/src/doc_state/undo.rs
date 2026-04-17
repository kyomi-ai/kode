//! Undo/redo history management.

use super::{DocState, HistoryEntry};

impl DocState {
    // ── Internal ──────────────────────────────────────────────────────

    /// Maximum number of undo entries to keep. Oldest entries are
    /// dropped when the stack exceeds this limit.
    const MAX_UNDO_DEPTH: usize = 100;

    /// Push the current doc and selection onto the undo stack.
    pub(super) fn push_undo(&mut self) {
        if self.undo_stack.len() >= Self::MAX_UNDO_DEPTH {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(HistoryEntry {
            doc: self.doc.clone(),
            selection: self.selection.clone(),
        });
    }

    // ── Public API ───────────────────────────────────────────────────

    /// Undo the last edit. Returns `true` if there was something to undo.
    pub fn undo(&mut self) -> bool {
        let Some(entry) = self.undo_stack.pop() else {
            return false;
        };

        // Push current state to redo stack.
        self.redo_stack.push(HistoryEntry {
            doc: self.doc.clone(),
            selection: self.selection.clone(),
        });

        self.doc = entry.doc;
        self.selection = entry.selection;
        true
    }

    /// Redo the last undone edit. Returns `true` if there was something to redo.
    pub fn redo(&mut self) -> bool {
        let Some(entry) = self.redo_stack.pop() else {
            return false;
        };

        // Push current state to undo stack.
        self.undo_stack.push(HistoryEntry {
            doc: self.doc.clone(),
            selection: self.selection.clone(),
        });

        self.doc = entry.doc;
        self.selection = entry.selection;
        true
    }
}
