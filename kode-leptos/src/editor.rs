// force rebuild
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

/// Global counter for generating unique editor instance IDs.
static INSTANCE_COUNTER: AtomicU32 = AtomicU32::new(0);

use kode_core::{Editor, Marker, MarkerSeverity, Position};
use leptos::prelude::*;
use leptos::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{
    CompositionEvent, FocusEvent, HtmlElement, HtmlTextAreaElement, KeyboardEvent, MouseEvent,
    Node,
};

use crate::diagnostics::DiagnosticProvider;
use crate::handle::EditorHandle;
use crate::highlight::{self, Language};
use crate::keys;
use crate::theme::Theme;

pub(crate) const LINE_HEIGHT: f64 = 20.0;
const VIEWPORT_BUFFER: usize = 20;
const MULTI_CLICK_MS: f64 = 500.0;
const GUTTER_PAD_LEFT: f64 = 8.0;
const GUTTER_PAD_RIGHT: f64 = 16.0;
const GUTTER_CHAR_WIDTH: f64 = 8.4;

// ── DOM measurement helpers ──────────────────────────────────────────────

pub(crate) fn measure_cursor_x(editor_el: &HtmlElement, line: usize, col: usize) -> Option<f64> {
    let editor_rect = editor_el.get_bounding_client_rect();
    let document = web_sys::window()?.document()?;
    let selector = format!("[data-line=\"{}\"]", line);
    let code_span = editor_el.query_selector(&selector).ok()??;

    let has_text = code_span.text_content().map(|t| !t.is_empty()).unwrap_or(false);

    if !has_text {
        let span_el: &HtmlElement = code_span.unchecked_ref();
        if let Some(parent) = span_el.parent_element() {
            let pr = parent.get_bounding_client_rect();
            return Some(pr.left() - editor_rect.left());
        }
        let sr = span_el.get_bounding_client_rect();
        return Some(sr.left() - editor_rect.left());
    }

    let range = document.create_range().ok()?;
    match find_text_node_at_offset(&code_span, col) {
        Some((node, offset)) => {
            let _ = range.set_start(&node, offset as u32);
            let _ = range.set_end(&node, offset as u32);
            if let Some(rects) = range.get_client_rects() {
                if let Some(r) = rects.get(0) {
                    return Some(r.left() - editor_rect.left());
                }
            }
            let _ = range.select_node_contents(&code_span);
            let r = range.get_bounding_client_rect();
            if col == 0 {
                Some(r.left() - editor_rect.left())
            } else {
                Some(r.right() - editor_rect.left())
            }
        }
        None => {
            let _ = range.select_node_contents(&code_span);
            let r = range.get_bounding_client_rect();
            Some(r.right() - editor_rect.left())
        }
    }
}

fn find_text_node_at_offset(element: &web_sys::Element, target_offset: usize) -> Option<(Node, usize)> {
    let mut remaining = target_offset;
    find_text_node_recursive(element.as_ref(), &mut remaining)
}

fn find_text_node_recursive(node: &Node, remaining: &mut usize) -> Option<(Node, usize)> {
    if node.node_type() == Node::TEXT_NODE {
        let text_len = node.text_content().map(|t| t.len()).unwrap_or(0);
        if *remaining <= text_len {
            return Some((node.clone(), *remaining));
        }
        *remaining -= text_len;
        return None;
    }
    let children = node.child_nodes();
    for i in 0..children.length() {
        if let Some(child) = children.get(i) {
            if let Some(result) = find_text_node_recursive(&child, remaining) {
                return Some(result);
            }
        }
    }
    None
}

fn compute_gutter_width(line_count: usize) -> f64 {
    let digits = format!("{}", line_count).len().max(2) as f64;
    digits * GUTTER_CHAR_WIDTH + GUTTER_PAD_LEFT + GUTTER_PAD_RIGHT
}

