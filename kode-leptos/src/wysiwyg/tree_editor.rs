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

use kode_doc::attrs::{AttrValue, get_attr};
use kode_doc::mark::MarkType;
use kode_doc::{DocState, FormattingState, GapSide, NodeType, Selection};
use leptos::prelude::*;
use leptos::tachys::view::any_view::AnyView;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{CompositionEvent, HtmlElement, KeyboardEvent, MouseEvent};

use crate::extension::{matches_key_descriptor, Extension, ExtensionKeyboardShortcut, ExtensionToolbarItem};
use crate::theme::Theme;
use crate::toolbar::{BuiltinButton, InjectCommand, SlashMenuItem, ToolbarItem, default_slash_menu_items, default_toolbar_items, dispatch_builtin_action};

use super::attachment::{AttachmentNodeType, ClickAttachmentRequest, DeleteAttachmentRequest};
use super::clipboard::{html_escape, extract_kode_markdown};
use super::doc_renderer::{doc_to_segments, RenderSegment};
use super::dom_helpers::apply_md_command;
use super::popover_position::compute_position_relative;

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
    /// Whether to show the fixed formatting toolbar at the top.
    #[prop(default = true)]
    show_fixed_toolbar: bool,
    /// Whether to show a floating toolbar near the text selection.
    #[prop(default = false)]
    show_floating_toolbar: bool,
    /// Whether to show the slash command menu on empty lines.
    #[prop(default = true)]
    show_slash_menu: bool,
    /// Editor theme.
    #[prop(into, default = Signal::stored(Theme::default()))]
    theme: Signal<Theme>,
    /// Editor extensions for custom code block rendering.
    #[prop(default = vec![])]
    extensions: Vec<Arc<dyn Extension>>,
    /// Override the max-width of the editor container (default: "800px").
    #[prop(into, optional)]
    container_max_width: Option<String>,
    /// Custom toolbar layout for the fixed toolbar.
    #[prop(optional)]
    toolbar_items: Option<Vec<ToolbarItem>>,
    /// Custom toolbar layout for the floating toolbar.
    #[prop(optional)]
    floating_toolbar_items: Option<Vec<ToolbarItem>>,
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
    /// Read-only mode: disables editing, hides toolbars, suppresses callbacks.
    #[prop(default = false)]
    readonly: bool,
    /// Called when user clicks the delete button on an attachment.
    /// Host app should delete from storage; the node is removed from the document.
    #[prop(optional)]
    on_delete_attachment: Option<Arc<dyn Fn(DeleteAttachmentRequest) + Send + Sync>>,
    /// Called when user clicks an image thumbnail or file chip.
    /// Host app can open a lightbox or download.
    #[prop(optional)]
    on_click_attachment: Option<Arc<dyn Fn(ClickAttachmentRequest) + Send + Sync>>,
    /// Called when the user drops, pastes, or picks a file for upload.
    /// The host app receives the file name, size, and type, plus a placeholder_id
    /// to correlate with the upload_complete signal.
    #[prop(optional)]
    on_upload: Option<Arc<dyn Fn(super::attachment::UploadTrigger) + Send + Sync>>,
    /// Signal for the host app to report upload completion.
    /// Write `Some(UploadComplete { ... })` to replace the placeholder with the real node,
    /// or write with `insert: None` to remove the placeholder (upload failed).
    #[prop(optional)]
    upload_complete: Option<RwSignal<Option<super::attachment::UploadComplete>>>,
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
    let mouse_selecting = std::rc::Rc::new(std::cell::Cell::new(false));
    let observing_dom_input = std::rc::Rc::new(std::cell::Cell::new(false));
    let dom_dirty = std::rc::Rc::new(std::cell::Cell::new(false));
    let mutation_observer: std::rc::Rc<std::cell::RefCell<Option<web_sys::MutationObserver>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let floating_pos: RwSignal<Option<(f64, f64, bool)>> = RwSignal::new(None);

    // ── Drag-and-drop state (Cell-based to avoid reactive re-renders) ──
    struct DragState {
        active: std::cell::Cell<bool>,
        source_start: std::cell::Cell<usize>,
        source_end: std::cell::Cell<usize>,
        target_pos: std::cell::Cell<usize>,
    }

    let drag_state = if enable_block_drag && !readonly {
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

    type ExtAction = Arc<dyn Fn(&mut kode_markdown::MarkdownEditor) + Send + Sync>;
    let ext_actions: Arc<Vec<ExtAction>> = Arc::new(
        extension_toolbar_items.iter().map(|item| Arc::clone(&item.action)).collect()
    );

    let slash_menu_index = RwSignal::new(0usize);
    let slash_menu_state: RwSignal<Option<(f64, f64, f64, bool)>> = RwSignal::new(None);
    let slash_trigger_el: std::rc::Rc<std::cell::RefCell<Option<web_sys::Element>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));

    // ── Link popover state ──────────────────────────────────────────────
    // `link_popup_open` is the trigger signal set by the toolbar Link button.
    // The popover itself is controlled by `link_popover_pos`.
    let link_popup_open = RwSignal::new(false);
    let link_popover_pos: RwSignal<Option<(f64, f64, bool)>> = RwSignal::new(None);
    let link_popover_url = RwSignal::new(String::new());
    let link_popover_edit_range: RwSignal<Option<(usize, usize)>> = RwSignal::new(None);

    // ── Table picker state ──────────────────────────────────────────────
    let table_picker_open = RwSignal::new(false);
    let table_picker_pos: RwSignal<Option<(f64, f64, bool)>> = RwSignal::new(None);
    let table_hover: RwSignal<(usize, usize)> = RwSignal::new((1, 1));

    // ── Table context menu state ────────────────────────────────────────
    let table_ctx_menu_pos: RwSignal<Option<(f64, f64)>> = RwSignal::new(None);

    let slash_menu_items: Arc<Vec<SlashMenuItem>> = Arc::new(if show_slash_menu {
        let mut items = default_slash_menu_items();
        for (i, ext_item) in extension_toolbar_items.iter().enumerate() {
            items.push(SlashMenuItem::Extension {
                label: ext_item.title.clone(),
                description: ext_item.description.clone(),
                ext_index: i,
            });
        }
        items
    } else {
        Vec::new()
    });
    let slash_item_count = slash_menu_items.len();

    let extensions: Arc<Vec<Arc<dyn Extension>>> = Arc::new(extensions);
    let language_aliases: Arc<Vec<(String, String)>> = Arc::new(language_aliases);

    // ── Notification helper ──────────────────────────────────────────────
    // IMPORTANT: notify must NOT lock the doc_state. The signal update
    // can synchronously trigger the render closure which also locks the
    // doc_state, causing a recursive lock panic in WASM's single-threaded mutex.
    let on_change_notify = if readonly { None } else { on_change.clone() };
    let notify = move |new_text: Option<String>| {
        version.update(|v| *v += 1);
        if let (Some(ref cb), Some(text)) = (&on_change_notify, new_text) {
            cb(text);
        }
    };

    // ── inject: insert content at cursor when the signal is written ────
    if let (Some(inject_signal), false) = (inject, readonly) {
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

    // ── upload_complete: replace placeholder with real node or remove it ──
    if let (Some(upload_complete_signal), false) = (upload_complete, readonly) {
        let doc_upload = doc_state.clone();
        let on_change_upload = on_change.clone();
        Effect::new(move |_| {
            let Some(complete) = upload_complete_signal.get() else { return };
            upload_complete_signal.set(None); // Consume the signal

            let Ok(mut ds) = doc_upload.lock() else { return };

            // Find the placeholder by scanning top-level doc children for an
            // UploadPlaceholder with matching placeholder_id attr.
            let placeholder_pos = find_placeholder_position(ds.doc(), &complete.placeholder_id);
            let Some(pos) = placeholder_pos else { return };

            match complete.insert {
                Some(ref attachment) => {
                    // Atomically replace placeholder with the real node (single undo entry).
                    let node = attachment_insert_to_node(attachment);
                    ds.replace_block_node(pos, node);
                }
                None => {
                    // Remove placeholder (upload failed).
                    ds.delete_range(pos, pos + 1);
                }
            }

            let md = ds.to_markdown();
            drop(ds);
            version.update(|v| *v += 1);
            if let Some(ref cb) = on_change_upload {
                cb(md);
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
    let mo_for_patch = mutation_observer.clone();
    let keydown_handled_for_mo = keydown_handled.clone();
    let dom_dirty_for_effect = dom_dirty.clone();
    let dom_dirty_for_mo = dom_dirty.clone();
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
                hide_gap_cursor(container_el);

                // Disconnect MutationObserver during patching to prevent
                // our own innerHTML writes from being observed.
                if let Some(ref mo) = *mo_for_patch.borrow() {
                    mo.disconnect();
                }

                let mut old = prev_segments_for_effect.borrow_mut();

                // If the DOM was modified by browser-native text input (MO
                // path), prev_segments is stale. Clear both the cache and
                // the DOM children so patch_segments does a full rebuild.
                if dom_dirty_for_effect.get() {
                    old.clear();
                    while let Some(child) = container_el.first_child() {
                        let _ = container_el.remove_child(&child);
                    }
                    dom_dirty_for_effect.set(false);
                }

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

                // Mount table hover controls (add row/column buttons).
                if !readonly {
                    mount_table_controls_in_container(container_el);
                }

                // Reconnect the observer after patching.
                if let Some(ref mo) = *mo_for_patch.borrow() {
                    let opts = web_sys::MutationObserverInit::new();
                    opts.set_character_data(true);
                    opts.set_character_data_old_value(true);
                    opts.set_child_list(true);
                    opts.set_subtree(true);
                    let _ = mo.observe_with_options(container_el, &opts);
                }
            }
        }

        // Keep the variables alive for the wasm32 cfg block above.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = (&segments_memo, &prev_segments_for_effect, &ext_ctx_for_effect, &can_drag_for_effect, &mo_for_patch, &dom_dirty_for_effect);
        }

        let doc_raf = doc_for_sel.clone();
        let editor_raf = editor_ref;
        let kd_handled_raf = keydown_handled_effect.clone();

        let cb = Closure::once(move || {
            let Some(container) = editor_raf.get_untracked() else { return };
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
        let kd_handled_input = keydown_handled.clone();
        let observing_input = observing_dom_input.clone();

        let beforeinput_cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
            if readonly { return; }
            let input_ev: web_sys::InputEvent = ev.unchecked_into();

            // During IME composition, let the browser handle it.
            if composing.get_untracked() {
                return;
            }

            let input_type = input_ev.input_type();

            let container: Option<web_sys::Element> = editor_input
                .get_untracked()
                .map(|el| el.unchecked_ref::<web_sys::Element>().clone());

            // For text insertion outside code blocks, let the browser modify the
            // DOM natively (ProseMirror-style). The MutationObserver will pick up
            // the change and sync it back to DocState. This allows OS features
            // like autocorrect and double-space-to-period to work.
            if matches!(input_type.as_str(), "insertText" | "insertReplacementText") {
                let Ok(mut ds) = doc_input.lock() else { return };
                if let Some(ref c) = container {
                    sync_selection_to_doc(&mut ds, c);
                }

                // Code blocks use syntax-highlighted spans that won't survive
                // browser mutations — fall back to the preventDefault path.
                let in_code_block = {
                    let resolved = ds.doc().resolve(ds.selection().head);
                    resolved.parent().node_type == NodeType::CodeBlock
                };

                if in_code_block {
                    input_ev.prevent_default();
                    if let Some(data) = input_ev.data() {
                        if !data.is_empty() {
                            ds.insert_text(&data);
                        }
                    }
                    let md = ds.to_markdown();
                    drop(ds);
                    kd_handled_input.set(true);
                    (notify_input)(Some(md));
                    return;
                }

                // Auto-convert list syntax: detect "- ", "* ", "1. " at block start.
                if let Some(data) = input_ev.data() {
                    if data.starts_with(' ') && ds.try_auto_convert_list_on_space() {
                        input_ev.prevent_default();
                        let md = ds.to_markdown();
                        drop(ds);
                        kd_handled_input.set(true);
                        (notify_input)(Some(md));
                        return;
                    }
                }

                // Let the browser handle text insertion natively. The
                // MutationObserver will diff the DOM change and sync it
                // back to DocState. Don't modify DocState here — the MO
                // callback computes its own ranges from the actual DOM diff.
                drop(ds);
                observing_input.set(true);
                kd_handled_input.set(true);
                return;
            }

            // All non-text-insertion input types: preventDefault and handle manually.
            input_ev.prevent_default();

            let Ok(mut ds) = doc_input.lock() else { return };

            if let Some(ref c) = container {
                sync_selection_to_doc(&mut ds, c);
            }

            // Use getTargetRanges() for OS-level text replacements.
            if let Some(ref c) = container {
                let target_ranges = input_ev.get_target_ranges();
                if target_ranges.length() > 0 {
                    if let Ok(range) = target_ranges.get(0).dyn_into::<web_sys::StaticRange>() {
                        let sn = range.start_container();
                        let en = range.end_container();
                        let so = range.start_offset();
                        let eo = range.end_offset();
                        let start_pos = node_offset_to_doc_pos(c, &sn, so);
                        let end_pos = node_offset_to_doc_pos(c, &en, eo);
                        if let (Some(s), Some(e)) = (start_pos, end_pos) {
                            if s != e && !ds.text_between(s, e).contains('\n') {
                                ds.set_selection(Selection::range(s, e));
                            }
                        }
                    }
                }
            }

            match input_type.as_str() {
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
                            ds.insert_from_markdown(&plain);
                        }
                    } else if let Some(data) = input_ev.data() {
                        if !data.is_empty() {
                            ds.insert_from_markdown(&data);
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
                "insertFromDrop" => {}
                _ => {}
            }

            let md = ds.to_markdown();
            drop(ds);
            kd_handled_input.set(true);
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
            if readonly { return; }
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
        let on_upload_paste = on_upload.clone();
        let paste_cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
            if readonly { return; }
            // Only handle paste here if beforeinput didn't handle it.
            // Check if the event is already defaultPrevented (which means
            // beforeinput already handled it).
            if ev.default_prevented() {
                return;
            }

            ev.prevent_default();
            let ev: web_sys::ClipboardEvent = ev.unchecked_into();
            let Some(dt) = ev.clipboard_data() else { return };

            // Check for file paste (e.g., screenshot) — only when on_upload is configured.
            if let Some(ref upload_fn) = on_upload_paste {
                let items = dt.items();
                let mut handled_file = false;
                for i in 0..items.length() {
                    let Some(item) = items.get(i) else { continue };
                    if item.kind() == "file" {
                        if let Ok(Some(file)) = item.get_as_file() {
                            handled_file = true;
                            let placeholder_id = format!("upload-{}-{}", js_sys::Date::now() as u64, i);

                            let Ok(mut ds) = doc_paste.lock() else { return };
                            let placeholder_attrs = {
                                let mut attrs = kode_doc::Attrs::new();
                                attrs.push(("placeholder_id".to_string(), kode_doc::AttrValue::String(placeholder_id.clone())));
                                attrs
                            };
                            let placeholder = kode_doc::Node::leaf_with_attrs(
                                NodeType::UploadPlaceholder,
                                placeholder_attrs,
                            );
                            let pos = ds.selection().from();
                            ds.insert_block_node(pos, placeholder);
                            let new_md = ds.to_markdown();
                            drop(ds);
                            (notify_paste)(Some(new_md));

                            let upload_fn = Arc::clone(upload_fn);
                            let file_name = file.name();
                            let file_size = file.size() as u64;
                            let file_type = file.type_();
                            let pid = placeholder_id.clone();

                            wasm_bindgen_futures::spawn_local(async move {
                                let promise = file.array_buffer();
                                let Ok(buffer) = wasm_bindgen_futures::JsFuture::from(promise).await else { return };
                                let array = js_sys::Uint8Array::new(&buffer);
                                let data = array.to_vec();

                                let trigger = super::attachment::UploadTrigger {
                                    name: file_name,
                                    size: file_size,
                                    content_type: file_type,
                                    placeholder_id: pid,
                                    data,
                                };
                                upload_fn(trigger);
                            });
                        }
                    }
                }
                if handled_file {
                    return;
                }
            }

            let html_data = dt.get_data("text/html").unwrap_or_default();
            let has_kode_md = html_data.contains("data-kode-md");
            let plain = dt.get_data("text/plain").unwrap_or_default();

            let Ok(mut ds) = doc_paste.lock() else { return };

            if has_kode_md {
                if let Some(md) = extract_kode_markdown(&html_data) {
                    ds.insert_from_markdown(&md);
                }
            } else if !plain.is_empty() {
                ds.insert_from_markdown(&plain);
            }

            let new_md = ds.to_markdown();
            drop(ds);
            (notify_paste)(Some(new_md));
        });

        // Store closures for attachment after mount
        let closures = std::cell::RefCell::new(Some((beforeinput_cb, copy_cb, cut_cb, paste_cb)));

        // ── MutationObserver for DOM-observation text input ────────────
        let doc_mo = doc_state.clone();
        let on_change_mo = on_change.clone();
        let observing_mo = observing_dom_input.clone();
        let mo_store = mutation_observer.clone();
        let editor_mo = editor_ref;

        Effect::new(move |_| {
            let Some(el) = editor_input.get_untracked() else { return };
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

            // Create the MutationObserver for DOM-observation text input.
            let container_for_mo: web_sys::Element =
                editor_mo.get_untracked().unwrap().unchecked_ref::<web_sys::Element>().clone();
            let mo_cb = Closure::<dyn FnMut(js_sys::Array, web_sys::MutationObserver)>::new({
                let doc_mo = doc_mo.clone();
                let on_change_mo = on_change_mo.clone();
                let observing_mo = observing_mo.clone();
                let container_mo = container_for_mo.clone();
                let kd_handled_mo = keydown_handled_for_mo.clone();
                let dom_dirty_mo = dom_dirty_for_mo.clone();
                move |records: js_sys::Array, _observer: web_sys::MutationObserver| {
                    if !observing_mo.get() {
                        return;
                    }
                    observing_mo.set(false);

                    let Ok(mut ds) = doc_mo.lock() else { return };

                    // Collect unique positioned blocks that were affected by
                    // mutations. We diff the full block text (DOM vs DocState)
                    // once per block, which correctly handles multi-record
                    // mutations like macOS autocorrect replacements.
                    let mut seen_starts: Vec<usize> = Vec::new();
                    let mut changed = false;

                    for i in 0..records.length() {
                        let record: web_sys::MutationRecord = records.get(i).unchecked_into();
                        let Some(target_node) = record.target() else { continue };

                        let Some(pos_el) = find_pos_ancestor(&target_node, &container_mo) else {
                            continue;
                        };
                        let Some(el_start) = pos_el.get_attribute("data-pos-start")
                            .and_then(|s| s.parse::<usize>().ok()) else { continue };

                        if seen_starts.contains(&el_start) {
                            continue;
                        }
                        seen_starts.push(el_start);

                        let el_end = pos_el.get_attribute("data-pos-end")
                            .and_then(|s| s.parse::<usize>().ok())
                            .unwrap_or(el_start);

                        let dom_text = pos_el.text_content().unwrap_or_default();
                        let doc_text = ds.text_between(el_start, el_end)
                            .trim_end_matches('\n')
                            .to_string();

                        if dom_text == doc_text {
                            continue;
                        }

                        let (prefix, suffix) = diff_strings(&doc_text, &dom_text);
                        let doc_len = doc_text.chars().count();
                        let dom_len = dom_text.chars().count();

                        let del_from = el_start + prefix;
                        let del_to = el_start + doc_len - suffix;

                        if del_from != del_to {
                            ds.set_selection(Selection::range(del_from, del_to));
                        } else {
                            ds.set_selection(Selection::cursor(del_from));
                        }

                        let inserted: String = dom_text.chars()
                            .skip(prefix)
                            .take(dom_len - prefix - suffix)
                            .collect();

                        if !inserted.is_empty() {
                            ds.insert_text(&inserted);
                        } else if del_from != del_to {
                            ds.backspace();
                        }
                        changed = true;
                    }

                    if !changed {
                        return;
                    }

                    // ProseMirror-style: do NOT re-render the DOM. The browser
                    // already applied the text change correctly. Just update
                    // DocState, emit on_change, update formatting, and fix the
                    // stale data-pos attributes in-place. Mark the DOM as
                    // dirty so the next full re-render rewrites all blocks.
                    dom_dirty_mo.set(true);
                    let md = ds.to_markdown();
                    let fmt = ds.formatting_at_cursor();
                    drop(ds);

                    if let Some(ref cb) = on_change_mo {
                        cb(md);
                    }
                    formatting_state.set(fmt);

                    // Update data-pos attributes on all positioned elements so
                    // subsequent operations map positions correctly.
                    refresh_pos_attributes(&container_mo, &doc_mo);

                    // Clear the keydown-handled flag so selectionchange events
                    // are processed normally (e.g., arrow keys, clicks).
                    kd_handled_mo.set(false);
                }
            });

            let observer = web_sys::MutationObserver::new(mo_cb.as_ref().unchecked_ref())
                .expect("MutationObserver::new failed");
            let opts = web_sys::MutationObserverInit::new();
            opts.set_character_data(true);
            opts.set_character_data_old_value(true);
            opts.set_child_list(true);
            opts.set_subtree(true);
            let _ = observer.observe_with_options(&container_for_mo, &opts);
            *mo_store.borrow_mut() = Some(observer);

            let mo_cb_wrap = send_wrapper::SendWrapper::new(mo_cb);

            let bi_fn: js_sys::Function = bi_cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
            let cp_fn: js_sys::Function = cp_cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
            let ct_fn: js_sys::Function = ct_cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
            let pa_fn: js_sys::Function = pa_cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
            let bi_wrap = send_wrapper::SendWrapper::new(bi_cb);
            let cp_wrap = send_wrapper::SendWrapper::new(cp_cb);
            let ct_wrap = send_wrapper::SendWrapper::new(ct_cb);
            let pa_wrap = send_wrapper::SendWrapper::new(pa_cb);

            let mo_store_cleanup = send_wrapper::SendWrapper::new(mo_store.clone());
            let cleanup_el: web_sys::EventTarget = target.clone();
            on_cleanup(move || {
                let _ = cleanup_el.remove_event_listener_with_callback("beforeinput", &bi_fn);
                let _ = cleanup_el.remove_event_listener_with_callback("copy", &cp_fn);
                let _ = cleanup_el.remove_event_listener_with_callback("cut", &ct_fn);
                let _ = cleanup_el.remove_event_listener_with_callback("paste", &pa_fn);
                if let Some(ref mo) = *mo_store_cleanup.borrow() {
                    mo.disconnect();
                }
                drop(bi_wrap);
                drop(cp_wrap);
                drop(ct_wrap);
                drop(pa_wrap);
                drop(mo_cb_wrap);
            });
        });
    }

    // ── File upload drag-and-drop handlers ──────────────────────────────
    if let Some(ref upload_fn) = on_upload {
        let editor_dragover = editor_ref;
        let dragover_cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
            ev.prevent_default(); // Required to allow drop
            if let Some(el) = editor_dragover.get_untracked() {
                let _ = el.class_list().add_1("wysiwyg-drag-over");
            }
        });

        let editor_dragleave = editor_ref;
        let dragleave_cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
            ev.prevent_default();
            if let Some(el) = editor_dragleave.get_untracked() {
                let _ = el.class_list().remove_1("wysiwyg-drag-over");
            }
        });

        let doc_drop = doc_state.clone();
        let notify_drop = notify.clone();
        let upload_fn_drop = Arc::clone(upload_fn);
        let editor_drop = editor_ref;
        let drop_cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
            ev.prevent_default();
            if readonly { return; }

            // Remove drag-over class.
            if let Some(el) = editor_drop.get_untracked() {
                let _ = el.class_list().remove_1("wysiwyg-drag-over");
            }

            let ev: web_sys::DragEvent = ev.unchecked_into();
            let Some(dt) = ev.data_transfer() else { return };
            let Some(files) = dt.files() else { return };

            for i in 0..files.length() {
                let Some(file) = files.get(i) else { continue };

                let placeholder_id = format!("upload-{}-{}", js_sys::Date::now() as u64, i);

                let Ok(mut ds) = doc_drop.lock() else { return };
                let placeholder_attrs = {
                    let mut attrs = kode_doc::Attrs::new();
                    attrs.push(("placeholder_id".to_string(), kode_doc::AttrValue::String(placeholder_id.clone())));
                    attrs
                };
                let placeholder = kode_doc::Node::leaf_with_attrs(
                    NodeType::UploadPlaceholder,
                    placeholder_attrs,
                );
                let pos = ds.selection().from();
                ds.insert_block_node(pos, placeholder);
                let new_md = ds.to_markdown();
                drop(ds);
                (notify_drop)(Some(new_md));

                let upload_fn = Arc::clone(&upload_fn_drop);
                let file_name = file.name();
                let file_size = file.size() as u64;
                let file_type = file.type_();
                let pid = placeholder_id.clone();

                wasm_bindgen_futures::spawn_local(async move {
                    let promise = file.array_buffer();
                    let Ok(buffer) = wasm_bindgen_futures::JsFuture::from(promise).await else { return };
                    let array = js_sys::Uint8Array::new(&buffer);
                    let data = array.to_vec();

                    let trigger = super::attachment::UploadTrigger {
                        name: file_name,
                        size: file_size,
                        content_type: file_type,
                        placeholder_id: pid,
                        data,
                    };
                    upload_fn(trigger);
                });
            }
        });

        let upload_closures = std::cell::RefCell::new(Some((dragover_cb, dragleave_cb, drop_cb)));
        let editor_upload_attach = editor_ref;

        Effect::new(move |_| {
            let Some(el) = editor_upload_attach.get_untracked() else { return };
            let Some((dov_cb, dlv_cb, drp_cb)) = upload_closures.borrow_mut().take() else {
                return; // Already attached.
            };
            let target: &web_sys::EventTarget = el.as_ref();
            let _ = target.add_event_listener_with_callback("dragover", dov_cb.as_ref().unchecked_ref());
            let _ = target.add_event_listener_with_callback("dragleave", dlv_cb.as_ref().unchecked_ref());
            let _ = target.add_event_listener_with_callback("drop", drp_cb.as_ref().unchecked_ref());

            let dov_fn: js_sys::Function = dov_cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
            let dlv_fn: js_sys::Function = dlv_cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
            let drp_fn: js_sys::Function = drp_cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
            let dov_wrap = send_wrapper::SendWrapper::new(dov_cb);
            let dlv_wrap = send_wrapper::SendWrapper::new(dlv_cb);
            let drp_wrap = send_wrapper::SendWrapper::new(drp_cb);

            let cleanup_el: web_sys::EventTarget = target.clone();
            on_cleanup(move || {
                let _ = cleanup_el.remove_event_listener_with_callback("dragover", &dov_fn);
                let _ = cleanup_el.remove_event_listener_with_callback("dragleave", &dlv_fn);
                let _ = cleanup_el.remove_event_listener_with_callback("drop", &drp_fn);
                drop(dov_wrap);
                drop(dlv_wrap);
                drop(drp_wrap);
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

            let Some(container) = editor_move.get_untracked() else { return };
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

            let Some(container) = editor_up.get_untracked() else { return };
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
                let Some(container) = editor_flip.get_untracked() else { return };
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

            let Some(container) = editor_cancel.get_untracked() else { return };
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
            let Some(el) = editor_attach.get_untracked() else { return };
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
    let on_delete_attachment_key = on_delete_attachment.clone();
    let extension_shortcuts_key: Arc<Vec<ExtensionKeyboardShortcut>> = Arc::clone(&extension_shortcuts);
    let keydown_handled_key = keydown_handled.clone();
    let ext_actions_key = Arc::clone(&ext_actions);
    let slash_menu_items_key = Arc::clone(&slash_menu_items);
    let slash_trigger_for_scroll = slash_trigger_el.clone();
    let on_keydown = move |ev: KeyboardEvent| {
        if composing.get_untracked() {
            return;
        }

        let ctrl = ev.ctrl_key() || ev.meta_key();
        let key = ev.key();

        if readonly {
            if ctrl && key == "c" {
                return;
            }
            match key.as_str() {
                "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight"
                | "Home" | "End" | "PageUp" | "PageDown"
                | "Tab" | "Escape" | "Shift" => return,
                _ => {}
            }
            ev.prevent_default();
            return;
        }

        let shift = ev.shift_key();

        // Clipboard shortcuts: let copy/cut/paste events handle these.
        match key.as_str() {
            "c" | "x" | "v" if ctrl => return,
            _ => {}
        }

        // ── Slash menu interaction ──────────────────────────────────────
        if slash_menu_state.get_untracked().is_some() {
            match key.as_str() {
                "ArrowDown" => {
                    slash_menu_index.update(|i| *i = (*i + 1) % slash_item_count);
                    ev.prevent_default();
                    return;
                }
                "ArrowUp" => {
                    slash_menu_index.update(|i| {
                        *i = if *i == 0 { slash_item_count - 1 } else { *i - 1 };
                    });
                    ev.prevent_default();
                    return;
                }
                "Enter" => {
                    let idx = slash_menu_index.get_untracked();
                    slash_menu_state.set(None);
                    if idx < slash_item_count {
                        let Ok(mut ds) = doc_key.lock() else { return };
                        match &slash_menu_items_key[idx] {
                            SlashMenuItem::Builtin { button, .. } => {
                                dispatch_builtin_action(&mut ds, *button);
                            }
                            SlashMenuItem::Extension { ext_index, .. } => {
                                if let Some(action) = ext_actions_key.get(*ext_index) {
                                    apply_md_command(&mut ds, |e| { (action)(e); });
                                } else {
                                    drop(ds);
                                    ev.prevent_default();
                                    return;
                                }
                            }
                        }
                        let md = ds.to_markdown();
                        drop(ds);
                        (notify_key)(Some(md));
                    }
                    ev.prevent_default();
                    return;
                }
                "Escape" | "/" => {
                    slash_menu_state.set(None);
                    ev.prevent_default();
                    return;
                }
                _ => {
                    slash_menu_state.set(None);
                }
            }
        }

        // ── Table context menu dismissal on Escape ─────────────────────
        if key == "Escape" && table_ctx_menu_pos.get_untracked().is_some() {
            ev.prevent_default();
            table_ctx_menu_pos.set(None);
            return;
        }

        // ── Table picker dismissal on Escape ──────────────────────────
        if key == "Escape" && table_picker_pos.get_untracked().is_some() {
            ev.prevent_default();
            table_picker_pos.set(None);
            return;
        }

        // ── Slash menu trigger: "/" on empty block ──────────────────────
        if show_slash_menu && key == "/" && !ctrl && !shift && slash_item_count > 0
            && editor_ref.get_untracked().is_some() {
                let Ok(ds) = doc_key.lock() else { return };
                let pos = ds.selection().head;
                let resolved = ds.doc().resolve(pos);
                if resolved.parent().node_type.is_textblock()
                    && resolved.parent().content.size() == 0
                {
                    let Some(window) = web_sys::window() else { return };
                    let Some(sel) = window.get_selection().ok().flatten() else { return };
                    let trigger = sel.focus_node()
                        .and_then(|n| n.dyn_ref::<web_sys::Element>().cloned()
                            .or_else(|| n.parent_element()));
                    if let Some(el) = trigger {
                        *slash_trigger_el.borrow_mut() = Some(el.clone());
                        drop(ds);
                        slash_menu_index.set(0);
                        slash_menu_state.set(compute_slash_menu_pos(&el));
                        ev.prevent_default();
                        return;
                    }
                    drop(ds);
                }
            }

        // Undo/Redo
        match key.as_str() {
            "z" if ctrl && shift => {
                // Sync selection from browser before redo
                if let Some(container) = editor_ref.get_untracked() {
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
                if let Some(container) = editor_ref.get_untracked() {
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
                // Check if backspace will delete an attachment block.
                if let Some(ref cb) = on_delete_attachment_key {
                    let pos = ds.selection().from();
                    let resolved = ds.doc().resolve(pos);
                    if !resolved.parent().node_type.is_textblock() {
                        // At a gap position, node_before might be an attachment.
                        if let Some(node_before) = resolved.node_before() {
                            if matches!(node_before.node_type, NodeType::ImageBlock | NodeType::FileBlock) {
                                let req = build_delete_request(node_before);
                                cb(req);
                            }
                        }
                    } else if resolved.parent_offset == 0 && resolved.parent().content.size() > 0 {
                        // At start of non-empty textblock: join_backward will delete a preceding atom.
                        // (Empty paragraphs are deleted themselves, not the adjacent attachment.)
                        let before_pos = resolved.before(resolved.depth);
                        if before_pos > 0 {
                            let before_resolved = ds.doc().resolve(before_pos);
                            if let Some(prev_node) = before_resolved.node_before() {
                                if matches!(prev_node.node_type, NodeType::ImageBlock | NodeType::FileBlock) {
                                    let req = build_delete_request(prev_node);
                                    cb(req);
                                }
                            }
                        }
                    }
                }
                ds.backspace();
                true
            }
            "Delete" => {
                // Check if delete-forward will delete an attachment block.
                if let Some(ref cb) = on_delete_attachment_key {
                    let pos = ds.selection().from();
                    let resolved = ds.doc().resolve(pos);
                    if !resolved.parent().node_type.is_textblock() {
                        // Gap position: node_after might be an attachment.
                        if let Some(node_after) = resolved.node_after() {
                            if matches!(node_after.node_type, NodeType::ImageBlock | NodeType::FileBlock) {
                                let req = build_delete_request(node_after);
                                cb(req);
                            }
                        }
                    } else if resolved.parent_offset == resolved.parent().content.size()
                        && resolved.parent().content.size() > 0
                    {
                        // End of non-empty textblock: next sibling block might be an attachment.
                        // (Empty paragraphs are deleted themselves, not the adjacent attachment.)
                        let after_pos = resolved.after(resolved.depth);
                        if after_pos < ds.doc().content.size() {
                            let after_resolved = ds.doc().resolve(after_pos);
                            if let Some(next_node) = after_resolved.node_after() {
                                if matches!(next_node.node_type, NodeType::ImageBlock | NodeType::FileBlock) {
                                    let req = build_delete_request(next_node);
                                    cb(req);
                                }
                            }
                        }
                    }
                }
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
            "ArrowRight" if ctrl && shift => {
                let in_table = {
                    let resolved = ds.doc().resolve(ds.selection().head);
                    (0..=resolved.depth).rev().any(|d| {
                        resolved.node(d).node_type == kode_doc::NodeType::TableCell
                    })
                };
                if in_table {
                    ds.insert_column_right();
                    true
                } else {
                    false
                }
            }
            "ArrowLeft" if ctrl && shift => {
                let in_table = {
                    let resolved = ds.doc().resolve(ds.selection().head);
                    (0..=resolved.depth).rev().any(|d| {
                        resolved.node(d).node_type == kode_doc::NodeType::TableCell
                    })
                };
                if in_table {
                    ds.insert_column_left();
                    true
                } else {
                    false
                }
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
        if readonly { return; }
        composing.set(true);
    };
    let doc_comp_end = doc_state.clone();
    let notify_comp = notify.clone();
    let on_composition_end = move |ev: CompositionEvent| {
        if readonly { return; }
        composing.set(false);
        if let Some(data) = ev.data() {
            if !data.is_empty() {
                let Ok(mut ds) = doc_comp_end.lock() else { return };
                // Sync selection from browser to know where to insert the composed text.
                if let Some(container) = editor_ref.get_untracked() {
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
        let mouse_selecting_selchange = mouse_selecting.clone();
        let selchange_cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_ev: web_sys::Event| {
            if composing.get_untracked() {
                return;
            }
            // After a keydown edit, the DocState position is authoritative.
            // Skip sync until the rAF restores the cursor.
            if kd_handled_selchange.get() {
                return;
            }
            let Some(container) = editor_selchange.get_untracked() else { return };
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

            if mouse_selecting_selchange.get() {
                return;
            }

            if show_floating_toolbar && !sel.is_collapsed() && sel.range_count() > 0 {
                if let Ok(range) = sel.get_range_at(0) {
                    let range_rect = range.get_bounding_client_rect();
                    let parent = container_el.parent_element();
                    let parent_rect = parent.as_ref()
                        .map(|p| p.get_bounding_client_rect())
                        .unwrap_or_else(|| container_el.get_bounding_client_rect());

                    let pos = compute_floating_toolbar_pos(&range_rect, &parent_rect);
                    floating_pos.set(Some(pos));
                }
            } else {
                floating_pos.set(None);
            }
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

    // ── Mouse selection tracking for floating toolbar ──────────────────────
    if show_floating_toolbar && !readonly {
        let mouse_selecting_down = mouse_selecting.clone();
        let mouse_selecting_up = mouse_selecting.clone();
        let editor_mousedown = editor_ref;
        let editor_mouseup = editor_ref;

        let mousedown_cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
            let Some(container) = editor_mousedown.get_untracked() else { return };
            let container_el: &web_sys::Element = container.as_ref();
            let Some(target) = ev.target() else { return };
            let Some(target_node) = target.dyn_ref::<web_sys::Node>() else { return };
            if !container_el.contains(Some(target_node)) {
                return;
            }
            mouse_selecting_down.set(true);
            floating_pos.set(None);
        });

        let mouseup_cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_ev: web_sys::Event| {
            if !mouse_selecting_up.get() {
                return;
            }
            mouse_selecting_up.set(false);

            let Some(container) = editor_mouseup.get_untracked() else { return };
            let container_el: &web_sys::Element = container.as_ref();
            let Some(window) = web_sys::window() else { return };
            let Some(sel) = window.get_selection().ok().flatten() else { return };

            if sel.is_collapsed() || sel.range_count() == 0 {
                return;
            }
            let Some(focus_node) = sel.focus_node() else { return };
            if !container_el.contains(Some(&focus_node)) {
                return;
            }

            if let Ok(range) = sel.get_range_at(0) {
                let range_rect = range.get_bounding_client_rect();
                let parent = container_el.parent_element();
                let parent_rect = parent.as_ref()
                    .map(|p| p.get_bounding_client_rect())
                    .unwrap_or_else(|| container_el.get_bounding_client_rect());

                let pos = compute_floating_toolbar_pos(&range_rect, &parent_rect);
                floating_pos.set(Some(pos));
            }
        });

        if let Some(document) = web_sys::window().and_then(|w| w.document()) {
            let _ = document.add_event_listener_with_callback(
                "mousedown",
                mousedown_cb.as_ref().unchecked_ref(),
            );
            let _ = document.add_event_listener_with_callback(
                "mouseup",
                mouseup_cb.as_ref().unchecked_ref(),
            );
        }

        let mousedown_fn: js_sys::Function = mousedown_cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
        let mouseup_fn: js_sys::Function = mouseup_cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
        let mousedown_cb = send_wrapper::SendWrapper::new(mousedown_cb);
        let mouseup_cb = send_wrapper::SendWrapper::new(mouseup_cb);
        on_cleanup(move || {
            if let Some(document) = web_sys::window().and_then(|w| w.document()) {
                let _ = document.remove_event_listener_with_callback("mousedown", &mousedown_fn);
                let _ = document.remove_event_listener_with_callback("mouseup", &mouseup_fn);
            }
            drop(mousedown_cb);
            drop(mouseup_cb);
        });
    }

    // ── Toolbar ───────────────────────────────────────────────────────────
    let toolbar_view = if show_fixed_toolbar && !readonly {
        let items = toolbar_items.unwrap_or_else(|| {
            let mut items = default_toolbar_items();
            for ext_item in extension_toolbar_items {
                items.push(ToolbarItem::Separator);
                items.push(ToolbarItem::ExtensionButton(ext_item));
            }
            items
        });

        let popover_triggers = ToolbarPopoverTriggers { link_popup_open, table_picker_open };
        let toolbar_views = render_toolbar_items(
            items, &doc_state, &notify, editor_ref, formatting_state, extension_active_state,
            popover_triggers,
        );

        Some(view! {
            <div class="kode-toolbar">
                {toolbar_views}
            </div>
        }.into_any())
    } else {
        None
    };

    // ── Floating toolbar ────────────────────────────────────────────────
    let floating_toolbar_view = if show_floating_toolbar && !readonly {
        let items = floating_toolbar_items.unwrap_or_else(default_toolbar_items);
        let popover_triggers = ToolbarPopoverTriggers { link_popup_open, table_picker_open };
        let toolbar_views = render_toolbar_items(
            items, &doc_state, &notify, editor_ref, formatting_state, extension_active_state,
            popover_triggers,
        );
        Some(view! {
            <div class="kode-floating-toolbar"
                style=move || {
                    match floating_pos.get() {
                        Some((top, left, flipped)) => {
                            let transform = if flipped {
                                "transform:translateX(-50%);"
                            } else {
                                "transform:translate(-50%,-100%);"
                            };
                            format!("display:flex;top:{top}px;left:{left}px;{transform}")
                        }
                        None => "display:none;".to_string(),
                    }
                }
                on:mousedown=|ev: MouseEvent| { ev.prevent_default(); }>
                {toolbar_views}
            </div>
        }.into_any())
    } else {
        None
    };

    // ── Slash menu view ───────────────────────────────────────────────
    let slash_menu_view = if show_slash_menu && !slash_menu_items.is_empty() && !readonly {
        let menu_items_view: Vec<AnyView> = slash_menu_items.iter().enumerate().map(|(i, item)| {
            let (icon_svg, icon_text, name, desc) = match item {
                SlashMenuItem::Builtin { button, label, description } => (
                    button.icon_svg().map(|s| s.to_string()),
                    button.label().to_string(),
                    label.to_string(),
                    description.to_string(),
                ),
                SlashMenuItem::Extension { label, description, .. } => (
                    None, label.clone(), label.clone(), description.clone(),
                ),
            };
            let doc_click = doc_state.clone();
            let notify_click = notify.clone();
            let items_click = Arc::clone(&slash_menu_items);
            let ext_actions_click = Arc::clone(&ext_actions);
            let on_click = move |_: MouseEvent| {
                slash_menu_state.set(None);
                let Ok(mut ds) = doc_click.lock() else { return };
                match &items_click[i] {
                    SlashMenuItem::Builtin { button, .. } => {
                        dispatch_builtin_action(&mut ds, *button);
                    }
                    SlashMenuItem::Extension { ext_index, .. } => {
                        if let Some(action) = ext_actions_click.get(*ext_index) {
                            apply_md_command(&mut ds, |e| { (action)(e); });
                        } else {
                            drop(ds);
                            return;
                        }
                    }
                }
                let md = ds.to_markdown();
                drop(ds);
                (notify_click)(Some(md));
            };
            view! {
                <div class=move || {
                        if slash_menu_index.get() == i { "kode-slash-menu-item selected" }
                        else { "kode-slash-menu-item" }
                    }
                    on:click=on_click
                    on:mousedown=|ev: MouseEvent| { ev.prevent_default(); }>
                    {if let Some(ref svg) = icon_svg {
                        view! { <span class="kode-slash-menu-item-icon" inner_html=svg.clone() /> }.into_any()
                    } else {
                        view! { <span class="kode-slash-menu-item-icon">{icon_text.clone()}</span> }.into_any()
                    }}
                    <span class="kode-slash-menu-item-name">{name.clone()}</span>
                    <span class="kode-slash-menu-item-desc">{desc.clone()}</span>
                </div>
            }.into_any()
        }).collect();

        Some(view! {
            <div class="kode-slash-menu"
                style=move || {
                    match slash_menu_state.get() {
                        Some((top, max_h, left, flip)) => {
                            let transform = if flip { "transform:translateY(-100%);" } else { "" };
                            format!("display:block;top:{top}px;left:{left}px;max-height:{max_h}px;{transform}")
                        }
                        None => "display:none;".to_string(),
                    }
                }
                on:mousedown=|ev: MouseEvent| { ev.prevent_default(); }>
                {menu_items_view}
            </div>
        }.into_any())
    } else {
        None
    };

    // ── Link popover view ──────────────────────────────────────────────
    let link_popover_view = if !readonly {
        let link_input_ref = NodeRef::<leptos::html::Input>::new();

        // When toolbar Link button fires, compute position from selection.
        {
            let doc_state_trigger = doc_state.clone();
            Effect::new(move || {
                if !link_popup_open.get() { return; }
                link_popup_open.set(false);

                let Some(container) = editor_ref.get_untracked() else { return };
                let container_el: &web_sys::Element = container.as_ref();
                let Some(parent) = container_el.parent_element() else { return };
                let parent_rect = parent.get_bounding_client_rect();

                // Check if cursor is already on a link (edit mode).
                if let Ok(ds) = doc_state_trigger.lock() {
                    if let Some((href, from, to)) = ds.link_at_cursor() {
                        drop(ds);
                        let Some(window) = web_sys::window() else { return };
                        let Some(sel) = window.get_selection().ok().flatten() else { return };
                        if sel.range_count() > 0 {
                            if let Ok(range) = sel.get_range_at(0) {
                                let rect = range.get_bounding_client_rect();
                                if let Some(pos) = compute_position_relative(&rect, &parent_rect, 60.0) {
                                    link_popover_url.set(href);
                                    link_popover_edit_range.set(Some((from, to)));
                                    floating_pos.set(None);
                                    link_popover_pos.set(Some((pos.top, pos.left, pos.flipped)));
                                }
                            }
                        }
                        return;
                    }
                    drop(ds);
                }

                // Insert mode — position from selection range.
                let Some(window) = web_sys::window() else { return };
                let Some(sel) = window.get_selection().ok().flatten() else { return };
                if sel.range_count() > 0 {
                    if let Ok(range) = sel.get_range_at(0) {
                        let rect = range.get_bounding_client_rect();
                        if let Some(pos) = compute_position_relative(&rect, &parent_rect, 60.0) {
                            link_popover_url.set(String::new());
                            link_popover_edit_range.set(None);
                            floating_pos.set(None);
                            link_popover_pos.set(Some((pos.top, pos.left, pos.flipped)));
                        }
                    }
                }
            });
        }

        // Auto-focus input when popover opens.
        Effect::new(move || {
            if link_popover_pos.get().is_some() {
                if let Some(input) = link_input_ref.get() {
                    let _ = input.focus();
                }
            }
        });

        // SVG icons for buttons.
        const CHECK_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>"#;
        const CLOSE_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>"#;
        const UNLINK_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m18.84 12.25 1.72-1.71a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="m5.17 11.75-1.71 1.71a5 5 0 0 0 7.07 7.07l1.71-1.71"/><line x1="8" y1="2" x2="8" y2="5"/><line x1="2" y1="8" x2="5" y2="8"/><line x1="16" y1="19" x2="16" y2="22"/><line x1="19" y1="16" x2="22" y2="16"/></svg>"#;

        let doc_key = doc_state.clone();
        let notify_key = notify.clone();
        let doc_apply_btn = doc_state.clone();
        let notify_apply_btn = notify.clone();
        let doc_unlink = doc_state.clone();
        let notify_unlink = notify.clone();

        Some(view! {
            <div class="kode-link-popover"
                style=move || {
                    match link_popover_pos.get() {
                        Some((top, left, flipped)) => {
                            let transform = if flipped { "transform:translateY(-100%);" } else { "" };
                            format!("display:flex;top:{top}px;left:{left}px;{transform}")
                        }
                        None => "display:none;".to_string(),
                    }
                }
                on:mousedown=|ev: MouseEvent| { ev.prevent_default(); }>
                <input
                    node_ref=link_input_ref
                    type="text"
                    class="kode-link-popover-input"
                    placeholder="https://example.com"
                    prop:value=move || link_popover_url.get()
                    on:input=move |ev| {
                        link_popover_url.set(event_target_value(&ev));
                    }
                    on:keydown=move |ev: web_sys::KeyboardEvent| {
                        if ev.key() == "Enter" {
                            ev.prevent_default();
                            let url = link_popover_url.get_untracked();
                            if !url.trim().is_empty() && url.trim() != "https://" {
                                let Ok(mut ds) = doc_key.lock() else { return };
                                if let Some((from, to)) = link_popover_edit_range.get_untracked() {
                                    ds.update_link(from, to, &url);
                                } else {
                                    ds.insert_link(&url);
                                }
                                let md = ds.to_markdown();
                                drop(ds);
                                (notify_key)(Some(md));
                            }
                            link_popover_pos.set(None);
                            link_popover_url.set(String::new());
                            link_popover_edit_range.set(None);
                            if let Some(el) = editor_ref.get_untracked() {
                                let el: &web_sys::HtmlElement = el.as_ref();
                                let _ = el.focus();
                            }
                        } else if ev.key() == "Escape" {
                            ev.prevent_default();
                            link_popover_pos.set(None);
                            link_popover_url.set(String::new());
                            link_popover_edit_range.set(None);
                            if let Some(el) = editor_ref.get_untracked() {
                                let el: &web_sys::HtmlElement = el.as_ref();
                                let _ = el.focus();
                            }
                        }
                    }
                />
                <button class="kode-link-popover-btn apply"
                    title="Apply"
                    inner_html=CHECK_SVG
                    on:click=move |_: MouseEvent| {
                        let url = link_popover_url.get_untracked();
                        if !url.trim().is_empty() && url.trim() != "https://" {
                            let Ok(mut ds) = doc_apply_btn.lock() else { return };
                            if let Some((from, to)) = link_popover_edit_range.get_untracked() {
                                ds.update_link(from, to, &url);
                            } else {
                                ds.insert_link(&url);
                            }
                            let md = ds.to_markdown();
                            drop(ds);
                            (notify_apply_btn)(Some(md));
                        }
                        link_popover_pos.set(None);
                        link_popover_url.set(String::new());
                        link_popover_edit_range.set(None);
                        if let Some(el) = editor_ref.get_untracked() {
                            let el: &web_sys::HtmlElement = el.as_ref();
                            let _ = el.focus();
                        }
                    } />
                {move || {
                    if link_popover_edit_range.get().is_some() {
                        let doc_u = doc_unlink.clone();
                        let notify_u = notify_unlink.clone();
                        Some(view! {
                            <button class="kode-link-popover-btn unlink"
                                title="Remove link"
                                inner_html=UNLINK_SVG
                                on:click=move |_: MouseEvent| {
                                    if let Some((from, to)) = link_popover_edit_range.get_untracked() {
                                        let Ok(mut ds) = doc_u.lock() else { return };
                                        ds.remove_link(from, to);
                                        let md = ds.to_markdown();
                                        drop(ds);
                                        (notify_u)(Some(md));
                                    }
                                    link_popover_pos.set(None);
                                    link_popover_url.set(String::new());
                                    link_popover_edit_range.set(None);
                                    if let Some(el) = editor_ref.get_untracked() {
                                        let el: &web_sys::HtmlElement = el.as_ref();
                                        let _ = el.focus();
                                    }
                                } />
                        })
                    } else {
                        None
                    }
                }}
                <button class="kode-link-popover-btn cancel"
                    title="Cancel"
                    inner_html=CLOSE_SVG
                    on:click=move |_: MouseEvent| {
                        link_popover_pos.set(None);
                        link_popover_url.set(String::new());
                        link_popover_edit_range.set(None);
                        if let Some(el) = editor_ref.get_untracked() {
                            let el: &web_sys::HtmlElement = el.as_ref();
                            let _ = el.focus();
                        }
                    } />
            </div>
        }.into_any())
    } else {
        None
    };

    // ── Table picker popover view ──────────────────────────────────────
    let table_picker_view = if !readonly {
        // When toolbar Table button fires, compute position from selection.
        {
            Effect::new(move || {
                if !table_picker_open.get() { return; }
                table_picker_open.set(false);
                table_hover.set((1, 1));

                let Some(container) = editor_ref.get_untracked() else { return };
                let container_el: &web_sys::Element = container.as_ref();
                let Some(parent) = container_el.parent_element() else { return };
                let parent_rect = parent.get_bounding_client_rect();

                let Some(window) = web_sys::window() else { return };
                let Some(sel) = window.get_selection().ok().flatten() else { return };
                if sel.range_count() > 0 {
                    if let Ok(range) = sel.get_range_at(0) {
                        let rect = range.get_bounding_client_rect();
                        if let Some(pos) = compute_position_relative(&rect, &parent_rect, 200.0) {
                            floating_pos.set(None);
                            table_picker_pos.set(Some((pos.top, pos.left, pos.flipped)));
                        }
                    }
                }
            });
        }

        let doc_pick = doc_state.clone();
        let notify_pick = notify.clone();

        Some(view! {
            <div class="kode-table-picker"
                style=move || {
                    match table_picker_pos.get() {
                        Some((top, left, flipped)) => {
                            let transform = if flipped { "transform:translateY(-100%);" } else { "" };
                            format!("display:block;top:{top}px;left:{left}px;{transform}")
                        }
                        None => "display:none;".to_string(),
                    }
                }
                on:mousedown=|ev: MouseEvent| { ev.prevent_default(); }>
                <div class="kode-table-picker-grid">
                    {(0..6).map(|row| {
                        view! {
                            <div class="kode-table-picker-row">
                                {(0..6).map(|col| {
                                    let doc_cell = doc_pick.clone();
                                    let notify_cell = notify_pick.clone();
                                    view! {
                                        <div
                                            class=move || {
                                                let (hc, hr) = table_hover.get();
                                                if col < hc && row < hr {
                                                    "kode-table-picker-cell selected"
                                                } else {
                                                    "kode-table-picker-cell"
                                                }
                                            }
                                            on:mouseenter=move |_| {
                                                table_hover.set((col + 1, row + 1));
                                            }
                                            on:click=move |_: MouseEvent| {
                                                let (cols, rows) = table_hover.get_untracked();
                                                let Ok(mut ds) = doc_cell.lock() else { return };
                                                ds.insert_table(cols, rows);
                                                let md = ds.to_markdown();
                                                drop(ds);
                                                (notify_cell)(Some(md));
                                                table_picker_pos.set(None);
                                                if let Some(el) = editor_ref.get_untracked() {
                                                    let el: &web_sys::HtmlElement = el.as_ref();
                                                    let _ = el.focus();
                                                }
                                            }
                                        />
                                    }
                                }).collect::<Vec<_>>()}
                            </div>
                        }
                    }).collect::<Vec<_>>()}
                </div>
                <div class="kode-table-picker-label">
                    {move || {
                        let (c, r) = table_hover.get();
                        format!("{c} \u{00d7} {r}")
                    }}
                </div>
            </div>
        }.into_any())
    } else {
        None
    };

    // ── Table context menu view ────────────────────────────────────────
    let table_ctx_menu_view = if !readonly {
        let make_ctx_handler = |op: fn(&mut DocState)| {
            let doc = doc_state.clone();
            let ntfy = notify.clone();
            move |_: MouseEvent| {
                let Ok(mut ds) = doc.lock() else { return };
                op(&mut ds);
                let md = ds.to_markdown();
                drop(ds);
                (ntfy)(Some(md));
                table_ctx_menu_pos.set(None);
                if let Some(el) = editor_ref.get_untracked() {
                    let el: &web_sys::HtmlElement = el.as_ref();
                    let _ = el.focus();
                }
            }
        };

        let on_insert_row_above = make_ctx_handler(DocState::insert_row_above);
        let on_insert_row_below = make_ctx_handler(DocState::insert_row_below);
        let on_insert_col_left = make_ctx_handler(DocState::insert_column_left);
        let on_insert_col_right = make_ctx_handler(DocState::insert_column_right);
        let on_delete_row = make_ctx_handler(DocState::delete_row);
        let on_delete_col = make_ctx_handler(DocState::delete_column);
        let on_delete_table = make_ctx_handler(DocState::delete_table);

        Some(view! {
            <div class="kode-table-context-menu"
                style=move || {
                    match table_ctx_menu_pos.get() {
                        Some((top, left)) => format!("display:block;top:{top}px;left:{left}px;"),
                        None => "display:none;".to_string(),
                    }
                }
                on:mousedown=|ev: MouseEvent| { ev.prevent_default(); }>
                <div class="kode-ctx-menu-item" on:click=on_insert_row_above>"Insert row above"</div>
                <div class="kode-ctx-menu-item" on:click=on_insert_row_below>"Insert row below"</div>
                <div class="kode-ctx-menu-separator" />
                <div class="kode-ctx-menu-item" on:click=on_insert_col_left>"Insert column left"</div>
                <div class="kode-ctx-menu-item" on:click=on_insert_col_right>"Insert column right"</div>
                <div class="kode-ctx-menu-separator" />
                <div class="kode-ctx-menu-item" on:click=on_delete_row>"Delete row"</div>
                <div class="kode-ctx-menu-item" on:click=on_delete_col>"Delete column"</div>
                <div class="kode-ctx-menu-separator" />
                <div class="kode-ctx-menu-item destructive" on:click=on_delete_table>"Delete table"</div>
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

        // Prevent focus loss when mousedown lands on the copy button,
        // table add controls, or cursor placement when Ctrl/Cmd+clicking a link.
        let mut walk = Some(target_el.clone());
        while let Some(ref current) = walk {
            if current.class_list().contains("wysiwyg-code-copy")
                || current.class_list().contains("kode-table-add-row-btn")
                || current.class_list().contains("kode-table-add-col-btn")
            {
                ev.prevent_default();
                return;
            }
            if !readonly
                && (ev.ctrl_key() || ev.meta_key())
                && current.tag_name().eq_ignore_ascii_case("A")
                && current.class_list().contains("wysiwyg-link")
            {
                ev.prevent_default();
                return;
            }
            if current.class_list().contains("wysiwyg-scroll-container") {
                break;
            }
            walk = current.parent_element();
        }

        if readonly { return; }

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
            if let Some(container) = editor_ref.get_untracked() {
                let container_el: &web_sys::Element = container.as_ref();
                show_gap_cursor(container_el, side, bs, be);
                let _ = container.dyn_ref::<HtmlElement>().map(|el| {
                    let _ = el.style().set_property("caret-color", "transparent");
                    let _ = el.focus();
                });
            }
        } else {
            drop(ds);
            if let Some(container) = editor_ref.get_untracked() {
                let _ = container.dyn_ref::<HtmlElement>().map(|el| el.focus());
            }
        }
        version.update(|v| *v += 1);
    };

    // ── Render ───────────────────────────────────────────────────────────
    let theme_css = move || theme.get().syntax_css("pre.kode-content");

    // ── Right-click context menu for tables ────────────────────────────
    let doc_ctx = doc_state.clone();
    let on_contextmenu = {
        move |ev: MouseEvent| {
            // Dismiss any existing menu first.
            table_ctx_menu_pos.set(None);
            if readonly { return; }
            let Some(target) = ev.target() else { return };
            let Some(target_el) = target.dyn_ref::<web_sys::Element>() else { return };
            let mut el = Some(target_el.clone());
            let mut cell_el: Option<web_sys::Element> = None;
            while let Some(ref current) = el {
                if current.tag_name().eq_ignore_ascii_case("TD")
                    || current.tag_name().eq_ignore_ascii_case("TH")
                {
                    cell_el = Some(current.clone());
                    break;
                }
                if current.class_list().contains("wysiwyg-scroll-container") {
                    break;
                }
                el = current.parent_element();
            }
            let Some(cell) = cell_el else { return };

            ev.prevent_default();

            // Sync DocState cursor to the right-clicked cell so operations
            // target the correct row/column.
            if let Some(pos_str) = cell.get_attribute("data-pos-start") {
                if let Ok(pos) = pos_str.parse::<usize>() {
                    if let Ok(mut ds) = doc_ctx.lock() {
                        ds.set_selection(Selection::cursor(pos + 1));
                    }
                }
            }

            let Some(container) = editor_ref.get_untracked() else { return };
            let container_el: &web_sys::Element = container.as_ref();
            let Some(parent) = container_el.parent_element() else { return };
            let parent_rect = parent.get_bounding_client_rect();
            let top = ev.client_y() as f64 - parent_rect.top();
            let left = ev.client_x() as f64 - parent_rect.left();
            table_ctx_menu_pos.set(Some((top, left)));
        }
    };

    // Clone attachment callbacks + doc_state for the click handler closure.
    let on_delete_attachment_click = on_delete_attachment.clone();
    let on_click_attachment_click = on_click_attachment.clone();
    let doc_state_click = doc_state.clone();
    let on_change_click = on_change.clone();

    view! {
        <style>{theme_css}</style>
        <style>{include_str!("../wysiwyg.css")}</style>
        <div class={if readonly { "wysiwyg-container wysiwyg-readonly" } else { "wysiwyg-container" }}
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
                class=if show_slash_menu { "wysiwyg-scroll-container tree-wysiwyg-scroll-container kode-slash-enabled" } else { "wysiwyg-scroll-container tree-wysiwyg-scroll-container" }
                contenteditable={(!readonly).then_some("true")}
                tabindex={readonly.then_some("0")}
                spellcheck="false"
                on:keydown=on_keydown
                on:mousedown=on_mousedown
                on:contextmenu=on_contextmenu
                on:click=move |ev: MouseEvent| {
                    let Some(target) = ev.target() else { return };
                    let Some(target_el) = target.dyn_ref::<web_sys::Element>() else { return };

                    // Dismiss table context menu on any click outside it.
                    if table_ctx_menu_pos.get_untracked().is_some() {
                        table_ctx_menu_pos.set(None);
                    }

                    // Walk up from click target to identify the action.
                    let mut el = Some(target_el.clone());
                    let mut copy_btn: Option<web_sys::Element> = None;
                    let mut delete_btn: Option<web_sys::Element> = None;
                    let mut attachment_block: Option<web_sys::Element> = None;
                    let mut link_el: Option<web_sys::Element> = None;
                    let mut table_add_row: Option<web_sys::Element> = None;
                    let mut table_add_col: Option<web_sys::Element> = None;
                    while let Some(ref current) = el {
                        if copy_btn.is_none() && current.class_list().contains("wysiwyg-code-copy") {
                            copy_btn = Some(current.clone());
                            break;
                        }
                        if current.class_list().contains("kode-table-add-row-btn") {
                            table_add_row = Some(current.clone());
                            break;
                        }
                        if current.class_list().contains("kode-table-add-col-btn") {
                            table_add_col = Some(current.clone());
                            break;
                        }
                        if delete_btn.is_none() && current.class_list().contains("wysiwyg-attachment-delete") {
                            delete_btn = Some(current.clone());
                        }
                        if attachment_block.is_none()
                            && (current.class_list().contains("wysiwyg-image-block")
                                || current.class_list().contains("wysiwyg-file-block"))
                        {
                            attachment_block = Some(current.clone());
                            break;
                        }
                        if link_el.is_none()
                            && current.tag_name().eq_ignore_ascii_case("A")
                            && current.class_list().contains("wysiwyg-link")
                        {
                            link_el = Some(current.clone());
                        }
                        if current.class_list().contains("wysiwyg-scroll-container") {
                            break;
                        }
                        el = current.parent_element();
                    }

                    // ── Table add-row button ────────────────────────────────
                    if let Some(ref btn) = table_add_row {
                        if readonly { return; }
                        ev.prevent_default();
                        ev.stop_propagation();
                        // Button is a sibling of the table inside .kode-table-wrapper
                        if let Some(wrapper) = btn.closest(".kode-table-wrapper").ok().flatten() {
                            if let Some(table) = wrapper.query_selector("table.wysiwyg-table").ok().flatten() {
                                if let Some(pos) = find_last_cell_pos(&table) {
                                    let Ok(mut ds) = doc_state_click.lock() else { return };
                                    ds.set_selection(Selection::cursor(pos));
                                    ds.insert_row_below();
                                    let md = ds.to_markdown();
                                    drop(ds);
                                    version.update(|v| *v += 1);
                                    if let Some(ref cb) = on_change_click {
                                        cb(md);
                                    }
                                }
                            }
                        }
                        return;
                    }

                    // ── Table add-column button ─────────────────────────────
                    if let Some(ref btn) = table_add_col {
                        if readonly { return; }
                        ev.prevent_default();
                        ev.stop_propagation();
                        // Button is a sibling of the table inside .kode-table-wrapper
                        if let Some(wrapper) = btn.closest(".kode-table-wrapper").ok().flatten() {
                            if let Some(table) = wrapper.query_selector("table.wysiwyg-table").ok().flatten() {
                                if let Some(pos) = find_last_cell_pos(&table) {
                                    let Ok(mut ds) = doc_state_click.lock() else { return };
                                    ds.set_selection(Selection::cursor(pos));
                                    ds.insert_column_right();
                                    let md = ds.to_markdown();
                                    drop(ds);
                                    version.update(|v| *v += 1);
                                    if let Some(ref cb) = on_change_click {
                                        cb(md);
                                    }
                                }
                            }
                        }
                        return;
                    }

                    // ── Code copy button ────────────────────────────────────
                    if let Some(btn) = copy_btn {
                        ev.prevent_default();
                        ev.stop_propagation();

                        let Some(pre) = btn.parent_element() else { return };
                        let Some(code_el) = pre.query_selector("code").ok().flatten() else { return };
                        let text = code_el.text_content().unwrap_or_default();

                        let btn_clone = btn.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let Some(window) = web_sys::window() else { return };
                            let clipboard = window.navigator().clipboard();
                            let promise = clipboard.write_text(&text);
                            let _ = wasm_bindgen_futures::JsFuture::from(promise).await;

                            btn_clone.set_inner_html(super::doc_renderer::CHECK_ICON_SVG);
                            let _ = btn_clone.class_list().add_1("copied");

                            let restore_btn = btn_clone.clone();
                            let cb = Closure::once(move || {
                                restore_btn.set_inner_html(super::doc_renderer::COPY_ICON_SVG);
                                let _ = restore_btn.class_list().remove_1("copied");
                            });
                            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                                cb.as_ref().unchecked_ref(),
                                2000,
                            );
                            cb.forget();
                        });
                        return;
                    }

                    // ── Attachment delete button ─────────────────────────────
                    if let (Some(_del_btn), Some(ref block)) = (&delete_btn, &attachment_block) {
                        if readonly { return; }

                        ev.prevent_default();
                        ev.stop_propagation();

                        let is_image = block.class_list().contains("wysiwyg-image-block");
                        let attachment_id = block.get_attribute("data-attachment-id")
                            .filter(|s| !s.is_empty());
                        let src_or_href = if is_image {
                            block.get_attribute("data-src").unwrap_or_default()
                        } else {
                            block.get_attribute("data-href").unwrap_or_default()
                        };

                        // Fire the callback before removing from document.
                        if let Some(ref cb) = on_delete_attachment_click {
                            cb(DeleteAttachmentRequest {
                                attachment_id: attachment_id.clone(),
                                src_or_href: src_or_href.clone(),
                            });
                        }

                        // Remove the block from the document.
                        let pos_start = block.get_attribute("data-pos-start")
                            .and_then(|s| s.parse::<usize>().ok());
                        let pos_end = block.get_attribute("data-pos-end")
                            .and_then(|s| s.parse::<usize>().ok());
                        if let (Some(ps), Some(pe)) = (pos_start, pos_end) {
                            let Ok(mut ds) = doc_state_click.lock() else { return };
                            ds.delete_range(ps, pe);
                            let md = ds.to_markdown();
                            drop(ds);
                            version.update(|v| *v += 1);
                            if let Some(ref cb) = on_change_click {
                                cb(md);
                            }
                        }
                        return;
                    }

                    // ── Attachment click (not on delete button) ──────────────
                    if let Some(ref block) = attachment_block {
                        let is_image = block.class_list().contains("wysiwyg-image-block");
                        let attachment_id = block.get_attribute("data-attachment-id")
                            .filter(|s| !s.is_empty());
                        let src_or_href = if is_image {
                            block.get_attribute("data-src").unwrap_or_default()
                        } else {
                            block.get_attribute("data-href").unwrap_or_default()
                        };
                        let node_type = if is_image {
                            AttachmentNodeType::Image
                        } else {
                            AttachmentNodeType::File
                        };

                        if let Some(ref cb) = on_click_attachment_click {
                            ev.prevent_default();
                            cb(ClickAttachmentRequest {
                                attachment_id,
                                src_or_href,
                                node_type,
                            });
                        }
                    }

                    // ── Link click ──────────────────────────────────────────
                    if let Some(ref a_el) = link_el {
                        if let Some(href) = a_el.get_attribute("href") {
                            let href_lower = href.trim().to_lowercase();
                            let safe = href_lower.starts_with("http://")
                                || href_lower.starts_with("https://")
                                || href_lower.starts_with("mailto:");
                            if safe {
                                let should_open = readonly || ev.ctrl_key() || ev.meta_key();
                                if should_open {
                                    ev.prevent_default();
                                    if let Some(window) = web_sys::window() {
                                        let _ = window.open_with_url_and_target_and_features(&href, "_blank", "noopener,noreferrer");
                                    }
                                } else if !readonly {
                                    // Plain click on link — open popover in edit mode.
                                    // Explicitly sync the DOM selection to DocState here
                                    // (selectionchange may not have fired yet), then use
                                    // link_at_cursor() for the full contiguous link range.
                                    let link_rect = a_el.get_bounding_client_rect();
                                    let Some(container) = editor_ref.get_untracked() else { return };
                                    let container_el: &web_sys::Element = container.as_ref();
                                    let Some(parent) = container_el.parent_element() else { return };
                                    let parent_rect = parent.get_bounding_client_rect();
                                    let edit_range = if let Ok(mut ds) = doc_state_click.lock() {
                                        sync_selection_to_doc(&mut ds, container_el);
                                        ds.link_at_cursor().map(|(_, from, to)| (from, to))
                                    } else {
                                        None
                                    };
                                    if let Some(pos) = compute_position_relative(&link_rect, &parent_rect, 60.0) {
                                        link_popover_url.set(href);
                                        link_popover_edit_range.set(edit_range);
                                        floating_pos.set(None);
                                        link_popover_pos.set(Some((pos.top, pos.left, pos.flipped)));
                                    }
                                }
                            }
                        }
                    }
                }
                on:scroll={
                    let slash_trigger_scroll = slash_trigger_for_scroll.clone();
                    move |_| {
                        // Close floating toolbar on scroll.
                        if floating_pos.get_untracked().is_some() {
                            floating_pos.set(None);
                        }
                        // Close link popover on scroll.
                        if link_popover_pos.get_untracked().is_some() {
                            link_popover_pos.set(None);
                            link_popover_url.set(String::new());
                            link_popover_edit_range.set(None);
                        }
                        // Close table picker on scroll.
                        if table_picker_pos.get_untracked().is_some() {
                            table_picker_pos.set(None);
                        }
                        // Close table context menu on scroll.
                        if table_ctx_menu_pos.get_untracked().is_some() {
                            table_ctx_menu_pos.set(None);
                        }
                        if slash_menu_state.get_untracked().is_none() { return; }
                        let trigger = slash_trigger_scroll.borrow();
                        if let Some(el) = trigger.as_ref() {
                            let rect = el.get_bounding_client_rect();
                            let Some(window) = web_sys::window() else { return };
                            let vh = window.inner_height()
                                .ok().and_then(|v| v.as_f64()).unwrap_or(800.0);
                            if rect.bottom() < 0.0 || rect.top() > vh {
                                slash_menu_state.set(None);
                            } else {
                                slash_menu_state.set(compute_slash_menu_pos(el));
                            }
                        }
                    }
                }
                on:compositionstart=on_composition_start
                on:compositionend=on_composition_end
            />

            {floating_toolbar_view}
            {link_popover_view}
            {table_picker_view}
            {table_ctx_menu_view}
            {slash_menu_view}
        </div>
    }
}

/// Compute position for the floating toolbar above/below the selection range.
///
/// Defaults to placing the toolbar ABOVE the selection. Flips below when the
/// selection is near the top of the viewport (less than 50 px of space).
/// Returns `(top, left, flipped)` in the container's coordinate space.
fn compute_floating_toolbar_pos(
    range_rect: &web_sys::DomRect,
    container_rect: &web_sys::DomRect,
) -> (f64, f64, bool) {
    let gap = 8.0;
    let flip = range_rect.top() < 50.0;
    let top = if flip {
        range_rect.bottom() - container_rect.top() + gap
    } else {
        range_rect.top() - container_rect.top() - gap
    };
    let left = (range_rect.left() + range_rect.right()) / 2.0 - container_rect.left();
    (top, left, flip)
}

fn compute_slash_menu_pos(el: &web_sys::Element) -> Option<(f64, f64, f64, bool)> {
    let el_rect = el.get_bounding_client_rect();
    let window = web_sys::window()?;
    let vh = window.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(800.0);
    let gap = 4.0;
    let pad = 8.0;
    let space_below = vh - el_rect.bottom() - pad;
    let space_above = el_rect.top() - pad;
    let flip = space_below < 200.0 && space_above > space_below;
    let (top, max_h) = if flip {
        (el_rect.top() - gap, space_above - gap)
    } else {
        (el_rect.bottom() + gap, space_below - gap)
    };
    Some((top, max_h.max(100.0), el_rect.left(), flip))
}

/// Build a `DeleteAttachmentRequest` from an ImageBlock or FileBlock node's attributes.
fn build_delete_request(node: &kode_doc::Node) -> DeleteAttachmentRequest {
    let attachment_id = match get_attr(&node.attrs, "attachment_id") {
        Some(AttrValue::String(s)) => Some(s.clone()),
        _ => None,
    };
    let src_or_href = match node.node_type {
        NodeType::ImageBlock => match get_attr(&node.attrs, "src") {
            Some(AttrValue::String(s)) => s.clone(),
            _ => String::new(),
        },
        NodeType::FileBlock => match get_attr(&node.attrs, "href") {
            Some(AttrValue::String(s)) => s.clone(),
            _ => String::new(),
        },
        _ => String::new(),
    };
    DeleteAttachmentRequest {
        attachment_id,
        src_or_href,
    }
}

/// Trigger signals for toolbar popovers (link popover, table picker, etc.).
/// Grouped into a struct to keep `render_toolbar_items` under the argument limit.
#[derive(Clone, Copy)]
struct ToolbarPopoverTriggers {
    link_popup_open: RwSignal<bool>,
    table_picker_open: RwSignal<bool>,
}

fn render_toolbar_items(
    items: Vec<ToolbarItem>,
    doc_state: &Arc<Mutex<DocState>>,
    notify: &(impl Fn(Option<String>) + Clone + 'static),
    editor_ref: NodeRef<leptos::html::Div>,
    formatting_state: RwSignal<FormattingState>,
    extension_active_state: RwSignal<Vec<(String, bool)>>,
    popover_triggers: ToolbarPopoverTriggers,
) -> Vec<AnyView> {
    let mut views: Vec<AnyView> = Vec::new();
    for item in items {
        match item {
            ToolbarItem::Separator => {
                views.push(view! { <div class="kode-toolbar-separator" /> }.into_any());
            }
            ToolbarItem::Spacer => {
                views.push(view! { <div class="kode-toolbar-spacer" /> }.into_any());
            }
            ToolbarItem::Slot(slot_view) => {
                views.push(slot_view);
            }
            ToolbarItem::Builtin(btn) => {
                let doc_tb = doc_state.clone();
                let notify_tb = notify.clone();
                let editor_ref_tb = editor_ref;
                let link_popup_tb = popover_triggers.link_popup_open;
                let table_picker_tb = popover_triggers.table_picker_open;
                let title = btn.title();
                let active = Signal::derive(move || btn.is_active(&formatting_state.get()));
                let class = Signal::derive(move || {
                    if active.get() { "kode-toolbar-button active".to_string() }
                    else { "kode-toolbar-button".to_string() }
                });
                let on_click = move |_: MouseEvent| {
                    if matches!(btn, BuiltinButton::Link) {
                        link_popup_tb.set(true);
                        return;
                    }
                    if matches!(btn, BuiltinButton::Table) {
                        table_picker_tb.set(true);
                        return;
                    }
                    let Ok(mut ds) = doc_tb.lock() else { return };
                    dispatch_builtin_action(&mut ds, btn);
                    let md = ds.to_markdown();
                    drop(ds);
                    (notify_tb)(Some(md));
                    if let Some(el) = editor_ref_tb.get_untracked() {
                        let el: &HtmlElement = el.as_ref();
                        let _ = el.focus();
                    }
                };
                if let Some(svg) = btn.icon_svg() {
                    views.push(view! {
                        <button title=title class=class on:click=on_click
                            on:mousedown=|ev: MouseEvent| { ev.prevent_default(); }
                            inner_html=svg />
                    }.into_any());
                } else {
                    let label = btn.label();
                    views.push(view! {
                        <button title=title class=class on:click=on_click
                            on:mousedown=|ev: MouseEvent| { ev.prevent_default(); }>
                            {label}
                        </button>
                    }.into_any());
                }
            }
            ToolbarItem::BuiltinWithView(btn, custom_view) => {
                let doc_tb = doc_state.clone();
                let notify_tb = notify.clone();
                let editor_ref_tb = editor_ref;
                let link_popup_tb = popover_triggers.link_popup_open;
                let table_picker_tb = popover_triggers.table_picker_open;
                let title = btn.title();
                let active = Signal::derive(move || btn.is_active(&formatting_state.get()));
                let class = Signal::derive(move || {
                    if active.get() { "kode-toolbar-button active".to_string() }
                    else { "kode-toolbar-button".to_string() }
                });
                let on_click = move |_: MouseEvent| {
                    if matches!(btn, BuiltinButton::Link) {
                        link_popup_tb.set(true);
                        return;
                    }
                    if matches!(btn, BuiltinButton::Table) {
                        table_picker_tb.set(true);
                        return;
                    }
                    let Ok(mut ds) = doc_tb.lock() else { return };
                    dispatch_builtin_action(&mut ds, btn);
                    let md = ds.to_markdown();
                    drop(ds);
                    (notify_tb)(Some(md));
                    if let Some(el) = editor_ref_tb.get_untracked() {
                        let el: &HtmlElement = el.as_ref();
                        let _ = el.focus();
                    }
                };
                views.push(view! {
                    <button title=title class=class on:click=on_click
                        on:mousedown=|ev: MouseEvent| { ev.prevent_default(); }>
                        {custom_view}
                    </button>
                }.into_any());
            }
            ToolbarItem::Custom(custom) => {
                let on_click = custom.on_click;
                let title = custom.title;
                let btn_class = custom.class.unwrap_or_else(|| "kode-toolbar-button".to_string());
                views.push(view! {
                    <button title=title class=btn_class
                        on:click=move |_: MouseEvent| { (on_click)(); }
                        on:mousedown=|ev: MouseEvent| { ev.prevent_default(); }>
                        {custom.label}
                    </button>
                }.into_any());
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
                    } else { false }
                });
                let class = Signal::derive(move || {
                    if active.get() { "kode-toolbar-button active".to_string() }
                    else { "kode-toolbar-button".to_string() }
                });
                let on_click = move |_: MouseEvent| {
                    let Ok(mut ds) = doc_ext_tb.lock() else { return };
                    let changed = apply_md_command(&mut ds, |e| { (action)(e); });
                    if changed {
                        let md_out = ds.to_markdown();
                        drop(ds);
                        (notify_ext_tb)(Some(md_out));
                    } else { drop(ds); }
                    if let Some(el) = editor_ref_tb.get_untracked() {
                        let el: &HtmlElement = el.as_ref();
                        let _ = el.focus();
                    }
                };
                views.push(view! {
                    <button title=title class=class on:click=on_click
                        on:mousedown=|ev: MouseEvent| { ev.prevent_default(); }>
                        {ext_item.label}
                    </button>
                }.into_any());
            }
        }
    }
    views
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

/// Walk all positioned DOM elements and update their `data-pos-start`/
/// `data-pos-end` attributes to match the current DocState. Called after the
/// MutationObserver syncs a text change without a full re-render.
fn refresh_pos_attributes(
    container: &web_sys::Element,
    doc_state: &Arc<Mutex<DocState>>,
) {
    let Ok(ds) = doc_state.lock() else { return };
    let doc = ds.doc();

    let Ok(positioned) = container.query_selector_all("[data-pos-start]") else { return };

    // Collect expected (content_start, content_end) for every positioned node
    // by walking the doc tree in document order.
    let mut expected: Vec<(usize, usize)> = Vec::new();
    collect_block_positions(doc, 0, &mut expected);

    // Match in order — there should be a 1:1 correspondence.
    let count = positioned.length().min(expected.len() as u32);
    for i in 0..count {
        let Some(node) = positioned.item(i) else { continue };
        let Ok(el) = node.dyn_into::<web_sys::Element>() else { continue };
        let (cs, ce) = expected[i as usize];
        let _ = el.set_attribute("data-pos-start", &cs.to_string());
        let _ = el.set_attribute("data-pos-end", &ce.to_string());
    }
}

/// Recursively collect (content_start, content_end) for all block nodes
/// that would have data-pos-start/data-pos-end attributes in the DOM.
fn collect_block_positions(
    node: &kode_doc::Node,
    start: usize,
    out: &mut Vec<(usize, usize)>,
) {
    if node.node_type == kode_doc::NodeType::Doc {
        let mut pos = 0usize;
        for child in node.content.iter() {
            collect_block_positions(child, pos, out);
            pos += child.node_size();
        }
        return;
    }

    if !node.node_type.is_block() {
        return;
    }

    // Leaf blocks (HR, image, file) and atomic extension blocks use
    // data-pos-start=start, data-pos-end=start+node_size.
    // Branch text blocks (paragraph, heading, code, etc.) use
    // data-pos-start=start+1, data-pos-end=start+1+content.size().
    if node.node_type.is_leaf() || node.is_atom() {
        out.push((start, start + node.node_size()));
    } else {
        let content_start = start + 1;
        let content_end = content_start + node.content.size();
        out.push((content_start, content_end));

        // Recurse into container blocks (blockquote, list, list_item, table).
        if node.content.child_count() > 0 {
            let mut pos = content_start;
            for child in node.content.iter() {
                if child.node_type.is_block() {
                    collect_block_positions(child, pos, out);
                }
                pos += child.node_size();
            }
        }
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

/// Returns (common_prefix_chars, common_suffix_chars) between two strings.
/// The changed region in `old` is `old[prefix..old_len-suffix]` and in `new`
/// is `new[prefix..new_len-suffix]`.
fn diff_strings(old: &str, new: &str) -> (usize, usize) {
    let old_chars: Vec<char> = old.chars().collect();
    let new_chars: Vec<char> = new.chars().collect();

    let prefix = old_chars.iter().zip(new_chars.iter()).take_while(|(a, b)| a == b).count();

    let suffix = old_chars[prefix..]
        .iter()
        .rev()
        .zip(new_chars[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();

    (prefix, suffix)
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

/// Mount hover "+" controls on a table element for adding rows and columns.
///
/// Wraps the table in a `kode-table-wrapper` div with `position: relative`,
/// since `<div>` elements are not valid children of `<table>`. The buttons
/// are appended as siblings of the table inside the wrapper.
///
/// Idempotent — skips if the wrapper is already present.
#[cfg(target_arch = "wasm32")]
fn mount_table_controls(table: &web_sys::Element) {
    // Check if already wrapped.
    if let Some(parent) = table.parent_element() {
        if parent.class_list().contains("kode-table-wrapper") {
            return;
        }
    }

    let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return };
    let Some(parent) = table.parent_node() else { return };

    // Create wrapper div — inherits contenteditable from the scroll container.
    let Ok(wrapper) = doc.create_element("div") else { return };
    let _ = wrapper.set_attribute("class", "kode-table-wrapper");

    // Insert wrapper before the table, then move the table inside.
    let _ = parent.insert_before(&wrapper, Some(table));
    let _ = wrapper.append_child(table);

    // "+" button below table (add row)
    if let Ok(row_btn) = doc.create_element("div") {
        let _ = row_btn.set_attribute("class", "kode-table-add-row-btn");
        let _ = row_btn.set_attribute("contenteditable", "false");
        row_btn.set_inner_html("+");
        let _ = wrapper.append_child(&row_btn);
    }

    // "+" button right of table (add column)
    if let Ok(col_btn) = doc.create_element("div") {
        let _ = col_btn.set_attribute("class", "kode-table-add-col-btn");
        let _ = col_btn.set_attribute("contenteditable", "false");
        col_btn.set_inner_html("+");
        let _ = wrapper.append_child(&col_btn);
    }
}

/// Find a cursor position inside the last cell of a table element.
/// Returns `data-pos-start` of the last `td` or `th` cell found in the DOM,
/// which places the cursor at the beginning of that cell's content.
fn find_last_cell_pos(table: &web_sys::Element) -> Option<usize> {
    let cells = table.query_selector_all("td, th").ok()?;
    let count = cells.length();
    if count == 0 { return None; }
    let last_cell: web_sys::Element = cells.item(count - 1)?.unchecked_into();
    last_cell.get_attribute("data-pos-start")?.parse::<usize>().ok()
}

/// Mount table controls on all table elements within a container.
#[cfg(target_arch = "wasm32")]
fn mount_table_controls_in_container(container: &web_sys::Element) {
    if let Ok(tables) = container.query_selector_all("table.wysiwyg-table") {
        for i in 0..tables.length() {
            if let Some(table) = tables.item(i) {
                if let Some(el) = table.dyn_ref::<web_sys::Element>() {
                    mount_table_controls(el);
                }
            }
        }
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

/// Find the position of an UploadPlaceholder node with the given placeholder_id.
/// Scans top-level doc children (position 0-based within the doc's content).
fn find_placeholder_position(doc: &kode_doc::Node, placeholder_id: &str) -> Option<usize> {
    let mut pos = 0;
    for child in doc.content.iter() {
        if child.node_type == NodeType::UploadPlaceholder {
            if let Some(AttrValue::String(ref id)) = get_attr(&child.attrs, "placeholder_id") {
                if id == placeholder_id {
                    return Some(pos);
                }
            }
        }
        pos += child.node_size();
    }
    None
}

/// Convert an `AttachmentInsert` into the corresponding document node.
fn attachment_insert_to_node(insert: &super::attachment::AttachmentInsert) -> kode_doc::Node {
    match insert {
        super::attachment::AttachmentInsert::Image { src, alt, attachment_id, width, height } => {
            kode_doc::Node::leaf_with_attrs(
                NodeType::ImageBlock,
                kode_doc::attrs::image_block_attrs(src, alt, attachment_id.as_deref(), *width, *height),
            )
        }
        super::attachment::AttachmentInsert::File { href, filename, attachment_id, size_bytes, content_type } => {
            kode_doc::Node::leaf_with_attrs(
                NodeType::FileBlock,
                kode_doc::attrs::file_block_attrs(href, filename, attachment_id.as_deref(), *size_bytes, content_type.as_deref()),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::diff_strings;

    #[test]
    fn append_single_char() {
        assert_eq!(diff_strings("hello", "hellox"), (5, 0));
    }

    #[test]
    fn prepend_single_char() {
        assert_eq!(diff_strings("hello", "xhello"), (0, 5));
    }

    #[test]
    fn insert_middle() {
        assert_eq!(diff_strings("helo", "hello"), (3, 1));
    }

    #[test]
    fn replace_char() {
        // "text " → "text." : prefix=4, suffix=0
        assert_eq!(diff_strings("text ", "text."), (4, 0));
    }

    #[test]
    fn autocorrect_space_to_period_space() {
        // macOS double-space-to-period: "text " → "text. "
        assert_eq!(diff_strings("text ", "text. "), (4, 1));
    }

    #[test]
    fn delete_suffix() {
        assert_eq!(diff_strings("hello", "hel"), (3, 0));
    }

    #[test]
    fn identical_strings() {
        assert_eq!(diff_strings("same", "same"), (4, 0));
    }

    #[test]
    fn empty_to_text() {
        assert_eq!(diff_strings("", "new"), (0, 0));
    }

    #[test]
    fn text_to_empty() {
        assert_eq!(diff_strings("old", ""), (0, 0));
    }

    #[test]
    fn unicode_chars() {
        assert_eq!(diff_strings("café", "café!"), (4, 0));
    }
}
