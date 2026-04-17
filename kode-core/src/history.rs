use crate::transaction::Transaction;

/// Undo/redo history using invertible transactions.
///
/// Consecutive single-character edits are coalesced into groups so that
/// undoing "typed a word" undoes the whole word, not one char at a time.
#[derive(Debug)]
pub struct History {
    undo_stack: Vec<Transaction>,
    redo_stack: Vec<Transaction>,
    /// Undo stack depth at the last clean point. None if never saved or
    /// the clean state has been diverged from (new edit after undo past clean).
    clean_depth: Option<usize>,
    /// When true, the next push will not coalesce with the previous entry.
    force_new_group: bool,
}

impl History {
    pub fn new() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            clean_depth: Some(0),
            force_new_group: false,
        }
    }

    /// Record a transaction. Clears the redo stack.
    /// Tries to coalesce with the most recent undo entry.
    pub fn push(&mut self, tx: Transaction) {
        // If we had a clean point in the redo future, it's now unreachable
        if let Some(depth) = self.clean_depth {
            if depth > self.undo_stack.len() {
                self.clean_depth = None;
            }
        }
        self.redo_stack.clear();

        if !self.force_new_group {
            if let Some(last) = self.undo_stack.last_mut() {
                if last.can_coalesce(&tx) {
                    last.merge(&tx);
                    return;
                }
            }
        }
        self.force_new_group = false;

        self.undo_stack.push(tx);
    }

    /// Pop the most recent transaction for undo. Returns its inverse
    /// (which should be applied to revert the change).
    pub fn undo(&mut self) -> Option<Transaction> {
        let tx = self.undo_stack.pop()?;
        let inverse = tx.inverse();
        self.redo_stack.push(tx);
        Some(inverse)
    }

    /// Pop the most recent redo transaction. Returns it
    /// (which should be re-applied).
    pub fn redo(&mut self) -> Option<Transaction> {
        let tx = self.redo_stack.pop()?;
        let re_apply = tx.clone();
        self.undo_stack.push(tx);
        Some(re_apply)
    }

    /// Mark the current state as clean (e.g., after save).
    /// Also forces the next edit into a new undo group so that
    /// undoing back to this point is always possible.
    pub fn mark_clean(&mut self) {
        self.clean_depth = Some(self.undo_stack.len());
        self.force_new_group = true;
    }

    /// Check if the document is dirty (modified since last save).
    pub fn is_dirty(&self) -> bool {
        self.clean_depth != Some(self.undo_stack.len())
    }

    /// True if there are entries to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// True if there are entries to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Clear all history and reset to clean state.
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.clean_depth = Some(0);
        self.force_new_group = false;
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::EditStep;

    #[test]
    fn undo_redo_basic() {
        let mut history = History::new();
        let tx = Transaction::single(EditStep::insert(0, "hello"));
        history.push(tx);

        assert!(history.can_undo());
        assert!(!history.can_redo());

        let undo_tx = history.undo().unwrap();
        assert_eq!(undo_tx.steps[0].offset, 0);
        assert_eq!(undo_tx.steps[0].deleted, "hello");
        assert!(undo_tx.steps[0].inserted.is_empty());

        assert!(!history.can_undo());
        assert!(history.can_redo());

        let redo_tx = history.redo().unwrap();
        assert_eq!(redo_tx.steps[0].inserted, "hello");
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut history = History::new();
        history.push(Transaction::single(EditStep::insert(0, "a")));
        history.push(Transaction::single(EditStep::insert(1, "b")));
        history.undo(); // undo "b" (coalesced with "a" so undoes "ab")

        // Can redo
        assert!(history.can_redo());

        // New edit clears redo
        history.push(Transaction::single(EditStep::insert(0, "x")));
        assert!(!history.can_redo());
    }

    #[test]
    fn coalescing_inserts() {
        let mut history = History::new();
        history.push(Transaction::single(EditStep::insert(0, "h")));
        history.push(Transaction::single(EditStep::insert(1, "i")));

        // Should be coalesced into one entry
        assert_eq!(history.undo_stack.len(), 1);
        assert_eq!(history.undo_stack[0].steps[0].inserted, "hi");
    }

    #[test]
    fn newline_breaks_coalescing() {
        let mut history = History::new();
        history.push(Transaction::single(EditStep::insert(0, "a")));
        history.push(Transaction::single(EditStep::insert(1, "\n")));

        // Should NOT coalesce across newline
        assert_eq!(history.undo_stack.len(), 2);
    }

    #[test]
    fn dirty_tracking() {
        let mut history = History::new();
        assert!(!history.is_dirty());

        history.push(Transaction::single(EditStep::insert(0, "x")));
        assert!(history.is_dirty());

        history.mark_clean();
        assert!(!history.is_dirty());

        // After mark_clean, next push starts a new group even if coalescible
        history.push(Transaction::single(EditStep::insert(1, "y")));
        assert!(history.is_dirty());

        // Undo back to clean point
        history.undo();
        assert!(!history.is_dirty());
    }
}