fn position_from_click(
    editor_el: &HtmlElement,
    client_x: f64,
    client_y: f64,
    scroll_top: f64,
    line_count: usize,
    buffer_line_len: impl Fn(usize) -> usize,
) -> Position {
    let rect = editor_el.get_bounding_client_rect();
    let gutter_w = compute_gutter_width(line_count);
    let y = client_y - rect.top() + scroll_top;
    let line = ((y / LINE_HEIGHT).floor() as usize).min(line_count.saturating_sub(1));
    let line_len = buffer_line_len(line);
    if line_len == 0 {
        return Position::new(line, 0);
    }

    let x = client_x - rect.left() - gutter_w;
    let selector = format!("[data-line=\"{}\"]", line);
    let code_span = match editor_el.query_selector(&selector) {
        Ok(Some(el)) => el,
        _ => return Position::new(line, 0),
    };
    let document = match web_sys::window().and_then(|w| w.document()) {
        Some(d) => d,
        None => return Position::new(line, 0),
    };
    let range = match document.create_range() {
        Ok(r) => r,
        Err(_) => return Position::new(line, 0),
    };

    let mut best_col = 0;
    let mut best_dist = f64::MAX;
    for col in 0..=line_len {
        if let Some((node, offset)) = find_text_node_at_offset(&code_span, col) {
            let _ = range.set_start(&node, offset as u32);
            let _ = range.set_end(&node, offset as u32);
            if let Some(rects) = range.get_client_rects() {
                if let Some(r) = rects.get(0) {
                    let char_x = r.left() - rect.left() - gutter_w;
                    let dist = (char_x - x).abs();
                    if dist < best_dist {
                        best_dist = dist;
                        best_col = col;
                    }
                }
            }
        }
    }
    Position::new(line, best_col)
}

fn copy_selection_to_textarea(editor: &Editor, textarea: &HtmlTextAreaElement) {
    let selected = editor.selected_text();
    if !selected.is_empty() {
        textarea.set_value(&selected);
        textarea.select();
    }
}

/// Highlight lines grouped by language using block-level parsing.
///
/// Groups consecutive lines that share the same language, highlights each group
/// as a single block (giving tree-sitter multi-line context), then reassembles
/// the per-line HTML in order.
fn highlight_lines_by_language(
    lines: &[&str],
    langs: &[highlight::Language],
) -> Vec<String> {
    debug_assert_eq!(lines.len(), langs.len(), "lines and langs must have same length");
    let mut result = Vec::with_capacity(lines.len());
    let mut i = 0;

    while i < lines.len() {
        let group_lang = &langs[i];
        let group_start = i;

        // Collect consecutive lines with the same language
        while i < lines.len() && &langs[i] == group_lang {
            i += 1;
        }

        let group_lines = &lines[group_start..i];
        let highlighted = highlight::highlight_block(group_lines, group_lang);
        result.extend(highlighted);
    }

    result
}

// ── Component ────────────────────────────────────────────────────────────

