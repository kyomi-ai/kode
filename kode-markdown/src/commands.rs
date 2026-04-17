use kode_core::{EditStep, Editor, Position, Transaction};

/// Describes which formatting is active at the current cursor position.
///
/// Used by toolbar UI to highlight active formatting buttons.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormattingState {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strikethrough: bool,
    /// 0 = no heading, 1-3 = H1-H3
    pub heading_level: u8,
    pub bullet_list: bool,
    pub ordered_list: bool,
    pub blockquote: bool,
}

/// Markdown-aware editing commands that operate on a kode-core Editor.
///
/// Each command reads the current editor state, then applies
/// text operations via the Editor API. All edits are pure text transforms.
///
/// Note: inline mark toggle commands read directly from the editor buffer,
/// not from the tree, so they always operate on current text.
pub struct MarkdownCommands;

impl MarkdownCommands {
    /// Toggle bold (**) around the current selection.
    /// If selection is already bold, removes the markers.
    /// If no selection, inserts `****` and places cursor between them.
    pub fn toggle_bold(editor: &mut Editor) {
        Self::toggle_inline_mark(editor, "**");
    }

    /// Toggle italic (*) around the current selection.
    pub fn toggle_italic(editor: &mut Editor) {
        Self::toggle_inline_mark(editor, "*");
    }

    /// Toggle inline code (`) around the current selection.
    pub fn toggle_inline_code(editor: &mut Editor) {
        Self::toggle_inline_mark(editor, "`");
    }

    /// Toggle strikethrough (~~) around the current selection.
    pub fn toggle_strikethrough(editor: &mut Editor) {
        Self::toggle_inline_mark(editor, "~~");
    }

    /// Set the heading level for the current line.
    /// Level 0 removes the heading prefix. Levels 1-6 set the corresponding heading.
    pub fn set_heading(editor: &mut Editor, level: u8) {
        let mut cursor = editor.cursor();
        let line_text = editor.buffer().line(cursor.line).to_string();

        // If cursor is on an empty/blank line, look at the previous line.
        // This handles the case where End/ArrowRight moves the cursor past a
        // heading's newline onto the blank separator line — the user's intent
        // is to change the heading they were just on, not create a new one.
        if line_text.trim().is_empty() && cursor.line > 0 {
            let prev_line_text = editor.buffer().line(cursor.line - 1).to_string();
            if prev_line_text.trim_start().starts_with('#') {
                cursor = Position::new(cursor.line - 1, editor.buffer().line_len(cursor.line - 1));
                editor.set_cursor(cursor);
            } else if level > 0 {
                return; // Don't create empty headings on random blank lines
            }
        }

        let line_text = editor.buffer().line(cursor.line).to_string();

        // Find existing heading prefix (all in chars since # and space are ASCII)
        let trimmed = line_text.trim_start();
        let content_start_chars = if trimmed.starts_with('#') {
            let hashes = trimmed.chars().take_while(|&c| c == '#').count();
            let rest: String = trimmed.chars().skip(hashes).collect();
            let space_after = if rest.starts_with(' ') { 1 } else { 0 };
            hashes + space_after
        } else {
            0
        };

        // Get the content without heading prefix
        let content: String = trimmed.chars().skip(content_start_chars).collect();
        let content = content.trim_end_matches('\n');

        // Build new line
        let new_line = if level == 0 {
            content.to_string()
        } else {
            let prefix: String = "#".repeat(level as usize);
            format!("{prefix} {content}")
        };

        // Replace the line content (not the newline)
        let line_start = Position::new(cursor.line, 0);
        let line_end_col = editor.buffer().line_len(cursor.line);
        let line_end = Position::new(cursor.line, line_end_col);

        editor.set_selection(line_start, line_end);
        editor.insert(&new_line);

        // Preserve cursor's relative position within the content.
        // The cursor was at `cursor.col` in the old line. Adjust for the
        // prefix length change: old prefix was `content_start_chars` chars,
        // new prefix is `level + 1` chars (e.g. "## " = 3 chars) or 0.
        let new_prefix_chars = if level == 0 { 0 } else { level as usize + 1 };
        let old_content_col = cursor.col.saturating_sub(content_start_chars);
        let new_col = (new_prefix_chars + old_content_col).min(new_line.chars().count());
        editor.set_cursor(Position::new(cursor.line, new_col));
    }

