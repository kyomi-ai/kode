/// A single edit step: insert, delete, or replace at a char offset.
/// Each step stores enough information to be inverted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditStep {
    /// Char offset where the edit starts.
    pub offset: usize,
    /// Text that was deleted (empty for pure inserts).
    pub deleted: String,
    /// Text that was inserted (empty for pure deletes).
    pub inserted: String,
}

impl EditStep {
    /// Create an insert step.
    pub fn insert(offset: usize, text: impl Into<String>) -> Self {
        Self {
            offset,
            deleted: String::new(),
            inserted: text.into(),
        }
    }

    /// Create a delete step.
    pub fn delete(offset: usize, deleted: impl Into<String>) -> Self {
        Self {
            offset,
            deleted: deleted.into(),
            inserted: String::new(),
        }
    }

    /// Create a replace step.
    pub fn replace(offset: usize, deleted: impl Into<String>, inserted: impl Into<String>) -> Self {
        Self {
            offset,
            deleted: deleted.into(),
            inserted: inserted.into(),
        }
    }

    /// Create the inverse of this step (for undo).
    pub fn inverse(&self) -> Self {
        Self {
            offset: self.offset,
            deleted: self.inserted.clone(),
            inserted: self.deleted.clone(),
        }
    }

    /// Number of chars inserted.
    pub fn inserted_len(&self) -> usize {
        self.inserted.chars().count()
    }

    /// Number of chars deleted.
    pub fn deleted_len(&self) -> usize {
        self.deleted.chars().count()
    }

    /// True if this is a single character insert (for coalescing).
    pub fn is_single_char_insert(&self) -> bool {
        self.deleted.is_empty() && self.inserted.chars().count() == 1
    }

    /// True if this is a single character delete (for coalescing).
    pub fn is_single_char_delete(&self) -> bool {
        self.inserted.is_empty() && self.deleted.chars().count() == 1
    }
}

/// A transaction groups one or more edit steps into an atomic operation.
/// Applying a transaction is all-or-nothing. Transactions can be inverted for undo.
#[derive(Debug, Clone)]
pub struct Transaction {
    pub steps: Vec<EditStep>,
    /// Cursor position before this transaction was applied (for undo restoration).
    pub cursor_before: Option<crate::Position>,
    /// Cursor position after this transaction was applied.
    pub cursor_after: Option<crate::Position>,
}

impl Transaction {
    /// Create a transaction from a single step.
    pub fn single(step: EditStep) -> Self {
        Self {
            steps: vec![step],
            cursor_before: None,
            cursor_after: None,
        }
    }

    /// Create a transaction from multiple steps.
    pub fn new(steps: Vec<EditStep>) -> Self {
        Self {
            steps,
            cursor_before: None,
            cursor_after: None,
        }
    }

    /// Set cursor positions for undo/redo.
    pub fn with_cursors(mut self, before: crate::Position, after: crate::Position) -> Self {
        self.cursor_before = Some(before);
        self.cursor_after = Some(after);
        self
    }

    /// Create the inverse transaction (for undo). Steps are reversed and individually inverted.
    /// Cursor positions are swapped.
    pub fn inverse(&self) -> Self {
        Self {
            steps: self.steps.iter().rev().map(|s| s.inverse()).collect(),
            cursor_before: self.cursor_after,
            cursor_after: self.cursor_before,
        }
    }

    /// True if this transaction can be coalesced with another.
    /// A single-char insert/delete can merge into an existing run if it
    /// continues at the expected position. Newlines break coalescing.
    pub fn can_coalesce(&self, other: &Transaction) -> bool {
        if self.steps.len() != 1 || other.steps.len() != 1 {
            return false;
        }
        let a = &self.steps[0];
        let b = &other.steps[0];

        // Coalesce a single-char insert continuing an insert run
        if a.deleted.is_empty() && !a.inserted.is_empty()
            && b.is_single_char_insert()
        {
            let a_last = a.inserted.chars().last().unwrap();
            let b_char = b.inserted.chars().next().unwrap();
            // Don't coalesce across newlines or word boundaries.
            // A space typed starts a new undo group (b_char == ' '), and
            // the first non-space after a space also starts a new group.
            if a_last == '\n' || b_char == '\n' || b_char == ' ' || a_last == ' ' {
                return false;
            }
            return b.offset == a.offset + a.inserted_len();
        }

        // Coalesce a single-char delete continuing a delete run
        if a.inserted.is_empty() && !a.deleted.is_empty()
            && b.is_single_char_delete()
        {
            let b_char = b.deleted.chars().next().unwrap();
            if b_char == '\n' {
                return false;
            }
            // Backspace: offset decreases by 1
            if b.offset + 1 == a.offset {
                return true;
            }
            // Forward delete: same offset as end of existing delete
            if b.offset == a.offset {
                return true;
            }
        }

        false
    }

