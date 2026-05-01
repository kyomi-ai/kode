//! Extension system for the kode WYSIWYG editor.
//!
//! Extensions can customize code block rendering, add toolbar buttons,
//! register keyboard shortcuts, and respond to editor lifecycle events.

use std::sync::Arc;

use kode_core::Editor;
use kode_markdown::{FormattingState, MarkdownEditor};
use leptos::tachys::view::any_view::AnyView;

/// Context passed to extension lifecycle hooks — a read-only view of editor state.
///
pub struct ExtensionEditorContext<'a> {
    /// The underlying kode-core `Editor` (cursor, selection, buffer).
    pub editor: &'a Editor,
    /// The full markdown source text.
    pub source: &'a str,
    /// Current cursor byte offset in the source.
    pub cursor_byte: usize,
    /// Current formatting state (inline + block).
    pub formatting: &'a FormattingState,
}

/// A toolbar button contributed by an extension.
pub struct ExtensionToolbarItem {
    /// Button content — text, icon, or any Leptos view.
    pub label: AnyView,
    /// Tooltip/title text.
    pub title: String,
    /// Toolbar group index (for visual grouping with separators).
    pub group: u8,
    /// Action to execute when clicked. Receives mutable editor access.
    pub action: Arc<dyn Fn(&mut MarkdownEditor) + Send + Sync>,
    /// Name used for active state matching (matched against `active_state` output).
    pub active_name: Option<String>,
}

/// A keyboard shortcut contributed by an extension.
pub struct ExtensionKeyboardShortcut {
    /// Key descriptor (e.g. "Mod-Shift-c", "Ctrl-Enter").
    ///
    /// Format: modifier segments joined by `-`, where modifiers are:
    /// - `Mod` or `Ctrl` — maps to Ctrl (or Cmd on macOS)
    /// - `Shift`
    /// - `Alt`
    ///
    /// The final segment is the key name (matching `KeyboardEvent.key()`).
    pub key: String,
    /// Handler. Returns `true` if the shortcut was consumed.
    pub handler: Arc<dyn Fn(&mut MarkdownEditor) -> bool + Send + Sync>,
}

/// A WYSIWYG editor extension that can customize rendering, toolbar,
/// keyboard shortcuts, and editor behavior.
pub trait Extension: Send + Sync {
    /// Unique name for this extension (e.g. "chartml", "table", "mermaid").
    fn name(&self) -> &str;

    // ── Block rendering ──────────────────────────────────────────

    /// Fenced code block languages this extension handles.
    ///
    /// When a fenced code block's language matches one of these strings,
    /// `render_code_block` is called instead of the default syntax-highlighted
    /// renderer.
    ///
    /// Return an empty slice if this extension doesn't handle code blocks.
    fn code_block_languages(&self) -> &[&str] {
        &[]
    }

    /// Render a custom fenced code block.
    ///
    /// Called when the block's language matches one from `code_block_languages()`.
    ///
    /// - `language`: the fenced block language identifier
    /// - `content`: the raw content inside the fenced block
    /// - `block_start`: position of the block start (token position in tree mode)
    /// - `block_end`: position of the block end (token position in tree mode)
    ///
    /// Positions are opaque integers used for identification, not arithmetic.
    /// In tree mode they are token positions; in legacy mode they are byte offsets.
    ///
    /// Returns a Leptos view to render, or `None` to fall back to the default
    /// syntax-highlighted code block.
    fn render_code_block(
        &self,
        _language: &str,
        _content: &str,
        _block_start: usize,
        _block_end: usize,
    ) -> Option<AnyView> {
        None
    }

    // ── Toolbar ──────────────────────────────────────────────────

    /// Additional toolbar buttons this extension provides.
    ///
    /// These are appended after the built-in toolbar buttons.
    /// Each item defines: label, title, group index, and action callback.
    fn toolbar_items(&self) -> Vec<ExtensionToolbarItem> {
        vec![]
    }

    // ── Keyboard shortcuts ───────────────────────────────────────

    /// Keyboard shortcuts this extension handles.
    ///
    /// Each shortcut has a key descriptor and handler function.
    /// Return `true` from the handler if the shortcut was consumed.
    fn keyboard_shortcuts(&self) -> Vec<ExtensionKeyboardShortcut> {
        vec![]
    }

    // ── Render pass ───────────────────────────────────────────────

    /// Called before each render pass of the document tree.
    ///
    /// Extensions that cache rendered views (e.g. expensive chart rendering)
    /// can use this to reset per-pass counters and prune stale cache entries.
    fn begin_render_pass(&self) {}

    // ── Lifecycle ────────────────────────────────────────────────

    /// Called when the editor is created. Use for one-time setup.
    ///
    fn on_create(&self, _ctx: &ExtensionEditorContext) {}

    /// Called when the editor is destroyed. Use for cleanup.
    ///
    fn on_destroy(&self) {}

    /// Called when the document content changes.
    ///
    fn on_update(&self, _ctx: &ExtensionEditorContext) {}

    /// Called when the cursor/selection changes.
    /// Extensions can use this to update their own reactive state.
    ///
    fn on_selection_update(&self, _ctx: &ExtensionEditorContext) {}

    // ── Formatting state ─────────────────────────────────────────

    /// Extend the formatting state with extension-specific active states.
    ///
    /// Called on every cursor move. Return a list of `(name, is_active)` pairs.
    /// These are used to highlight extension toolbar buttons whose
    /// `active_name` matches.
    fn active_state(&self, _ctx: &ExtensionEditorContext) -> Vec<(&str, bool)> {
        vec![]
    }
}