#[component]
pub fn CodeEditor(
    #[prop(into, default = Signal::stored(Language::PLAIN))]
    language: Signal<Language>,
    #[prop(into, default = Signal::stored(String::new()))]
    content: Signal<String>,
    #[prop(into, default = Signal::stored(Theme::default()))]
    theme: Signal<Theme>,
    #[prop(optional)]
    on_change: Option<Arc<dyn Fn(String) + Send + Sync>>,
    #[prop(optional)]
    on_ready: Option<Arc<dyn Fn(EditorHandle) + Send + Sync>>,
    /// Reactive list of diagnostic providers. Each provider is called (debounced)
    /// when text changes. Results are merged and rendered as marker underlines.
    /// Providers can be swapped at runtime (e.g. when the language changes).
    #[prop(into, default = Signal::stored(vec![]))]
    diagnostic_providers: Signal<Vec<DiagnosticProvider>>,
    /// Override the debounce delay (in ms) before calling providers. Default: 300ms.
    #[prop(optional)]
    diagnostic_debounce_ms: Option<i32>,
    /// Reactive list of completion providers.
    #[prop(into, default = Signal::stored(vec![]))]
    completion_providers: Signal<Vec<crate::completion::CompletionProviderConfig>>,
    /// Ghost text shown at line 1 col 1 when the editor is empty.
    /// Empty string (default) renders nothing.
    #[prop(into, default = Signal::stored(String::new()))]
    placeholder: Signal<String>,
) -> impl IntoView {
    let editor = Arc::new(Mutex::new(Editor::new(&content.get_untracked())));
    let text_version = RwSignal::new(0u64);
    let cursor_version = RwSignal::new(0u64);
    let focused = RwSignal::new(false);
    let composing = RwSignal::new(false);
    let scroll_top = RwSignal::new(0.0f64);
    let viewport_height = RwSignal::new(500.0f64);
    let cursor_visible = RwSignal::new(true);
    let dragging = RwSignal::new(false);
    let markers: RwSignal<Vec<Marker>> = RwSignal::new(Vec::new());
    let marker_version: RwSignal<u64> = RwSignal::new(0);
    let completion_state = RwSignal::new(crate::completion::CompletionState::Idle);
    let completion_trigger: RwSignal<Option<kode_core::CompletionTrigger>> = RwSignal::new(None);

    let last_click_time = RwSignal::new(0.0f64);
    let click_count = RwSignal::new(0u32);
    let last_click_line = RwSignal::new(0usize);

    let blink_timer_id = RwSignal::new(0i32);
    let blink_interval_id = RwSignal::new(0i32);

    let textarea_ref = NodeRef::<leptos::html::Textarea>::new();
    let editor_ref = NodeRef::<leptos::html::Div>::new();
    let scroll_container_ref = NodeRef::<leptos::html::Div>::new();

    // Unique IDs for direct DOM manipulation — supports multiple editors on one page
    let instance_id = INSTANCE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let editor_root_id = format!("kode-editor-root-{instance_id}");
    let cursor_el_id = format!("kode-cursor-el-{instance_id}");
    let overlay_el_id = format!("kode-overlay-el-{instance_id}");
    let fg_overlay_el_id = format!("kode-fg-overlay-el-{instance_id}");

    // ── Emit handle via on_ready ──────────────────────────────────────
    if let Some(ref on_ready) = on_ready {
        let handle = EditorHandle::new(
            editor.clone(),
            on_change.clone(),
            text_version,
            cursor_version,
            markers,
            marker_version,
        );
        on_ready(handle);
    }

    // ── Notification helpers ─────────────────────────────────────────
    let editor_notify = editor.clone();
    let on_change_notify = on_change.clone();
    let notify_text = move || {
        text_version.update(|v| *v += 1);
        cursor_version.update(|v| *v += 1);
        if let Some(ref cb) = on_change_notify {
            cb(editor_notify.lock().unwrap().text());
        }
    };
    let notify_cursor = move || {
        cursor_version.update(|v| *v += 1);
    };

    // ── Blink management ─────────────────────────────────────────────
    let reset_blink = move || {
        let Some(window) = web_sys::window() else { return };
        let old_timer = blink_timer_id.get_untracked();
        let old_interval = blink_interval_id.get_untracked();
        if old_timer != 0 { window.clear_timeout_with_handle(old_timer); }
        if old_interval != 0 { window.clear_interval_with_handle(old_interval); }
        blink_timer_id.set(0);
        blink_interval_id.set(0);
        cursor_visible.set(true);

        let cb = Closure::once(move || {
            let Some(window) = web_sys::window() else { return };
            let blink = Closure::wrap(Box::new(move || {
                cursor_visible.update(|v| *v = !*v);
            }) as Box<dyn FnMut()>);
            let id = window
                .set_interval_with_callback_and_timeout_and_arguments_0(
                    blink.as_ref().unchecked_ref(), 500,
                ).unwrap_or(0);
            blink_interval_id.set(id);
            blink.forget();
        });
        let timer = window
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                cb.as_ref().unchecked_ref(), 500,
            ).unwrap_or(0);
        blink_timer_id.set(timer);
        cb.forget();
    };

    // ── Content sync ─────────────────────────────────────────────────
    let editor_sync = editor.clone();
    Effect::new(move |prev: Option<String>| {
        let new_content = content.get();
        if prev.as_ref() != Some(&new_content) {
            let mut ed = editor_sync.lock().unwrap();
            if ed.text() != new_content {
                *ed = Editor::new(&new_content);
                text_version.update(|v| *v += 1);
                cursor_version.update(|v| *v += 1);
            }
        }
        new_content
    });

    // ── Auto-clear markers on text edit (only when no diagnostic providers) ──
    // When providers are registered, they manage the marker lifecycle.
    Effect::new(move |_| {
        let _tv = text_version.get();
        if diagnostic_providers.get_untracked().is_empty()
            && !markers.get_untracked().is_empty()
        {
            markers.set(Vec::new());
            marker_version.update(|v| *v += 1);
        }
    });

    // ── Diagnostic provider pipeline ─────────────────────────────────
    {
        let editor_diag = editor.clone();
        crate::diagnostics::spawn_diagnostic_pipeline(
            diagnostic_providers,
            editor_diag,
            text_version,
            markers,
            marker_version,
            diagnostic_debounce_ms,
        );
    }

    // ── Completion provider pipeline ──────────────────────────────────
    {
        let editor_comp = editor.clone();
        crate::completion::spawn_completion_pipeline(
            completion_providers,
            editor_comp,
            text_version,
            cursor_version,
            completion_state,
            completion_trigger,
        );
    }

    // ── Measure viewport on mount ────────────────────────────────────
    Effect::new(move |_| {
        if let Some(el) = scroll_container_ref.get() {
            let el: &HtmlElement = el.as_ref();
            let h = el.client_height() as f64;
            if h > 0.0 {
                viewport_height.set(h);
            }
        }
    });

    // ── Post-render: position cursor & selection via direct DOM manipulation ──
    // This runs AFTER the content DOM is committed, so measure_cursor_x works.
    let editor_post = editor.clone();
    let editor_root_id_eff = editor_root_id.clone();
    let cursor_el_id_eff = cursor_el_id.clone();
    let overlay_el_id_eff = overlay_el_id.clone();
    let fg_overlay_el_id_eff = fg_overlay_el_id.clone();
    Effect::new(move |_| {
        let _cv = cursor_version.get();
        let _tv = text_version.get();
        let _mv = marker_version.get();
        let is_focused = focused.get();
        let cvis = cursor_visible.get();
        let current_markers = markers.get();

        // Schedule DOM update for next animation frame so content DOM is committed
        let editor_raf = editor_post.clone();
        let editor_root_id = editor_root_id_eff.clone();
        let cursor_el_id = cursor_el_id_eff.clone();
        let overlay_el_id = overlay_el_id_eff.clone();
        let fg_overlay_el_id = fg_overlay_el_id_eff.clone();
        let cb = Closure::once(move || {
            let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };

            let ed = editor_raf.lock().unwrap();
            let sel = ed.selection();
            let cursor = ed.cursor();
            let line_count = ed.buffer().len_lines();
            drop(ed);

            // Position cursor
            if let Some(cursor_el) = document.get_element_by_id(&cursor_el_id) {
                let cursor_el: &HtmlElement = cursor_el.unchecked_ref();
                if is_focused && cvis {
                    let top = cursor.line as f64 * LINE_HEIGHT;
                    // Measure horizontal position from DOM
                    let left = document.get_element_by_id(&editor_root_id)
                        .and_then(|el| {
                            let el: &HtmlElement = el.unchecked_ref();
                            measure_cursor_x(el, cursor.line, cursor.col)
                        })
                        .unwrap_or_else(|| {
                            compute_gutter_width(line_count) + cursor.col as f64 * GUTTER_CHAR_WIDTH
                        });
                    let style = cursor_el.style();
                    let _ = style.set_property("top", &format!("{}px", top));
                    let _ = style.set_property("left", &format!("{}px", left));
                    let _ = style.set_property("display", "block");
                } else {
                    let _ = cursor_el.style().set_property("display", "none");
                }
            }

            // Position selection highlights
            if let Some(overlay_el) = document.get_element_by_id(&overlay_el_id) {
                // Clear old selection divs
                while let Some(child) = overlay_el.query_selector(".kode-selection").ok().flatten() {
                    let _ = overlay_el.remove_child(&child);
                }
                // Clear old current-line divs
                while let Some(child) = overlay_el.query_selector(".kode-current-line").ok().flatten() {
                    let _ = overlay_el.remove_child(&child);
                }
                let editor_root = document.get_element_by_id(&editor_root_id);
                let gutter_w = compute_gutter_width(line_count);

                if !sel.is_cursor() {
                    let start = sel.start();
                    let end = sel.end();
                    for line in start.line..=end.line {
                        let top = line as f64 * LINE_HEIGHT;

                        // Compute left pixel position — never less than gutter width
                        let left_px = if line == start.line && start.col > 0 {
                            editor_root.as_ref().and_then(|el| {
                                let el: &HtmlElement = el.unchecked_ref();
                                measure_cursor_x(el, line, start.col)
                            }).unwrap_or(gutter_w)
                        } else {
                            gutter_w
                        };

                        let right_style = if line == end.line {
                            let right_px = editor_root.as_ref().and_then(|el| {
                                let el: &HtmlElement = el.unchecked_ref();
                                measure_cursor_x(el, line, end.col)
                            });
                            match right_px {
                                Some(px) => format!("width:{}px;", px - left_px),
                                None => "right:0;".to_string(),
                            }
                        } else {
                            "right:0;".to_string()
                        };

                        if let Ok(div) = document.create_element("div") {
                            let _ = div.set_attribute("class", "kode-selection");
                            let _ = div.set_attribute("style", &format!(
                                "position:absolute;top:{}px;left:{}px;height:{}px;{};pointer-events:none;",
                                top, left_px, LINE_HEIGHT, right_style
                            ));
                            let _ = overlay_el.append_child(&div);
                        }
                    }
                } else if is_focused {
                    let top = cursor.line as f64 * LINE_HEIGHT;
                    if let Ok(div) = document.create_element("div") {
                        let _ = div.set_attribute("class", "kode-current-line");
                        let _ = div.set_attribute("style", &format!(
                            "position:absolute;top:{}px;left:{}px;right:0;height:{}px;pointer-events:none;",
                            top, gutter_w, LINE_HEIGHT
                        ));
                        let _ = overlay_el.append_child(&div);
                    }
                }

                // Render marker underlines in the foreground overlay (above text)
                if let Some(fg_overlay_el) = document.get_element_by_id(&fg_overlay_el_id) {
                // Clear old marker divs from fg overlay
                while let Some(child) = fg_overlay_el.query_selector(".kode-marker-error,.kode-marker-warning,.kode-marker-info,.kode-marker-hint").ok().flatten() {
                    let _ = fg_overlay_el.remove_child(&child);
                }
                for marker in &current_markers {
                    let css_class = match marker.severity {
                        MarkerSeverity::Error => "kode-marker-error",
                        MarkerSeverity::Warning => "kode-marker-warning",
                        MarkerSeverity::Info => "kode-marker-info",
                        MarkerSeverity::Hint => "kode-marker-hint",
                    };

                    for line in marker.start.line..=marker.end.line {
                        let top = line as f64 * LINE_HEIGHT;

                        let start_col = if line == marker.start.line { marker.start.col } else { 0 };
                        let end_col = if line == marker.end.line { marker.end.col } else {
                            // Use a large value; measure_cursor_x will clamp to line end
                            usize::MAX
                        };

                        let left_px = editor_root.as_ref().and_then(|el| {
                            let el: &HtmlElement = el.unchecked_ref();
                            measure_cursor_x(el, line, start_col)
                        }).unwrap_or(gutter_w);

                        let right_px = editor_root.as_ref().and_then(|el| {
                            let el: &HtmlElement = el.unchecked_ref();
                            measure_cursor_x(el, line, end_col)
                        });

                        let width_style = match right_px {
                            Some(px) if px > left_px => format!("width:{}px;", px - left_px),
                            _ => "width:20px;".to_string(),
                        };

                        if let Ok(div) = document.create_element("div") {
                            let _ = div.set_attribute("class", css_class);
                            let _ = div.set_attribute("title", &marker.message);
                            let _ = div.set_attribute("style", &format!(
                                "top:{}px;left:{}px;height:{}px;{}",
                                top, left_px, LINE_HEIGHT, width_style
                            ));
                            let _ = fg_overlay_el.append_child(&div);
                        }
                    }
                }
                } // end fg_overlay_el
            }
        });
        let _ = web_sys::window().and_then(|w| {
            w.request_animation_frame(cb.as_ref().unchecked_ref()).ok()
        });
        cb.forget();
    });

    // ── Scroll cursor into view (after DOM update) ───────────────────
    let editor_scroll = editor.clone();
    Effect::new(move |_| {
        let _cv = cursor_version.get();
        let cursor_line = editor_scroll.lock().unwrap().cursor().line;
        let Some(el) = scroll_container_ref.get() else { return };
        let el: &HtmlElement = el.as_ref();
        let st = el.scroll_top() as f64;
        let vh = el.client_height() as f64;
        if vh <= 0.0 { return; }

        let cursor_top = cursor_line as f64 * LINE_HEIGHT;
        let cursor_bottom = cursor_top + LINE_HEIGHT;

        if cursor_top < st {
            el.set_scroll_top(cursor_top as i32);
        } else if cursor_bottom > st + vh {
            el.set_scroll_top((cursor_bottom - vh) as i32);
        }
    });

    // ── Keydown ──────────────────────────────────────────────────────
    let editor_key = editor.clone();
    let notify_text_key = notify_text.clone();
    let notify_cursor_key = notify_cursor;
    let reset_blink_key = reset_blink;
    let on_keydown = move |ev: KeyboardEvent| {
        if composing.get_untracked() { return; }

        let mut ed = editor_key.lock().unwrap();
        let old_text = ed.text();
        let old_cursor = ed.cursor();

        // Completion keyboard intercept — must come before normal key handling
        {
            let key = ev.key();
            let ctrl = ev.ctrl_key() || ev.meta_key();
            let shift = ev.shift_key();
            let providers_snapshot = completion_providers.get_untracked();
            let mut comp_state = completion_state.get_untracked();
            let result = crate::completion::handle_completion_keydown(
                &mut comp_state,
                &mut ed,
                &key,
                ctrl,
                shift,
                &completion_trigger,
                &providers_snapshot,
            );
            completion_state.set(comp_state);
            match result {
                crate::completion::CompletionKeyResult::Consumed => {
                    ev.prevent_default();
                    let new_text = ed.text();
                    if new_text != old_text {
                        drop(ed);
                        (notify_text_key)();
                    } else {
                        drop(ed);
                    }
                    reset_blink_key();
                    return;
                }
                crate::completion::CompletionKeyResult::Ignored => {
                    // Fall through to normal handling
                }
            }
        }

        let ctrl = ev.ctrl_key() || ev.meta_key();
        match ev.key().as_str() {
            "c" if ctrl => {
                if let Some(ta) = textarea_ref.get() {
                    copy_selection_to_textarea(&ed, ta.as_ref());
                }
                return;
            }
            "x" if ctrl => {
                if let Some(ta) = textarea_ref.get() {
                    copy_selection_to_textarea(&ed, ta.as_ref());
                }
                ed.delete_selection();
                drop(ed);
                (notify_text_key)();
                reset_blink_key();
                ev.prevent_default();
                return;
            }
            "v" if ctrl => return,
            _ => {}
        }

        let handled = keys::handle_keydown(&mut ed, &ev);
        if handled {
            ev.prevent_default();
            let new_text = ed.text();
            let new_cursor = ed.cursor();

            // Update completion filter after text-changing keys (e.g. Backspace)
            let mut comp_state = completion_state.get_untracked();
            if comp_state.is_active() && new_text != old_text {
                if let Some(ws) = comp_state.word_start() {
                    // Dismiss if cursor moved before word_start
                    if new_cursor.line != ws.line || new_cursor.col < ws.col {
                        comp_state.dismiss();
                    } else {
                        let line_text = new_text.lines().nth(new_cursor.line).unwrap_or("");
                        let start_col = ws.col.min(line_text.len());
                        let end_col = new_cursor.col.min(line_text.len());
                        let prefix = &line_text[start_col..end_col];
                        comp_state.update_filter(prefix);
                    }
                    completion_state.set(comp_state);
                }
            }

            drop(ed);
            if new_text != old_text {
                (notify_text_key)();
            } else if new_cursor != old_cursor {
                (notify_cursor_key)();
            }
            reset_blink_key();
        }
    };

    // ── Text input ───────────────────────────────────────────────────
    let editor_input = editor.clone();
    let notify_text_input = notify_text.clone();
    let reset_blink_input = reset_blink;
    let on_input = move |_: web_sys::Event| {
        if composing.get_untracked() { return; }
        if let Some(ta) = textarea_ref.get() {
            let ta: &HtmlTextAreaElement = ta.as_ref();
            let value = ta.value();
            if !value.is_empty() {
                ta.set_value("");
                editor_input.lock().unwrap().insert(&value);

                // Fire completion triggers after text insertion
                let providers_snapshot = completion_providers.get_untracked();
                if value.len() == 1 {
                    let ch = value.chars().next().unwrap();
                    let is_trigger = providers_snapshot.iter().any(|cfg| cfg.trigger_characters.contains(&ch));
                    let has_typing = providers_snapshot.iter().any(|cfg| cfg.activate_on_typing);
                    if is_trigger {
                        completion_trigger.set(Some(kode_core::CompletionTrigger::TriggerCharacter(ch)));
                    } else if has_typing {
                        completion_trigger.set(Some(kode_core::CompletionTrigger::Typing));
                    }
                }
                // Update filter if completion is active
                let mut comp_state = completion_state.get_untracked();
                if comp_state.is_active() {
                    let ed = editor_input.lock().unwrap();
                    let cursor = ed.cursor();
                    if let Some(word_start) = comp_state.word_start() {
                        let text = ed.text();
                        let line_text = text.lines().nth(cursor.line).unwrap_or("");
                        let start_col = word_start.col.min(line_text.len());
                        let end_col = cursor.col.min(line_text.len());
                        let prefix = &line_text[start_col..end_col];
                        comp_state.update_filter(prefix);
                    }
                    drop(ed);
                    completion_state.set(comp_state);
                }

                (notify_text_input)();
                reset_blink_input();
            }
        }
    };

    // ── Composition ──────────────────────────────────────────────────
    let on_composition_start = move |_: CompositionEvent| { composing.set(true); };
    let editor_comp = editor.clone();
    let notify_text_comp = notify_text.clone();
    let reset_blink_comp = reset_blink;
    let on_composition_end = move |ev: CompositionEvent| {
        composing.set(false);
        if let Some(data) = ev.data() {
            if !data.is_empty() {
                editor_comp.lock().unwrap().insert(&data);
                if let Some(ta) = textarea_ref.get() {
                    let ta: &HtmlTextAreaElement = ta.as_ref();
                    ta.set_value("");
                }
                (notify_text_comp)();
                reset_blink_comp();
            }
        }
    };

    // ── Focus ────────────────────────────────────────────────────────
    let on_focus = move |_: FocusEvent| { focused.set(true); };
    let on_blur = move |_: FocusEvent| {
        focused.set(false);
        cursor_visible.set(false);
        completion_state.set(crate::completion::CompletionState::Idle);
    };

    // ── Mouse ────────────────────────────────────────────────────────
    let editor_mouse = editor.clone();
    let notify_cursor_mouse = notify_cursor;
    let reset_blink_mouse = reset_blink;
    let on_mousedown = move |ev: MouseEvent| {
        ev.prevent_default();
        completion_state.set(crate::completion::CompletionState::Idle);
        if let Some(ta) = textarea_ref.get() {
            let ta_el: &HtmlElement = ta.as_ref();
            let _ = ta_el.focus();
        }
        let editor_el = match editor_ref.get() {
            Some(el) => el,
            None => return,
        };
        let editor_el: &HtmlElement = editor_el.as_ref();

        let mut ed = editor_mouse.lock().unwrap();
        let lc = ed.buffer().len_lines();
        let pos = position_from_click(
            editor_el, ev.client_x() as f64, ev.client_y() as f64,
            scroll_top.get_untracked(), lc,
            |line| ed.buffer().line_len(line),
        );

        let now = js_sys::Date::now();
        let on_same_line = last_click_line.get_untracked() == pos.line;
        if now - last_click_time.get_untracked() < MULTI_CLICK_MS && on_same_line {
            click_count.update(|c| *c += 1);
        } else {
            click_count.set(1);
        }
        last_click_time.set(now);
        last_click_line.set(pos.line);
        let clicks = click_count.get_untracked();

        if ev.shift_key() {
            ed.extend_selection(pos);
        } else if clicks == 2 {
            ed.set_cursor(pos);
            ed.select_word();
        } else if clicks >= 3 {
            ed.set_cursor(pos);
            ed.select_line();
            click_count.set(0);
        } else {
            ed.set_cursor(pos);
        }
        drop(ed);
        dragging.set(true);
        (notify_cursor_mouse)();
        reset_blink_mouse();
    };

    let editor_move = editor.clone();
    let notify_cursor_move = notify_cursor;
    let on_mousemove = move |ev: MouseEvent| {
        if !dragging.get_untracked() { return; }
        let editor_el = match editor_ref.get() {
            Some(el) => el,
            None => return,
        };
        let editor_el: &HtmlElement = editor_el.as_ref();
        let mut ed = editor_move.lock().unwrap();
        let lc = ed.buffer().len_lines();
        let pos = position_from_click(
            editor_el, ev.client_x() as f64, ev.client_y() as f64,
            scroll_top.get_untracked(), lc,
            |line| ed.buffer().line_len(line),
        );
        ed.extend_selection(pos);
        drop(ed);
        (notify_cursor_move)();
    };

    let on_mouseup = move |_: MouseEvent| { dragging.set(false); };

    // ── Scroll ───────────────────────────────────────────────────────
    let on_scroll = move |_: web_sys::Event| {
        if let Some(el) = scroll_container_ref.get() {
            let el: &HtmlElement = el.as_ref();
            scroll_top.set(el.scroll_top() as f64);
            let h = el.client_height() as f64;
            if h > 0.0 { viewport_height.set(h); }
        }
        completion_state.set(crate::completion::CompletionState::Idle);
    };

    // ── Render ───────────────────────────────────────────────────────
    let editor_render = editor.clone();
    let editor_popup = editor.clone();
    let notify_text_popup = {
        let notify = notify_text.clone();
        Arc::new(notify)
    };

    view! {
        <style>{move || theme.get().syntax_css("pre.kode-content")}</style>
        <style>{include_str!("editor.css")}</style>
        <div class="kode-editor" id={editor_root_id.clone()} node_ref=editor_ref
            style=move || theme.get().to_css_vars()
            on:mousedown=on_mousedown on:mousemove=on_mousemove on:mouseup=on_mouseup>

            <textarea node_ref=textarea_ref class="kode-hidden-textarea"
                autocapitalize="off" autocomplete="off" spellcheck="false"
                on:keydown=on_keydown on:input=on_input
                on:compositionstart=on_composition_start on:compositionend=on_composition_end
                on:focus=on_focus on:blur=on_blur />

            <div class="kode-scroll-container" node_ref=scroll_container_ref on:scroll=on_scroll>
                // Content layer (selection/current-line divs injected here by Effect)
                <pre class="kode-content">
                    {move || {
                        let _tv = text_version.get();
                        let lang = language.get();
                        let st = scroll_top.get();
                        let vh = viewport_height.get();

                        let ed = editor_render.lock().unwrap();
                        let line_count = ed.buffer().len_lines();
                        let buffer_empty = ed.buffer().is_empty();
                        let total_height = line_count as f64 * LINE_HEIGHT;
                        let gutter_digits = format!("{}", line_count).len().max(2);

                        let first_visible = ((st / LINE_HEIGHT).floor() as usize).min(line_count.saturating_sub(1));
                        let visible_count = ((vh / LINE_HEIGHT).ceil() as usize) + 1;
                        let render_start = first_visible.saturating_sub(VIEWPORT_BUFFER);
                        let render_end = (first_visible + visible_count + VIEWPORT_BUFFER).min(line_count);

                        // For markdown, scan lines before the viewport to establish
                        // fenced code block state (e.g. which language to highlight).
                        let mut fence_tracker = if lang.name() == "markdown" {
                            let mut ft = highlight::FenceTracker::new();
                            for pre_idx in 0..render_start {
                                let pre_line = ed.buffer().line(pre_idx).to_string();
                                let pre_line = pre_line.trim_end_matches('\n').trim_end_matches('\r');
                                ft.process_line(pre_line);
                            }
                            Some(ft)
                        } else {
                            None
                        };

                        let mut line_texts: Vec<(usize, String)> = Vec::with_capacity(render_end - render_start);
                        for line_idx in render_start..render_end {
                            let line_slice = ed.buffer().line(line_idx);
                            let line_str = line_slice.to_string();
                            let line_str = line_str.trim_end_matches('\n').trim_end_matches('\r');
                            line_texts.push((line_idx, line_str.to_string()));
                        }
                        drop(ed);

                        // Compute per-line languages and highlight as blocks.
                        // For non-markdown: all lines share the same language, highlight as one block.
                        // For markdown: group consecutive lines by effective language and highlight each group.
                        let per_line_langs: Vec<highlight::Language> = if let Some(ref mut ft) = fence_tracker {
                            line_texts.iter().map(|(_, s)| ft.process_line(s)).collect()
                        } else {
                            vec![lang.clone(); line_texts.len()]
                        };

                        let highlighted: Vec<String> = highlight_lines_by_language(
                            &line_texts.iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>(),
                            &per_line_langs,
                        );

                        let mut lines_html = Vec::with_capacity(line_texts.len());
                        for (i, (line_idx, _)) in line_texts.iter().enumerate() {
                            let top = *line_idx as f64 * LINE_HEIGHT;
                            let line_num = line_idx + 1;
                            let gutter_text = format!("{:>width$}", line_num, width = gutter_digits);
                            let content_html = &highlighted[i];

                            lines_html.push(view! {
                                <div style=format!(
                                    "position:absolute;top:{}px;left:0;right:0;height:{}px;display:flex;line-height:{}px;",
                                    top, LINE_HEIGHT, LINE_HEIGHT)>
                                    <div class="kode-gutter">{gutter_text}</div>
                                    <div style="flex:1;white-space:pre;overflow:hidden;">
                                        <span data-line=*line_idx inner_html=content_html.clone()
                                            style=format!("min-height:{}px;display:inline-block;", LINE_HEIGHT) />
                                    </div>
                                </div>
                            });
                        }

                        let placeholder_text = placeholder.get();
                        let placeholder_view = (buffer_empty && !placeholder_text.is_empty()).then(|| {
                            let gutter_spacer = format!("{:>width$}", 1, width = gutter_digits);
                            view! {
                                <div style=format!(
                                    "position:absolute;top:0;left:0;right:0;height:{}px;display:flex;line-height:{}px;pointer-events:none;",
                                    LINE_HEIGHT, LINE_HEIGHT)>
                                    <div class="kode-gutter" style="visibility:hidden;">{gutter_spacer}</div>
                                    <div style="flex:1;white-space:pre;overflow:hidden;">
                                        <span class="kode-placeholder">{placeholder_text}</span>
                                    </div>
                                </div>
                            }
                        });

                        view! {
                            <div style=format!("position:relative;height:{}px;min-height:100%;", total_height)>
                                // Background highlights — selection + current-line + markers
                                <div id=overlay_el_id.clone()
                                    style="position:absolute;top:0;left:0;right:0;bottom:0;pointer-events:none;" />
                                // Text lines (later in DOM = on top of highlights)
                                {lines_html}
                                {placeholder_view}
                            </div>
                        }
                    }}
                </pre>

                // Foreground overlay (AFTER content = above text) — cursor + markers
                <div id=fg_overlay_el_id class="kode-overlay"
                    style="position:absolute;top:0;left:0;right:0;bottom:0;pointer-events:none;">
                    <div id=cursor_el_id class="kode-cursor"
                        style="position:absolute;width:2px;pointer-events:none;display:none;"
                        style:height=format!("{}px", LINE_HEIGHT) />
                </div>
            </div>
            <crate::completion::CompletionPopup
                completion_state=completion_state
                editor_root_id=editor_root_id.clone()
                scroll_top=scroll_top
                editor=editor_popup
                on_text_change=notify_text_popup
            />
        </div>
    }
}
