//! Tree-based WYSIWYG editor component.
//!
//! This module implements a WYSIWYG editor that uses `kode_doc::DocState` and the
//! tree-based `render_doc()` renderer instead of the flat markdown string approach
//! used by `component.rs`.
//!
//! Key differences from the old `WysiwygEditor`:
//! - Document state is `DocState` (tree-based), not `MarkdownEditor` (flat string)
//! - Rendering uses `render_doc()`, not `render_blocks()`
//! - Positions are tree token positions, not byte offsets
//! - DOM elements use `data-pos-start`/`data-pos-end` attributes for cursor mapping

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use kode_doc::mark::MarkType;

use kode_doc::{DocState, FormattingState, Selection};
use leptos::prelude::*;
use leptos::tachys::view::any_view::AnyView;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{ClipboardEvent, CompositionEvent, FocusEvent, HtmlElement, HtmlTextAreaElement, KeyboardEvent, MouseEvent};

/// Monotonic counter for unique per-instance element IDs.
/// Relaxed ordering: only uniqueness matters, no ordering dependency.
static INSTANCE_COUNTER: AtomicU32 = AtomicU32::new(0);

use std::hash::{Hash, Hasher};

use kode_doc::node::Node;

use crate::extension::{matches_key_descriptor, Extension, ExtensionKeyboardShortcut, ExtensionToolbarItem};
use crate::theme::Theme;
use crate::toolbar::{InjectCommand, ToolbarItem, default_toolbar_items, dispatch_builtin_action};
use crate::wysiwyg::doc_renderer::render_block_node;

// ── Keyed block rendering ───────────────────────────────────────────────────

/// A top-level block extracted from the document tree for keyed rendering.
/// `<For>` uses the `key` field to determine which blocks changed between
/// render passes. Unchanged blocks preserve their DOM and reactive state.
#[derive(Clone)]
struct BlockItem {
    /// Stable key: (ordinal, content_hash). Ordinal disambiguates identical blocks.
    key: (usize, u64),
    /// The block node (cloned from the document tree).
    node: Node,
    /// Absolute token position in the document.
    position: usize,
}

impl PartialEq for BlockItem {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

/// Extract top-level block items from the document for keyed rendering.
fn extract_block_items(doc: &Node) -> Vec<BlockItem> {
    let mut items = Vec::new();
    let mut pos = 0; // content_start = 0 (matches render_doc's convention)
    for (i, child) in doc.content.iter().enumerate() {
        let hash = hash_node_content(child);
        items.push(BlockItem {
            key: (i, hash),
            node: child.clone(),
            position: pos,
        });
        pos += child.node_size();
    }
    items
}

/// Fast content hash for a block node — used as part of the keyed rendering key.
/// Hashes the full serialized markdown (not just text_content) so that mark
/// changes (bold, italic, links) also produce different keys.
fn hash_node_content(node: &Node) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // Serialize to markdown captures everything: text, marks, attributes,
    // block type, nesting. Any change produces a different hash.
    let md = kode_doc::serialize::serialize_markdown(node);
    md.hash(&mut hasher);
    // Also hash node_size so that content-length changes which the markdown
    // serializer normalizes away (e.g. trailing newlines in code blocks)
    // still produce a different key.
    node.node_size().hash(&mut hasher);
    hasher.finish()
}

// Extracted helper modules
use super::click::get_char_offset_from_point;
use super::clipboard::{html_escape, extract_kode_markdown};
use super::cursor::{
    find_element_for_pos, find_last_positioned_element, find_deepest_pos_child,
    measure_char_offset_position, next_text_pos, vertical_cursor_move,
};
use super::dom_helpers::{apply_md_command, find_ancestor_with_attr, find_ancestor_with_pos_attrs, parse_data_attr};
use super::selection::render_selection_highlights;