    /// Toggle a block quote on the current line or selection.
    /// If already quoted, removes the `> ` prefix. Otherwise adds it.
    /// Uses atomic transaction for multi-line operations.
    pub fn toggle_blockquote(editor: &mut Editor) {
        let sel = editor.selection();
        let cursor = editor.cursor();
        let start_line = sel.start().line;
        let end_line = sel.end().line;

        // Check if all lines in range are already quoted
        let all_quoted = (start_line..=end_line).all(|line| {
            let text = editor.buffer().line(line).to_string();
            text.starts_with("> ") || text.starts_with(">")
        });

        // Build steps top-to-bottom, tracking offset changes
        let mut steps = Vec::new();
        let mut offset_delta: isize = 0;
        for line in start_line..=end_line {
            let line_text = editor.buffer().line(line).to_string();
            let line_start_char = editor.buffer().line_to_char(line);
            let adjusted_offset = (line_start_char as isize + offset_delta) as usize;
            let content = line_text.trim_end_matches('\n');
            let content_chars = content.chars().count();

            if all_quoted {
                let unquoted = if let Some(s) = content.strip_prefix("> ") {
                    s
                } else if let Some(s) = content.strip_prefix('>') {
                    s
                } else {
                    content
                };
                let new_chars = unquoted.chars().count();
                steps.push(EditStep::replace(adjusted_offset, content.to_string(), unquoted.to_string()));
                offset_delta += new_chars as isize - content_chars as isize;
            } else {
                let new_text = format!("> {content}");
                let new_chars = new_text.chars().count();
                steps.push(EditStep::replace(adjusted_offset, content.to_string(), new_text));
                offset_delta += new_chars as isize - content_chars as isize;
            }
        }

        if !steps.is_empty() {
            editor.apply_transaction(Transaction::new(steps));
            // Restore cursor position adjusted for prefix change
            let prefix_delta: isize = if all_quoted { -2 } else { 2 };
            let new_col = (cursor.col as isize + prefix_delta).max(0) as usize;
            let line_len = editor.buffer().line_len(cursor.line);
            editor.set_cursor(Position::new(cursor.line, new_col.min(line_len)));
        }
    }

    /// Toggle a bullet list prefix on the current line or selection.
    /// If already a list item, removes `- `. Otherwise adds `- `.
    /// Lines already prefixed are skipped when adding (no double-prefix).
    pub fn toggle_bullet_list(editor: &mut Editor) {
        Self::toggle_list_prefix(editor, "- ");
    }

    /// Toggle an ordered list prefix on the current line or selection.
    pub fn toggle_ordered_list(editor: &mut Editor) {
        let sel = editor.selection();
        let start_line = sel.start().line;
        let end_line = sel.end().line;

        let all_ordered = (start_line..=end_line).all(|line| {
            let text = editor.buffer().line(line).to_string();
            let trimmed = text.trim_start();
            Self::strip_ordered_prefix(trimmed).is_some()
        });

        let mut steps = Vec::new();
        let mut offset_delta: isize = 0;
        for line in start_line..=end_line {
            let line_text = editor.buffer().line(line).to_string();
            let line_start_char = editor.buffer().line_to_char(line);
            let adjusted_offset = (line_start_char as isize + offset_delta) as usize;
            let content = line_text.trim_end_matches('\n');
            let content_chars = content.chars().count();

            if all_ordered {
                let trimmed = content.trim_start();
                let leading_indent = &content[..content.len() - trimmed.len()]; // safe: whitespace is ASCII
                let stripped = Self::strip_ordered_prefix(trimmed).unwrap_or(trimmed);
                let new_text = format!("{leading_indent}{stripped}");
                let new_chars = new_text.chars().count();
                steps.push(EditStep::replace(adjusted_offset, content.to_string(), new_text));
                offset_delta += new_chars as isize - content_chars as isize;
            } else {
                let num = line - start_line + 1;
                // Strip existing list prefix or block prefix (heading/blockquote)
                let inner = content
                    .strip_prefix("- ")
                    .or_else(|| content.strip_prefix("* "))
                    .or_else(|| content.strip_prefix("+ "))
                    .unwrap_or_else(|| Self::strip_block_prefix(content));
                let new_text = format!("{num}. {inner}");
                let new_chars = new_text.chars().count();
                steps.push(EditStep::replace(adjusted_offset, content.to_string(), new_text));
                offset_delta += new_chars as isize - content_chars as isize;
            }
        }

        if !steps.is_empty() {
            editor.apply_transaction(Transaction::new(steps));
        }
    }

    /// Insert a link at the cursor: `[text](url)`
    /// If there's a selection, it becomes the link text.
    pub fn insert_link(editor: &mut Editor, url: &str) {
        let selected = editor.selected_text();
        if selected.is_empty() {
            editor.insert(&format!("[]({})", url));
            // Move cursor between [] for typing link text
            let cursor = editor.cursor();
            let url_chars = url.chars().count();
            let new_col = cursor.col - url_chars - 3; // back to after [
            editor.set_cursor(Position::new(cursor.line, new_col));
        } else {
            editor.insert(&format!("[{}]({})", selected, url));
        }
    }

    /// Insert a fenced code block at the cursor.
    pub fn insert_code_block(editor: &mut Editor, language: &str) {
        let has_selection = !editor.selection().is_cursor();
        let selected = editor.selected_text();

        if has_selection {
            editor.insert(&format!("```{language}\n{selected}\n```"));
        } else {
            editor.insert(&format!("```{language}\n\n```"));
            // Move cursor to the empty line inside the code block
            let cursor = editor.cursor();
            if cursor.line > 0 {
                editor.set_cursor(Position::new(cursor.line - 1, 0));
            }
        }
    }

