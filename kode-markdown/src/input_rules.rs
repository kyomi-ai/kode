use kode_core::{Editor, Position};

/// Markdown input rules — auto-behaviors triggered by specific keystrokes.
///
/// These are called by the view layer when the user presses Enter, Tab, etc.
/// They inspect the current context and apply markdown-aware transformations.
pub struct InputRules;

impl InputRules {
    /// Handle Enter key press. Returns true if a rule was applied.
    ///
    /// Rules:
    /// - In a list item: continue the list with a new item
    /// - In an empty list item: exit the list (remove the marker)
    /// - In a blockquote: continue the quote
    /// - In a fenced code block: just insert newline (no special behavior)
    pub fn handle_enter(editor: &mut Editor) -> bool {
        let cursor = editor.cursor();
        let line_text = editor.buffer().line(cursor.line).to_string();
        let trimmed = line_text.trim_end_matches('\n');

        // Check for empty list item → exit list
        if let Some(prefix) = Self::list_prefix(trimmed) {
            let prefix_chars = prefix.chars().count();
            let content_after_prefix: String = trimmed.chars().skip(prefix_chars).collect();
            if content_after_prefix.trim().is_empty() {
                // Empty list item — remove the marker
                let line_start = Position::new(cursor.line, 0);
                let line_end = Position::new(cursor.line, editor.buffer().line_len(cursor.line));
                editor.set_selection(line_start, line_end);
                editor.insert("");
                return true;
            }

            // Non-empty list item — split at cursor position.
            // Text after cursor moves to the new list item.
            let next_prefix = Self::next_list_prefix(&prefix);
            let line_len = editor.buffer().line_len(cursor.line);

            if cursor.col >= prefix_chars && cursor.col < line_len {
                // Cursor is mid-content (past prefix): select from cursor to end,
                // replace with newline + prefix + text-after-cursor
                let after_cursor: String = trimmed
                    .chars()
                    .skip(cursor.col)
                    .collect();
                let after_cursor = after_cursor.trim_end_matches('\n');
                let line_end = Position::new(cursor.line, line_len);
                editor.set_selection(cursor, line_end);
                editor.insert(&format!("\n{next_prefix}{after_cursor}"));
                // Place cursor right after the new prefix
                let new_line = cursor.line + 1;
                let new_col = next_prefix.chars().count();
                editor.set_cursor(Position::new(new_line, new_col));
            } else {
                // Cursor at end of line: just continue list
                editor.insert(&format!("\n{next_prefix}"));
            }
            return true;
        }

        // Check for blockquote continuation
        if trimmed.starts_with("> ") || trimmed == ">" {
            if trimmed == ">" || trimmed == "> " {
                // Empty blockquote line — exit quote
                let line_start = Position::new(cursor.line, 0);
                let line_end = Position::new(cursor.line, editor.buffer().line_len(cursor.line));
                editor.set_selection(line_start, line_end);
                editor.insert("");
                return true;
            }
            // Split blockquote at cursor, trimming the leading space from moved text
            let line_len = editor.buffer().line_len(cursor.line);
            if cursor.col >= 2 && cursor.col < line_len {
                let after_cursor: String = trimmed.chars().skip(cursor.col).collect();
                let after_trimmed = after_cursor.trim_start();
                let line_end = Position::new(cursor.line, line_len);
                editor.set_selection(cursor, line_end);
                editor.insert(&format!("\n> {after_trimmed}"));
                editor.set_cursor(Position::new(cursor.line + 1, 2));
            } else {
                editor.insert("\n> ");
            }
            return true;
        }

        false // no rule applied, caller should insert plain newline
    }

    /// Handle Tab key press. Returns true if a rule was applied.
    ///
    /// Rules:
    /// - In a list item: increase indent level
    pub fn handle_tab(editor: &mut Editor) -> bool {
        let cursor = editor.cursor();
        let line_text = editor.buffer().line(cursor.line).to_string();
        let trimmed_start = line_text.trim_end_matches('\n');

        if Self::list_prefix(trimmed_start).is_some() {
            // Add 2 spaces of indent at line start
            let line_start = Position::new(cursor.line, 0);
            editor.set_cursor(line_start);
            editor.insert("  ");
            // Restore cursor position (shifted by 2)
            editor.set_cursor(Position::new(cursor.line, cursor.col + 2));
            return true;
        }

        false
    }

    /// Handle Shift+Tab key press. Returns true if a rule was applied.
    ///
    /// Rules:
    /// - In an indented list item: decrease indent level
    pub fn handle_shift_tab(editor: &mut Editor) -> bool {
        let cursor = editor.cursor();
        let line_text = editor.buffer().line(cursor.line).to_string();

        // Check if line starts with whitespace (indented)
        let indent = line_text.chars().take_while(|c| c.is_whitespace() && *c != '\n').count();
        if indent >= 2 && Self::list_prefix(line_text.trim_start()).is_some() {
            // Remove 2 spaces of indent from line start
            let line_start = Position::new(cursor.line, 0);
            let indent_end = Position::new(cursor.line, 2);
            editor.set_selection(line_start, indent_end);
            editor.insert("");
            // Restore cursor position (shifted back by 2)
            let new_col = cursor.col.saturating_sub(2);
            editor.set_cursor(Position::new(cursor.line, new_col));
            return true;
        }

        false
    }

