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
use kode_doc::{DocState, FormattingState, Selection};
use leptos::prelude::*;
use leptos::tachys::view::any_view::AnyView;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{CompositionEvent, HtmlElement, KeyboardEvent, MouseEvent};

use crate::extension::{matches_key_descriptor, Extension, ExtensionKeyboardShortcut, ExtensionToolbarItem};
use crate::theme::Theme;
use crate::toolbar::{InjectCommand, ToolbarItem, default_toolbar_items, dispatch_builtin_action};

use super::clipboard::{html_escape, extract_kode_markdown};
use super::doc_renderer::doc_to_html;
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

    // ── HTML content memo ────────────────────────────────────────────────
    let doc_for_html = doc_state.clone();
    let extensions_for_html = Arc::clone(&extensions);
    let aliases_for_html = Arc::clone(&language_aliases);
    let drag_state_html =
        send_wrapper::SendWrapper::new(drag_state.as_ref().map(std::rc::Rc::clone));
    let html_cache = send_wrapper::SendWrapper::new(std::cell::RefCell::new(String::new()));
    let html_content = Memo::new(move |_| {
        // Track version to maintain reactive dependency even when returning cached HTML.
        let _v = version.get();

        // Suppress re-renders during an active drag to avoid destroying the DOM mid-drag.
        if let Some(ref ds) = *drag_state_html {
            if ds.active.get() {
                return html_cache.borrow().clone();
            }
        }

        let Ok(ds) = doc_for_html.lock() else {
            return String::new();
        };
        // Notify extensions that a new render pass is starting.
        for ext in extensions_for_html.iter() {
            ext.begin_render_pass();
        }
        let html = doc_to_html(ds.doc(), &extensions_for_html, &aliases_for_html);
        *html_cache.borrow_mut() = html.clone();
        html
    });

    // ── Selection restore + extension mounting after re-render ─────────
    let doc_for_sel = doc_state.clone();
    let extensions_for_effect = Arc::clone(&extensions);
    Effect::new(move |_| {
        let _v = version.get();
        let is_composing = composing.get();

        // Compute formatting state for toolbar active buttons.
        // Lock must be released before signal writes to avoid recursive-lock panics.
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

        let doc_raf = doc_for_sel.clone();
        let editor_raf = editor_ref;
        let exts_raf = Arc::clone(&extensions_for_effect);
        let ext_ctx = extension_context.clone();
        let can_drag_block_raf = can_drag_block.as_ref().map(Arc::clone);

        let cb = Closure::once(move || {
            let Some(container) = editor_raf.get() else { return };
            let container_el: &web_sys::Element = container.as_ref();

            let Ok(ds) = doc_raf.lock() else { return };
            let sel = ds.selection().clone();
            drop(ds);

            let head = sel.head;
            let anchor = sel.anchor;

            if sel.is_cursor() {
                restore_cursor(container_el, head);
            } else {
                restore_range(container_el, anchor, head);
            }

            mount_extension_views(container_el, &exts_raf, ext_ctx.as_deref());

            if enable_block_drag {
                mount_drag_handles(container_el, can_drag_block_raf.as_deref());
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

                // Extension blocks store block-level positions; all other
                // blocks store content positions (1 token inside the block).
                // Adjust non-extension blocks to block boundaries so the
                // insert lands between blocks, not inside one.
                let is_extension = el.has_attribute("data-kode-extension");
                let block_start = if is_extension { raw_start } else { raw_start.saturating_sub(1) };
                let block_end = if is_extension { raw_end } else { raw_end + 1 };

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

            // Commit the move.
            let Ok(mut ds) = doc_up.lock() else { return };
            ds.move_block(source_start, source_end, target);
            let md = ds.to_markdown();
            drop(ds);
            (notify_up)(Some(md));
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

        // Sync selection before handling the shortcut.
        if let Some(container) = editor_ref.get() {
            let container_el: &web_sys::Element = container.as_ref();
            sync_selection_to_doc(&mut ds, container_el);
        }

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
            "a" if ctrl => {
                // Select all
                let doc_size = ds.doc().content.size();
                if doc_size > 0 {
                    ds.set_selection(Selection::range(0, doc_size));
                }
                true
            }
            // ── Tab / Shift+Tab list indentation ────────────────────
            "Tab" if !ctrl => {
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
            _ => false,
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
        let selchange_cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_ev: web_sys::Event| {
            if composing.get_untracked() {
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
                on:compositionstart=on_composition_start
                on:compositionend=on_composition_end
                inner_html=move || html_content.get()
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

    if sel.is_collapsed() {
        ds.set_selection(Selection::cursor(head));
    } else if let Some(anchor_node) = sel.anchor_node() {
        let anchor = node_offset_to_doc_pos(container, &anchor_node, sel.anchor_offset())
            .unwrap_or(head);
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

    // Use TreeWalker to iterate text nodes in document order.
    let Ok(walker) = document.create_tree_walker_with_what_to_show(
        root_el,
        0x4, // NodeFilter.SHOW_TEXT
    ) else {
        return 0;
    };

    let mut count = 0usize;

    // Advance to the first text node (currentNode starts at the root element,
    // which is not a text node despite the SHOW_TEXT filter).
    if walker.next_node().ok().flatten().is_none() {
        // No text nodes at all — check element-level offset below.
    } else {
        loop {
            let Some(node) = walker.current_node().dyn_ref::<web_sys::Node>().cloned() else {
                break;
            };

            if node == *target_node {
                if let Some(text) = node.text_content() {
                    let char_offset = utf16_offset_to_char_count(&text, target_offset as usize);
                    count += char_offset;
                }
                return count;
            }

            if let Some(text) = node.text_content() {
                count += text.chars().count();
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
/// count characters from the start of the element to the nth child.
fn count_chars_up_to_child_index(el: &web_sys::Element, child_index: usize) -> usize {
    let children = el.child_nodes();
    let mut count = 0;
    for i in 0..child_index.min(children.length() as usize) {
        if let Some(child) = children.get(i as u32) {
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

    // Walk text nodes inside the positioned element.
    let Ok(walker) = document.create_tree_walker_with_what_to_show(
        &target_el,
        0x4, // NodeFilter.SHOW_TEXT
    ) else {
        return None;
    };

    let mut chars_remaining = target_char_offset;

    // Advance past the root element to the first text node.
    // (TreeWalker.currentNode starts at the root element regardless of the
    // SHOW_TEXT filter — only navigation methods like nextNode() respect it.)
    if walker.next_node().ok().flatten().is_none() {
        // No text nodes (empty element) — position cursor inside it.
        return Some((target_el.unchecked_into::<web_sys::Node>(), 0));
    }

    loop {
        let node = walker.current_node();

        if let Some(text) = node.text_content() {
            let text_char_count = text.chars().count();
            if chars_remaining <= text_char_count {
                // Found the right text node. Convert char offset to UTF-16 offset.
                let utf16_offset = char_count_to_utf16_offset(&text, chars_remaining);
                return Some((node, utf16_offset));
            }
            chars_remaining -= text_char_count;
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

/// Mount live extension views into the placeholder divs created by `doc_to_html`.
///
/// The string-based HTML renderer emits `<div class="kode-extension-block"
/// data-kode-extension="lang" ...>` with the raw content inside a
/// `<div class="kode-extension-content">` child. This function replaces that
/// static content with the live Leptos views returned by each extension's
/// `render_code_block()`.
fn mount_extension_views(
    container: &web_sys::Element,
    extensions: &[Arc<dyn crate::extension::Extension>],
    extension_context: Option<&(dyn Fn() + Send + Sync)>,
) {
    if extensions.is_empty() {
        return;
    }

    let Ok(blocks) = container.query_selector_all("div.kode-extension-block[data-kode-extension]")
    else {
        return;
    };

    for i in 0..blocks.length() {
        let Some(node) = blocks.item(i) else { continue };
        let block_el: web_sys::Element = node.unchecked_into();

        let lang = block_el
            .get_attribute("data-kode-extension")
            .unwrap_or_default();
        let pos_start: usize = block_el
            .get_attribute("data-pos-start")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let pos_end: usize = block_el
            .get_attribute("data-pos-end")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        // Read the raw content from the placeholder child. If it's missing,
        // a previous rAF already mounted a live view into this block — skip.
        let content = match block_el.query_selector(".kode-extension-content") {
            Ok(Some(el)) => el.text_content().unwrap_or_default(),
            _ => continue,
        };

        let ext = extensions.iter().find(|ext| {
            ext.code_block_languages().contains(&lang.as_str())
        });
        let Some(ext) = ext else { continue };

        let owner = Owner::new();
        let result = owner.with(|| {
            if let Some(ctx_fn) = extension_context {
                ctx_fn();
            }
            ext.render_code_block(&lang, &content, pos_start, pos_end)
                .map(|view| view.build())
        });

        let Some(mut state) = result else { continue };

        block_el.set_inner_html("");
        state.mount(&block_el, None);

        // Leak both — dropping Owner disposes reactive nodes while
        // spawn_local futures from the extension are in-flight, causing
        // re-entrant wasm-bindgen executor panics. The leak is bounded:
        // new owners are only created when innerHTML resets with fresh
        // placeholders (content changes), not on every keystroke — the
        // skip-when-placeholder-missing guard above prevents re-mounting
        // into blocks that a previous rAF already handled.
        std::mem::forget(state);
        std::mem::forget(owner);
    }
}

/// Mount drag handles on extension blocks for drag-and-drop reordering.
///
/// Queries all atomic extension blocks inside `container` and prepends a
/// `.kode-drag-handle` element to each one. The handle carries
/// `data-block-start` / `data-block-end` attributes so pointer-event
/// handlers can identify the source block without walking the DOM.
fn mount_drag_handles(
    container: &web_sys::Element,
    can_drag: Option<&(dyn Fn(usize, usize) -> bool + Send + Sync)>,
) {
    let Ok(blocks) = container.query_selector_all("div.kode-extension-block[data-kode-extension]")
    else { return };

    for i in 0..blocks.length() {
        let Some(node) = blocks.item(i) else { continue };
        let block_el: web_sys::Element = node.unchecked_into();

        // Skip if handle already mounted.
        if block_el.query_selector(".kode-drag-handle").ok().flatten().is_some() {
            continue;
        }

        let pos_start: usize = block_el.get_attribute("data-pos-start")
            .and_then(|s| s.parse().ok()).unwrap_or(0);
        let pos_end: usize = block_el.get_attribute("data-pos-end")
            .and_then(|s| s.parse().ok()).unwrap_or(0);

        // Check if this block should be draggable.
        if let Some(filter) = can_drag {
            if !filter(pos_start, pos_end) {
                continue;
            }
        }

        // Make the block positioned for the absolute handle.
        let block_html: &HtmlElement = block_el.unchecked_ref();
        let _ = block_html.style().set_property("position", "relative");

        // Create the drag handle element.
        let doc = web_sys::window().and_then(|w| w.document());
        let Some(doc) = doc else { continue };
        let Ok(handle) = doc.create_element("div") else { continue };
        let _ = handle.set_attribute("class", "kode-drag-handle");
        let _ = handle.set_attribute("contenteditable", "false");
        let _ = handle.set_attribute("data-block-start", &pos_start.to_string());
        let _ = handle.set_attribute("data-block-end", &pos_end.to_string());
        handle.set_inner_html("\u{283F}"); // braille pattern dots-123456 (grip icon)

        // Prepend the handle to the block.
        let _ = block_el.prepend_with_node_1(&handle);
    }
}

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

    let first_is_ext = first.has_attribute("data-kode-extension");
    let last_is_ext = last.has_attribute("data-kode-extension");

    let group_start = if first_is_ext { first_raw } else { first_raw.saturating_sub(1) };
    let group_end = if last_is_ext { last_raw } else { last_raw + 1 };

    Some((group_start, group_end))
}