    /// Insert a paragraph break (Enter key), properly closing and reopening
    /// any active inline markers (`**`, `*`, `` ` ``, `~~`) so formatting
    /// is not broken across the line boundary.
    ///
    /// `newline` controls what is inserted: `"\n\n"` for a normal paragraph
    /// break, `"\n"` for a soft break (Shift+Enter).
    pub fn insert_paragraph_break(editor: &mut Editor, newline: &str) {
        let cursor = editor.cursor();
        let line_text = editor.buffer().line(cursor.line).to_string();
        // Text from start of line up to cursor (in chars)
        let before_cursor: String = line_text.chars().take(cursor.col).collect();

        let active = Self::active_inline_markers(&before_cursor);
        if active.is_empty() {
            editor.insert(newline);
        } else {
            // Build: close markers (reverse order) + newline + reopen markers (original order)
            let mut buf = String::new();
            for m in active.iter().rev() {
                buf.push_str(m);
            }
            buf.push_str(newline);
            for m in &active {
                buf.push_str(m);
            }
            editor.insert(&buf);
        }
    }

    /// Insert a horizontal rule.
    pub fn insert_horizontal_rule(editor: &mut Editor) {
        let cursor = editor.cursor();
        let at_line_start = cursor.col == 0;
        let prefix = if at_line_start { "" } else { "\n" };
        editor.insert(&format!("{prefix}---\n"));
    }

    // ── Formatting state query ────────────────────────────────────────

    /// Determine which formatting is active at the current cursor position.
    ///
    /// Inline formatting (bold, italic, code, strikethrough) is detected by
    /// scanning text before the cursor for open markers via `active_inline_markers`.
    /// Block formatting (headings, lists, blockquotes) is detected by checking
    /// the line prefix.
    pub fn formatting_at_cursor(editor: &Editor) -> FormattingState {
        let cursor = editor.cursor();
        let line_text = editor.buffer().line(cursor.line).to_string();
        let before_cursor: String = line_text.chars().take(cursor.col).collect();

        // Inline formatting: reuse active_inline_markers
        let active = Self::active_inline_markers(&before_cursor);

        // Block formatting: check line prefix
        let trimmed = line_text.trim_start();
        let heading_level = if trimmed.starts_with("### ") || trimmed == "###" {
            3
        } else if trimmed.starts_with("## ") || trimmed == "##" {
            2
        } else if trimmed.starts_with("# ") || trimmed == "#" {
            1
        } else {
            0
        };

        let ordered_list = {
            let digit_count = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
            if digit_count > 0 {
                let rest = &trimmed[digit_count..];
                rest.starts_with(". ") || rest.starts_with(") ")
            } else {
                false
            }
        };

        FormattingState {
            bold: active.contains(&"**"),
            italic: active.contains(&"*"),
            code: active.contains(&"`"),
            strikethrough: active.contains(&"~~"),
            heading_level,
            bullet_list: trimmed.starts_with("- ")
                || trimmed.starts_with("* ")
                || trimmed.starts_with("+ "),
            ordered_list,
            blockquote: trimmed.starts_with("> ") || trimmed == ">",
        }
    }

    // ── Private helpers ──────────────────────────────────────────────────

    /// Scan text from line start to cursor and return a list of inline markers
    /// that are currently "open" (i.e. have an odd number of occurrences).
    ///
    /// Returns markers in the order they were opened, which matters for
    /// correct nesting (e.g. `**` before `*` in `***bold-italic***`).
    ///
    /// Handles the tricky `*` vs `**` disambiguation: `**` is consumed first
    /// (greedy), then remaining lone `*` is italic.
    pub fn active_inline_markers(text: &str) -> Vec<&'static str> {
        // We track open/close counts for each marker type.
        // Order matters: check longer markers first to avoid false matches.
        let mut bold_count = 0usize;
        let mut italic_count = 0usize;
        let mut code_count = 0usize;
        let mut strike_count = 0usize;

        // Track the order markers were opened so we can close/reopen in correct order.
        // Each entry is the marker string; we push when toggling open, remove when toggling closed.
        let mut open_stack: Vec<&'static str> = Vec::new();

        let chars: Vec<char> = text.chars().collect();
        let len = chars.len();
        let mut i = 0;

