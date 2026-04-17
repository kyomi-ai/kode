use kode_core::Editor;
use web_sys::KeyboardEvent;

/// Page size for PageUp/PageDown (lines). The view layer should set this
/// based on actual viewport height, but 30 is a reasonable default.
const PAGE_LINES: usize = 30;

/// Returns true if the key event was handled (should prevent default).
pub fn handle_keydown(editor: &mut Editor, ev: &KeyboardEvent) -> bool {
    let key = ev.key();
    let ctrl = ev.ctrl_key() || ev.meta_key();
    let shift = ev.shift_key();

    // Order matters: most-specific guards (ctrl+shift) before less-specific (shift, ctrl, plain).
    match key.as_str() {
        // ── Ctrl+Shift combos (must come first) ──────────────────────
        "ArrowLeft" if ctrl && shift => {
            editor.extend_selection_word_left();
            true
        }
        "ArrowRight" if ctrl && shift => {
            editor.extend_selection_word_right();
            true
        }
        "Home" if ctrl && shift => {
            editor.extend_selection_to_start();
            true
        }
        "End" if ctrl && shift => {
            editor.extend_selection_to_end();
            true
        }

        // ── Shift combos ─────────────────────────────────────────────
        "ArrowLeft" if shift => {
            editor.extend_selection_left();
            true
        }
        "ArrowRight" if shift => {
            editor.extend_selection_right();
            true
        }
        "ArrowUp" if shift => {
            editor.extend_selection_up();
            true
        }
        "ArrowDown" if shift => {
            editor.extend_selection_down();
            true
        }
        "Home" if shift => {
            editor.extend_selection_to_line_start();
            true
        }
        "End" if shift => {
            editor.extend_selection_to_line_end();
            true
        }
        "PageUp" if shift => {
            // Extend selection by page (not common but consistent)
            let head = editor.selection().head;
            let new_line = head.line.saturating_sub(PAGE_LINES);
            let max_col = editor.buffer().line_len(new_line);
            editor.extend_selection(kode_core::Position::new(new_line, head.col.min(max_col)));
            true
        }
        "PageDown" if shift => {
            let head = editor.selection().head;
            let last = editor.buffer().len_lines().saturating_sub(1);
            let new_line = (head.line + PAGE_LINES).min(last);
            let max_col = editor.buffer().line_len(new_line);
            editor.extend_selection(kode_core::Position::new(new_line, head.col.min(max_col)));
            true
        }

        // ── Ctrl combos ──────────────────────────────────────────────
        "ArrowLeft" if ctrl => {
            editor.move_word_left();
            true
        }
        "ArrowRight" if ctrl => {
            editor.move_word_right();
            true
        }
        "Home" if ctrl => {
            editor.move_to_start();
            true
        }
        "End" if ctrl => {
            editor.move_to_end();
            true
        }
        "Backspace" if ctrl => {
            editor.delete_word_back();
            true
        }
        "Delete" if ctrl => {
            editor.delete_word_forward();
            true
        }
        "a" if ctrl => {
            editor.select_all();
            true
        }
        "z" if ctrl && shift => {
            editor.redo();
            true
        }
        "z" if ctrl => {
            editor.undo();
            true
        }
        "y" if ctrl => {
            editor.redo();
            true
        }
        "d" if ctrl && shift => {
            editor.duplicate_lines();
            true
        }

        // Let clipboard shortcuts pass through to the textarea
        "c" | "x" | "v" if ctrl => false,

        // Let browser handle find/replace until we have our own
        "f" | "h" if ctrl => false,

        // ── Plain keys ───────────────────────────────────────────────
        "ArrowLeft" => {
            editor.move_left();
            true
        }
        "ArrowRight" => {
            editor.move_right();
            true
        }
        "ArrowUp" => {
            editor.move_up();
            true
        }
        "ArrowDown" => {
            editor.move_down();
            true
        }
        "Home" => {
            editor.move_to_line_start();
            true
        }
        "End" => {
            editor.move_to_line_end();
            true
        }
        "PageUp" => {
            editor.page_up(PAGE_LINES);
            true
        }
        "PageDown" => {
            editor.page_down(PAGE_LINES);
            true
        }
        "Backspace" => {
            editor.backspace();
            true
        }
        "Delete" => {
            editor.delete_forward();
            true
        }
        "Enter" => {
            editor.insert_newline();
            true
        }
        "Tab" if shift => {
            editor.outdent();
            true
        }
        "Tab" => {
            editor.indent();
            true
        }

        // Everything else: let it flow through to the textarea for text input
        _ => false,
    }
}