/// Check whether a markdown string contains block-level structure that requires
/// `insert_from_markdown` to preserve (headings, code fences, blockquotes,
/// horizontal rules). Plain paragraphs and list items are handled better by
/// `insert_text_multiline` which uses `split_block` for accurate list handling.
/// Tree-based WYSIWYG editor component.
///
/// Drop-in replacement for `WysiwygEditor` that uses `DocState` and the tree-based
/// renderer instead of the flat markdown string approach. Same props interface.
#[component]
pub fn TreeWysiwygEditor(
    /// The markdown content signal (input).
    #[prop(into, default = Signal::stored(String::new()))]
    content: Signal<String>,
    /// Callback fired when the document changes.
    #[prop(optional)]
    on_change: Option<Arc<dyn Fn(String) + Send + Sync>>,
    /// Whether to show the formatting toolbar.
    #[prop(default = true)]
    show_toolbar: bool,
    /// Editor theme.
    #[prop(into, default = Signal::stored(Theme::default()))]
    theme: Signal<Theme>,
    /// Editor extensions for custom code block rendering.
    #[prop(default = vec![])]
    extensions: Vec<Arc<dyn Extension>>,
    /// Override the max-width of the editor container (default: "800px").
    #[prop(into, optional)]
    container_max_width: Option<String>,
    /// Custom toolbar layout. When `None`, the default built-in buttons are shown.
    /// When `Some`, the application fully controls which buttons appear, their
    /// order, and any custom content (see `ToolbarItem`).
    #[prop(optional)]
    toolbar_items: Option<Vec<ToolbarItem>>,
    /// Inject content at the current cursor position. Write a command to
    /// insert; the editor executes it and resets the signal to `None`.
    /// Useful for modal-driven insertions (e.g., dashboard links, chart blocks).
    #[prop(optional)]
    inject: Option<RwSignal<Option<InjectCommand>>>,
    /// Map custom code-block language names to built-in highlighter languages.
    /// Each entry is `(custom_name, builtin_name)`, e.g. `("chartml", "yaml")`.
    #[prop(default = vec![])]
    language_aliases: Vec<(String, String)>,
) -> impl IntoView {
    // ── State ────────────────────────────────────────────────────────────
    let doc_state = Arc::new(Mutex::new(DocState::from_markdown(
        &content.get_untracked(),
    )));

    // Reactive version counters — bumped to trigger re-render / cursor reposition.
    let text_version = RwSignal::new(0u64);
    let cursor_version = RwSignal::new(0u64);
    let focused = RwSignal::new(false);
    let composing = RwSignal::new(false);
    let dragging = RwSignal::new(false);
    let formatting_state = RwSignal::new(FormattingState::default());
    let extension_active_state = RwSignal::new(Vec::<(String, bool)>::new());
    // Track the last text_version for which extension state was computed,
    // so we skip the expensive to_markdown() roundtrip on cursor-only changes.
    let last_ext_text_version = std::cell::Cell::new(0u64);

    // Multi-click detection state.
    let last_click_time = RwSignal::new(0.0f64);
    let click_count = RwSignal::new(0u32);

    // Blink timer handles.
    let blink_timer_id = RwSignal::new(0i32);
    let blink_interval_id = RwSignal::new(0i32);

    let textarea_ref = NodeRef::<leptos::html::Textarea>::new();
    let scroll_container_ref = NodeRef::<leptos::html::Div>::new();

    // Unique per-instance element IDs to avoid collisions when multiple editors
    // are mounted on the same page.
    let instance_id = INSTANCE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let cursor_el_id = format!("tree-wysiwyg-cursor-{instance_id}");
    let overlay_el_id = format!("tree-wysiwyg-overlay-{instance_id}");

    // ── Extension data ───────────────────────────────────────────────────
    // Collect keyboard shortcuts and toolbar items from extensions once.
    let extension_shortcuts: Arc<Vec<ExtensionKeyboardShortcut>> = Arc::new(
        extensions.iter().flat_map(|ext| ext.keyboard_shortcuts()).collect(),
    );
    let extension_toolbar_items: Vec<ExtensionToolbarItem> =
        extensions.iter().flat_map(|ext| ext.toolbar_items()).collect();
    let extensions: Arc<Vec<Arc<dyn Extension>>> = Arc::new(extensions);
    let language_aliases: Arc<Vec<(String, String)>> = Arc::new(language_aliases);

    // ── Notification helpers ─────────────────────────────────────────────
    // IMPORTANT: notify_text must NOT lock the doc_state. The signal update
    // can synchronously trigger the render closure which also locks the
    // doc_state, causing a recursive lock panic in WASM's single-threaded mutex.
    let on_change_notify = on_change.clone();
    let notify_text = move |new_text: Option<String>| {
        text_version.update(|v| *v += 1);
        cursor_version.update(|v| *v += 1);
        if let (Some(ref cb), Some(text)) = (&on_change_notify, new_text) {
            cb(text);
        }
    };
    let notify_cursor: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        cursor_version.update(|v| *v += 1);
    });

    // ── inject: insert content at cursor when the signal is written ────
    if let Some(inject_signal) = inject {
        let doc_inject = doc_state.clone();
        let on_change_inject = on_change.clone();
        Effect::new(move || {
            if let Some(cmd) = inject_signal.get() {
                {
                    let mut ds = doc_inject.lock().unwrap();
                    match &cmd {
                        InjectCommand::Text(text) => ds.insert_text(text),
                        InjectCommand::Link { text, url } => ds.insert_link_with_text(text, url),
                    }
                }
                // Read markdown AFTER releasing the lock (see notify_text comment).
                let md = doc_inject.lock().unwrap().to_markdown();
                text_version.update(|v| *v += 1);
                cursor_version.update(|v| *v += 1);
                if let Some(ref cb) = on_change_inject {
                    cb(md);
                }
                inject_signal.set(None);
            }
        });
    }

    // ── Blink management ─────────────────────────────────────────────────
    let reset_blink = {
        let cursor_id: Arc<str> = Arc::from(cursor_el_id.as_str());
        move || {
            let Some(window) = web_sys::window() else {
                return;
            };
            let old_timer = blink_timer_id.get_untracked();
            let old_interval = blink_interval_id.get_untracked();
            if old_timer != 0 {
                window.clear_timeout_with_handle(old_timer);
            }
            if old_interval != 0 {
                window.clear_interval_with_handle(old_interval);
            }
            blink_timer_id.set(0);
            blink_interval_id.set(0);

            // Show cursor immediately on any action.
            if let Some(document) = window.document() {
                if let Some(el) = document.get_element_by_id(&cursor_id) {
                    let _ = el.unchecked_ref::<HtmlElement>().style().set_property("visibility", "visible");
                }
            }

            // After 500ms of no activity, start the blink interval.
            //
            // Closure leak note: `Closure::once` is consumed after the single
            // timeout callback fires — ownership is handed to the browser for
            // exactly one invocation (same one-shot pattern as rAF). The
            // interval closure inside uses `forget()` which is the pre-existing
            // pattern from the code editor. The leak per keypress is bounded
            // because the old timer is cancelled (via `clear_timeout` /
            // `clear_interval` above) before creating a new one — the leaked
            // closure from the previous cycle will never fire. On component
            // unmount, `on_cleanup` clears both timer IDs to prevent stale
            // interval callbacks from firing after the component is gone.
            let cursor_id_timeout = cursor_id.clone();
            let cb = Closure::once(move || {
                let Some(window) = web_sys::window() else {
                    return;
                };
                let blink = Closure::wrap(Box::new(move || {
                    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
                        return;
                    };
                    if let Some(el) = document.get_element_by_id(&cursor_id_timeout) {
                        let el: &HtmlElement = el.unchecked_ref();
                        let current = el.style().get_property_value("visibility").unwrap_or_default();
                        let next = if current == "hidden" { "visible" } else { "hidden" };
                        let _ = el.style().set_property("visibility", next);
                    }
                }) as Box<dyn FnMut()>);
                let id = window
                    .set_interval_with_callback_and_timeout_and_arguments_0(
                        blink.as_ref().unchecked_ref(),
                        500,
                    )
                    .unwrap_or(0);
                blink_interval_id.set(id);
                blink.forget();
            });
            let timer = window
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    cb.as_ref().unchecked_ref(),
                    500,
                )
                .unwrap_or(0);
            blink_timer_id.set(timer);
            cb.forget();
        }
    };
    // Wrap in Arc so the closure can be shared across multiple event handlers.
    let reset_blink: Arc<dyn Fn()> = Arc::new(reset_blink);

    // Clean up blink timers when the component unmounts to prevent the
    // leaked interval closure from firing after the component is gone.
    on_cleanup(move || {
        if let Some(window) = web_sys::window() {
            let timer = blink_timer_id.get_untracked();
            let interval = blink_interval_id.get_untracked();
            if timer != 0 {
                window.clear_timeout_with_handle(timer);
            }
            if interval != 0 {
                window.clear_interval_with_handle(interval);
            }
        }
    });

    // ── Content sync (external content signal -> DocState) ───────────────
    let doc_sync = doc_state.clone();
    Effect::new(move |prev: Option<String>| {
        let new_content = content.get();
        if prev.as_ref() != Some(&new_content) {
            let needs_update = {
                let Ok(mut ds) = doc_sync.lock() else {
                    return new_content;
                };
                let current = ds.to_markdown();
                if current != new_content {
                    ds.set_from_markdown(&new_content);
                    true
                } else {
                    false
                }
            }; // lock dropped here
            if needs_update {
                text_version.update(|v| *v += 1);
                cursor_version.update(|v| *v += 1);
            }
        }
        new_content
    });

    // ── Post-render: position cursor via direct DOM manipulation ─────────
    let doc_cursor = doc_state.clone();
    let extensions_cursor = Arc::clone(&extensions);
    let cursor_el_id_effect = cursor_el_id.clone();
    let overlay_el_id_effect = overlay_el_id.clone();
    let scroll_container_ref_effect = scroll_container_ref;
    Effect::new(move |_| {
        let _cv = cursor_version.get();
        let _tv = text_version.get();
        let is_focused = focused.get();

        // Compute formatting state for toolbar active buttons.
        // Extension active states are only recomputed when the document
        // text actually changes (text_version), not on cursor-only changes,
        // to avoid the expensive to_markdown() + re-parse roundtrip on
        // every keypress and click.
        if let Ok(ds) = doc_cursor.lock() {
            let fmt = ds.formatting_at_cursor();

            if !extensions_cursor.is_empty() && _tv != last_ext_text_version.get() {
                last_ext_text_version.set(_tv);
                let source = ds.to_markdown();
                let temp_ed = kode_markdown::MarkdownEditor::new(&source);
                let md_fmt = kode_markdown::FormattingState {
                    bold: fmt.bold,
                    italic: fmt.italic,
                    code: fmt.code,
                    strikethrough: fmt.strikethrough,
                    heading_level: fmt.heading_level,
                    bullet_list: fmt.bullet_list,
                    ordered_list: fmt.ordered_list,
                    blockquote: fmt.blockquote,
                };
                let cursor_byte = kode_doc::tree_pos_to_byte_offset(
                    ds.doc(),
                    &source,
                    ds.selection().head,
                );
                let ctx = crate::extension::ExtensionEditorContext {
                    editor: temp_ed.editor(),
                    source: &source,
                    cursor_byte,
                    formatting: &md_fmt,
                };
                let ext_states: Vec<(String, bool)> = extensions_cursor
                    .iter()
                    .flat_map(|ext| {
                        ext.active_state(&ctx)
                            .into_iter()
                            .map(|(name, active)| (name.to_owned(), active))
                    })
                    .collect();
                extension_active_state.set(ext_states);
            }

            formatting_state.set(fmt);
        }

        let doc_raf = doc_cursor.clone();
        let cursor_id_raf = cursor_el_id_effect.clone();
        let overlay_id_raf = overlay_el_id_effect.clone();
        let scroll_ref_raf = scroll_container_ref_effect;
        let cb = Closure::once(move || {
            let Some(document) = web_sys::window().and_then(|w| w.document()) else {
                return;
            };

            let Ok(ds) = doc_raf.lock() else { return };
            let head = ds.selection().head;
            let sel = ds.selection().clone();
            drop(ds);

            let Some(cursor_el) = document.get_element_by_id(&cursor_id_raf) else {
                return;
            };
            let cursor_el: &HtmlElement = cursor_el.unchecked_ref();

            // Clear old selection highlight divs.
            if let Some(overlay_el) = document.get_element_by_id(&overlay_id_raf) {
                while let Some(child) = overlay_el.query_selector(".kode-selection").ok().flatten() {
                    let _ = overlay_el.remove_child(&child);
                }
            }

            if !is_focused {
                let _ = cursor_el.style().set_property("display", "none");
                return;
            }

            let container: web_sys::Element = match scroll_ref_raf.get() {
                Some(el) => el.unchecked_ref::<web_sys::Element>().clone(),
                None => {
                    let _ = cursor_el.style().set_property("display", "none");
                    return;
                }
            };

            // ── Find the DOM element containing the cursor position ─────
            let target_el = find_element_for_pos(&container, head);
            let target_el = match target_el {
                Some(el) => el,
                None => {
                    // Cursor past end: use last positioned element.
                    match find_last_positioned_element(&container) {
                        Some(el) => el,
                        None => {
                            let _ = cursor_el.style().set_property("display", "none");
                            return;
                        }
                    }
                }
            };

            let el_start = parse_data_attr(&target_el, "data-pos-start").unwrap_or(0);
            let char_offset_in_el = head.saturating_sub(el_start);
            // Compute cursor height from the target element's line-height.
            let cursor_height = {
                let Some(window) = web_sys::window() else {
                    return;
                };
                let computed = window.get_computed_style(&target_el).ok().flatten();
                computed
                    .and_then(|cs| cs.get_property_value("line-height").ok())
                    .and_then(|lh| lh.trim_end_matches("px").parse::<f64>().ok())
                    .unwrap_or(24.0)
            };

            // Check if cursor is right after a newline (code block).
            // measure_char_offset_position returns None for \n chars, so
            // detect this first and use the previous char's position + line offset.
            let is_after_newline = char_offset_in_el > 0
                && target_el.text_content()
                    .map(|t| t.chars().nth(char_offset_in_el - 1) == Some('\n'))
                    .unwrap_or(false);

            let measured = if is_after_newline {
                // Measure position of the char BEFORE the newline to get the
                // correct y, then offset down by one line.
                measure_char_offset_position(&target_el, char_offset_in_el - 1)
                    .map(|(_, prev_y)| {
                        let el_rect = target_el.get_bounding_client_rect();
                        (el_rect.left(), prev_y + cursor_height)
                    })
            } else {
                measure_char_offset_position(&target_el, char_offset_in_el)
            };

            if let Some((x, y)) = measured {
                let container_rect = container.get_bounding_client_rect();
                let left = x - container_rect.left();
                let top = y - container_rect.top() + container.scroll_top() as f64;
                let style = cursor_el.style();
                let _ = style.set_property("top", &format!("{}px", top));
                let _ = style.set_property("left", &format!("{}px", left));
                let _ = style.set_property("height", &format!("{}px", cursor_height));
                let _ = style.set_property("display", "block");
                let _ = style.set_property("visibility", "visible");
            } else {
                // Fallback: position at the element's top-left. For empty
                // elements (empty paragraphs, new documents), the element may
                // have zero width — use the parent's content area instead.
                let container_rect = container.get_bounding_client_rect();
                let el_rect = target_el.get_bounding_client_rect();
                let (left, top) = if el_rect.height() > 0.0 {
                    (
                        el_rect.left() - container_rect.left(),
                        el_rect.top() - container_rect.top() + container.scroll_top() as f64,
                    )
                } else if let Some(parent) = target_el.parent_element() {
                    let pr = parent.get_bounding_client_rect();
                    (
                        pr.left() - container_rect.left(),
                        pr.top() - container_rect.top() + container.scroll_top() as f64,
                    )
                } else {
                    (0.0, container.scroll_top() as f64)
                };
                let style = cursor_el.style();
                let _ = style.set_property("top", &format!("{}px", top));
                let _ = style.set_property("left", &format!("{}px", left));
                let _ = style.set_property("height", &format!("{}px", cursor_height));
                let _ = style.set_property("display", "block");
                let _ = style.set_property("visibility", "visible");
            }

            // ── Scroll cursor into view ────────────────────────────────
            // If the cursor is outside the visible area of the scroll
            // container, scroll to bring it into view.
            if cursor_el.style().get_property_value("display").ok().as_deref() == Some("block") {
                if let Ok(top_str) = cursor_el.style().get_property_value("top") {
                    if let Ok(cursor_top) = top_str.trim_end_matches("px").parse::<f64>() {
                        let scroll_top = container.scroll_top() as f64;
                        let container_height = container.get_bounding_client_rect().height();
                        const SCROLL_MARGIN_PX: f64 = 10.0;
                        let margin = cursor_height + SCROLL_MARGIN_PX;
                        if cursor_top < scroll_top + margin {
                            container.set_scroll_top((cursor_top - margin).max(0.0) as i32);
                        } else if cursor_top > scroll_top + container_height - margin {
                            container.set_scroll_top((cursor_top - container_height + margin) as i32);
                        }
                    }
                }
            }

            // ── Selection highlight ─────────────────────────────────────
            if !sel.is_cursor() {
                render_selection_highlights(
                    &document,
                    &container,
                    &overlay_id_raf,
                    &sel,
                );
            }
        });
        let _ = web_sys::window()
            .and_then(|w| w.request_animation_frame(cb.as_ref().unchecked_ref()).ok());
        // `forget()` is acceptable here: `Closure::once` is consumed after the
        // single rAF callback fires, so ownership is handed to the browser for
        // exactly one invocation. This is the standard WASM pattern for one-shot
        // callbacks — no persistent listener is leaked.
        cb.forget();
    });

    // ── Keydown ──────────────────────────────────────────────────────────
    let doc_key = doc_state.clone();
    let notify_text_key = notify_text.clone();
    let reset_blink_key = reset_blink.clone();
    let extension_shortcuts_key: Arc<Vec<ExtensionKeyboardShortcut>> = Arc::clone(&extension_shortcuts);
    let on_keydown = move |ev: KeyboardEvent| {
        if composing.get_untracked() {
            return;
        }

        let ctrl = ev.ctrl_key() || ev.meta_key();
        let shift = ev.shift_key();
        let key = ev.key();

        // Clipboard: Ctrl+C and Ctrl+X are handled by the copy/cut event
        // listeners which have access to clipboardData for structured content.
        // We still need to populate the textarea before the browser fires the
        // copy/cut event so the browser knows there is content to copy.
        // Ctrl+V is handled by the paste event listener.
        match key.as_str() {
            "c" | "x" if ctrl => {
                if let Ok(ds) = doc_key.lock() {
                    let sel = ds.selection();
                    if !sel.is_cursor() {
                        let from = sel.from();
                        let to = sel.to();
                        let selected = ds.text_between(from, to);
                        if let Some(ta) = textarea_ref.get() {
                            let ta: &HtmlTextAreaElement = ta.as_ref();
                            ta.set_value(&selected);
                            ta.select();
                        }
                    }
                }
                return; // copy/cut event listener handles the rest
            }
            "v" if ctrl => return, // paste event listener handles it
            _ => {}
        }

        // Undo/Redo (before acquiring the lock, since these are self-contained).
        match key.as_str() {
            "z" if ctrl && shift => {
                let Ok(mut ds) = doc_key.lock() else { return };
                ds.redo();
                let md = ds.to_markdown();
                drop(ds);
                (notify_text_key)(Some(md));
                reset_blink_key();
                ev.prevent_default();
                return;
            }
            "z" if ctrl => {
                let Ok(mut ds) = doc_key.lock() else { return };
                ds.undo();
                let md = ds.to_markdown();
                drop(ds);
                (notify_text_key)(Some(md));
                reset_blink_key();
                ev.prevent_default();
                return;
            }
            _ => {}
        }

        let Ok(mut ds) = doc_key.lock() else { return };

        let handled = match key.as_str() {
            // ── Inline formatting shortcuts ──────────────────────────
            "b" if ctrl => {
                ds.toggle_mark(MarkType::Strong);
                true
            }
            "i" if ctrl => {
                ds.toggle_mark(MarkType::Em);
                true
            }
            "u" if ctrl => {
                ds.toggle_mark(MarkType::Strike);
                true
            }
            "`" if ctrl => {
                ds.toggle_mark(MarkType::Code);
                true
            }

            // ── Navigation ───────────────────────────────────────────
            "Enter" => {
                ds.split_block();
                true
            }
            "Backspace" => {
                ds.backspace();
                true
            }
            "Delete" => {
                ds.delete_forward();
                true
            }
            "ArrowLeft" if shift => {
                // Extend selection left.
                let head = ds.selection().head;
                if head > 0 {
                    let new_head = next_text_pos(ds.doc(), head, false);
                    let anchor = ds.selection().anchor;
                    ds.set_selection(Selection::range(anchor, new_head));
                }
                true
            }
            "ArrowRight" if shift => {
                // Extend selection right.
                let head = ds.selection().head;
                let doc_size = ds.doc().content.size();
                if head < doc_size {
                    let new_head = next_text_pos(ds.doc(), head, true);
                    let anchor = ds.selection().anchor;
                    ds.set_selection(Selection::range(anchor, new_head));
                }
                true
            }
            "ArrowLeft" => {
                let sel = ds.selection().clone();
                if !sel.is_cursor() {
                    // Collapse selection to the left edge.
                    ds.set_selection(Selection::cursor(sel.from()));
                } else if sel.head > 0 {
                    let new_pos = next_text_pos(ds.doc(), sel.head, false);
                    ds.set_selection(Selection::cursor(new_pos));
                }
                true
            }
            "ArrowRight" => {
                let sel = ds.selection().clone();
                if !sel.is_cursor() {
                    // Collapse selection to the right edge.
                    ds.set_selection(Selection::cursor(sel.to()));
                } else {
                    let doc_size = ds.doc().content.size();
                    if sel.head < doc_size {
                        let new_pos = next_text_pos(ds.doc(), sel.head, true);
                        ds.set_selection(Selection::cursor(new_pos));
                    }
                }
                true
            }
            "ArrowDown" | "ArrowUp" => {
                // Vertical cursor movement uses DOM measurement: find the
                // current cursor pixel position, offset by one line-height
                // up or down, then use caretRangeFromPoint to find the
                // character position at the new coordinates.
                //
                // We need to drop the DocState lock, do DOM measurement,
                // then re-acquire the lock to set the new position.
                let head = ds.selection().head;
                drop(ds);

                let container_el: Option<web_sys::Element> = scroll_container_ref
                    .get()
                    .map(|el| el.unchecked_ref::<web_sys::Element>().clone());
                let doc = web_sys::window().and_then(|w| w.document());
                let new_pos = container_el.as_ref().and_then(|c| {
                    vertical_cursor_move(doc.as_ref()?, c, head, key == "ArrowDown")
                });

                if let Some(target) = new_pos {
                    let Ok(mut ds2) = doc_key.lock() else { ev.prevent_default(); return; };
                    if shift {
                        let anchor = ds2.selection().anchor;
                        ds2.set_selection(Selection::range(anchor, target));
                    } else {
                        ds2.set_selection(Selection::cursor(target));
                    }
                    let md = ds2.to_markdown();
                    drop(ds2);
                    (notify_text_key)(Some(md));
                }

                reset_blink_key();
                ev.prevent_default();
                return; // skip the common path below since we dropped ds
            }
            "Home" => {
                if ctrl {
                    // Ctrl+Home: move to start of document (first textblock content).
                    // Position 2 is the content start of the first block in a standard doc.
                    let target = ds.doc().resolve(2);
                    let doc_start = if target.parent().node_type.is_textblock() {
                        target.before(target.depth) + 1
                    } else {
                        2
                    };
                    if shift {
                        let anchor = ds.selection().anchor;
                        ds.set_selection(Selection::range(anchor, doc_start));
                    } else {
                        ds.set_selection(Selection::cursor(doc_start));
                    }
                } else {
                    // Move to start of current textblock.
                    let resolved = ds.doc().resolve(ds.selection().head);
                    if resolved.parent().node_type.is_textblock() {
                        let block_start = resolved.before(resolved.depth) + 1;
                        if shift {
                            let anchor = ds.selection().anchor;
                            ds.set_selection(Selection::range(anchor, block_start));
                        } else {
                            ds.set_selection(Selection::cursor(block_start));
                        }
                    }
                }
                true
            }
            "End" => {
                if ctrl {
                    // Ctrl+End: move to end of document (last textblock content).
                    let doc_end_pos = ds.doc().node_size().saturating_sub(2);
                    // doc_end_pos is inside the last block, resolve to find textblock end.
                    if doc_end_pos >= 2 {
                        let target = ds.doc().resolve(doc_end_pos);
                        let doc_end = if target.parent().node_type.is_textblock() {
                            target.after(target.depth) - 1
                        } else {
                            doc_end_pos
                        };
                        if shift {
                            let anchor = ds.selection().anchor;
                            ds.set_selection(Selection::range(anchor, doc_end));
                        } else {
                            ds.set_selection(Selection::cursor(doc_end));
                        }
                    }
                } else {
                    // Move to end of current textblock.
                    let resolved = ds.doc().resolve(ds.selection().head);
                    if resolved.parent().node_type.is_textblock() {
                        let block_end = resolved.after(resolved.depth) - 1;
                        if shift {
                            let anchor = ds.selection().anchor;
                            ds.set_selection(Selection::range(anchor, block_end));
                        } else {
                            ds.set_selection(Selection::cursor(block_end));
                        }
                    }
                }
                true
            }
            "a" if ctrl => {
                // Select all: from first text position to last.
                let doc_size = ds.doc().content.size();
                let first = if doc_size > 0 {
                    next_text_pos(ds.doc(), 0, true)
                } else {
                    0
                };
                // Find the last valid text position by scanning backward from doc end.
                let last = if doc_size > 0 {
                    next_text_pos(ds.doc(), doc_size, false)
                } else {
                    0
                };
                ds.set_selection(Selection::range(first, last));
                true
            }
            _ => false,
        };

        // Extension keyboard shortcuts (checked after built-in shortcuts).
        let handled = if !handled {
            let mut ext_handled = false;
            for shortcut in extension_shortcuts_key.iter() {
                if matches_key_descriptor(&ev, &shortcut.key) {
                    // Extension shortcuts expect &mut MarkdownEditor. Bridge via
                    // markdown roundtrip, but only commit if handler returns true.
                    let md = ds.to_markdown();
                    let mut temp = kode_markdown::MarkdownEditor::new(&md);
                    let fired = (shortcut.handler)(&mut temp);
                    if fired {
                        temp.sync_tree();
                        let new_md = temp.text();
                        if new_md != md {
                            ds.set_from_markdown(&new_md);
                        }
                        ext_handled = true;
                        break;
                    }
                }
            }
            ext_handled
        } else {
            true
        };

        if handled {
            let md = ds.to_markdown();
            drop(ds);
            (notify_text_key)(Some(md));
            reset_blink_key();
            ev.prevent_default();
        } else {
            drop(ds);
        }
    };

    // ── Text input (via hidden textarea) ─────────────────────────────────
    let doc_input = doc_state.clone();
    let notify_text_input = notify_text.clone();
    let reset_blink_input = Arc::clone(&reset_blink);
    let on_input = move |_ev: leptos::ev::Event| {
        if composing.get_untracked() {
            return;
        }
        let Some(ta) = textarea_ref.get() else { return };
        let ta: &HtmlTextAreaElement = ta.as_ref();
        let val = ta.value();
        if val.is_empty() {
            return;
        }
        ta.set_value("");

        let Ok(mut ds) = doc_input.lock() else { return };
        ds.insert_text(&val);
        let md = ds.to_markdown();
        drop(ds);
        (notify_text_input)(Some(md));
        reset_blink_input();
    };

    // ── Clipboard events (copy / cut / paste) ─────────────────────────────
    // These intercept the browser's clipboard events to put structured markdown
    // on the clipboard and to parse it back on paste.

    // Clipboard event listeners are attached via addEventListener after the
    // textarea is mounted, because Leptos's `on:` attribute tuple has a size
    // limit and copy/cut/paste aren't in its standard typed event set.
    {
        let doc_copy = doc_state.clone();
        let doc_cut = doc_state.clone();
        let doc_paste = doc_state.clone();
        let notify_text_cut = notify_text.clone();
        let notify_text_paste = notify_text.clone();
        let reset_blink_cut = Arc::clone(&reset_blink);
        let reset_blink_paste = Arc::clone(&reset_blink);

        let copy_cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
            let Ok(ds) = doc_copy.lock() else { return };
            let sel = ds.selection();
            if sel.is_cursor() {
                return;
            }
            let from = sel.from();
            let to = sel.to();
            let md = ds.selected_markdown();
            let plain = ds.text_between(from, to);
            drop(ds);

            ev.prevent_default();
            let ev: ClipboardEvent = ev.unchecked_into();
            if let Some(dt) = ev.clipboard_data() {
                let _ = dt.set_data("text/plain", &plain);
                let html = format!("<pre data-kode-md>{}</pre>", html_escape(&md));
                let _ = dt.set_data("text/html", &html);
            }
        });

        let cut_cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
            let Ok(mut ds) = doc_cut.lock() else { return };
            let sel = ds.selection().clone();
            if sel.is_cursor() {
                return;
            }
            let from = sel.from();
            let to = sel.to();
            let md = ds.selected_markdown();
            let plain = ds.text_between(from, to);

            ev.prevent_default();
            let ev: ClipboardEvent = ev.unchecked_into();
            if let Some(dt) = ev.clipboard_data() {
                let _ = dt.set_data("text/plain", &plain);
                let html = format!("<pre data-kode-md>{}</pre>", html_escape(&md));
                let _ = dt.set_data("text/html", &html);
            }

            ds.backspace();
            let new_md = ds.to_markdown();
            drop(ds);
            (notify_text_cut)(Some(new_md));
            reset_blink_cut();
        });

        let paste_cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
            ev.prevent_default();
            if let Some(ta) = textarea_ref.get() {
                let ta: &HtmlTextAreaElement = ta.as_ref();
                ta.set_value("");
            }

            let ev: ClipboardEvent = ev.unchecked_into();
            let Some(dt) = ev.clipboard_data() else { return };

            let html = dt.get_data("text/html").unwrap_or_default();
            let has_kode_md = html.contains("data-kode-md");
            let plain = dt.get_data("text/plain").unwrap_or_default();

            let Ok(mut ds) = doc_paste.lock() else { return };

            if has_kode_md {
                if let Some(md) = extract_kode_markdown(&html) {
                    // Our own markdown — always use insert_from_markdown
                    // to preserve all formatting (inline marks, block types).
                    ds.insert_from_markdown(&md);
                }
            } else if !plain.is_empty() {
                ds.insert_text_multiline(&plain);
            }

            let new_md = ds.to_markdown();
            drop(ds);
            (notify_text_paste)(Some(new_md));
            reset_blink_paste();
        });

        // Attach to the textarea once it's mounted. Store closures in Rc so
        // they can be removed in on_cleanup when the component is unmounted.
        let clipboard_closures = std::cell::RefCell::new(Some((copy_cb, cut_cb, paste_cb)));
        Effect::new(move |_| {
            let Some(ta) = textarea_ref.get() else { return };
            let Some((copy_cb, cut_cb, paste_cb)) = clipboard_closures.borrow_mut().take() else {
                return; // Already attached.
            };
            let el: &web_sys::EventTarget = ta.as_ref();
            let _ = el.add_event_listener_with_callback(
                "copy",
                copy_cb.as_ref().unchecked_ref(),
            );
            let _ = el.add_event_listener_with_callback(
                "cut",
                cut_cb.as_ref().unchecked_ref(),
            );
            let _ = el.add_event_listener_with_callback(
                "paste",
                paste_cb.as_ref().unchecked_ref(),
            );

            // Store references for cleanup and register removal on unmount.
            let copy_fn: js_sys::Function = copy_cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
            let cut_fn: js_sys::Function = cut_cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
            let paste_fn: js_sys::Function = paste_cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
            // Leak closures so JS can call them — the on_cleanup will
            // remove the listeners, and the leaked closures become inert.
            copy_cb.forget();
            cut_cb.forget();
            paste_cb.forget();

            let cleanup_el: web_sys::EventTarget = el.clone();
            on_cleanup(move || {
                let _ = cleanup_el.remove_event_listener_with_callback("copy", &copy_fn);
                let _ = cleanup_el.remove_event_listener_with_callback("cut", &cut_fn);
                let _ = cleanup_el.remove_event_listener_with_callback("paste", &paste_fn);
            });
        });

        // ── Position attribute update ────────────────────────────────────
        // When <For> preserves unchanged blocks, their data-pos-start/end
        // attributes become stale if preceding blocks changed size. This
        // Effect walks the DOM after each text change and updates positions
        // to keep cursor mapping accurate.
        let doc_for_pos = doc_state.clone();
        let scroll_ref_pos = scroll_container_ref;
        Effect::new(move |_| {
            let _tv = text_version.get();
            let Some(container) = scroll_ref_pos.get() else { return };
            let Ok(ds) = doc_for_pos.lock() else { return };
            let doc = ds.doc();

            // When <For> preserves a block, ALL its data-pos-start/end
            // attributes are stale if preceding blocks changed size. Fix by
            // computing the position delta and applying it to every element
            // with position attributes inside the block.
            // Blocks are direct children of the scroll container (no wrapper div).
            let container_el: &web_sys::Element = container.as_ref();
            let mut expected_pos = 0usize; // content_start = 0 (matches render_doc)
            let mut child = container_el.first_element_child();
            for block in doc.content.iter() {
                let Some(el) = child.as_ref() else { break };
                let content_start = expected_pos + 1;
                // Read the current (possibly stale) pos from the DOM.
                // If the block has no data-pos-start (e.g., extension blocks),
                // look at the first child that does.
                let current_pos = read_pos_start(el).unwrap_or(content_start);
                let delta = content_start as i64 - current_pos as i64;
                if delta != 0 {
                    shift_all_positions(el, delta);
                }
                expected_pos += block.node_size();
                child = el.next_element_sibling();
            }
        });
    }

    // ── Composition events (IME) ─────────────────────────────────────────
    let on_composition_start = move |_: CompositionEvent| {
        composing.set(true);
    };
    let doc_comp_end = doc_state.clone();
    let notify_text_comp = notify_text.clone();
    let on_composition_end = move |ev: CompositionEvent| {
        composing.set(false);
        if let Some(data) = ev.data() {
            if !data.is_empty() {
                let Ok(mut ds) = doc_comp_end.lock() else { return };
                ds.insert_text(&data);
                let md = ds.to_markdown();
                drop(ds);
                (notify_text_comp)(Some(md));
            }
        }
        // Clear the textarea so stale IME text doesn't re-fire.
        if let Some(ta) = textarea_ref.get() {
            let ta: &HtmlTextAreaElement = ta.as_ref();
            ta.set_value("");
        }
    };

    // ── Focus / Blur ─────────────────────────────────────────────────────
    let on_focus = move |_: FocusEvent| {
        focused.set(true);
    };
    let on_blur = move |_: FocusEvent| {
        focused.set(false);
    };

    // ── Click handler ────────────────────────────────────────────────────
    let doc_click = doc_state.clone();
    let notify_cursor_click = Arc::clone(&notify_cursor);
    let reset_blink_click = Arc::clone(&reset_blink);
    let on_container_mousedown = move |ev: MouseEvent| {
        let client_x = ev.client_x() as f64;
        let client_y = ev.client_y() as f64;

        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };

        // Find the clicked element.
        let target_el = match document.element_from_point(client_x as f32, client_y as f32) {
            Some(el) => el,
            None => return,
        };

        // If the click is inside an extension-rendered block, let it through
        // without stealing focus or repositioning the cursor. Extensions
        // handle their own click events (e.g., chart edit buttons).
        if find_ancestor_with_attr(&target_el, "data-kode-extension").is_some() {
            return;
        }

        // Prevent the browser's default mousedown behavior which would focus
        // the container div instead of our hidden textarea.
        ev.prevent_default();
        // Focus the hidden textarea so keyboard events work.
        if let Some(ta) = textarea_ref.get() {
            let ta: &HtmlTextAreaElement = ta.as_ref();
            let _ = ta.focus();
        }

        let pos_el = match find_ancestor_with_pos_attrs(&target_el) {
            Some(el) => el,
            None => return,
        };

        // If the element has a nested child with pos attrs (e.g., LI
        // containing a SPAN/P), prefer the innermost child. This ensures
        // clicking in a list item's empty space resolves to the text
        // content, not the wrapper.
        let pos_el = find_deepest_pos_child(&pos_el).unwrap_or(pos_el);

        let el_start = match parse_data_attr(&pos_el, "data-pos-start") {
            Some(v) => v,
            None => return,
        };

        // Determine the character offset within the element using caret APIs.
        let char_in_el = get_char_offset_from_point(&document, client_x, client_y, &pos_el)
            .unwrap_or(0);

        // Clamp: tree_pos must not exceed posEnd. This happens when
        // caretRangeFromPoint returns an offset based on extra DOM content
        // (e.g., language labels in code blocks, list markers) that isn't
        // part of the document model's content range.
        let el_end = parse_data_attr(&pos_el, "data-pos-end").unwrap_or(usize::MAX);
        let tree_pos = (el_start + char_in_el).min(el_end);


        // Multi-click detection (double-click = word, triple-click = line).
        let now = js_sys::Date::now();
        let prev_time = last_click_time.get_untracked();
        let prev_clicks = click_count.get_untracked();
        const MULTI_CLICK_MS: f64 = 500.0;
        let clicks = if now - prev_time < MULTI_CLICK_MS {
            prev_clicks + 1
        } else {
            1
        };
        last_click_time.set(now);
        click_count.set(clicks);

        let Ok(mut ds) = doc_click.lock() else { return };

        if ev.shift_key() {
            // Extend selection.
            let anchor = ds.selection().anchor;
            ds.set_selection(Selection::range(anchor, tree_pos));
            drop(ds);
        } else if clicks == 2 {
            // Double-click: select word.
            ds.set_selection(Selection::cursor(tree_pos));
            ds.select_word();
            drop(ds);
        } else if clicks >= 3 {
            // Triple-click: select line (entire textblock).
            ds.set_selection(Selection::cursor(tree_pos));
            ds.select_line();
            drop(ds);
        } else {
            ds.set_selection(Selection::cursor(tree_pos));
            drop(ds);
            dragging.set(true);
        }

        (notify_cursor_click)();
        reset_blink_click();
    };

    // ── Mouse move (drag selection) ──────────────────────────────────────
    let doc_move = doc_state.clone();
    let notify_cursor_move = Arc::clone(&notify_cursor);
    let on_mousemove = move |ev: MouseEvent| {
        if !dragging.get_untracked() {
            return;
        }

        let client_x = ev.client_x() as f64;
        let client_y = ev.client_y() as f64;

        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };

        let target_el = match document.element_from_point(client_x as f32, client_y as f32) {
            Some(el) => el,
            None => return,
        };

        let pos_el = match find_ancestor_with_pos_attrs(&target_el) {
            Some(el) => el,
            None => return,
        };

        let el_start = match parse_data_attr(&pos_el, "data-pos-start") {
            Some(v) => v,
            None => return,
        };

        let char_in_el = get_char_offset_from_point(&document, client_x, client_y, &pos_el)
            .unwrap_or(0);

        let tree_pos = el_start + char_in_el;

        let Ok(mut ds) = doc_move.lock() else { return };
        let anchor = ds.selection().anchor;
        ds.set_selection(Selection::range(anchor, tree_pos));
        drop(ds);

        (notify_cursor_move)();
    };

    // Attach mouseup to the document so drag-outside-then-release still
    // clears the dragging state (same pattern as the code editor's scroll mouseup).
    {
        let mouseup_cb = Closure::wrap(Box::new(move |_: MouseEvent| {
            dragging.set(false);
        }) as Box<dyn FnMut(MouseEvent)>);

        if let Some(document) = web_sys::window().and_then(|w| w.document()) {
            let _ = document.add_event_listener_with_callback(
                "mouseup",
                mouseup_cb.as_ref().unchecked_ref(),
            );
        }

        // Store the closure so it stays alive, and remove the listener on cleanup.
        let mouseup_cb = send_wrapper::SendWrapper::new(mouseup_cb);
        on_cleanup(move || {
            if let Some(document) = web_sys::window().and_then(|w| w.document()) {
                let _ = document.remove_event_listener_with_callback(
                    "mouseup",
                    mouseup_cb.as_ref().as_ref().unchecked_ref(),
                );
            }
            drop(mouseup_cb);
        });
    }

    // ── Toolbar ───────────────────────────────────────────────────────────
    // Build the toolbar from ToolbarItems — either custom (from prop) or default.
    // All styling uses CSS classes from wysiwyg.css (`.kode-toolbar-*`).
    let toolbar_view = if show_toolbar {
        let items = toolbar_items.unwrap_or_else(|| {
            // Default toolbar + extension buttons appended after
            let mut items = default_toolbar_items();
            for ext_item in extension_toolbar_items {
                items.push(ToolbarItem::Separator);
                items.push(ToolbarItem::ExtensionButton(ext_item));
            }
            items
        });

        let mut toolbar_views: Vec<AnyView> = Vec::new();

        for item in items {
            match item {
                ToolbarItem::Separator => {
                    toolbar_views.push(
                        view! { <div class="kode-toolbar-separator" /> }.into_any()
                    );
                }
                ToolbarItem::Spacer => {
                    toolbar_views.push(
                        view! { <div class="kode-toolbar-spacer" /> }.into_any()
                    );
                }
                ToolbarItem::Slot(slot_view) => {
                    toolbar_views.push(slot_view);
                }
                ToolbarItem::Builtin(btn) => {
                    let doc_tb = doc_state.clone();
                    let notify_text_tb = notify_text.clone();
                    let reset_blink_tb = Arc::clone(&reset_blink);
                    let textarea_ref_tb = textarea_ref;
                    let label = btn.label();
                    let title = btn.title();

                    let active = Signal::derive(move || {
                        btn.is_active(&formatting_state.get())
                    });

                    let class = Signal::derive(move || {
                        if active.get() {
                            "kode-toolbar-button active".to_string()
                        } else {
                            "kode-toolbar-button".to_string()
                        }
                    });

                    let on_click = move |_: MouseEvent| {
                        let Ok(mut ds) = doc_tb.lock() else { return };
                        dispatch_builtin_action(&mut ds, btn);
                        let md = ds.to_markdown();
                        drop(ds);
                        (notify_text_tb)(Some(md));
                        reset_blink_tb();
                        if let Some(ta) = textarea_ref_tb.get() {
                            let ta: &HtmlTextAreaElement = ta.as_ref();
                            let _ = ta.focus();
                        }
                    };

                    toolbar_views.push(
                        view! {
                            <button
                                title=title
                                class=class
                                on:click=on_click
                                on:mousedown=|ev: MouseEvent| { ev.prevent_default(); }
                            >
                                {label}
                            </button>
                        }.into_any(),
                    );
                }
                ToolbarItem::BuiltinWithView(btn, custom_view) => {
                    let doc_tb = doc_state.clone();
                    let notify_text_tb = notify_text.clone();
                    let reset_blink_tb = Arc::clone(&reset_blink);
                    let textarea_ref_tb = textarea_ref;
                    let title = btn.title();

                    let active = Signal::derive(move || {
                        btn.is_active(&formatting_state.get())
                    });

                    let class = Signal::derive(move || {
                        if active.get() {
                            "kode-toolbar-button active".to_string()
                        } else {
                            "kode-toolbar-button".to_string()
                        }
                    });

                    let on_click = move |_: MouseEvent| {
                        let Ok(mut ds) = doc_tb.lock() else { return };
                        dispatch_builtin_action(&mut ds, btn);
                        let md = ds.to_markdown();
                        drop(ds);
                        (notify_text_tb)(Some(md));
                        reset_blink_tb();
                        if let Some(ta) = textarea_ref_tb.get() {
                            let ta: &HtmlTextAreaElement = ta.as_ref();
                            let _ = ta.focus();
                        }
                    };

                    toolbar_views.push(
                        view! {
                            <button
                                title=title
                                class=class
                                on:click=on_click
                                on:mousedown=|ev: MouseEvent| { ev.prevent_default(); }
                            >
                                {custom_view}
                            </button>
                        }.into_any(),
                    );
                }
                ToolbarItem::Custom(custom) => {
                    let on_click = custom.on_click;
                    let title = custom.title;
                    let btn_class = custom.class.unwrap_or_else(|| "kode-toolbar-button".to_string());

                    toolbar_views.push(
                        view! {
                            <button
                                title=title
                                class=btn_class
                                on:click=move |_: MouseEvent| { (on_click)(); }
                                on:mousedown=|ev: MouseEvent| { ev.prevent_default(); }
                            >
                                {custom.label}
                            </button>
                        }.into_any(),
                    );
                }
                ToolbarItem::ExtensionButton(ext_item) => {
                    let doc_ext_tb = doc_state.clone();
                    let notify_text_ext_tb = notify_text.clone();
                    let reset_blink_ext_tb = Arc::clone(&reset_blink);
                    let textarea_ref_ext_tb = textarea_ref;
                    let action = ext_item.action;
                    let title = ext_item.title;
                    let active_name = ext_item.active_name;

                    let active = Signal::derive(move || {
                        if let Some(ref name) = active_name {
                            let states = extension_active_state.get();
                            states.iter().any(|(n, a)| n == name && *a)
                        } else {
                            false
                        }
                    });

                    let class = Signal::derive(move || {
                        if active.get() {
                            "kode-toolbar-button active".to_string()
                        } else {
                            "kode-toolbar-button".to_string()
                        }
                    });

                    let on_click = move |_: MouseEvent| {
                        let Ok(mut ds) = doc_ext_tb.lock() else { return };
                        let changed = apply_md_command(&mut ds, |e| { (action)(e); });
                        if changed {
                            let md_out = ds.to_markdown();
                            drop(ds);
                            (notify_text_ext_tb)(Some(md_out));
                        } else {
                            drop(ds);
                        }
                        reset_blink_ext_tb();
                        if let Some(ta) = textarea_ref_ext_tb.get() {
                            let ta: &HtmlTextAreaElement = ta.as_ref();
                            let _ = ta.focus();
                        }
                    };

                    toolbar_views.push(
                        view! {
                            <button
                                title=title
                                class=class
                                on:click=on_click
                                on:mousedown=|ev: MouseEvent| { ev.prevent_default(); }
                            >
                                {ext_item.label}
                            </button>
                        }.into_any(),
                    );
                }
            }
        }

        Some(view! {
            <div class="kode-toolbar">
                {toolbar_views}
            </div>
        }.into_any())
    } else {
        None
    };

    // ── Render ───────────────────────────────────────────────────────────
    let theme_css = move || theme.get().syntax_css("pre.kode-content");

    view! {
        <style>{theme_css}</style>
        <style>{include_str!("../wysiwyg.css")}</style>
        <div class="wysiwyg-container"
            style=move || {
                let mut css = theme.get().to_css_vars();
                if let Some(ref mw) = container_max_width {
                    css.push_str(&format!("; max-width: {mw}"));
                }
                css
            }
            on:mousedown=on_container_mousedown
            on:mousemove=on_mousemove>

            {toolbar_view}

            <textarea node_ref=textarea_ref class="kode-hidden-textarea"
                autocapitalize="off" autocomplete="off" spellcheck="false"
                on:keydown=on_keydown on:input=on_input
                on:compositionstart=on_composition_start on:compositionend=on_composition_end
                on:focus=on_focus on:blur=on_blur />

            <div class="wysiwyg-scroll-container tree-wysiwyg-scroll-container"
                node_ref=scroll_container_ref>
                // Keyed block rendering: extract block items reactively,
                // then use <For> so unchanged blocks preserve their DOM
                // and reactive state (including ChartMLChart components).
                {
                    let doc_for_blocks = doc_state.clone();
                    let blocks = Memo::new(move |_| {
                        let _tv = text_version.get();
                        let Ok(ds) = doc_for_blocks.lock() else {
                            return Vec::new();
                        };
                        let items = extract_block_items(ds.doc());
                        drop(ds);
                        items
                    });

                    let extensions_for_render = Arc::clone(&extensions);
                    let aliases_for_render = Arc::clone(&language_aliases);
                    view! {
                        <For
                            each=move || blocks.get()
                            key=|b| b.key
                            let:block
                        >
                            {
                                let exts = Arc::clone(&extensions_for_render);
                                let aliases = Arc::clone(&aliases_for_render);
                                for ext in exts.iter() {
                                    ext.begin_render_pass();
                                }
                                render_block_node(&block.node, block.position, &exts, &aliases)
                            }
                        </For>
                    }.into_any()
                }

                // Overlay: cursor + selection highlights, positioned via Effect
                <div id=overlay_el_id
                    style="position:absolute;top:0;left:0;right:0;bottom:0;pointer-events:none;z-index:1;">
                    <div id=cursor_el_id class="kode-cursor"
                        style="position:absolute;width:2px;pointer-events:none;z-index:2;display:none;height:1.4em;background:var(--kode-cursor);" />
                </div>
            </div>
        </div>
    }
}