    /// Handle Backspace at the start of a list item or blockquote.
    /// Returns true if a rule was applied.
    pub fn handle_backspace_at_prefix(editor: &mut Editor) -> bool {
        let cursor = editor.cursor();
        let line_text = editor.buffer().line(cursor.line).to_string();
        let trimmed = line_text.trim_end_matches('\n');

        // Only apply if cursor is right after the prefix
        if let Some(prefix) = Self::list_prefix(trimmed) {
            let prefix_char_len = prefix.chars().count();
            if cursor.col == prefix_char_len {
                // Remove the list prefix
                let line_start = Position::new(cursor.line, 0);
                let prefix_end = Position::new(cursor.line, prefix_char_len);
                editor.set_selection(line_start, prefix_end);
                editor.insert("");
                return true;
            }
        }

        if trimmed.starts_with("> ") && cursor.col == 2 {
            let line_start = Position::new(cursor.line, 0);
            let prefix_end = Position::new(cursor.line, 2);
            editor.set_selection(line_start, prefix_end);
            editor.insert("");
            return true;
        }

        false
    }

    // ── Helpers ──────────────────────────────────────────────────────────

    /// Detect if a line starts with a list marker. Returns the full prefix
    /// including trailing space (e.g., "- ", "1. ", "  - ").
    fn list_prefix(line: &str) -> Option<String> {
        let indent: String = line.chars().take_while(|c| *c == ' ').collect();
        let after_indent = &line[indent.len()..];

        // Task list markers (check before bullet markers since they start with `- `)
        if after_indent.starts_with("- [ ] ") || after_indent.starts_with("- [x] ") {
            return Some(format!("{indent}{}", &after_indent[..6]));
        }

        // Bullet markers
        if after_indent.starts_with("- ")
            || after_indent.starts_with("* ")
            || after_indent.starts_with("+ ")
        {
            return Some(format!("{indent}{}", &after_indent[..2]));
        }

        // Ordered list markers
        if let Some(dot_pos) = after_indent.find(". ") {
            let num_part = &after_indent[..dot_pos];
            if !num_part.is_empty() && num_part.chars().all(|c| c.is_ascii_digit()) {
                return Some(format!("{indent}{}", &after_indent[..dot_pos + 2]));
            }
        }
        if let Some(paren_pos) = after_indent.find(") ") {
            let num_part = &after_indent[..paren_pos];
            if !num_part.is_empty() && num_part.chars().all(|c| c.is_ascii_digit()) {
                return Some(format!("{indent}{}", &after_indent[..paren_pos + 2]));
            }
        }

        None
    }