        // Inside inline code, other markers are literal text — skip until closing backtick.
        while i < len {
            if chars[i] == '`' {
                code_count += 1;
                if code_count % 2 == 1 {
                    open_stack.push("`");
                } else {
                    Self::remove_last_marker(&mut open_stack, "`");
                }
                i += 1;
                // If we just opened an inline code span, skip until the closing backtick
                if code_count % 2 == 1 {
                    while i < len && chars[i] != '`' {
                        i += 1;
                    }
                    // Don't consume the closing backtick here — let the outer loop handle it
                }
                continue;
            }

            // Only process other markers when NOT inside inline code
            if code_count.is_multiple_of(2) {
                if chars[i] == '~' && i + 1 < len && chars[i + 1] == '~' {
                    strike_count += 1;
                    if strike_count % 2 == 1 {
                        open_stack.push("~~");
                    } else {
                        Self::remove_last_marker(&mut open_stack, "~~");
                    }
                    i += 2;
                    continue;
                }

                if chars[i] == '*' && i + 1 < len && chars[i + 1] == '*' {
                    // Check for *** (bold+italic simultaneously)
                    if i + 2 < len && chars[i + 2] == '*' {
                        bold_count += 1;
                        italic_count += 1;
                        if bold_count % 2 == 1 {
                            open_stack.push("**");
                        } else {
                            Self::remove_last_marker(&mut open_stack, "**");
                        }
                        if italic_count % 2 == 1 {
                            open_stack.push("*");
                        } else {
                            Self::remove_last_marker(&mut open_stack, "*");
                        }
                        i += 3;
                        continue;
                    }
                    bold_count += 1;
                    if bold_count % 2 == 1 {
                        open_stack.push("**");
                    } else {
                        Self::remove_last_marker(&mut open_stack, "**");
                    }
                    i += 2;
                    continue;
                }

                if chars[i] == '*' {
                    italic_count += 1;
                    if italic_count % 2 == 1 {
                        open_stack.push("*");
                    } else {
                        Self::remove_last_marker(&mut open_stack, "*");
                    }
                    i += 1;
                    continue;
                }
            }

            i += 1;
        }

