//! Tree-based WYSIWYG editor component using `contenteditable`.
//!
//! This module implements a WYSIWYG editor that uses `kode_doc::DocState` as the
//! source of truth and renders into a `contenteditable="true"` div. The browser
//! handles cursor rendering, selection highlighting, click-to-position mapping,
//! and resize adaptation natively.
//!
//! All user input is intercepted via `beforeinput` and `keydown` events. The
//! browser's Selection is mapped to DocState positions using `data-pos-start`/
//! `data-pos-end` attributes on block elements, and restored after each
//! re-render.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use kode_doc::mark::MarkType;
use kode_doc::{DocState, FormattingState, GapSide, Selection};
use leptos::prelude::*;
use leptos::tachys::view::any_view::AnyView;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{CompositionEvent, HtmlElement, KeyboardEvent, MouseEvent};

use crate::extension::{matches_key_descriptor, Extension, ExtensionKeyboardShortcut, ExtensionToolbarItem};
use crate::theme::Theme;
use crate::toolbar::{InjectCommand, ToolbarItem, default_toolbar_items, dispatch_builtin_action};

use super::clipboard::{html_escape, extract_kode_markdown};
use super::doc_renderer::{doc_to_segments, RenderSegment};
use super::dom_helpers::apply_md_command;

/// Tree-based WYSIWYG editor component using `contenteditable`.
///
/// Drop-in replacement for the previous hidden-textarea approach. Same props
/// interface, but internally uses the browser's native editing surface.
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
    #[prop(optional)]
    toolbar_items: Option<Vec<ToolbarItem>>,
    /// Inject content at the current cursor position.
    #[prop(optional)]
    inject: Option<RwSignal<Option<InjectCommand>>>,
    /// Map custom code-block language names to built-in highlighter languages.
    #[prop(default = vec![])]
    language_aliases: Vec<(String, String)>,
    /// Context setup for extension views. Called under each mounted extension
    /// view's reactive `Owner` before `render_code_block`, so the closure can
    /// call `provide_context(...)` to make framework-level contexts (providers,
    /// cache backends, etc.) available to the rendered components.
    #[prop(optional)]
    extension_context: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Enable drag-and-drop reordering of blocks. When true, draggable
    /// blocks show a grip handle on hover that can be grabbed to reorder.
    #[prop(default = false)]
    enable_block_drag: bool,
    /// Filter which blocks are draggable. Receives the block's
    /// `(data-pos-start, data-pos-end)` token positions. When `None` and
    /// `enable_block_drag` is true, all atomic extension blocks are
    /// draggable by default.
    #[prop(optional)]
    can_drag_block: Option<Arc<dyn Fn(usize, usize) -> bool + Send + Sync>>,
) -> impl IntoView {
    // ── State ────────────────────────────────────────────────────────────
    let atomic_langs: HashSet<String> = extensions
        .iter()
        .flat_map(|ext| ext.code_block_languages())
        .map(|s| s.to_string())
        .collect();
    let doc_state = Arc::new(Mutex::new(DocState::from_markdown_with_atoms(
        &content.get_untracked(),
        atomic_langs,
    )));

    // Reactive version counter — bumped to trigger re-render + selection restore.
    let version = RwSignal::new(0u64);
    let composing = RwSignal::new(false);
    let formatting_state = RwSignal::new(FormattingState::default());
    let extension_active_state = RwSignal::new(Vec::<(String, bool)>::new());
    let last_ext_version = std::cell::Cell::new(0u64);

    let editor_ref = NodeRef::<leptos::html::Div>::new();
    let keydown_handled = std::rc::Rc::new(std::cell::Cell::new(false));

    // ── Drag-and-drop state (Cell-based to avoid reactive re-renders) ──
    struct DragState {
        active: std::cell::Cell<bool>,
        source_start: std::cell::Cell<usize>,
        source_end: std::cell::Cell<usize>,
        target_pos: std::cell::Cell<usize>,
    }

    let drag_state = if enable_block_drag {
        Some(std::rc::Rc::new(DragState {
            active: std::cell::Cell::new(false),
            source_start: std::cell::Cell::new(0),
            source_end: std::cell::Cell::new(0),
            target_pos: std::cell::Cell::new(0),
        }))
    } else {
        None
    };

    // ── Extension data ───────────────────────────────────────────────────
    let extension_shortcuts: Arc<Vec<ExtensionKeyboardShortcut>> = Arc::new(
        extensions.iter().flat_map(|ext| ext.keyboard_shortcuts()).collect(),
    );
    let extension_toolbar_items: Vec<ExtensionToolbarItem> =
        extensions.iter().flat_map(|ext| ext.toolbar_items()).collect();
    let extensions: Arc<Vec<Arc<dyn Extension>>> = Arc::new(extensions);
    let language_aliases: Arc<Vec<(String, String)>> = Arc::new(language_aliases);

    // ── Notification helper ──────────────────────────────────────────────
    // IMPORTANT: notify must NOT lock the doc_state. The signal update
    // can synchronously trigger the render closure which also locks the
    // doc_state, causing a recursive lock panic in WASM's single-threaded mutex.
    let on_change_notify = on_change.clone();
    let notify = move |new_text: Option<String>| {
        version.update(|v| *v += 1);
        if let (Some(ref cb), Some(text)) = (&on_change_notify, new_text) {
            cb(text);
        }
    };

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
                let md = doc_inject.lock().unwrap().to_markdown();
                version.update(|v| *v += 1);
                if let Some(ref cb) = on_change_inject {
                    cb(md);
                }
                inject_signal.set(None);
            }
        });
    }

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
            };
            if needs_update {
                version.update(|v| *v += 1);
            }
        }
        new_content
    });

    // ── Segment-based content memo ─────────────────────────────────────────
    // Produces a Vec<RenderSegment> instead of a flat HTML string so the
    // Effect can patch the DOM at the block level — text blocks get their
    // innerHTML updated while extension blocks persist untouched.
    let doc_for_html = doc_state.clone();
    let extensions_for_html = Arc::clone(&extensions);
    let aliases_for_html = Arc::clone(&language_aliases);
    let drag_state_html =
        send_wrapper::SendWrapper::new(drag_state.as_ref().map(std::rc::Rc::clone));
    let segments_cache = send_wrapper::SendWrapper::new(
        std::cell::RefCell::new(Vec::<RenderSegment>::new()),
    );
    let segments_memo = Memo::new(move |_| {
        let _v = version.get();

        if let Some(ref ds) = *drag_state_html {
            if ds.active.get() {
                return segments_cache.borrow().clone();
            }
        }

        let Ok(ds) = doc_for_html.lock() else {
            return Vec::new();
        };
        for ext in extensions_for_html.iter() {
            ext.begin_render_pass();
        }
        let segs = doc_to_segments(ds.doc(), &extensions_for_html, &aliases_for_html);
        *segments_cache.borrow_mut() = segs.clone();
        segs
    });

    // Previous segments for diffing — stored outside the Memo so the
    // Effect can compare old vs new on each cycle.
    let prev_segments: send_wrapper::SendWrapper<std::cell::RefCell<Vec<RenderSegment>>> =
        send_wrapper::SendWrapper::new(std::cell::RefCell::new(Vec::new()));

    // ── Segment-based DOM patching + selection restore ──────────────────
    let doc_for_sel = doc_state.clone();
    let extensions_for_effect = Arc::clone(&extensions);
    let prev_segments_for_effect = prev_segments.clone();
    let ext_ctx_for_effect = extension_context.clone();
    let can_drag_for_effect = can_drag_block.clone();
    let keydown_handled_effect = keydown_handled.clone();
    Effect::new(move |_| {
        let _v = version.get();
        let is_composing = composing.get();

        // Compute formatting state for toolbar active buttons.
        let (fmt, ext_states, should_update_ext) = {
            let Ok(ds) = doc_for_sel.lock() else { return };
            let fmt = ds.formatting_at_cursor();
            let should_update = !extensions_for_effect.is_empty() && _v != last_ext_version.get();
            let ext_states = if should_update {
                Some(compute_extension_active_states(&ds, &fmt, &extensions_for_effect))
            } else {
                None
            };
            (fmt, ext_states, should_update)
        };
        if should_update_ext {
            last_ext_version.set(_v);
        }
        if let Some(states) = ext_states {
            extension_active_state.set(states);
        }
        formatting_state.set(fmt);

        // Don't restore selection during composition — it would interrupt IME.
        if is_composing {
            return;
        }

        // Patch the DOM synchronously using segment diff — extension blocks
        // persist across renders, only text blocks get innerHTML updates.
        #[cfg(target_arch = "wasm32")]
        {
            let new_segs = segments_memo.get();
            if let Some(container) = editor_ref.get_untracked() {
                let container_el: &web_sys::Element = container.as_ref();
                // Remove the gap cursor before patching so it doesn't
                // shift child indices and corrupt the slot matching.
                hide_gap_cursor(container_el);
                let mut old = prev_segments_for_effect.borrow_mut();
                patch_segments(
                    container_el,
                    &old,
                    &new_segs,
                    &extensions_for_effect,
                    ext_ctx_for_effect.as_deref(),
                    enable_block_drag,
                    can_drag_for_effect.as_deref(),
                );
                *old = new_segs;
            }
        }

        // Keep the variables alive for the wasm32 cfg block above.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = (&segments_memo, &prev_segments_for_effect, &ext_ctx_for_effect, &can_drag_for_effect);
        }

        let doc_raf = doc_for_sel.clone();
        let editor_raf = editor_ref;
        let kd_handled_raf = keydown_handled_effect.clone();

        let cb = Closure::once(move || {
            let Some(container) = editor_raf.get() else { return };
            let container_el: &web_sys::Element = container.as_ref();

            let Ok(ds) = doc_raf.lock() else { return };
            let sel = ds.selection().clone();
            let gap_info = ds.gap_cursor_info();
            drop(ds);

            let head = sel.head;
            let anchor = sel.anchor;

            if let Some((side, block_start, block_end)) = gap_info {
                show_gap_cursor(container_el, side, block_start, block_end);
                let _ = container_el.dyn_ref::<HtmlElement>()
                    .map(|el| el.style().set_property("caret-color", "transparent"));
                // Keep kd_handled set — selectionchange must stay suppressed
                // while the gap cursor is active, otherwise the browser's
                // stale cursor position overwrites the gap position.
            } else {
                hide_gap_cursor(container_el);
                kd_handled_raf.set(false);
                if sel.is_cursor() {
                    restore_cursor(container_el, head);
                } else {
                    restore_range(container_el, anchor, head);
                }
            }
        });
        let _ = web_sys::window()
            .and_then(|w| w.request_animation_frame(cb.as_ref().unchecked_ref()).ok());
        cb.forget();
    });

    // ── beforeinput handler ─────────────────────────────────────────────
    // Attached via addEventListener after mount because Leptos doesn't have
    // typed support for InputEvent/beforeinput.
    {
        let doc_input = doc_state.clone();
        let notify_input = notify.clone();
        let editor_input = editor_ref;

        let beforeinput_cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
            let input_ev: web_sys::InputEvent = ev.unchecked_into();

            // During IME composition, let the browser handle it.
            if composing.get_untracked() {
                return;
            }

            let input_type = input_ev.input_type();

            // Prevent the browser from modifying the DOM directly.
            input_ev.prevent_default();

            let container: Option<web_sys::Element> = editor_input
                .get()
                .map(|el| el.unchecked_ref::<web_sys::Element>().clone());

            let Ok(mut ds) = doc_input.lock() else { return };

            // Sync browser selection -> DocState before the operation.
            if let Some(ref c) = container {
                sync_selection_to_doc(&mut ds, c);
            }

            match input_type.as_str() {
                "insertText" => {
                    if let Some(data) = input_ev.data() {
                        if !data.is_empty() {
                            ds.insert_text(&data);
                        }
                    }
                }
                "insertParagraph" | "insertLineBreak" => {
                    ds.split_block();
                }
                "deleteContentBackward" | "deleteSoftLineBackward" | "deleteWordBackward" => {
                    ds.backspace();
                }
                "deleteContentForward" | "deleteSoftLineForward" | "deleteWordForward" => {
                    ds.delete_forward();
                }
                "insertFromPaste" => {
                    // Read clipboard data from the event's dataTransfer.
                    let dt = input_ev.data_transfer();
                    if let Some(dt) = dt {
                        let html_data = dt.get_data("text/html").unwrap_or_default();
                        let has_kode_md = html_data.contains("data-kode-md");
                        let plain = dt.get_data("text/plain").unwrap_or_default();

                        if has_kode_md {
                            if let Some(md) = extract_kode_markdown(&html_data) {
                                ds.insert_from_markdown(&md);
                            }
                        } else if !plain.is_empty() {
                            ds.insert_text_multiline(&plain);
                        }
                    } else if let Some(data) = input_ev.data() {
                        // Some browsers put paste data in .data() instead of dataTransfer
                        if !data.is_empty() {
                            ds.insert_text_multiline(&data);
                        }
                    }
                }
                "formatBold" => {
                    ds.toggle_mark(MarkType::Strong);
                }
                "formatItalic" => {
                    ds.toggle_mark(MarkType::Em);
                }
                "formatUnderline" => {
                    ds.toggle_mark(MarkType::Strike);
                }
                "insertFromDrop" => {
                    // Ignore drag-and-drop for now
                }
                _ => {
                    // All other input types are prevented (we already called
                    // preventDefault) — the browser won't modify the DOM.
                }
            }

            let md = ds.to_markdown();
            drop(ds);
            (notify_input)(Some(md));
        });

        // ── Copy/Cut handlers ───────────────────────────────────────────
        let doc_copy = doc_state.clone();
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
            let ev: web_sys::ClipboardEvent = ev.unchecked_into();
            if let Some(dt) = ev.clipboard_data() {
                let _ = dt.set_data("text/plain", &plain);
                let html = format!("<pre data-kode-md>{}</pre>", html_escape(&md));
                let _ = dt.set_data("text/html", &html);
            }
        });

        let doc_cut = doc_state.clone();
        let notify_cut = notify.clone();
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
            let ev: web_sys::ClipboardEvent = ev.unchecked_into();
            if let Some(dt) = ev.clipboard_data() {
                let _ = dt.set_data("text/plain", &plain);
                let html = format!("<pre data-kode-md>{}</pre>", html_escape(&md));
                let _ = dt.set_data("text/html", &html);
            }

            ds.backspace();
            let new_md = ds.to_markdown();
            drop(ds);
            (notify_cut)(Some(new_md));
        });

        // ── Paste handler (fallback for browsers without dataTransfer on beforeinput)
        let doc_paste = doc_state.clone();
        let notify_paste = notify.clone();
        let paste_cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
            // Only handle paste here if beforeinput didn't handle it.
            // Check if the event is already defaultPrevented (which means
            // beforeinput already handled it).
            if ev.default_prevented() {
                return;
            }

            ev.prevent_default();
            let ev: web_sys::ClipboardEvent = ev.unchecked_into();
            let Some(dt) = ev.clipboard_data() else { return };

            let html_data = dt.get_data("text/html").unwrap_or_default();
            let has_kode_md = html_data.contains("data-kode-md");
            let plain = dt.get_data("text/plain").unwrap_or_default();

            let Ok(mut ds) = doc_paste.lock() else { return };

            if has_kode_md {
                if let Some(md) = extract_kode_markdown(&html_data) {
                    ds.insert_from_markdown(&md);
                }
            } else if !plain.is_empty() {
                ds.insert_text_multiline(&plain);
            }

            let new_md = ds.to_markdown();
            drop(ds);
            (notify_paste)(Some(new_md));
        });

        // Store closures for attachment after mount
        let closures = std::cell::RefCell::new(Some((beforeinput_cb, copy_cb, cut_cb, paste_cb)));

        Effect::new(move |_| {
            let Some(el) = editor_input.get() else { return };
            let Some((bi_cb, cp_cb, ct_cb, pa_cb)) = closures.borrow_mut().take() else {
                return; // Already attached.
            };
            let target: &web_sys::EventTarget = el.as_ref();
            let _ = target.add_event_listener_with_callback(
                "beforeinput",
                bi_cb.as_ref().unchecked_ref(),
            );
            let _ = target.add_event_listener_with_callback(
                "copy",
                cp_cb.as_ref().unchecked_ref(),
            );
            let _ = target.add_event_listener_with_callback(
                "cut",
                ct_cb.as_ref().unchecked_ref(),
            );
            let _ = target.add_event_listener_with_callback(
                "paste",
                pa_cb.as_ref().unchecked_ref(),
            );

            let bi_fn: js_sys::Function = bi_cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
            let cp_fn: js_sys::Function = cp_cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
            let ct_fn: js_sys::Function = ct_cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
            let pa_fn: js_sys::Function = pa_cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
            let bi_wrap = send_wrapper::SendWrapper::new(bi_cb);
            let cp_wrap = send_wrapper::SendWrapper::new(cp_cb);
            let ct_wrap = send_wrapper::SendWrapper::new(ct_cb);
            let pa_wrap = send_wrapper::SendWrapper::new(pa_cb);

            let cleanup_el: web_sys::EventTarget = target.clone();
            on_cleanup(move || {
                let _ = cleanup_el.remove_event_listener_with_callback("beforeinput", &bi_fn);
                let _ = cleanup_el.remove_event_listener_with_callback("copy", &cp_fn);
                let _ = cleanup_el.remove_event_listener_with_callback("cut", &ct_fn);
                let _ = cleanup_el.remove_event_listener_with_callback("paste", &pa_fn);
                drop(bi_wrap);
                drop(cp_wrap);
                drop(ct_wrap);
                drop(pa_wrap);
            });
        });
    }

    // ── Drag-and-drop pointer event handlers ────────────────────────────
    if let Some(ref drag) = drag_state {
        let drag_down = std::rc::Rc::clone(drag);
        let pointerdown_cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
            let ev: web_sys::PointerEvent = ev.unchecked_into();

            // Only handle if the target is a drag handle.
            let Some(target) = ev.target() else { return };
            let Ok(target_el) = target.dyn_into::<web_sys::Element>() else { return };
            if !target_el.class_list().contains("kode-drag-handle") {
                return;
            }

            // Read block positions from the handle's data attributes.
            let start: usize = target_el.get_attribute("data-block-start")
                .and_then(|s| s.parse().ok()).unwrap_or(0);
            let end: usize = target_el.get_attribute("data-block-end")
                .and_then(|s| s.parse().ok()).unwrap_or(0);

            if start == 0 && end == 0 { return; }

            drag_down.active.set(true);
            drag_down.source_start.set(start);
            drag_down.source_end.set(end);
            drag_down.target_pos.set(start);

            // Capture pointer on the handle element.
            let _ = target_el.set_pointer_capture(ev.pointer_id());

            ev.prevent_default();
            ev.stop_propagation();

            // Add dragging class to body for user-select:none.
            if let Some(body) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.body()) {
                let _ = body.class_list().add_1("kode-dragging");
            }

            // Highlight just the dragged block (not the whole grid group).
            // In grid layouts this shows which specific chart is being moved.
            if let Some(block) = target_el.parent_element() {
                let _ = block.class_list().add_1("kode-block-dragging");
            }
        });

        let drag_move = std::rc::Rc::clone(drag);
        let editor_move = editor_ref;
        let pointermove_cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
            if !drag_move.active.get() { return; }

            let ev: web_sys::PointerEvent = ev.unchecked_into();
            let client_y = ev.client_y() as f64;

            let Some(container) = editor_move.get() else { return };
            let container_el: &web_sys::Element = container.as_ref();

            // Find all top-level blocks and determine which gap the pointer is in.
            let Ok(blocks) = container_el.query_selector_all("[data-pos-start]") else { return };

            // Remove any existing drop indicator.
            if let Ok(Some(old)) = container_el.query_selector(".kode-drop-indicator") {
                old.remove();
            }

            let mut best_target_pos = 0usize;
            let mut best_insert_el: Option<web_sys::Element> = None;
            let mut best_insert_before = true;
            let mut best_dist = f64::MAX;
            let source_start = drag_move.source_start.get();
            let source_end = drag_move.source_end.get();

            for i in 0..blocks.length() {
                let Some(node) = blocks.item(i) else { continue };
                let Ok(el) = node.dyn_into::<web_sys::Element>() else { continue };

                // Find the top-level ancestor of this block (direct child of container).
                // Blocks may be nested inside grid wrappers (.kode-block-grid > .kode-grid-item).
                let top_level = find_top_level_ancestor(&el, container_el);
                let Some(top_el) = top_level else { continue };

                let rect = el.get_bounding_client_rect();
                let raw_start: usize = el.get_attribute("data-pos-start")
                    .and_then(|s| s.parse().ok()).unwrap_or(0);
                let raw_end: usize = el.get_attribute("data-pos-end")
                    .and_then(|s| s.parse().ok()).unwrap_or(0);

                // Extension blocks and leaf nodes (HR) store block-level
                // positions; branch blocks (paragraphs, headings, etc.) store
                // content positions (1 token inside). Adjust content-positioned
                // blocks to block boundaries so the insert lands between blocks.
                let uses_block_positions = el.has_attribute("data-kode-extension")
                    || el.tag_name().eq_ignore_ascii_case("hr");
                let block_start = if uses_block_positions { raw_start } else { raw_start.saturating_sub(1) };
                let block_end = if uses_block_positions { raw_end } else { raw_end + 1 };

                // When the block is inside a grid wrapper, the indicator is placed
                // at the grid boundary, so target_pos must match: use the first
                // block's start or last block's end within the grid group.
                let (group_start, group_end) = if top_el != el {
                    resolve_grid_group_positions(&top_el)
                        .unwrap_or((block_start, block_end))
                } else {
                    (block_start, block_end)
                };

                let top_dist = (client_y - rect.top()).abs();
                if top_dist < best_dist {
                    best_dist = top_dist;
                    best_target_pos = group_start;
                    best_insert_el = Some(top_el.clone());
                    best_insert_before = true;
                }

                let bottom_dist = (client_y - rect.bottom()).abs();
                if bottom_dist < best_dist {
                    best_dist = bottom_dist;
                    best_target_pos = group_end;
                    best_insert_el = Some(top_el);
                    best_insert_before = false;
                }
            }

            // Don't show indicator if target is within the source block.
            if best_target_pos >= source_start && best_target_pos <= source_end {
                drag_move.target_pos.set(source_start);
                return;
            }

            drag_move.target_pos.set(best_target_pos);

            // Create and insert the drop indicator.
            if let Some(ref insert_el) = best_insert_el {
                let doc = web_sys::window().and_then(|w| w.document());
                if let Some(doc) = doc {
                    if let Ok(indicator) = doc.create_element("div") {
                        let _ = indicator.set_attribute("class", "kode-drop-indicator");
                        let _ = indicator.set_attribute("contenteditable", "false");
                        if best_insert_before {
                            let _ = container_el.insert_before(&indicator, Some(insert_el));
                        } else if let Some(next) = insert_el.next_element_sibling() {
                            let _ = container_el.insert_before(&indicator, Some(&next));
                        } else {
                            let _ = container_el.append_child(&indicator);
                        }
                    }
                }
            }

            // Auto-scroll near edges.
            let container_rect = container_el.get_bounding_client_rect();
            let scroll_zone = 40.0;
            let scroll_speed = 8.0;
            if client_y - container_rect.top() < scroll_zone {
                container_el.set_scroll_top(container_el.scroll_top() - scroll_speed as i32);
            } else if container_rect.bottom() - client_y < scroll_zone {
                container_el.set_scroll_top(container_el.scroll_top() + scroll_speed as i32);
            }
        });

        let drag_up = std::rc::Rc::clone(drag);
        let doc_up = doc_state.clone();
        let notify_up = notify.clone();
        let editor_up = editor_ref;
        let pointerup_cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
            let _ev: web_sys::PointerEvent = ev.unchecked_into();
            if !drag_up.active.get() { return; }

            drag_up.active.set(false);

            // Remove dragging classes.
            if let Some(body) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.body()) {
                let _ = body.class_list().remove_1("kode-dragging");
            }

            let Some(container) = editor_up.get() else { return };
            let container_el: &web_sys::Element = container.as_ref();

            // Remove drop indicator.
            if let Ok(Some(indicator)) = container_el.query_selector(".kode-drop-indicator") {
                indicator.remove();
            }

            // Remove block-dragging class from all blocks.
            if let Ok(dragging_blocks) = container_el.query_selector_all(".kode-block-dragging") {
                for i in 0..dragging_blocks.length() {
                    if let Some(node) = dragging_blocks.item(i) {
                        let el: web_sys::Element = node.unchecked_into();
                        let _ = el.class_list().remove_1("kode-block-dragging");
                    }
                }
            }

            let source_start = drag_up.source_start.get();
            let source_end = drag_up.source_end.get();
            let target = drag_up.target_pos.get();

            // Don't move if target is same as source.
            if target >= source_start && target <= source_end {
                return;
            }

            // FLIP animation: snapshot extension block positions before move.
            let old_rects: Vec<(String, web_sys::DomRect)> = {
                let mut rects = Vec::new();
                if let Ok(all_blocks) = container_el.query_selector_all("div.kode-extension-block[data-kode-extension]") {
                    for i in 0..all_blocks.length() {
                        if let Some(node) = all_blocks.item(i) {
                            let el: web_sys::Element = node.unchecked_into();
                            let text = el.text_content().unwrap_or_default();
                            // Use first 50 chars of text content as a fingerprint
                            // to match blocks across renders.
                            let key: String = text.chars().take(50).collect();
                            rects.push((key, el.get_bounding_client_rect()));
                        }
                    }
                }
                rects
            };

            // Commit the move.
            let Ok(mut ds) = doc_up.lock() else { return };
            ds.move_block(source_start, source_end, target);
            let md = ds.to_markdown();
            drop(ds);
            (notify_up)(Some(md));

            // Schedule FLIP animation after the DOM rebuilds.
            let old_rects_js = send_wrapper::SendWrapper::new(old_rects);
            let editor_flip = editor_up;
            let flip_cb = Closure::once(move || {
                let Some(container) = editor_flip.get() else { return };
                let container_el: &web_sys::Element = container.as_ref();

                let Ok(new_blocks) = container_el.query_selector_all("div.kode-extension-block[data-kode-extension]") else { return };

                for i in 0..new_blocks.length() {
                    let Some(node) = new_blocks.item(i) else { continue };
                    let el: web_sys::Element = node.unchecked_into();
                    let text = el.text_content().unwrap_or_default();
                    let key: String = text.chars().take(50).collect();
                    let new_rect = el.get_bounding_client_rect();

                    // Find the matching old rect by content fingerprint.
                    if let Some((_, old_rect)) = old_rects_js.iter().find(|(k, _)| k == &key) {
                        let dx = old_rect.left() - new_rect.left();
                        let dy = old_rect.top() - new_rect.top();

                        // Skip if the block didn't move.
                        if dx.abs() < 1.0 && dy.abs() < 1.0 { continue; }

                        let html_el: &web_sys::HtmlElement = el.unchecked_ref();
                        let style = html_el.style();

                        // Invert: suppress transition first, then snap to old position.
                        let _ = style.set_property("transition", "none");
                        let _ = style.set_property("transform", &format!("translate({dx}px, {dy}px)"));

                        // Play: animate to new position on next frame.
                        let el_clone = el.clone();
                        let play_cb = Closure::once(move || {
                            let html_el: &web_sys::HtmlElement = el_clone.unchecked_ref();
                            let style = html_el.style();
                            let _ = style.set_property("transition", "transform 200ms ease-out");
                            let _ = style.set_property("transform", "");

                            // Clean up after animation completes.
                            let el_cleanup = el_clone.clone();
                            let cleanup = Closure::once(move || {
                                let html_el: &web_sys::HtmlElement = el_cleanup.unchecked_ref();
                                let _ = html_el.style().remove_property("transition");
                                let _ = html_el.style().remove_property("transform");
                            });
                            let _ = web_sys::window().and_then(|w| {
                                w.set_timeout_with_callback_and_timeout_and_arguments_0(
                                    cleanup.as_ref().unchecked_ref(), 250
                                ).ok()
                            });
                            cleanup.forget(); // one-shot timeout
                        });
                        let _ = web_sys::window().and_then(|w| {
                            w.request_animation_frame(play_cb.as_ref().unchecked_ref()).ok()
                        });
                        play_cb.forget(); // one-shot rAF
                    }
                }
            });
            let _ = web_sys::window().and_then(|w| {
                w.request_animation_frame(flip_cb.as_ref().unchecked_ref()).ok()
            });
            flip_cb.forget(); // one-shot rAF
        });

        // pointercancel: clean up drag state without committing (touch/stylus cancel, OS gesture).
        let drag_cancel = std::rc::Rc::clone(drag);
        let editor_cancel = editor_ref;
        let pointercancel_cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_ev: web_sys::Event| {
            if !drag_cancel.active.get() { return; }

            drag_cancel.active.set(false);

            if let Some(body) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.body()) {
                let _ = body.class_list().remove_1("kode-dragging");
            }

            let Some(container) = editor_cancel.get() else { return };
            let container_el: &web_sys::Element = container.as_ref();

            if let Ok(Some(indicator)) = container_el.query_selector(".kode-drop-indicator") {
                indicator.remove();
            }

            if let Ok(dragging_blocks) = container_el.query_selector_all(".kode-block-dragging") {
                for i in 0..dragging_blocks.length() {
                    if let Some(node) = dragging_blocks.item(i) {
                        let el: web_sys::Element = node.unchecked_into();
                        let _ = el.class_list().remove_1("kode-block-dragging");
                    }
                }
            }
        });

        // Attach pointer event listeners using the same pattern as beforeinput/copy/cut/paste.
        let drag_closures = std::cell::RefCell::new(Some((pointerdown_cb, pointermove_cb, pointerup_cb, pointercancel_cb)));
        let editor_attach = editor_ref;

        Effect::new(move |_| {
            let Some(el) = editor_attach.get() else { return };
            let Some((pd, pm, pu, pc)) = drag_closures.borrow_mut().take() else {
                return; // Already attached.
            };

            let target: &web_sys::EventTarget = el.as_ref();
            let _ = target.add_event_listener_with_callback("pointerdown", pd.as_ref().unchecked_ref());
            let _ = target.add_event_listener_with_callback("pointermove", pm.as_ref().unchecked_ref());
            let _ = target.add_event_listener_with_callback("pointerup", pu.as_ref().unchecked_ref());
            let _ = target.add_event_listener_with_callback("pointercancel", pc.as_ref().unchecked_ref());

            let pd_fn: js_sys::Function = pd.as_ref().unchecked_ref::<js_sys::Function>().clone();
            let pm_fn: js_sys::Function = pm.as_ref().unchecked_ref::<js_sys::Function>().clone();
            let pu_fn: js_sys::Function = pu.as_ref().unchecked_ref::<js_sys::Function>().clone();
            let pc_fn: js_sys::Function = pc.as_ref().unchecked_ref::<js_sys::Function>().clone();
            let pd_wrap = send_wrapper::SendWrapper::new(pd);
            let pm_wrap = send_wrapper::SendWrapper::new(pm);
            let pu_wrap = send_wrapper::SendWrapper::new(pu);
            let pc_wrap = send_wrapper::SendWrapper::new(pc);

            let cleanup_el: web_sys::EventTarget = target.clone();
            on_cleanup(move || {
                let _ = cleanup_el.remove_event_listener_with_callback("pointerdown", &pd_fn);
                let _ = cleanup_el.remove_event_listener_with_callback("pointermove", &pm_fn);
                let _ = cleanup_el.remove_event_listener_with_callback("pointerup", &pu_fn);
                let _ = cleanup_el.remove_event_listener_with_callback("pointercancel", &pc_fn);
                drop(pd_wrap);
                drop(pm_wrap);
                drop(pu_wrap);
                drop(pc_wrap);
            });
        });
    }

    // ── Keydown ──────────────────────────────────────────────────────────
    let doc_key = doc_state.clone();
    let notify_key = notify.clone();
    let extension_shortcuts_key: Arc<Vec<ExtensionKeyboardShortcut>> = Arc::clone(&extension_shortcuts);
    let keydown_handled_key = keydown_handled.clone();
    let on_keydown = move |ev: KeyboardEvent| {
        if composing.get_untracked() {
            return;
        }

        let ctrl = ev.ctrl_key() || ev.meta_key();
        let shift = ev.shift_key();
        let key = ev.key();

        // Clipboard shortcuts: let copy/cut/paste events handle these.
        match key.as_str() {
            "c" | "x" | "v" if ctrl => return,
            _ => {}
        }

        // Undo/Redo
        match key.as_str() {
            "z" if ctrl && shift => {
                // Sync selection from browser before redo
                if let Some(container) = editor_ref.get() {
                    let container_el: &web_sys::Element = container.as_ref();
                    let Ok(mut ds) = doc_key.lock() else { return };
                    sync_selection_to_doc(&mut ds, container_el);
                    ds.redo();
                    let md = ds.to_markdown();
                    drop(ds);
                    (notify_key)(Some(md));
                }
                ev.prevent_default();
                return;
            }
            "z" if ctrl => {
                if let Some(container) = editor_ref.get() {
                    let container_el: &web_sys::Element = container.as_ref();
                    let Ok(mut ds) = doc_key.lock() else { return };
                    sync_selection_to_doc(&mut ds, container_el);
                    ds.undo();
                    let md = ds.to_markdown();
                    drop(ds);
                    (notify_key)(Some(md));
                }
                ev.prevent_default();
                return;
            }
            _ => {}
        }

        let Ok(mut ds) = doc_key.lock() else { return };

        // The DocState position is authoritative for all keyboard
        // operations. It's kept in sync by selectionchange (mouse) and
        // beforeinput (text input). No browser sync needed here.
        let handled = match key.as_str() {
            "ArrowRight" if !ctrl && !shift => {
                ds.move_right();
                true
            }
            "ArrowLeft" if !ctrl && !shift => {
                ds.move_left();
                true
            }
            "ArrowDown" if !ctrl && !shift => {
                if ds.gap_cursor_info().is_some() {
                    ds.move_right();
                    true
                } else {
                    // Check if the next block after the current textblock is
                    // atomic. If so, move to the gap before it rather than
                    // letting the browser skip past it.
                    let pos = ds.selection().head;
                    let resolved = ds.doc().resolve(pos);
                    if resolved.parent().node_type.is_textblock() && resolved.depth > 0 {
                        let after_pos = resolved.after(resolved.depth);
                        let after_resolved = ds.doc().resolve(after_pos.min(ds.doc().content.size()));
                        if after_resolved.node_after().is_some_and(|n| n.is_atom()) {
                            ds.set_selection_raw(Selection::cursor(after_pos));
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
            }
            "ArrowUp" if !ctrl && !shift => {
                if ds.gap_cursor_info().is_some() {
                    ds.move_left();
                    true
                } else {
                    let pos = ds.selection().head;
                    let resolved = ds.doc().resolve(pos);
                    if resolved.parent().node_type.is_textblock() && resolved.depth > 0 {
                        let before_pos = resolved.before(resolved.depth);
                        if before_pos > 0 {
                            let before_resolved = ds.doc().resolve(before_pos);
                            if before_resolved.node_before().is_some_and(|n| n.is_atom()) {
                                ds.set_selection_raw(Selection::cursor(before_pos));
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
            }
            "Enter" if !ctrl => {
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
            _ => false,
        };

        let handled = if !handled {
            match key.as_str() {
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
            "a" if ctrl => {
                let doc_size = ds.doc().content.size();
                if doc_size > 0 {
                    ds.set_selection(Selection::range(0, doc_size));
                }
                true
            }
            "Tab" if !ctrl => {
                let in_table = {
                    let resolved = ds.doc().resolve(ds.selection().head);
                    (0..=resolved.depth).rev().any(|d| {
                        resolved.node(d).node_type == kode_doc::NodeType::TableCell
                    })
                };
                if in_table {
                    if shift {
                        ds.move_to_prev_cell()
                    } else if !ds.move_to_next_cell() {
                        ds.insert_row_below();
                        true
                    } else {
                        true
                    }
                } else {
                    let md = ds.to_markdown();
                    let cursor_byte = kode_doc::tree_pos_to_byte_offset(ds.doc(), &md, ds.selection().head);
                    let mut temp = kode_markdown::MarkdownEditor::new(&md);
                    let char_idx = temp.buffer().byte_to_char(cursor_byte);
                    let pos = temp.buffer().char_to_pos(char_idx);
                    temp.set_cursor(pos);

                    let applied = if shift {
                        kode_markdown::InputRules::handle_shift_tab(temp.editor_mut())
                    } else {
                        kode_markdown::InputRules::handle_tab(temp.editor_mut())
                    };

                    if applied {
                        temp.sync_tree();
                        let new_md = temp.text();
                        let new_cursor = temp.cursor();
                        let new_char_idx = temp.buffer().pos_to_char(new_cursor);
                        let new_byte = temp.buffer().char_to_byte(new_char_idx);
                        ds.set_from_markdown(&new_md);
                        let new_tree_pos = kode_doc::byte_offset_to_tree_pos(ds.doc(), &new_md, new_byte);
                        ds.set_selection(Selection::cursor(new_tree_pos));
                    }
                    applied
                }
            }
            _ => false,
            }
        } else {
            true
        };

        // Extension keyboard shortcuts (checked after built-in shortcuts).
        let handled = if !handled {
            let mut ext_handled = false;
            for shortcut in extension_shortcuts_key.iter() {
                if matches_key_descriptor(&ev, &shortcut.key) {
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
            keydown_handled_key.set(true);
            let md = ds.to_markdown();
            drop(ds);
            (notify_key)(Some(md));
            ev.prevent_default();
        } else {
            drop(ds);
        }
    };

    // ── Composition events (IME) ─────────────────────────────────────────
    let on_composition_start = move |_: CompositionEvent| {
        composing.set(true);
    };
    let doc_comp_end = doc_state.clone();
    let notify_comp = notify.clone();
    let on_composition_end = move |ev: CompositionEvent| {
        composing.set(false);
        if let Some(data) = ev.data() {
            if !data.is_empty() {
                let Ok(mut ds) = doc_comp_end.lock() else { return };
                // Sync selection from browser to know where to insert the composed text.
                if let Some(container) = editor_ref.get() {
                    let container_el: &web_sys::Element = container.as_ref();
                    sync_selection_to_doc(&mut ds, container_el);
                }
                ds.insert_text(&data);
                let md = ds.to_markdown();
                drop(ds);
                (notify_comp)(Some(md));
            }
        }
    };

    // ── selectionchange: sync browser selection to DocState for toolbar ──
    {
        let doc_selchange = doc_state.clone();
        let editor_selchange = editor_ref;
        let extensions_for_effectchange = Arc::clone(&extensions);
        let kd_handled_selchange = keydown_handled.clone();
        let selchange_cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_ev: web_sys::Event| {
            if composing.get_untracked() {
                return;
            }
            // After a keydown edit, the DocState position is authoritative.
            // Skip sync until the rAF restores the cursor.
            if kd_handled_selchange.get() {
                return;
            }
            let Some(container) = editor_selchange.get() else { return };
            let container_el: &web_sys::Element = container.as_ref();

            // Only sync if the selection is inside our editor.
            let Some(window) = web_sys::window() else { return };
            let Some(sel) = window.get_selection().ok().flatten() else { return };
            let Some(focus_node) = sel.focus_node() else { return };
            if !container_el.contains(Some(&focus_node)) {
                return;
            }

            let Ok(mut ds) = doc_selchange.lock() else { return };
            sync_selection_to_doc(&mut ds, container_el);

            if let Some((side, block_start, block_end)) = ds.gap_cursor_info() {
                let fmt = ds.formatting_at_cursor();
                let ext_states = if !extensions_for_effectchange.is_empty() {
                    Some(compute_extension_active_states(&ds, &fmt, &extensions_for_effectchange))
                } else {
                    None
                };
                drop(ds);
                show_gap_cursor(container_el, side, block_start, block_end);
                if let Some(states) = ext_states {
                    extension_active_state.set(states);
                }
                formatting_state.set(fmt);
                return;
            }

            hide_gap_cursor(container_el);
            let fmt = ds.formatting_at_cursor();

            let ext_states = if !extensions_for_effectchange.is_empty() {
                Some(compute_extension_active_states(&ds, &fmt, &extensions_for_effectchange))
            } else {
                None
            };
            drop(ds);

            if let Some(states) = ext_states {
                extension_active_state.set(states);
            }
            formatting_state.set(fmt);
        });

        // Attach selectionchange to the document (it doesn't fire on individual elements).
        if let Some(document) = web_sys::window().and_then(|w| w.document()) {
            let _ = document.add_event_listener_with_callback(
                "selectionchange",
                selchange_cb.as_ref().unchecked_ref(),
            );
        }

        let selchange_fn: js_sys::Function = selchange_cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
        let selchange_cb = send_wrapper::SendWrapper::new(selchange_cb);
        on_cleanup(move || {
            if let Some(document) = web_sys::window().and_then(|w| w.document()) {
                let _ = document.remove_event_listener_with_callback(
                    "selectionchange",
                    &selchange_fn,
                );
            }
            drop(selchange_cb);
        });
    }

    // ── Toolbar ───────────────────────────────────────────────────────────
    let toolbar_view = if show_toolbar {
        let items = toolbar_items.unwrap_or_else(|| {
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
                    let notify_tb = notify.clone();
                    let editor_ref_tb = editor_ref;
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
                        (notify_tb)(Some(md));
                        // Re-focus the contenteditable div.
                        if let Some(el) = editor_ref_tb.get() {
                            let el: &HtmlElement = el.as_ref();
                            let _ = el.focus();
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
                    let notify_tb = notify.clone();
                    let editor_ref_tb = editor_ref;
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
                        (notify_tb)(Some(md));
                        if let Some(el) = editor_ref_tb.get() {
                            let el: &HtmlElement = el.as_ref();
                            let _ = el.focus();
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
                    let notify_ext_tb = notify.clone();
                    let editor_ref_tb = editor_ref;
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
                            (notify_ext_tb)(Some(md_out));
                        } else {
                            drop(ds);
                        }
                        if let Some(el) = editor_ref_tb.get() {
                            let el: &HtmlElement = el.as_ref();
                            let _ = el.focus();
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

    // ── Mousedown on atomic blocks → gap cursor ──────────────────────────
    let doc_md = doc_state.clone();
    let on_mousedown = move |ev: MouseEvent| {
        let Some(target) = ev.target() else { return };
        let Some(target_el) = target.dyn_ref::<web_sys::Element>() else { return };

        // Walk up from the click target to find an atomic extension block.
        let mut el = Some(target_el.clone());
        let mut ext_el: Option<web_sys::Element> = None;
        while let Some(ref current) = el {
            if current.has_attribute("data-kode-extension") {
                ext_el = Some(current.clone());
                break;
            }
            if current.has_attribute("contenteditable")
                && current.get_attribute("contenteditable").as_deref() == Some("true")
            {
                break;
            }
            el = current.parent_element();
        }

        let Some(block) = ext_el else { return };

        ev.prevent_default();

        let Some(ps) = block.get_attribute("data-pos-start") else { return };
        let Some(pe) = block.get_attribute("data-pos-end") else { return };
        let Ok(pos_start) = ps.parse::<usize>() else { return };
        let Ok(pos_end) = pe.parse::<usize>() else { return };

        // Before or after? Compare click Y to block center.
        let rect = block.get_bounding_client_rect();
        let mid_y = rect.top() + rect.height() / 2.0;
        let gap_pos = if (ev.client_y() as f64) < mid_y {
            pos_start
        } else {
            pos_end
        };

        let Ok(mut ds) = doc_md.lock() else { return };
        ds.set_selection_raw(Selection::cursor(gap_pos));

        if let Some((side, bs, be)) = ds.gap_cursor_info() {
            drop(ds);
            if let Some(container) = editor_ref.get() {
                let container_el: &web_sys::Element = container.as_ref();
                show_gap_cursor(container_el, side, bs, be);
                let _ = container.dyn_ref::<HtmlElement>().map(|el| {
                    let _ = el.style().set_property("caret-color", "transparent");
                    let _ = el.focus();
                });
            }
        } else {
            drop(ds);
            if let Some(container) = editor_ref.get() {
                let _ = container.dyn_ref::<HtmlElement>().map(|el| el.focus());
            }
        }
        version.update(|v| *v += 1);
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
            }>

            {toolbar_view}

            <div
                node_ref=editor_ref
                class="wysiwyg-scroll-container tree-wysiwyg-scroll-container"
                contenteditable="true"
                spellcheck="false"
                on:keydown=on_keydown
                on:mousedown=on_mousedown
                on:compositionstart=on_composition_start
                on:compositionend=on_composition_end
            />
        </div>
    }
}

// ── Selection mapping: Browser → DocState ──────────────────────────────────

/// Map the browser's current Selection to a DocState position and update
/// the DocState's selection accordingly.
fn sync_selection_to_doc(ds: &mut DocState, container: &web_sys::Element) {
    let Some(window) = web_sys::window() else { return };
    let Some(sel) = window.get_selection().ok().flatten() else { return };

    let focus_pos = match sel.focus_node() {
        Some(ref focus_node) => {
            if !container.contains(Some(focus_node)) {
                return;
            }
            node_offset_to_doc_pos(container, focus_node, sel.focus_offset())
        }
        None => return,
    };

    let Some(head) = focus_pos else { return };

    let head = ds.snap_out_of_atom(head);

    if sel.is_collapsed() {
        ds.set_selection(Selection::cursor(head));
    } else if let Some(anchor_node) = sel.anchor_node() {
        let anchor = node_offset_to_doc_pos(container, &anchor_node, sel.anchor_offset())
            .unwrap_or(head);
        let anchor = ds.snap_out_of_atom(anchor);
        ds.set_selection(Selection::range(anchor, head));
    } else {
        ds.set_selection(Selection::cursor(head));
    }
}

/// Convert a DOM (node, offset) pair to a DocState position.
///
/// Walks up from the node to find the nearest ancestor with a `data-pos-start`
/// attribute, then counts characters from that element's start to the given
/// (node, offset) point.
fn node_offset_to_doc_pos(
    container: &web_sys::Element,
    node: &web_sys::Node,
    offset: u32,
) -> Option<usize> {
    // Find the nearest ancestor (or self) with data-pos-start.
    let pos_el = find_pos_ancestor(node, container)?;
    let el_start: usize = pos_el.get_attribute("data-pos-start")?.parse().ok()?;

    // Count characters from the element start to (node, offset).
    let char_offset = count_chars_to_point(&pos_el, node, offset);

    // Clamp to the element's end position.
    let el_end: usize = pos_el
        .get_attribute("data-pos-end")
        .and_then(|s| s.parse().ok())
        .unwrap_or(usize::MAX);

    Some((el_start + char_offset).min(el_end))
}

/// Walk up from a node to find the nearest Element ancestor (or self) that
/// has a `data-pos-start` attribute.
fn find_pos_ancestor(node: &web_sys::Node, container: &web_sys::Element) -> Option<web_sys::Element> {
    let mut current: Option<web_sys::Node> = Some(node.clone());
    while let Some(ref n) = current {
        if let Ok(el) = n.clone().dyn_into::<web_sys::Element>() {
            if el.has_attribute("data-pos-start") {
                return Some(el);
            }
            // Stop at the container boundary.
            if &el == container {
                return None;
            }
        }
        current = n.parent_node();
    }
    None
}

/// Count text characters from the start of `root_el` to the point
/// specified by `(target_node, target_offset)` using a TreeWalker.
fn count_chars_to_point(
    root_el: &web_sys::Element,
    target_node: &web_sys::Node,
    target_offset: u32,
) -> usize {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return 0;
    };

    // Walk ALL nodes (text + elements) to correctly account for <br> elements
    // which occupy 1 document position but contain 0 text characters.
    let Ok(walker) = document.create_tree_walker_with_what_to_show(
        root_el,
        0x5, // SHOW_ELEMENT (0x1) | SHOW_TEXT (0x4)
    ) else {
        return 0;
    };

    let mut count = 0usize;

    if walker.next_node().ok().flatten().is_none() {
        // No child nodes at all.
    } else {
        loop {
            let node = walker.current_node();

            if node == *target_node {
                // For text nodes, add the char offset within the node.
                if node.node_type() == web_sys::Node::TEXT_NODE {
                    if let Some(text) = node.text_content() {
                        let char_offset = utf16_offset_to_char_count(&text, target_offset as usize);
                        count += char_offset;
                    }
                }
                return count;
            }

            if node.node_type() == web_sys::Node::TEXT_NODE {
                if let Some(text) = node.text_content() {
                    count += text.chars().count();
                }
            } else if let Some(el) = node.dyn_ref::<web_sys::Element>() {
                if el.tag_name() == "BR" && !el.has_attribute("data-guard") {
                    count += 1;
                }
            }

            if walker.next_node().ok().flatten().is_none() {
                break;
            }
        }
    }

    // If target_node is the element itself (not a text node), the offset
    // refers to child node index. Walk children to count chars up to that index.
    if target_node == root_el.unchecked_ref::<web_sys::Node>() {
        return count_chars_up_to_child_index(root_el, target_offset as usize);
    }

    count
}

/// When Selection points to an element node with offset = child index,
/// count document positions from the start of the element to the nth child.
/// Text nodes contribute their character count; `<br>` elements contribute 1.
fn count_chars_up_to_child_index(el: &web_sys::Element, child_index: usize) -> usize {
    let children = el.child_nodes();
    let mut count = 0;
    for i in 0..child_index.min(children.length() as usize) {
        if let Some(child) = children.get(i as u32) {
            if child.node_type() == web_sys::Node::ELEMENT_NODE {
                if let Some(child_el) = child.dyn_ref::<web_sys::Element>() {
                    if child_el.tag_name() == "BR" {
                        if !child_el.has_attribute("data-guard") {
                            count += 1;
                        }
                        continue;
                    }
                }
            }
            if let Some(text) = child.text_content() {
                count += text.chars().count();
            }
        }
    }
    count
}

/// Convert a UTF-16 code unit offset to a Rust `char` count.
fn utf16_offset_to_char_count(text: &str, utf16_offset: usize) -> usize {
    let mut utf16_pos = 0;
    let mut char_count = 0;
    for ch in text.chars() {
        if utf16_pos >= utf16_offset {
            break;
        }
        utf16_pos += ch.len_utf16();
        char_count += 1;
    }
    char_count
}

// ── Selection restore: DocState → Browser ──────────────────────────────────

/// Restore a collapsed cursor (no range) after re-render.
fn restore_cursor(container: &web_sys::Element, doc_pos: usize) {
    let Some(window) = web_sys::window() else { return };
    let Some(sel) = window.get_selection().ok().flatten() else { return };

    if let Some((node, offset)) = find_text_point(container, doc_pos) {
        let _ = sel.collapse_with_offset(Some(&node), offset as u32);
    }
}

/// Restore a range selection after re-render.
fn restore_range(container: &web_sys::Element, anchor_pos: usize, head_pos: usize) {
    let Some(window) = web_sys::window() else { return };
    let Some(sel) = window.get_selection().ok().flatten() else { return };

    let anchor = find_text_point(container, anchor_pos);
    let head = find_text_point(container, head_pos);

    match (anchor, head) {
        (Some((a_node, a_off)), Some((h_node, h_off))) => {
            let _ = sel.set_base_and_extent(
                &a_node, a_off as u32,
                &h_node, h_off as u32,
            );
        }
        (None, Some((h_node, h_off))) => {
            let _ = sel.collapse_with_offset(Some(&h_node), h_off as u32);
        }
        (Some((a_node, a_off)), None) => {
            let _ = sel.collapse_with_offset(Some(&a_node), a_off as u32);
        }
        (None, None) => {}
    }
}

/// Find the DOM text node and UTF-16 offset for a given DocState position.
///
/// Searches for the element whose `data-pos-start`/`data-pos-end` range
/// contains the target position, then walks text nodes to find the exact
/// point.
fn find_text_point(container: &web_sys::Element, doc_pos: usize) -> Option<(web_sys::Node, usize)> {
    // Find the deepest positioned element containing doc_pos.
    let target_el = find_element_for_pos_html(container, doc_pos)?;
    let el_start: usize = target_el.get_attribute("data-pos-start")?.parse().ok()?;
    let target_char_offset = doc_pos.saturating_sub(el_start);

    let document = web_sys::window().and_then(|w| w.document())?;

    // Walk ALL child nodes (text + elements) to account for <br> elements
    // which occupy 1 document position but contain 0 text characters.
    let Ok(walker) = document.create_tree_walker_with_what_to_show(
        &target_el,
        0x5, // SHOW_ELEMENT (0x1) | SHOW_TEXT (0x4)
    ) else {
        return None;
    };

    let mut chars_remaining = target_char_offset;

    if walker.next_node().ok().flatten().is_none() {
        return Some((target_el.unchecked_into::<web_sys::Node>(), 0));
    }

    loop {
        let node = walker.current_node();

        if node.node_type() == web_sys::Node::TEXT_NODE {
            if let Some(text) = node.text_content() {
                let text_char_count = text.chars().count();
                if chars_remaining <= text_char_count {
                    let utf16_offset = char_count_to_utf16_offset(&text, chars_remaining);
                    return Some((node, utf16_offset));
                }
                chars_remaining -= text_char_count;
            }
        } else if let Some(el) = node.dyn_ref::<web_sys::Element>() {
            if el.tag_name() == "BR" && !el.has_attribute("data-guard") {
                if chars_remaining == 0 {
                    // Cursor is before this <br>.
                    if let Some(parent) = el.parent_node() {
                        let children = parent.child_nodes();
                        for i in 0..children.length() {
                            if children.get(i) == Some(node.clone()) {
                                return Some((parent, i as usize));
                            }
                        }
                    }
                } else if chars_remaining == 1 {
                    // Cursor is right after this <br>.
                    if let Some(parent) = el.parent_node() {
                        let children = parent.child_nodes();
                        for i in 0..children.length() {
                            if children.get(i) == Some(node.clone()) {
                                return Some((parent, (i + 1) as usize));
                            }
                        }
                    }
                }
                chars_remaining = chars_remaining.saturating_sub(1);
            }
        }

        if walker.next_node().ok().flatten().is_none() {
            break;
        }
    }

    // If we exhausted all text nodes, position at the end of the last one.
    // Re-walk to find the last text node.
    let Ok(walker) = document.create_tree_walker_with_what_to_show(
        &target_el,
        0x4, // NodeFilter.SHOW_TEXT
    ) else {
        return None;
    };

    let mut last_node: Option<web_sys::Node> = None;
    let mut last_len = 0usize;

    // Advance past the root element to the first text node (same reason as above).
    if walker.next_node().ok().flatten().is_some() {
        loop {
            let node = walker.current_node();
            if let Some(text) = node.text_content() {
                last_len = text.encode_utf16().count();
                last_node = Some(node);
            }
            if walker.next_node().ok().flatten().is_none() {
                break;
            }
        }
    }

    last_node.map(|n| (n, last_len))
}

/// Find the deepest element whose `data-pos-start`/`data-pos-end` range
/// contains the given position, searching all descendants of `container`.
fn find_element_for_pos_html(container: &web_sys::Element, pos: usize) -> Option<web_sys::Element> {
    // Query all elements with data-pos-start.
    let Ok(all) = container.query_selector_all("[data-pos-start]") else {
        return None;
    };

    let mut best: Option<web_sys::Element> = None;
    let mut best_size = usize::MAX;

    for i in 0..all.length() {
        let Some(node) = all.get(i) else { continue };
        let Ok(el) = node.dyn_into::<web_sys::Element>() else { continue };

        let start: usize = el.get_attribute("data-pos-start")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let end: usize = el.get_attribute("data-pos-end")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        if pos >= start && pos <= end {
            let size = end - start;
            // Prefer the smallest (deepest) matching element.
            if size < best_size {
                best_size = size;
                best = Some(el);
            }
        }
    }

    best
}

/// Convert a Rust char count to a UTF-16 code unit offset.
fn char_count_to_utf16_offset(text: &str, char_count: usize) -> usize {
    let mut utf16_offset = 0;
    for (i, ch) in text.chars().enumerate() {
        if i >= char_count {
            break;
        }
        utf16_offset += ch.len_utf16();
    }
    utf16_offset
}

/// Compute extension active states from the current DocState and formatting.
///
/// Builds the `ExtensionEditorContext` required by `Extension::active_state`,
/// collecting results into a `Vec<(String, bool)>` suitable for
/// `extension_active_state.set(...)`.
fn compute_extension_active_states(
    ds: &DocState,
    fmt: &FormattingState,
    extensions: &[Arc<dyn crate::extension::Extension>],
) -> Vec<(String, bool)> {
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
    extensions
        .iter()
        .flat_map(|ext| {
            ext.active_state(&ctx)
                .into_iter()
                .map(|(name, active)| (name.to_owned(), active))
        })
        .collect()
}

// ── Segment-based DOM patching ───────────────────────────────────────────

/// Patch the container's DOM children to match `new_segments`.
///
/// Text blocks are rendered as wrapper `<div>` elements whose innerHTML
/// contains the block's HTML. Extension blocks are persistent
/// `<div contenteditable="false">` elements with Leptos views mounted inside.
///
/// On each call, the old and new segment lists are walked in parallel:
/// - Text blocks: update innerHTML only if the HTML string changed.
/// - Extension blocks: preserve the DOM element; only re-mount if the
///   raw content changed.
/// - Segment count changes: append new elements or remove excess.
///
/// Grid grouping: consecutive extension blocks with `col_span` are wrapped
/// in `<div class="kode-block-grid">` with `<div class="kode-grid-item">`
/// children. The grid wrapper is treated as a single DOM child of the container.
#[cfg(target_arch = "wasm32")]
fn patch_segments(
    container: &web_sys::Element,
    old_segments: &[RenderSegment],
    new_segments: &[RenderSegment],
    extensions: &[Arc<dyn crate::extension::Extension>],
    extension_context: Option<&(dyn Fn() + Send + Sync)>,
    enable_block_drag: bool,
    can_drag_block: Option<&(dyn Fn(usize, usize) -> bool + Send + Sync)>,
) {
    let doc = match web_sys::window().and_then(|w| w.document()) {
        Some(d) => d,
        None => return,
    };

    // Group new segments into "DOM slots" — each slot is either a single
    // text block, a single non-grid extension block, or a grid group of
    // consecutive extension blocks with col_span.
    let new_slots = group_into_slots(new_segments);
    let old_slots = group_into_slots(old_segments);

    let children = container.children();
    let old_child_count = children.length() as usize;

    // Walk slots in parallel, patching the DOM.
    let max_len = new_slots.len().max(old_slots.len());
    for i in 0..max_len {
        let old_slot = old_slots.get(i);
        let new_slot = new_slots.get(i);
        let existing_child = if i < old_child_count {
            children.item(i as u32)
        } else {
            None
        };

        match (old_slot, new_slot) {
            // Slot removed — handled by the trailing cleanup loop below.
            (Some(_), None) => {}
            // New slot added — create and append.
            (None, Some(new)) => {
                let el = create_slot_element(&doc, new, extensions, extension_context, enable_block_drag, can_drag_block);
                let _ = container.append_child(&el);
            }
            // Both exist — diff and patch in place.
            (Some(old), Some(new)) => {
                if let Some(child) = existing_child {
                    patch_slot_in_place(
                        &doc,
                        &child,
                        old,
                        new,
                        extensions,
                        extension_context,
                        enable_block_drag,
                        can_drag_block,
                    );
                } else {
                    let el = create_slot_element(&doc, new, extensions, extension_context, enable_block_drag, can_drag_block);
                    let _ = container.append_child(&el);
                }
            }
            (None, None) => break,
        }
    }

    // Remove any trailing old children beyond the new slot count.
    while container.children().length() as usize > new_slots.len() {
        if let Some(last) = container.last_child() {
            let _ = container.remove_child(&last);
        } else {
            break;
        }
    }
}

/// A DOM slot: one top-level child of the container.
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug)]
enum DomSlot<'a> {
    Text { html: &'a str },
    Extension { lang: &'a str, content: &'a str, pos_start: usize, pos_end: usize },
    Grid { items: Vec<(&'a str, &'a str, usize, usize, u8)> }, // (lang, content, start, end, span)
}

/// Group a flat segment list into DOM slots, merging consecutive col_span
/// extension blocks into grid groups.
#[cfg(target_arch = "wasm32")]
fn group_into_slots<'a>(segments: &'a [RenderSegment]) -> Vec<DomSlot<'a>> {
    let mut slots = Vec::new();
    let mut i = 0;

    while i < segments.len() {
        match &segments[i] {
            RenderSegment::TextBlock { html } => {
                slots.push(DomSlot::Text { html });
                i += 1;
            }
            RenderSegment::ExtensionBlock { lang, content, pos_start, pos_end, col_span } => {
                if let Some(span) = col_span {
                    // Start a grid group — collect consecutive extension blocks with col_span.
                    let mut items = vec![(lang.as_str(), content.as_str(), *pos_start, *pos_end, *span)];
                    i += 1;
                    while i < segments.len() {
                        if let RenderSegment::ExtensionBlock { lang: l, content: c, pos_start: ps, pos_end: pe, col_span: Some(s) } = &segments[i] {
                            items.push((l.as_str(), c.as_str(), *ps, *pe, *s));
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    // Solo col_span blocks are also wrapped in a grid for consistent layout.
                    slots.push(DomSlot::Grid { items });
                } else {
                    slots.push(DomSlot::Extension {
                        lang,
                        content,
                        pos_start: *pos_start,
                        pos_end: *pos_end,
                    });
                    i += 1;
                }
            }
        }
    }

    slots
}

/// Create a new DOM element for a slot.
#[cfg(target_arch = "wasm32")]
fn create_slot_element(
    doc: &web_sys::Document,
    slot: &DomSlot<'_>,
    extensions: &[Arc<dyn crate::extension::Extension>],
    extension_context: Option<&(dyn Fn() + Send + Sync)>,
    enable_block_drag: bool,
    can_drag_block: Option<&(dyn Fn(usize, usize) -> bool + Send + Sync)>,
) -> web_sys::Element {
    match slot {
        DomSlot::Text { html } => {
            let wrapper = doc.create_element("div").unwrap();
            let _ = wrapper.set_attribute("class", "kode-text-segment");
            wrapper.set_inner_html(html);
            wrapper
        }
        DomSlot::Extension { lang, content, pos_start, pos_end } => {
            let block = create_extension_element(doc, lang, content, *pos_start, *pos_end, extensions, extension_context);
            if enable_block_drag {
                mount_drag_handle_on_element(&block, *pos_start, *pos_end, can_drag_block);
            }
            block
        }
        DomSlot::Grid { items } => {
            let grid = doc.create_element("div").unwrap();
            let _ = grid.set_attribute("class", "kode-block-grid");
            for &(lang, content, ps, pe, span) in items {
                let grid_item = doc.create_element("div").unwrap();
                let _ = grid_item.set_attribute("class", "kode-grid-item");
                let _ = grid_item.set_attribute("data-col-span", &span.to_string());
                let block = create_extension_element(doc, lang, content, ps, pe, extensions, extension_context);
                if enable_block_drag {
                    mount_drag_handle_on_element(&block, ps, pe, can_drag_block);
                }
                let _ = grid_item.append_child(&block);
                let _ = grid.append_child(&grid_item);
            }
            grid
        }
    }
}

/// Mount a Leptos extension view into an element.
///
/// Creates a new reactive `Owner`, calls `render_code_block`, mounts the
/// resulting view, and intentionally leaks both the `Owner` and the view
/// state. The leak prevents reactive disposal panics from in-flight
/// `spawn_local` futures inside the extension. It is bounded: new owners
/// are only created when extension content actually changes, not on every
/// keystroke.
#[cfg(target_arch = "wasm32")]
fn mount_extension_view(
    block: &web_sys::Element,
    ext: &dyn crate::extension::Extension,
    lang: &str,
    content: &str,
    pos_start: usize,
    pos_end: usize,
    extension_context: Option<&(dyn Fn() + Send + Sync)>,
) {
    let owner = Owner::new();
    let result = owner.with(|| {
        if let Some(ctx_fn) = extension_context {
            ctx_fn();
        }
        ext.render_code_block(lang, content, pos_start, pos_end)
            .map(|view| view.build())
    });
    if let Some(mut state) = result {
        state.mount(block, None);
        std::mem::forget(state);
        std::mem::forget(owner);
    }
}

/// Create a persistent extension block element with a mounted Leptos view.
#[cfg(target_arch = "wasm32")]
fn create_extension_element(
    doc: &web_sys::Document,
    lang: &str,
    content: &str,
    pos_start: usize,
    pos_end: usize,
    extensions: &[Arc<dyn crate::extension::Extension>],
    extension_context: Option<&(dyn Fn() + Send + Sync)>,
) -> web_sys::Element {
    let block = doc.create_element("div").unwrap();
    let _ = block.set_attribute("contenteditable", "false");
    let _ = block.set_attribute("class", "kode-extension-block");
    let _ = block.set_attribute("data-kode-extension", lang);
    let _ = block.set_attribute("data-pos-start", &pos_start.to_string());
    let _ = block.set_attribute("data-pos-end", &pos_end.to_string());
    let _ = block.set_attribute("data-kode-content", content);

    if let Some(ext) = extensions.iter().find(|e| e.code_block_languages().contains(&lang)) {
        mount_extension_view(&block, ext.as_ref(), lang, content, pos_start, pos_end, extension_context);
    }

    block
}

/// Patch a DOM slot in place, preserving extension blocks when content matches.
#[cfg(target_arch = "wasm32")]
fn patch_slot_in_place(
    doc: &web_sys::Document,
    child: &web_sys::Element,
    old: &DomSlot<'_>,
    new: &DomSlot<'_>,
    extensions: &[Arc<dyn crate::extension::Extension>],
    extension_context: Option<&(dyn Fn() + Send + Sync)>,
    enable_block_drag: bool,
    can_drag_block: Option<&(dyn Fn(usize, usize) -> bool + Send + Sync)>,
) {
    match (old, new) {
        // Text → Text: update innerHTML if changed.
        (DomSlot::Text { html: old_html }, DomSlot::Text { html: new_html }) => {
            if old_html != new_html {
                child.set_inner_html(new_html);
            }
        }
        // Extension → Extension: preserve DOM if content matches.
        (
            DomSlot::Extension { content: old_content, .. },
            DomSlot::Extension { lang, content: new_content, pos_start, pos_end },
        ) => {
            let _ = child.set_attribute("data-pos-start", &pos_start.to_string());
            let _ = child.set_attribute("data-pos-end", &pos_end.to_string());
            if let Ok(Some(handle)) = child.query_selector(".kode-drag-handle") {
                let _ = handle.set_attribute("data-block-start", &pos_start.to_string());
                let _ = handle.set_attribute("data-block-end", &pos_end.to_string());
            }
            if old_content != new_content {
                child.set_inner_html("");
                let _ = child.set_attribute("data-kode-content", new_content);
                if let Some(ext) = extensions.iter().find(|e| e.code_block_languages().contains(lang)) {
                    mount_extension_view(child, ext.as_ref(), lang, new_content, *pos_start, *pos_end, extension_context);
                }
                if enable_block_drag {
                    mount_drag_handle_on_element(child, *pos_start, *pos_end, can_drag_block);
                }
            }
        }
        // Grid → Grid: patch items in place.
        (DomSlot::Grid { items: old_items }, DomSlot::Grid { items: new_items }) => {
            let grid_children = child.children();
            let max_items = old_items.len().max(new_items.len());

            for j in 0..max_items {
                let old_item = old_items.get(j);
                let new_item = new_items.get(j);
                let grid_item_el = grid_children.item(j as u32);

                match (old_item, new_item) {
                    // Handled by the trailing cleanup loop below.
                    (Some(_), None) => {}
                    (None, Some(&(lang, content, ps, pe, span))) => {
                        let grid_item = doc.create_element("div").unwrap();
                        let _ = grid_item.set_attribute("class", "kode-grid-item");
                        let _ = grid_item.set_attribute("data-col-span", &span.to_string());
                        let block = create_extension_element(doc, lang, content, ps, pe, extensions, extension_context);
                        if enable_block_drag {
                            mount_drag_handle_on_element(&block, ps, pe, can_drag_block);
                        }
                        let _ = grid_item.append_child(&block);
                        let _ = child.append_child(&grid_item);
                    }
                    (Some(&(_, old_content, _, _, old_span)), Some(&(lang, new_content, ps, pe, new_span))) => {
                        if let Some(grid_item) = grid_item_el {
                            if old_span != new_span {
                                let _ = grid_item.set_attribute("data-col-span", &new_span.to_string());
                            }
                            // Find the extension block inside the grid item.
                            if let Ok(Some(block)) = grid_item.query_selector(".kode-extension-block") {
                                let _ = block.set_attribute("data-pos-start", &ps.to_string());
                                let _ = block.set_attribute("data-pos-end", &pe.to_string());
                                if let Ok(Some(handle)) = block.query_selector(".kode-drag-handle") {
                                    let _ = handle.set_attribute("data-block-start", &ps.to_string());
                                    let _ = handle.set_attribute("data-block-end", &pe.to_string());
                                }
                                if old_content != new_content {
                                    block.set_inner_html("");
                                    let _ = block.set_attribute("data-kode-content", new_content);
                                    if let Some(ext) = extensions.iter().find(|e| e.code_block_languages().contains(&lang)) {
                                        mount_extension_view(&block, ext.as_ref(), lang, new_content, ps, pe, extension_context);
                                    }
                                    if enable_block_drag {
                                        mount_drag_handle_on_element(&block, ps, pe, can_drag_block);
                                    }
                                }
                            }
                        }
                    }
                    (None, None) => break,
                }
            }

            // Remove trailing grid items.
            while child.children().length() as usize > new_items.len() {
                if let Some(last) = child.last_child() {
                    let _ = child.remove_child(&last);
                } else {
                    break;
                }
            }
        }
        // Type changed — replace the element entirely.
        _ => {
            let new_el = create_slot_element(doc, new, extensions, extension_context, enable_block_drag, can_drag_block);
            if let Some(parent) = child.parent_node() {
                let _ = parent.replace_child(&new_el, child);
            }
        }
    }
}

/// Mount a drag handle on a single extension block element.
#[cfg(target_arch = "wasm32")]
fn mount_drag_handle_on_element(
    block: &web_sys::Element,
    pos_start: usize,
    pos_end: usize,
    can_drag: Option<&(dyn Fn(usize, usize) -> bool + Send + Sync)>,
) {
    if block.query_selector(".kode-drag-handle").ok().flatten().is_some() {
        return;
    }
    if let Some(filter) = can_drag {
        if !filter(pos_start, pos_end) {
            return;
        }
    }
    let block_html: &HtmlElement = block.unchecked_ref();
    let _ = block_html.style().set_property("position", "relative");
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return };
    let Ok(handle) = doc.create_element("div") else { return };
    let _ = handle.set_attribute("class", "kode-drag-handle");
    let _ = handle.set_attribute("contenteditable", "false");
    let _ = handle.set_attribute("data-block-start", &pos_start.to_string());
    let _ = handle.set_attribute("data-block-end", &pos_end.to_string());
    handle.set_inner_html("\u{283F}");
    let _ = block.prepend_with_node_1(&handle);
}

// mount_extension_views and mount_drag_handles removed —
// replaced by patch_segments() which handles both inline.

/// Walk up from `el` to find the direct child of `container`.
/// Returns `None` if `el` is not a descendant of `container`.
/// Bounded to ~4 hops (block → grid-item → block-grid → container).
fn find_top_level_ancestor(
    el: &web_sys::Element,
    container: &web_sys::Element,
) -> Option<web_sys::Element> {
    let mut current = el.clone();
    loop {
        match current.parent_element() {
            Some(parent) if &parent == container => return Some(current),
            Some(parent) => current = parent,
            None => return None,
        }
    }
}

/// For a grid group wrapper, find the block-level start of its first child
/// and block-level end of its last child. Used to align drop target positions
/// with the grid boundary when the indicator is placed at group edges.
fn resolve_grid_group_positions(grid_wrapper: &web_sys::Element) -> Option<(usize, usize)> {
    let Ok(blocks) = grid_wrapper.query_selector_all("[data-pos-start]") else {
        return None;
    };
    if blocks.length() == 0 {
        return None;
    }
    let first: web_sys::Element = blocks.item(0)?.unchecked_into();
    let last: web_sys::Element = blocks.item(blocks.length() - 1)?.unchecked_into();

    let first_raw: usize = first.get_attribute("data-pos-start")?.parse().ok()?;
    let last_raw: usize = last.get_attribute("data-pos-end")?.parse().ok()?;

    let first_block_level = first.has_attribute("data-kode-extension")
        || first.tag_name().eq_ignore_ascii_case("hr");
    let last_block_level = last.has_attribute("data-kode-extension")
        || last.tag_name().eq_ignore_ascii_case("hr");

    let group_start = if first_block_level { first_raw } else { first_raw.saturating_sub(1) };
    let group_end = if last_block_level { last_raw } else { last_raw + 1 };

    Some((group_start, group_end))
}

// ── Gap cursor rendering ─────────────────────────────────────────────────
//
// The gap cursor is a thin element inserted into the document flow at the
// gap position — between block elements. It scrolls naturally with the
// content because it IS content, not a floating overlay.

/// Insert a gap cursor element into the document flow next to an atomic block.
fn show_gap_cursor(
    container: &web_sys::Element,
    side: GapSide,
    block_start: usize,
    block_end: usize,
) {
    hide_gap_cursor(container);

    let selector = format!(
        "[data-pos-start=\"{}\"][data-pos-end=\"{}\"]",
        block_start, block_end,
    );
    let block_el = match container.query_selector(&selector) {
        Ok(Some(el)) => el,
        _ => return,
    };

    let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return };
    let Ok(gc) = doc.create_element("div") else { return };
    let _ = gc.set_attribute("class", "kode-gap-cursor");
    let _ = gc.set_attribute("contenteditable", "false");

    // The gap cursor is position:absolute inside the scroll container
    // (position:relative). Use the block's offset position within the
    // container so it scrolls naturally with the content.
    let block_html: &web_sys::HtmlElement = block_el.unchecked_ref();
    let top = block_html.offset_top() as f64;
    let height = block_html.offset_height() as f64;
    let left = match side {
        GapSide::Before => block_html.offset_left() as f64,
        GapSide::After => block_html.offset_left() as f64 + block_html.offset_width() as f64,
    };
    let gc_html: &web_sys::HtmlElement = gc.unchecked_ref();
    let _ = gc_html.style().set_property("top", &format!("{top}px"));
    let _ = gc_html.style().set_property("left", &format!("{left}px"));
    let _ = gc_html.style().set_property("height", &format!("{height}px"));

    // Walk up to find the direct child of the container (the block may be
    // inside a grid wrapper).
    let insert_ref = find_container_child(container, &block_el);
    let insert_ref = insert_ref.as_ref().unwrap_or(&block_el);

    match side {
        GapSide::Before => {
            let _ = container.insert_before(&gc, Some(insert_ref));
        }
        GapSide::After => {
            let _ = container.insert_before(&gc, insert_ref.next_sibling().as_ref());
        }
    }

    let _ = container.dyn_ref::<HtmlElement>()
        .map(|el| el.style().set_property("caret-color", "transparent"));
}

/// Remove any gap cursor element from the container.
fn hide_gap_cursor(container: &web_sys::Element) {
    if let Ok(Some(gc)) = container.query_selector(".kode-gap-cursor") {
        gc.remove();
    }
    let _ = container.dyn_ref::<HtmlElement>()
        .map(|el| el.style().remove_property("caret-color"));
}

/// Walk up from an element to find the direct child of `container`.
fn find_container_child(
    container: &web_sys::Element,
    el: &web_sys::Element,
) -> Option<web_sys::Element> {
    let mut current = Some(el.clone());
    while let Some(ref node) = current {
        if let Some(parent) = node.parent_element() {
            if &parent == container {
                return current;
            }
        }
        current = node.parent_element();
    }
    None
}