    /// Compute the prefix for the next list item.
    /// Increments ordered list numbers, preserves bullet style.
    fn next_list_prefix(prefix: &str) -> String {
        let indent: String = prefix.chars().take_while(|c| *c == ' ').collect();
        let after_indent = &prefix[indent.len()..];

        // Task list → new unchecked item
        if after_indent.starts_with("- [ ] ") || after_indent.starts_with("- [x] ") {
            return format!("{indent}- [ ] ");
        }

        // Ordered list → increment number
        if let Some(dot_pos) = after_indent.find(". ") {
            let num_part = &after_indent[..dot_pos];
            if let Ok(n) = num_part.parse::<usize>() {
                return format!("{indent}{}. ", n + 1);
            }
        }
        if let Some(paren_pos) = after_indent.find(") ") {
            let num_part = &after_indent[..paren_pos];
            if let Ok(n) = num_part.parse::<usize>() {
                return format!("{indent}{}) ", n + 1);
            }
        }

        // Bullet list → same marker
        prefix.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kode_core::Position;

    #[test]
    fn list_prefix_detection() {
        assert_eq!(InputRules::list_prefix("- item"), Some("- ".to_string()));
        assert_eq!(InputRules::list_prefix("* item"), Some("* ".to_string()));
        assert_eq!(InputRules::list_prefix("1. item"), Some("1. ".to_string()));
        assert_eq!(
            InputRules::list_prefix("  - nested"),
            Some("  - ".to_string())
        );
        assert_eq!(InputRules::list_prefix("not a list"), None);
        assert_eq!(
            InputRules::list_prefix("- [ ] task"),
            Some("- [ ] ".to_string())
        );
    }

    #[test]
    fn next_prefix_bullet() {
        assert_eq!(InputRules::next_list_prefix("- "), "- ");
        assert_eq!(InputRules::next_list_prefix("  - "), "  - ");
    }

    #[test]
    fn next_prefix_ordered() {
        assert_eq!(InputRules::next_list_prefix("1. "), "2. ");
        assert_eq!(InputRules::next_list_prefix("3. "), "4. ");
        assert_eq!(InputRules::next_list_prefix("  5. "), "  6. ");
    }

    #[test]
    fn next_prefix_task() {
        assert_eq!(InputRules::next_list_prefix("- [ ] "), "- [ ] ");
        assert_eq!(InputRules::next_list_prefix("- [x] "), "- [ ] ");
    }

    #[test]
    fn enter_continues_bullet_list() {
        let mut ed = Editor::new("- item 1");
        ed.set_cursor(Position::new(0, 8));
        let handled = InputRules::handle_enter(&mut ed);
        assert!(handled);
        assert_eq!(ed.text(), "- item 1\n- ");
    }

    #[test]
    fn enter_continues_ordered_list() {
        let mut ed = Editor::new("1. first");
        ed.set_cursor(Position::new(0, 8));
        let handled = InputRules::handle_enter(&mut ed);
        assert!(handled);
        assert_eq!(ed.text(), "1. first\n2. ");
    }

    #[test]
    fn enter_exits_empty_list_item() {
        let mut ed = Editor::new("- item 1\n- ");
        ed.set_cursor(Position::new(1, 2));
        let handled = InputRules::handle_enter(&mut ed);
        assert!(handled);
        assert_eq!(ed.text(), "- item 1\n");
    }

    #[test]
    fn enter_continues_blockquote() {
        let mut ed = Editor::new("> quote text");
        ed.set_cursor(Position::new(0, 12));
        let handled = InputRules::handle_enter(&mut ed);
        assert!(handled);
        assert_eq!(ed.text(), "> quote text\n> ");
    }

    #[test]
    fn enter_mid_blockquote_splits_without_double_space() {
        let mut ed = Editor::new("> hello world");
        ed.set_cursor(Position::new(0, 7)); // after "> hello"
        let handled = InputRules::handle_enter(&mut ed);
        assert!(handled);
        assert_eq!(ed.text(), "> hello\n> world");
    }

    #[test]
    fn enter_exits_empty_blockquote() {
        let mut ed = Editor::new("> text\n> ");
        ed.set_cursor(Position::new(1, 2));
        let handled = InputRules::handle_enter(&mut ed);
        assert!(handled);
        assert_eq!(ed.text(), "> text\n");
    }

    #[test]
    fn tab_indents_list_item() {
        let mut ed = Editor::new("- item");
        ed.set_cursor(Position::new(0, 6));
        let handled = InputRules::handle_tab(&mut ed);
        assert!(handled);
        assert_eq!(ed.text(), "  - item");
        assert_eq!(ed.cursor(), Position::new(0, 8));
    }

    #[test]
    fn shift_tab_outdents_list_item() {
        let mut ed = Editor::new("  - item");
        ed.set_cursor(Position::new(0, 8));
        let handled = InputRules::handle_shift_tab(&mut ed);
        assert!(handled);
        assert_eq!(ed.text(), "- item");
        assert_eq!(ed.cursor(), Position::new(0, 6));
    }

    #[test]
    fn backspace_removes_list_prefix() {
        let mut ed = Editor::new("- ");
        ed.set_cursor(Position::new(0, 2));
        let handled = InputRules::handle_backspace_at_prefix(&mut ed);
        assert!(handled);
        assert_eq!(ed.text(), "");
    }

    #[test]
    fn no_rule_for_plain_text() {
        let mut ed = Editor::new("just plain text");
        ed.set_cursor(Position::new(0, 15));
        let handled = InputRules::handle_enter(&mut ed);
        assert!(!handled);
        // Text should be unchanged
        assert_eq!(ed.text(), "just plain text");
    }

    #[test]
    fn tab_no_effect_on_plain_text() {
        let mut ed = Editor::new("not a list");
        ed.set_cursor(Position::new(0, 10));
        let handled = InputRules::handle_tab(&mut ed);
        assert!(!handled);
        assert_eq!(ed.text(), "not a list");
    }

    #[test]
    fn enter_mid_list_item_splits_content() {
        let mut ed = Editor::new("- hello world");
        ed.set_cursor(Position::new(0, 7)); // after "- hello"
        let handled = InputRules::handle_enter(&mut ed);
        assert!(handled);
        assert_eq!(ed.text(), "- hello\n-  world");
        assert_eq!(ed.cursor(), Position::new(1, 2)); // after "- "
    }

    #[test]
    fn enter_mid_ordered_list_splits() {
        let mut ed = Editor::new("1. hello world");
        ed.set_cursor(Position::new(0, 8)); // after "1. hello"
        let handled = InputRules::handle_enter(&mut ed);
        assert!(handled);
        assert_eq!(ed.text(), "1. hello\n2.  world");
    }

    #[test]
    fn multi_digit_ordered_list_continuation() {
        let mut ed = Editor::new("10. item ten");
        ed.set_cursor(Position::new(0, 12));
        let handled = InputRules::handle_enter(&mut ed);
        assert!(handled);
        assert_eq!(ed.text(), "10. item ten\n11. ");
    }
}