/// Read the `data-pos-start` attribute from an element, or from its first
/// descendant that has one.
fn read_pos_start(el: &web_sys::Element) -> Option<usize> {
    if let Some(val) = el.get_attribute("data-pos-start") {
        return val.parse().ok();
    }
    // Check descendants (e.g., extension blocks wrap in a div without pos attrs)
    let nodes = el.query_selector_all("[data-pos-start]").ok()?;
    if nodes.length() > 0 {
        let first = nodes.get(0)?;
        let first_el: web_sys::Element = first.dyn_into().ok()?;
        first_el.get_attribute("data-pos-start")?.parse().ok()
    } else {
        None
    }
}

/// Shift ALL `data-pos-start` and `data-pos-end` attributes inside an element
/// by `delta` tokens. This handles the case where `<For>` preserved a block
/// but preceding blocks changed size, shifting all positions.
fn shift_all_positions(el: &web_sys::Element, delta: i64) {
    let Ok(nodes) = el.query_selector_all("[data-pos-start]") else { return };
    for i in 0..nodes.length() {
        let Some(node) = nodes.get(i) else { continue };
        let Ok(child_el) = node.dyn_into::<web_sys::Element>() else { continue };
        if let Some(start_str) = child_el.get_attribute("data-pos-start") {
            if let Ok(start) = start_str.parse::<i64>() {
                let _ = child_el.set_attribute("data-pos-start", &(start + delta).to_string());
            }
        }
        if let Some(end_str) = child_el.get_attribute("data-pos-end") {
            if let Ok(end) = end_str.parse::<i64>() {
                let _ = child_el.set_attribute("data-pos-end", &(end + delta).to_string());
            }
        }
    }
    // Also update the element itself if it has position attrs
    if let Some(start_str) = el.get_attribute("data-pos-start") {
        if let Ok(start) = start_str.parse::<i64>() {
            let _ = el.set_attribute("data-pos-start", &(start + delta).to_string());
        }
    }
    if let Some(end_str) = el.get_attribute("data-pos-end") {
        if let Ok(end) = end_str.parse::<i64>() {
            let _ = el.set_attribute("data-pos-end", &(end + delta).to_string());
        }
    }
}