/// Check whether a `KeyboardEvent` matches an extension key descriptor.
///
/// Descriptor format: `"Mod-Shift-c"`, `"Ctrl-Enter"`, `"Alt-Shift-x"`, etc.
/// - `Mod` and `Ctrl` both map to `ctrl_key() || meta_key()`
/// - `Shift` maps to `shift_key()`
/// - `Alt` maps to `alt_key()`
/// - The remaining (non-modifier) portion is matched case-insensitively against `ev.key()`.
///
/// Parsing consumes known modifier prefixes from left; whatever remains is the
/// key name.  This handles descriptors like `"Ctrl--"` (ctrl + minus) correctly,
/// where a naive split-on-`-` would break.
/// Parse a key descriptor into its modifier flags and key name.
///
/// Returns `(need_ctrl, need_shift, need_alt, key_name)`.
/// Returns `None` if the descriptor is malformed (empty key name).
fn parse_key_descriptor(descriptor: &str) -> Option<(bool, bool, bool, &str)> {
    let mut need_ctrl = false;
    let mut need_shift = false;
    let mut need_alt = false;
    let mut remaining = descriptor;

    loop {
        if let Some(rest) = remaining.strip_prefix("Mod-") {
            need_ctrl = true;
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("Ctrl-") {
            need_ctrl = true;
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("Meta-") {
            need_ctrl = true;
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("Shift-") {
            need_shift = true;
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("Alt-") {
            need_alt = true;
            remaining = rest;
        } else {
            break;
        }
    }

    if remaining.is_empty() {
        None
    } else {
        Some((need_ctrl, need_shift, need_alt, remaining))
    }
}

pub(crate) fn matches_key_descriptor(ev: &web_sys::KeyboardEvent, descriptor: &str) -> bool {
    let Some((need_ctrl, need_shift, need_alt, key_part)) = parse_key_descriptor(descriptor)
    else {
        return false;
    };

    let has_ctrl = ev.ctrl_key() || ev.meta_key();
    if need_ctrl != has_ctrl {
        return false;
    }
    if need_shift != ev.shift_key() {
        return false;
    }
    if need_alt != ev.alt_key() {
        return false;
    }

    ev.key().eq_ignore_ascii_case(key_part)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_key() {
        let (ctrl, shift, alt, key) = parse_key_descriptor("a").unwrap();
        assert!(!ctrl && !shift && !alt);
        assert_eq!(key, "a");
    }

    #[test]
    fn parse_ctrl_key() {
        let (ctrl, shift, alt, key) = parse_key_descriptor("Ctrl-b").unwrap();
        assert!(ctrl && !shift && !alt);
        assert_eq!(key, "b");
    }

    #[test]
    fn parse_mod_key() {
        let (ctrl, shift, alt, key) = parse_key_descriptor("Mod-b").unwrap();
        assert!(ctrl && !shift && !alt);
        assert_eq!(key, "b");
    }

    #[test]
    fn parse_mod_shift_key() {
        let (ctrl, shift, alt, key) = parse_key_descriptor("Mod-Shift-c").unwrap();
        assert!(ctrl && shift && !alt);
        assert_eq!(key, "c");
    }

    #[test]
    fn parse_ctrl_alt_key() {
        let (ctrl, shift, alt, key) = parse_key_descriptor("Ctrl-Alt-Delete").unwrap();
        assert!(ctrl && !shift && alt);
        assert_eq!(key, "Delete");
    }

    #[test]
    fn parse_ctrl_minus() {
        // "Ctrl--" means Ctrl + the minus key
        let (ctrl, shift, alt, key) = parse_key_descriptor("Ctrl--").unwrap();
        assert!(ctrl && !shift && !alt);
        assert_eq!(key, "-");
    }

    #[test]
    fn parse_just_minus() {
        let (ctrl, shift, alt, key) = parse_key_descriptor("-").unwrap();
        assert!(!ctrl && !shift && !alt);
        assert_eq!(key, "-");
    }

    #[test]
    fn parse_enter() {
        let (ctrl, shift, alt, key) = parse_key_descriptor("Enter").unwrap();
        assert!(!ctrl && !shift && !alt);
        assert_eq!(key, "Enter");
    }

    #[test]
    fn parse_mod_enter() {
        let (ctrl, shift, alt, key) = parse_key_descriptor("Mod-Enter").unwrap();
        assert!(ctrl && !shift && !alt);
        assert_eq!(key, "Enter");
    }

    #[test]
    fn parse_empty_returns_none() {
        assert!(parse_key_descriptor("").is_none());
    }

    #[test]
    fn parse_trailing_dash_returns_none() {
        // "Ctrl-" has no key name after the modifier
        assert!(parse_key_descriptor("Ctrl-").is_none());
    }

    #[test]
    fn parse_all_modifiers() {
        let (ctrl, shift, alt, key) =
            parse_key_descriptor("Ctrl-Shift-Alt-x").unwrap();
        assert!(ctrl && shift && alt);
        assert_eq!(key, "x");
    }

    #[test]
    fn parse_meta_maps_to_ctrl() {
        let (ctrl, _, _, key) = parse_key_descriptor("Meta-b").unwrap();
        assert!(ctrl);
        assert_eq!(key, "b");
    }
}