    /// Merge another transaction into this one (for coalescing).
    pub fn merge(&mut self, other: &Transaction) {
        if self.steps.len() != 1 || other.steps.len() != 1 {
            return;
        }
        // Update cursor_after to the merged transaction's end position
        if other.cursor_after.is_some() {
            self.cursor_after = other.cursor_after;
        }

        let a = &mut self.steps[0];
        let b = &other.steps[0];

        if a.deleted.is_empty() && b.is_single_char_insert() {
            // Append insert
            a.inserted.push_str(&b.inserted);
        } else if a.inserted.is_empty() && b.is_single_char_delete() {
            if b.offset + 1 == a.offset {
                // Backspace: prepend deleted char
                a.deleted.insert_str(0, &b.deleted);
                a.offset = b.offset;
            } else if b.offset == a.offset {
                // Forward delete: append deleted char
                a.deleted.push_str(&b.deleted);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_inverse() {
        let step = EditStep::insert(5, "hello");
        let inv = step.inverse();
        assert_eq!(inv.offset, 5);
        assert_eq!(inv.deleted, "hello");
        assert!(inv.inserted.is_empty());
    }

    #[test]
    fn delete_inverse() {
        let step = EditStep::delete(3, "abc");
        let inv = step.inverse();
        assert_eq!(inv.offset, 3);
        assert!(inv.deleted.is_empty());
        assert_eq!(inv.inserted, "abc");
    }

    #[test]
    fn replace_inverse() {
        let step = EditStep::replace(0, "old", "new");
        let inv = step.inverse();
        assert_eq!(inv.deleted, "new");
        assert_eq!(inv.inserted, "old");
    }

    #[test]
    fn transaction_inverse() {
        let tx = Transaction::new(vec![
            EditStep::insert(0, "a"),
            EditStep::insert(1, "b"),
        ]);
        let inv = tx.inverse();
        assert_eq!(inv.steps.len(), 2);
        // Reversed order
        assert_eq!(inv.steps[0].offset, 1);
        assert_eq!(inv.steps[0].deleted, "b");
        assert_eq!(inv.steps[1].offset, 0);
        assert_eq!(inv.steps[1].deleted, "a");
    }

    #[test]
    fn coalesce_inserts() {
        let a = Transaction::single(EditStep::insert(0, "a"));
        let b = Transaction::single(EditStep::insert(1, "b"));
        let c = Transaction::single(EditStep::insert(2, "\n"));
        assert!(a.can_coalesce(&b));
        assert!(!b.can_coalesce(&c)); // newline breaks coalescing
    }

    #[test]
    fn coalesce_backspaces() {
        let a = Transaction::single(EditStep::delete(3, "d"));
        let b = Transaction::single(EditStep::delete(2, "c"));
        assert!(a.can_coalesce(&b));
    }

    #[test]
    fn merge_inserts() {
        let mut a = Transaction::single(EditStep::insert(0, "a"));
        let b = Transaction::single(EditStep::insert(1, "b"));
        a.merge(&b);
        assert_eq!(a.steps[0].inserted, "ab");
    }

    #[test]
    fn merge_backspaces() {
        let mut a = Transaction::single(EditStep::delete(3, "d"));
        let b = Transaction::single(EditStep::delete(2, "c"));
        a.merge(&b);
        assert_eq!(a.steps[0].deleted, "cd");
        assert_eq!(a.steps[0].offset, 2);
    }

    // ── Coalescing space bug ──────────────────────────────────────────────

    /// Typing a space after a word should start a new undo group, not append
    /// to the current word's group.  The rule only fires to break coalescing
    /// when a_last==' ' AND b_char!=' ', which means the space itself slips
    /// into the existing word group instead of opening a fresh one.
    ///
    /// Correct behaviour: `can_coalesce` returns false when b_char == ' ',
    /// so each space-separated word is its own undo unit.
    #[test]
    fn space_typed_after_word_breaks_coalescing() {
        let a = Transaction::single(EditStep::insert(0, "hello"));
        let b = Transaction::single(EditStep::insert(5, " "));
        // Currently returns true (bug): space gets coalesced into "hello"'s group.
        // Should return false so the space is a separate undo step.
        assert!(
            !a.can_coalesce(&b),
            "space after a word should break coalescing, not join the word's group"
        );
    }
}
