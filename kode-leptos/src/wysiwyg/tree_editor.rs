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
) -> impl IntoView {
    // ── State ────────────────────────────────────────────────────────────
    let doc_state = Arc::new(Mutex::new(DocState::from_markdown(
        &content.get_untracked(),
    )));

    // Reactive version counter — bumped to trigger re-render + selection restore.
    let version = RwSignal::new(0u64);
    let composing = RwSignal::new(false);
    let formatting_state = RwSignal::new(FormattingState::default());
    let extension_active_state = RwSignal::new(Vec::<(String, bool)>::new());
    let last_ext_version = std::cell::Cell::new(0u64);

    let editor_ref = NodeRef::<leptos::html::Div>::new();

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
    let html_content = Memo::new(move |_| {
        let _v = version.get();
        let Ok(ds) = doc_for_html.lock() else {
            return String::new();
        };
        // Notify extensions that a new render pass is starting.
        for ext in extensions_for_html.iter() {
            ext.begin_render_pass();
        }
        doc_to_html(ds.doc(), &extensions_for_html, &aliases_for_html)
    });

    // ── Selection restore after re-render ───────────────────────────────
    let doc_for_sel = doc_state.clone();
    let extensions_for_sel = Arc::clone(&extensions);
    Effect::new(move |_| {
        let _v = version.get();
        let is_composing = composing.get();

        // Compute formatting state for toolbar active buttons.
        // Lock must be released before signal writes to avoid recursive-lock panics.
        let (fmt, ext_states, should_update_ext) = {
            let Ok(ds) = doc_for_sel.lock() else { return };
            let fmt = ds.formatting_at_cursor();
            let should_update = !extensions_for_sel.is_empty() && _v != last_ext_version.get();
            let ext_states = if should_update {
                Some(compute_extension_active_states(&ds, &fmt, &extensions_for_sel))
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
        let extensions_for_selchange = Arc::clone(&extensions);
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

            let ext_states = if !extensions_for_selchange.is_empty() {
                Some(compute_extension_active_states(&ds, &fmt, &extensions_for_selchange))
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

    loop {
        let Some(node) = walker.current_node().dyn_ref::<web_sys::Node>().cloned() else {
            break;
        };

        // Check if this text node IS the target.
        if node == *target_node {
            // The target_offset is in UTF-16 code units. Convert to char count.
            if let Some(text) = node.text_content() {
                let char_offset = utf16_offset_to_char_count(&text, target_offset as usize);
                count += char_offset;
            }
            return count;
        }

        // If target is an element and this text node is a child of the target
        // at or before the target_offset child index, count appropriately.

        // Count all chars in this text node and move on.
        if let Some(text) = node.text_content() {
            count += text.chars().count();
        }

        if walker.next_node().ok().flatten().is_none() {
            break;
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