        // Return only markers that are still open (odd count)
        open_stack
    }

    /// Remove the last occurrence of `marker` from the stack (used when a marker closes).
    fn remove_last_marker(stack: &mut Vec<&'static str>, marker: &str) {
        if let Some(pos) = stack.iter().rposition(|m| *m == marker) {
            stack.remove(pos);
        }
    }

    fn toggle_inline_mark(editor: &mut Editor, mark: &str) {
        let sel = editor.selection();
        let mark_chars = mark.chars().count();

        if sel.is_cursor() {
            // No selection: insert paired marks and place cursor between them
            editor.insert(&format!("{mark}{mark}"));
            let cursor = editor.cursor();
            let new_col = cursor.col - mark_chars;
            editor.set_cursor(Position::new(cursor.line, new_col));
            return;
        }

        let selected = editor.selected_text();
        let start = sel.start();
        let end = sel.end();

        // Check if the selection is already wrapped in this mark.
        // Read from the editor buffer directly (not the tree, which may be stale).
        let start_char = editor.buffer().pos_to_char(start);
        let end_char = editor.buffer().pos_to_char(end);
        let total_chars = editor.buffer().len_chars();

        // Check if mark characters exist immediately before/after selection (O(log n) via rope)
        let has_mark_before = start_char >= mark_chars && {
            let before: String = editor.buffer().rope()
                .slice((start_char - mark_chars)..start_char)
                .to_string();
            before == mark
        };
        let has_mark_after = end_char + mark_chars <= total_chars && {
            let after: String = editor.buffer().rope()
                .slice(end_char..(end_char + mark_chars))
                .to_string();
            after == mark
        };

        if has_mark_before && has_mark_after {
            // Remove marks: select mark+content+mark and replace with just content
            let outer_start = Position::new(start.line, start.col - mark_chars);
            let outer_end = Position::new(end.line, end.col + mark_chars);
            editor.set_selection(outer_start, outer_end);
            editor.insert(&selected);
        } else {
            // Add marks around selection
            editor.insert(&format!("{mark}{selected}{mark}"));
        }
    }

    fn toggle_list_prefix(editor: &mut Editor, prefix: &str) {
        let sel = editor.selection();
        let start_line = sel.start().line;
        let end_line = sel.end().line;

        let all_prefixed = (start_line..=end_line).all(|line| {
            let text = editor.buffer().line(line).to_string();
            text.starts_with(prefix)
        });

        let prefix_chars = prefix.chars().count();
        let mut steps = Vec::new();
        let mut offset_delta: isize = 0;
        for line in start_line..=end_line {
            let line_text = editor.buffer().line(line).to_string();
            let line_start_char = editor.buffer().line_to_char(line);
            let adjusted_offset = (line_start_char as isize + offset_delta) as usize;
            let content = line_text.trim_end_matches('\n');
            let content_chars = content.chars().count();

            if all_prefixed {
                let stripped: String = content.chars().skip(prefix_chars).collect();
                let new_chars = stripped.chars().count();
                steps.push(EditStep::replace(adjusted_offset, content.to_string(), stripped));
                offset_delta += new_chars as isize - content_chars as isize;
            } else {
                if content.starts_with(prefix) {
                    continue; // skip — already prefixed
                }
                // Strip heading prefix (# ## ###) or blockquote (>) before adding list prefix
                let stripped = Self::strip_block_prefix(content);
                let new_text = format!("{prefix}{stripped}");
                let new_chars = new_text.chars().count();
                steps.push(EditStep::replace(adjusted_offset, content.to_string(), new_text));
                offset_delta += new_chars as isize - content_chars as isize;
            }
        }

        if !steps.is_empty() {
            editor.apply_transaction(Transaction::new(steps));
        }
    }

    /// Strip block-level markdown prefixes (headings, blockquotes) from a line.
    /// Used when converting a heading/blockquote to a list item.
    fn strip_block_prefix(s: &str) -> &str {
        let trimmed = s.trim_start();
        // Strip heading prefixes: # ## ### etc.
        if trimmed.starts_with('#') {
            let after_hashes = trimmed.trim_start_matches('#');
            let stripped = after_hashes.strip_prefix(' ').unwrap_or(after_hashes);
            return stripped;
        }
        // Strip blockquote prefix: >
        if let Some(after) = trimmed.strip_prefix('>') {
            return after.strip_prefix(' ').unwrap_or(after);
        }
        s
    }

    /// Strip an ordered list prefix (e.g., "1. ", "10) ") from the start of a string.
    /// Returns the content after the prefix, or None if no ordered prefix found.
    fn strip_ordered_prefix(s: &str) -> Option<&str> {
        let digit_count = s.chars().take_while(|c| c.is_ascii_digit()).count();
        if digit_count == 0 {
            return None;
        }
        let rest = &s[digit_count..]; // safe: digits are ASCII
        if let Some(after) = rest.strip_prefix(". ") {
            Some(after)
        } else if let Some(after) = rest.strip_prefix(") ") {
            Some(after)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kode_core::Position;

    #[test]
    fn toggle_bold_no_selection() {
        let mut ed = Editor::new("hello");
        ed.set_cursor(Position::new(0, 5));
        MarkdownCommands::toggle_bold(&mut ed);
        assert_eq!(ed.text(), "hello****");
        assert_eq!(ed.cursor(), Position::new(0, 7)); // between the **
    }

    #[test]
    fn toggle_bold_with_selection() {
        let mut ed = Editor::new("hello world");
        ed.set_selection(Position::new(0, 6), Position::new(0, 11));
        MarkdownCommands::toggle_bold(&mut ed);
        assert_eq!(ed.text(), "hello **world**");
    }

    #[test]
    fn toggle_bold_remove() {
        let mut ed = Editor::new("hello **world**");
        // Select "world" (between the **)
        ed.set_selection(Position::new(0, 8), Position::new(0, 13));
        MarkdownCommands::toggle_bold(&mut ed);
        assert_eq!(ed.text(), "hello world");
    }

    #[test]
    fn set_heading_level() {
        let mut ed = Editor::new("Hello world");
        ed.set_cursor(Position::new(0, 0));
        MarkdownCommands::set_heading(&mut ed, 2);
        assert_eq!(ed.text(), "## Hello world");

        // Change level
        MarkdownCommands::set_heading(&mut ed, 1);
        assert_eq!(ed.text(), "# Hello world");

        // Remove heading
        MarkdownCommands::set_heading(&mut ed, 0);
        assert_eq!(ed.text(), "Hello world");
    }

    #[test]
    fn set_heading_non_ascii() {
        let mut ed = Editor::new("日本語");
        ed.set_cursor(Position::new(0, 0));
        MarkdownCommands::set_heading(&mut ed, 2);
        assert_eq!(ed.text(), "## 日本語");
        // Cursor was at col 0 (start of content). After adding "## " prefix (3 chars),
        // cursor preserves its position relative to the content → col 3.
        assert_eq!(ed.cursor().col, 3);
    }

    #[test]
    fn set_heading_empty_line() {
        let mut ed = Editor::new("");
        ed.set_cursor(Position::new(0, 0));
        MarkdownCommands::set_heading(&mut ed, 1);
        assert_eq!(ed.text(), "# ");
        MarkdownCommands::set_heading(&mut ed, 0);
        assert_eq!(ed.text(), "");
    }

    #[test]
    fn toggle_blockquote() {
        let mut ed = Editor::new("Hello\nWorld");
        ed.set_selection(Position::new(0, 0), Position::new(1, 5));
        MarkdownCommands::toggle_blockquote(&mut ed);
        assert_eq!(ed.text(), "> Hello\n> World");

        // Toggle off — undo the atomic transaction
        ed.undo();
        assert_eq!(ed.text(), "Hello\nWorld");
    }

    #[test]
    fn toggle_blockquote_atomic_undo() {
        let mut ed = Editor::new("Line 1\nLine 2\nLine 3");
        ed.set_selection(Position::new(0, 0), Position::new(2, 6));
        MarkdownCommands::toggle_blockquote(&mut ed);
        assert_eq!(ed.text(), "> Line 1\n> Line 2\n> Line 3");

        // Single undo should revert all 3 lines
        ed.undo();
        assert_eq!(ed.text(), "Line 1\nLine 2\nLine 3");
    }

    #[test]
    fn toggle_bullet_list() {
        let mut ed = Editor::new("Item 1\nItem 2");
        ed.set_selection(Position::new(0, 0), Position::new(1, 6));
        MarkdownCommands::toggle_bullet_list(&mut ed);
        assert_eq!(ed.text(), "- Item 1\n- Item 2");

        // Undo should revert atomically
        ed.undo();
        assert_eq!(ed.text(), "Item 1\nItem 2");
    }

    #[test]
    fn toggle_bullet_list_mixed_lines() {
        let mut ed = Editor::new("- already\nplain");
        ed.set_selection(Position::new(0, 0), Position::new(1, 5));
        MarkdownCommands::toggle_bullet_list(&mut ed);
        // Should add prefix only to plain line, not double-prefix the listed line
        assert_eq!(ed.text(), "- already\n- plain");
    }

    #[test]
    fn toggle_ordered_list() {
        let mut ed = Editor::new("First\nSecond");
        ed.set_selection(Position::new(0, 0), Position::new(1, 6));
        MarkdownCommands::toggle_ordered_list(&mut ed);
        assert_eq!(ed.text(), "1. First\n2. Second");
    }

    #[test]
    fn toggle_ordered_list_remove_with_dots_in_content() {
        let mut ed = Editor::new("1. Dr. Smith\n2. Mr. Jones");
        ed.set_selection(Position::new(0, 0), Position::new(1, 13));
        MarkdownCommands::toggle_ordered_list(&mut ed);
        // Should only strip the "1. " / "2. " prefix, preserving "Dr. Smith"
        assert_eq!(ed.text(), "Dr. Smith\nMr. Jones");
    }

    #[test]
    fn toggle_ordered_list_preserves_indent() {
        let mut ed = Editor::new("  1. nested");
        ed.set_selection(Position::new(0, 0), Position::new(0, 11));
        MarkdownCommands::toggle_ordered_list(&mut ed);
        assert_eq!(ed.text(), "  nested");
    }

    #[test]
    fn insert_link_with_selection() {
        let mut ed = Editor::new("click here for more");
        ed.set_selection(Position::new(0, 6), Position::new(0, 10));
        MarkdownCommands::insert_link(&mut ed, "https://example.com");
        assert_eq!(ed.text(), "click [here](https://example.com) for more");
    }

    #[test]
    fn insert_link_non_ascii_url() {
        let mut ed = Editor::new("click ");
        ed.set_cursor(Position::new(0, 6));
        MarkdownCommands::insert_link(&mut ed, "https://日本.jp");
        assert_eq!(ed.text(), "click [](https://日本.jp)");
        // Cursor should be at col 7 (after "[")
        assert_eq!(ed.cursor(), Position::new(0, 7));
    }

    #[test]
    fn insert_code_block() {
        let mut ed = Editor::new("Some text\n");
        ed.set_cursor(Position::new(1, 0));
        MarkdownCommands::insert_code_block(&mut ed, "sql");
        assert_eq!(ed.text(), "Some text\n```sql\n\n```");
        assert_eq!(ed.cursor(), Position::new(2, 0));
    }

    #[test]
    fn insert_code_block_at_start_of_empty_doc() {
        let mut ed = Editor::new("");
        ed.set_cursor(Position::new(0, 0));
        MarkdownCommands::insert_code_block(&mut ed, "rust");
        assert_eq!(ed.text(), "```rust\n\n```");
        assert_eq!(ed.cursor(), Position::new(1, 0));
    }

    #[test]
    fn insert_code_block_with_selection() {
        let mut ed = Editor::new("SELECT 1");
        ed.select_all();
        MarkdownCommands::insert_code_block(&mut ed, "sql");
        assert_eq!(ed.text(), "```sql\nSELECT 1\n```");
    }

    #[test]
    fn toggle_italic() {
        let mut ed = Editor::new("hello world");
        ed.set_selection(Position::new(0, 6), Position::new(0, 11));
        MarkdownCommands::toggle_italic(&mut ed);
        assert_eq!(ed.text(), "hello *world*");
    }

    #[test]
    fn toggle_inline_code() {
        let mut ed = Editor::new("use this function");
        ed.set_selection(Position::new(0, 9), Position::new(0, 17));
        MarkdownCommands::toggle_inline_code(&mut ed);
        assert_eq!(ed.text(), "use this `function`");
    }

    #[test]
    fn toggle_bold_undo_restores_original() {
        let mut ed = Editor::new("hello world");
        ed.set_selection(Position::new(0, 6), Position::new(0, 11));
        MarkdownCommands::toggle_bold(&mut ed);
        assert_eq!(ed.text(), "hello **world**");

        ed.undo();
        assert_eq!(ed.text(), "hello world");
    }

    // ── insert_paragraph_break tests ──────────────────────────────────

    #[test]
    fn paragraph_break_inside_bold() {
        let mut ed = Editor::new("**bold text**");
        // Cursor at col 7: ** b o l d   (space) → 7 chars before cursor
        // before_cursor = "**bold " → ** is open
        // insert: close "**" + "\n\n" + reopen "**"
        // result: "**bold **\n\n**text**"
        ed.set_cursor(Position::new(0, 7));
        MarkdownCommands::insert_paragraph_break(&mut ed, "\n\n");
        assert_eq!(ed.text(), "**bold **\n\n**text**");
    }

    #[test]
    fn paragraph_break_inside_italic() {
        let mut ed = Editor::new("*italic text*");
        // Cursor at col 8: * i t a l i c   (space) → 8 chars
        ed.set_cursor(Position::new(0, 8));
        MarkdownCommands::insert_paragraph_break(&mut ed, "\n\n");
        assert_eq!(ed.text(), "*italic *\n\n*text*");
    }

    #[test]
    fn paragraph_break_inside_inline_code() {
        let mut ed = Editor::new("`some code`");
        // Cursor at col 5: ` s o m e → 5 chars
        ed.set_cursor(Position::new(0, 5));
        MarkdownCommands::insert_paragraph_break(&mut ed, "\n\n");
        assert_eq!(ed.text(), "`some`\n\n` code`");
    }

    #[test]
    fn paragraph_break_inside_strikethrough() {
        let mut ed = Editor::new("~~struck text~~");
        // Cursor at col 8: ~ ~ s t r u c k → 8 chars
        ed.set_cursor(Position::new(0, 8));
        MarkdownCommands::insert_paragraph_break(&mut ed, "\n\n");
        assert_eq!(ed.text(), "~~struck~~\n\n~~ text~~");
    }

    #[test]
    fn paragraph_break_no_markers() {
        let mut ed = Editor::new("plain text");
        ed.set_cursor(Position::new(0, 5));
        MarkdownCommands::insert_paragraph_break(&mut ed, "\n\n");
        assert_eq!(ed.text(), "plain\n\n text");
    }

    #[test]
    fn paragraph_break_closed_markers_not_reopened() {
        // If bold is already closed before cursor, don't reopen
        let mut ed = Editor::new("**bold** and more");
        // Cursor at col 13: after "and "
        ed.set_cursor(Position::new(0, 13));
        MarkdownCommands::insert_paragraph_break(&mut ed, "\n\n");
        assert_eq!(ed.text(), "**bold** and \n\nmore");
    }

    #[test]
    fn paragraph_break_nested_bold_italic() {
        let mut ed = Editor::new("***bold italic text***");
        // Cursor at col 15: "***bold italic " (15 chars)
        // Active markers: ["**", "*"] (bold first, then italic)
        // Close in reverse: "*" then "**" = "***"
        // Reopen: "**" then "*" = "***"
        ed.set_cursor(Position::new(0, 15));
        MarkdownCommands::insert_paragraph_break(&mut ed, "\n\n");
        assert_eq!(ed.text(), "***bold italic ***\n\n***text***");
    }

    #[test]
    fn paragraph_break_soft_break_inside_bold() {
        let mut ed = Editor::new("**bold text**");
        ed.set_cursor(Position::new(0, 7));
        MarkdownCommands::insert_paragraph_break(&mut ed, "\n");
        assert_eq!(ed.text(), "**bold **\n**text**");
    }

    #[test]
    fn paragraph_break_markers_inside_code_ignored() {
        // ** inside backticks should not count as bold
        let mut ed = Editor::new("`code **not bold**`");
        ed.set_cursor(Position::new(0, 10));
        MarkdownCommands::insert_paragraph_break(&mut ed, "\n\n");
        // Only ` is active (code span is open at cursor... wait, let me check)
        // ` c o d e   * * n o t ...
        // 0 1 2 3 4 5 6 7 8 9 10
        // At col 10: before = "`code **no"
        // ` opens (code_count=1, odd → open)
        // Inside code span, skip until closing `. There's no closing ` before col 10.
        // So only ` is active.
        assert_eq!(ed.text(), "`code **no`\n\n`t bold**`");
    }

    // ── active_inline_markers unit tests ────────────────────────────────

    #[test]
    fn active_markers_empty() {
        assert!(MarkdownCommands::active_inline_markers("plain text").is_empty());
    }

    #[test]
    fn active_markers_bold_open() {
        assert_eq!(MarkdownCommands::active_inline_markers("**bold "), vec!["**"]);
    }

    #[test]
    fn active_markers_bold_closed() {
        assert!(MarkdownCommands::active_inline_markers("**bold** after").is_empty());
    }

    #[test]
    fn active_markers_italic_open() {
        assert_eq!(MarkdownCommands::active_inline_markers("*italic "), vec!["*"]);
    }

    #[test]
    fn active_markers_code_open() {
        assert_eq!(MarkdownCommands::active_inline_markers("`code "), vec!["`"]);
    }

    #[test]
    fn active_markers_strike_open() {
        assert_eq!(MarkdownCommands::active_inline_markers("~~strike "), vec!["~~"]);
    }

    #[test]
    fn active_markers_bold_italic_open() {
        let markers = MarkdownCommands::active_inline_markers("***bold italic ");
        assert_eq!(markers, vec!["**", "*"]);
    }

    #[test]
    fn active_markers_code_hides_bold() {
        // ** inside code span should not register as bold
        assert_eq!(MarkdownCommands::active_inline_markers("`code **bold "), vec!["`"]);
    }

    #[test]
    fn toggle_bold_with_unicode() {
        let mut ed = Editor::new("hello café world");
        // Select "café"
        ed.set_selection(Position::new(0, 6), Position::new(0, 10));
        MarkdownCommands::toggle_bold(&mut ed);
        assert_eq!(ed.text(), "hello **café** world");
    }

    // ── Bug #93: Triple *** markers ────────────────────────────────────

    #[test]
    fn active_markers_triple_star_open() {
        // *** opens both bold and italic; active_inline_markers should return both
        let markers = MarkdownCommands::active_inline_markers("Text ***bold italic");
        assert_eq!(markers, vec!["**", "*"]);
    }

    #[test]
    fn paragraph_break_inside_triple_star_bold_italic() {
        // Input: "Text ***bold italic|stuff*** end" — cursor at col 19
        // Expected: close *** then newline then reopen ***
        let mut ed = Editor::new("Text ***bold italicstuff*** end");
        // "Text ***bold italic" = 19 chars
        ed.set_cursor(Position::new(0, 19));
        MarkdownCommands::insert_paragraph_break(&mut ed, "\n\n");
        assert_eq!(ed.text(), "Text ***bold italic***\n\n***stuff*** end");
    }

    // ── Bug #96: Enter at bold-italic boundary ────────────────────────

    #[test]
    fn active_markers_bold_near_triple_star_boundary() {
        // "**bold te" — bold is open (one ** seen, not closed)
        let markers = MarkdownCommands::active_inline_markers("**bold te");
        assert_eq!(markers, vec!["**"]);
    }

    #[test]
    fn paragraph_break_at_bold_italic_boundary() {
        // Input: "**bold te|xt***italic text*"  cursor at col 9
        // before_cursor = "**bold te" → active = ["**"]
        // Expected: close ** + newline + reopen **
        let mut ed = Editor::new("**bold text***italic text*");
        ed.set_cursor(Position::new(0, 9));
        MarkdownCommands::insert_paragraph_break(&mut ed, "\n\n");
        assert_eq!(ed.text(), "**bold te**\n\n**xt***italic text*");
    }

    // ── Bug #112: Adjacent bold spans ─────────────────────────────────

    #[test]
    fn active_markers_adjacent_bold_spans_between() {
        // "**first** " — bold opened then closed = even count, not active
        let markers = MarkdownCommands::active_inline_markers("**first** ");
        assert!(markers.is_empty());
    }

    #[test]
    fn paragraph_break_between_adjacent_bold_spans() {
        // Enter between two bold spans at the space: no active markers
        let mut ed = Editor::new("**first** **second**");
        // Cursor at col 10 (the space between "** " and "**second**")
        ed.set_cursor(Position::new(0, 10));
        MarkdownCommands::insert_paragraph_break(&mut ed, "\n\n");
        assert_eq!(ed.text(), "**first** \n\n**second**");
    }

    // ── Bug #119: Single-char bold split ──────────────────────────────

    #[test]
    fn paragraph_break_single_char_bold_cursor_after_open() {
        // Cursor right after opening ** in "**X**" (col 2)
        // before_cursor = "**" → active = ["**"]
        // insert: "**\n\n**" → result: "****\n\n**X**"
        // This is a degenerate case: empty bold on first line.
        let mut ed = Editor::new("**X**");
        ed.set_cursor(Position::new(0, 2));
        MarkdownCommands::insert_paragraph_break(&mut ed, "\n\n");
        assert_eq!(ed.text(), "****\n\n**X**");
    }

    #[test]
    fn paragraph_break_single_char_bold_cursor_inside() {
        // Cursor between * and X and closing ** in "Before **X** after"
        // Cursor at col 9 = after "Before **X" (inside the bold content, after X)
        // before_cursor = "Before **X" → ** open, active = ["**"]
        // insert: "**\n\n**" → "Before **X**\n\n** after"
        let mut ed = Editor::new("Before **X** after");
        ed.set_cursor(Position::new(0, 10));
        MarkdownCommands::insert_paragraph_break(&mut ed, "\n\n");
        assert_eq!(ed.text(), "Before **X**\n\n**** after");
    }

    #[test]
    fn active_markers_single_char_bold_cursor_outside() {
        // "Before **X** aft" — bold opened then closed, not active
        let markers = MarkdownCommands::active_inline_markers("Before **X** aft");
        assert!(markers.is_empty());
    }

    // ── Bug #113: Shift+Enter should check heading context ────────────
    // (This bug is in component.rs, tested via integration. Unit test
    //  verifies insert_paragraph_break with "\n" still works for non-heading lines.)

    #[test]
    fn paragraph_break_soft_break_no_markers() {
        let mut ed = Editor::new("plain text here");
        ed.set_cursor(Position::new(0, 6));
        MarkdownCommands::insert_paragraph_break(&mut ed, "\n");
        assert_eq!(ed.text(), "plain \ntext here");
    }
}
