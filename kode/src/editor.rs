use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use editor_core::{Command, CommandExecutor, CursorCommand, EditCommand};
use leptos::prelude::*;
use leptos::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{
    CompositionEvent, FocusEvent, HtmlElement, HtmlTextAreaElement, KeyboardEvent, MouseEvent,
    Node,
};

use crate::highlight::{self, Language};

const LINE_HEIGHT: f64 = 20.0;
const MULTI_CLICK_MS: f64 = 500.0;

// ── DOM measurement helpers ──────────────────────────────────────────────

fn measure_cursor_position(
    editor_el: &HtmlElement,
    line: usize,
    col: usize,
) -> Option<(f64, f64)> {
    let editor_rect = editor_el.get_bounding_client_rect();
    let document = web_sys::window()?.document()?;

    let selector = format!("[data-line=\"{}\"]", line);
    let code_span = editor_el.query_selector(&selector).ok()??;

    let has_text = code_span
        .text_content()
        .map(|t| !t.is_empty())
        .unwrap_or(false);

    if !has_text {
        // Empty line — the span and its parent div may have zero height.
        // Use the parent div's position (it has min-height set to LINE_HEIGHT).
        let span_el: &HtmlElement = code_span.unchecked_ref();
        if let Some(parent) = span_el.parent_element() {
            let pr = parent.get_bounding_client_rect();
            return Some((pr.left() - editor_rect.left(), pr.top() - editor_rect.top()));
        }
        let sr = span_el.get_bounding_client_rect();
        return Some((sr.left() - editor_rect.left(), sr.top() - editor_rect.top()));
    }

    let range = document.create_range().ok()?;

    match find_text_node_at_offset(&code_span, col) {
        Some((node, offset)) => {
            let _ = range.set_start(&node, offset as u32);
            let _ = range.set_end(&node, offset as u32);
            if let Some(rects) = range.get_client_rects() {
                if rects.length() > 0 {
                    let r = rects.get(0).unwrap();
                    return Some((r.left() - editor_rect.left(), r.top() - editor_rect.top()));
                }
            }
            // Fallback: use range over full span content
            let _ = range.select_node_contents(&code_span);
            let r = range.get_bounding_client_rect();
            if col == 0 {
                Some((r.left() - editor_rect.left(), r.top() - editor_rect.top()))
            } else {
                Some((r.right() - editor_rect.left(), r.top() - editor_rect.top()))
            }
        }
        None => {
            // Col is past end of text — use right edge of content
            let _ = range.select_node_contents(&code_span);
            let r = range.get_bounding_client_rect();
            Some((r.right() - editor_rect.left(), r.top() - editor_rect.top()))
        }
    }
}

fn find_text_node_at_offset(
    element: &web_sys::Element,
    target_offset: usize,
) -> Option<(Node, usize)> {
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

fn position_from_click(
    editor_el: &HtmlElement,
    client_x: f64,
    client_y: f64,
    scroll_top: f64,
    line_count: usize,
) -> (usize, usize) {
    let y = client_y - editor_el.get_bounding_client_rect().top() + scroll_top;
    let line = ((y / LINE_HEIGHT).floor() as usize).min(line_count.saturating_sub(1));

    let selector = format!("[data-line=\"{}\"]", line);
    let code_span = match editor_el.query_selector(&selector).ok().flatten() {
        Some(el) => el,
        None => return (line, 0),
    };

    let document = match web_sys::window().and_then(|w| w.document()) {
        Some(d) => d,
        None => return (line, 0),
    };

    let text_nodes = collect_text_nodes(&code_span);
    let mut global_offset = 0usize;
    let mut best_col = 0usize;
    let mut best_dist = f64::MAX;

    for text_node in &text_nodes {
        let text_len = text_node.text_content().map(|t| t.len()).unwrap_or(0);
        for local_offset in 0..=text_len {
            if let Ok(range) = document.create_range() {
                let _ = range.set_start(text_node, local_offset as u32);
                let _ = range.set_end(text_node, local_offset as u32);
                if let Some(rects) = range.get_client_rects() {
                    if rects.length() > 0 {
                        let r = rects.get(0).unwrap();
                        let dist = (r.left() - client_x).abs();
                        if dist < best_dist {
                            best_dist = dist;
                            best_col = global_offset + local_offset;
                        }
                    }
                }
            }
        }
        global_offset += text_len;
    }

    (line, best_col)
}

fn collect_text_nodes(node: &web_sys::Element) -> Vec<Node> {
    let mut result = Vec::new();
    collect_text_nodes_recursive(node.as_ref(), &mut result);
    result
}

fn collect_text_nodes_recursive(node: &Node, result: &mut Vec<Node>) {
    if node.node_type() == Node::TEXT_NODE {
        result.push(node.clone());
        return;
    }
    let children = node.child_nodes();
    for i in 0..children.length() {
        if let Some(child) = children.get(i) {
            collect_text_nodes_recursive(&child, result);
        }
    }
}

/// Convert line/column to byte offset in text.
fn line_col_to_offset(text: &str, line: usize, col: usize) -> usize {
    let mut offset = 0;
    for (i, l) in text.lines().enumerate() {
        if i == line {
            return offset + col.min(l.len());
        }
        offset += l.len() + 1;
    }
    text.len()
}

// ── Editor component ─────────────────────────────────────────────────────

#[component]
pub fn CodeEditor(
    #[prop(into, default = Signal::stored(Language::Plain))]
    language: Signal<Language>,
    #[prop(into, default = Signal::stored(String::new()))]
    content: Signal<String>,
    #[prop(optional)] on_change: Option<Arc<dyn Fn(String) + Send + Sync>>,
) -> impl IntoView {
    let executor = Arc::new(Mutex::new(CommandExecutor::new(
        &content.get_untracked(),
        80,
    )));

    let (text_version, set_text_version) = signal(0u64);
    let (cursor_version, set_cursor_version) = signal(0u64);
    let (focused, set_focused) = signal(false);
    let (composing, set_composing) = signal(false);
    let (scroll_top, set_scroll_top) = signal(0.0f64);
    let (cursor_visible, set_cursor_visible) = signal(true);

    // Mouse drag state
    let (dragging, set_dragging) = signal(false);

    // Multi-click tracking (for double/triple click)
    let last_click_time = Rc::new(Cell::new(0.0f64));
    let click_count = Rc::new(Cell::new(0u32));
    let last_click_line = Rc::new(Cell::new(0usize));

    let textarea_ref = NodeRef::<leptos::html::Textarea>::new();
    let editor_ref = NodeRef::<leptos::html::Div>::new();
    let scroll_container_ref = NodeRef::<leptos::html::Div>::new();

    // ── Blink timer ──────────────────────────────────────────────────
    let blink_timer_id = Rc::new(Cell::new(0i32));
    let blink_interval_id = Rc::new(Cell::new(0i32));

    let reset_blink = {
        let blink_timer_id = blink_timer_id.clone();
        let blink_interval_id = blink_interval_id.clone();
        move || {
            let window = web_sys::window().unwrap();
            let tid = blink_timer_id.get();
            if tid != 0 {
                window.clear_timeout_with_handle(tid);
            }
            let iid = blink_interval_id.get();
            if iid != 0 {
                window.clear_interval_with_handle(iid);
                blink_interval_id.set(0);
            }
            set_cursor_visible.set(true);

            let blink_interval_id = blink_interval_id.clone();
            let window_inner = window.clone();
            let cb = Closure::once_into_js(move || {
                let toggle = Closure::wrap(Box::new(move || {
                    set_cursor_visible.update(|v| *v = !*v);
                }) as Box<dyn FnMut()>);
                let iid = window_inner
                    .set_interval_with_callback_and_timeout_and_arguments_0(
                        toggle.as_ref().unchecked_ref(),
                        500,
                    )
                    .unwrap_or(0);
                blink_interval_id.set(iid);
                toggle.forget();
            });
            let tid = window
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    cb.as_ref().unchecked_ref(),
                    500,
                )
                .unwrap_or(0);
            blink_timer_id.set(tid);
        }
    };

    let on_change = Arc::new(on_change);

    let notify_cursor = {
        let reset_blink = reset_blink.clone();
        move || {
            set_cursor_version.update(|v| *v += 1);
            reset_blink();
        }
    };

    let notify_text = {
        let executor = executor.clone();
        let on_change = on_change.clone();
        let notify_cursor = notify_cursor.clone();
        move || {
            set_text_version.update(|v| *v += 1);
            notify_cursor();
            if let Some(ref cb) = *on_change {
                cb(executor.lock().unwrap().editor().get_text());
            }
        }
    };

    // ── External content/language watchers ────────────────────────────
    {
        let executor = executor.clone();
        let mut prev_content = content.get_untracked();
        Effect::new(move |_| {
            let new_content = content.get();
            if new_content != prev_content {
                prev_content = new_content.clone();
                let mut exec = executor.lock().unwrap();
                let old_len = exec.editor().char_count();
                if old_len > 0 {
                    let _ = exec.execute(Command::Edit(EditCommand::Delete {
                        start: 0,
                        length: old_len,
                    }));
                }
                if !new_content.is_empty() {
                    let _ = exec.execute(Command::Edit(EditCommand::Insert {
                        offset: 0,
                        text: new_content,
                    }));
                }
                let _ = exec.execute(Command::Cursor(CursorCommand::MoveTo {
                    line: 0,
                    column: 0,
                }));
                drop(exec);
                set_text_version.update(|v| *v += 1);
                set_cursor_version.update(|v| *v += 1);
            }
        });
    }

    Effect::new(move |_| {
        let _ = language.get();
        set_text_version.update(|v| *v += 1);
    });

    // ── Mouse handlers ───────────────────────────────────────────────

    let on_editor_mousedown = {
        let executor = executor.clone();
        let notify_cursor = notify_cursor.clone();
        let last_click_time = last_click_time.clone();
        let click_count = click_count.clone();
        let last_click_line = last_click_line.clone();
        move |ev: MouseEvent| {
            ev.prevent_default();

            if let Some(textarea) = textarea_ref.get() {
                let el: &HtmlTextAreaElement = textarea.as_ref();
                let _ = el.focus();
            }

            let Some(editor) = editor_ref.get() else {
                return;
            };
            let editor_el: &HtmlElement = editor.as_ref();
            let line_count = executor.lock().unwrap().editor().line_count();
            let (line, col) = position_from_click(
                editor_el,
                ev.client_x() as f64,
                ev.client_y() as f64,
                scroll_top.get_untracked(),
                line_count,
            );

            // Multi-click detection
            let now = js_sys::Date::now();
            let prev_time = last_click_time.get();
            let prev_line = last_click_line.get();

            if now - prev_time < MULTI_CLICK_MS && line == prev_line {
                click_count.set(click_count.get() + 1);
            } else {
                click_count.set(1);
            }
            last_click_time.set(now);
            last_click_line.set(line);

            let clicks = click_count.get();
            let mut exec = executor.lock().unwrap();

            match clicks {
                3 => {
                    // Triple click: select line
                    let _ = exec.execute(Command::Cursor(CursorCommand::MoveTo {
                        line,
                        column: col,
                    }));
                    let _ = exec.execute(Command::Cursor(CursorCommand::SelectLine));
                }
                2 => {
                    // Double click: select word
                    let _ = exec.execute(Command::Cursor(CursorCommand::MoveTo {
                        line,
                        column: col,
                    }));
                    let _ = exec.execute(Command::Cursor(CursorCommand::SelectWord));
                }
                _ => {
                    if ev.shift_key() {
                        // Shift+click: extend selection to clicked position
                        let target = editor_core::Position::new(line, col);
                        if exec.editor().selection().is_none() {
                            let pos = exec.editor().cursor_position();
                            let _ = exec.execute(Command::Cursor(CursorCommand::SetSelection {
                                start: pos,
                                end: pos,
                            }));
                        }
                        let _ = exec.execute(Command::Cursor(CursorCommand::ExtendSelection {
                            to: target,
                        }));
                    } else {
                        // Single click: clear selection, set cursor
                        let _ = exec.execute(Command::Cursor(CursorCommand::ClearSelection));
                        let _ = exec.execute(Command::Cursor(CursorCommand::MoveTo {
                            line,
                            column: col,
                        }));
                    }
                    // Start drag
                    set_dragging.set(true);
                }
            }

            drop(exec);
            notify_cursor();
        }
    };

    let on_editor_mousemove = {
        let executor = executor.clone();
        let notify_cursor = notify_cursor.clone();
        move |ev: MouseEvent| {
            if !dragging.get_untracked() {
                return;
            }

            let Some(editor) = editor_ref.get() else {
                return;
            };
            let editor_el: &HtmlElement = editor.as_ref();
            let line_count = executor.lock().unwrap().editor().line_count();
            let (line, col) = position_from_click(
                editor_el,
                ev.client_x() as f64,
                ev.client_y() as f64,
                scroll_top.get_untracked(),
                line_count,
            );

            let mut exec = executor.lock().unwrap();
            // If no selection yet (first drag movement), set anchor at current cursor
            if exec.editor().selection().is_none() {
                let pos = exec.editor().cursor_position();
                let _ = exec.execute(Command::Cursor(CursorCommand::SetSelection {
                    start: pos,
                    end: pos,
                }));
            }
            let target = editor_core::Position::new(line, col);
            let _ = exec.execute(Command::Cursor(CursorCommand::ExtendSelection {
                to: target,
            }));
            drop(exec);
            notify_cursor();
        }
    };

    let on_editor_mouseup = move |_ev: MouseEvent| {
        set_dragging.set(false);
    };

    // ── Keyboard handler ─────────────────────────────────────────────

    let on_keydown = {
        let executor = executor.clone();
        let notify_cursor = notify_cursor.clone();
        let notify_text = notify_text.clone();
        move |ev: KeyboardEvent| {
            if composing.get_untracked() {
                return;
            }

            let key = ev.key();
            let ctrl = ev.ctrl_key() || ev.meta_key();
            let shift = ev.shift_key();

            // ── Shift+Arrow: extend selection ────────────────────────
            if shift
                && matches!(
                    key.as_str(),
                    "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" | "Home" | "End"
                )
            {
                ev.prevent_default();
                let mut exec = executor.lock().unwrap();
                if exec.editor().selection().is_none() {
                    let pos = exec.editor().cursor_position();
                    let _ = exec.execute(Command::Cursor(CursorCommand::SetSelection {
                        start: pos,
                        end: pos,
                    }));
                }
                let move_cmd = match key.as_str() {
                    "ArrowLeft" if ctrl => Command::Cursor(CursorCommand::MoveWordLeft),
                    "ArrowRight" if ctrl => Command::Cursor(CursorCommand::MoveWordRight),
                    "ArrowLeft" => Command::Cursor(CursorCommand::MoveGraphemeLeft),
                    "ArrowRight" => Command::Cursor(CursorCommand::MoveGraphemeRight),
                    "ArrowUp" => Command::Cursor(CursorCommand::MoveBy {
                        delta_line: -1,
                        delta_column: 0,
                    }),
                    "ArrowDown" => Command::Cursor(CursorCommand::MoveBy {
                        delta_line: 1,
                        delta_column: 0,
                    }),
                    "Home" => Command::Cursor(CursorCommand::MoveToLineStart),
                    "End" => Command::Cursor(CursorCommand::MoveToLineEnd),
                    _ => unreachable!(),
                };
                let _ = exec.execute(move_cmd);
                let new_pos = exec.editor().cursor_position();
                let _ = exec.execute(Command::Cursor(CursorCommand::ExtendSelection {
                    to: new_pos,
                }));
                drop(exec);
                notify_cursor();
                return;
            }

            // ── Main key dispatch ────────────────────────────────────
            enum Action {
                Cursor(Command),
                Text(Command),
                None,
            }

            let action = match key.as_str() {
                // Movement: clear selection first, then move
                "ArrowLeft" if ctrl => {
                    let mut exec = executor.lock().unwrap();
                    let _ = exec.execute(Command::Cursor(CursorCommand::ClearSelection));
                    let _ = exec.execute(Command::Cursor(CursorCommand::MoveWordLeft));
                    drop(exec);
                    ev.prevent_default();
                    notify_cursor();
                    return;
                }
                "ArrowRight" if ctrl => {
                    let mut exec = executor.lock().unwrap();
                    let _ = exec.execute(Command::Cursor(CursorCommand::ClearSelection));
                    let _ = exec.execute(Command::Cursor(CursorCommand::MoveWordRight));
                    drop(exec);
                    ev.prevent_default();
                    notify_cursor();
                    return;
                }
                "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" | "Home" | "End" => {
                    let move_cmd = match key.as_str() {
                        "ArrowLeft" => Command::Cursor(CursorCommand::MoveGraphemeLeft),
                        "ArrowRight" => Command::Cursor(CursorCommand::MoveGraphemeRight),
                        "ArrowUp" => Command::Cursor(CursorCommand::MoveBy {
                            delta_line: -1,
                            delta_column: 0,
                        }),
                        "ArrowDown" => Command::Cursor(CursorCommand::MoveBy {
                            delta_line: 1,
                            delta_column: 0,
                        }),
                        "Home" => Command::Cursor(CursorCommand::MoveToLineStart),
                        "End" => Command::Cursor(CursorCommand::MoveToLineEnd),
                        _ => unreachable!(),
                    };
                    let mut exec = executor.lock().unwrap();
                    let _ = exec.execute(Command::Cursor(CursorCommand::ClearSelection));
                    let _ = exec.execute(move_cmd);
                    drop(exec);
                    ev.prevent_default();
                    notify_cursor();
                    return;
                }

                // Text editing (editor-core handles selection deletion automatically)
                "Backspace" if ctrl => Action::Text(Command::Edit(EditCommand::DeleteWordBack)),
                "Delete" if ctrl => Action::Text(Command::Edit(EditCommand::DeleteWordForward)),
                "Backspace" => Action::Text(Command::Edit(EditCommand::Backspace)),
                "Delete" => Action::Text(Command::Edit(EditCommand::DeleteForward)),
                "Enter" => Action::Text(Command::Edit(EditCommand::InsertNewline {
                    auto_indent: true,
                })),
                "Tab" if shift => Action::Text(Command::Edit(EditCommand::Outdent)),
                "Tab" => Action::Text(Command::Edit(EditCommand::InsertTab)),

                // Ctrl shortcuts
                "a" if ctrl => {
                    let exec = executor.lock().unwrap();
                    let line_count = exec.editor().line_count();
                    let last_line = line_count.saturating_sub(1);
                    let text = exec.editor().get_text();
                    let last_col = text.lines().last().map(|l| l.len()).unwrap_or(0);
                    drop(exec);
                    Action::Cursor(Command::Cursor(CursorCommand::SetSelection {
                        start: editor_core::Position::new(0, 0),
                        end: editor_core::Position::new(last_line, last_col),
                    }))
                }
                "z" if ctrl && shift => Action::Text(Command::Edit(EditCommand::Redo)),
                "z" if ctrl => Action::Text(Command::Edit(EditCommand::Undo)),
                "y" if ctrl => Action::Text(Command::Edit(EditCommand::Redo)),
                "d" if ctrl => {
                    Action::Text(Command::Edit(EditCommand::DuplicateLines))
                }

                // Clipboard
                "c" if ctrl => {
                    copy_selection_to_textarea(&executor, &textarea_ref);
                    return; // Let browser handle copy
                }
                "x" if ctrl => {
                    copy_selection_to_textarea(&executor, &textarea_ref);
                    let _ = executor
                        .lock()
                        .unwrap()
                        .execute(Command::Edit(EditCommand::Backspace));
                    notify_text();
                    return;
                }
                "v" if ctrl => return, // Let browser paste into textarea

                // Find/Replace (placeholder — will be implemented next)
                "f" if ctrl => {
                    ev.prevent_default();
                    // TODO: open find panel
                    return;
                }
                "h" if ctrl => {
                    ev.prevent_default();
                    // TODO: open find+replace panel
                    return;
                }

                _ => {
                    if key.len() == 1 && !ctrl {
                        return; // Let it flow to input event
                    }
                    Action::None
                }
            };

            match action {
                Action::Cursor(cmd) => {
                    ev.prevent_default();
                    let _ = executor.lock().unwrap().execute(cmd);
                    notify_cursor();
                }
                Action::Text(cmd) => {
                    ev.prevent_default();
                    let _ = executor.lock().unwrap().execute(cmd);
                    notify_text();
                }
                Action::None => {}
            }
        }
    };

    // ── Text input ───────────────────────────────────────────────────

    let on_input = {
        let executor = executor.clone();
        let notify_text = notify_text.clone();
        move |_ev: web_sys::Event| {
            if composing.get_untracked() {
                return;
            }
            if let Some(textarea) = textarea_ref.get() {
                let el: &HtmlTextAreaElement = textarea.as_ref();
                let value = el.value();
                if !value.is_empty() {
                    let _ = executor
                        .lock()
                        .unwrap()
                        .execute(Command::Edit(EditCommand::InsertText { text: value }));
                    el.set_value("");
                    notify_text();
                }
            }
        }
    };

    let on_composition_start = move |_ev: CompositionEvent| {
        set_composing.set(true);
    };

    let on_composition_end = {
        let executor = executor.clone();
        let notify_text = notify_text.clone();
        move |ev: CompositionEvent| {
            set_composing.set(false);
            if let Some(data) = ev.data() {
                if !data.is_empty() {
                    let _ = executor
                        .lock()
                        .unwrap()
                        .execute(Command::Edit(EditCommand::InsertText { text: data }));
                    if let Some(textarea) = textarea_ref.get() {
                        let el: &HtmlTextAreaElement = textarea.as_ref();
                        el.set_value("");
                    }
                    notify_text();
                }
            }
        }
    };

    let on_focus = move |_ev: FocusEvent| set_focused.set(true);
    let on_blur = move |_ev: FocusEvent| set_focused.set(false);

    // Native scroll handler — reads scrollTop from the scroll container
    let on_scroll = {
        move |_ev: web_sys::Event| {
            if let Some(el) = scroll_container_ref.get() {
                let html_el: &web_sys::HtmlElement = el.as_ref();
                set_scroll_top.set(html_el.scroll_top() as f64);
            }
        }
    };

    // ── Rendering ────────────────────────────────────────────────────

    let theme_css = highlight::theme_css();

    let container_style = format!(
        "position: relative; width: 100%; height: 100%; \
         font-family: 'JetBrains Mono', 'Fira Code', 'Cascadia Code', 'Consolas', monospace; \
         font-size: 14px; line-height: {}px; overflow: hidden; cursor: text; border-radius: 8px;",
        LINE_HEIGHT
    );

    let textarea_style = "position: absolute; left: -9999px; top: 0; width: 1px; height: 1px; \
                          opacity: 0; white-space: pre; font-size: 14px;";

    let executor_content = executor.clone();
    let executor_cursor = executor.clone();

    view! {
        <style>{theme_css}</style>
        <div
            node_ref=editor_ref
            class="kode-editor"
            style=container_style
            on:mousedown=on_editor_mousedown
            on:mousemove=on_editor_mousemove
            on:mouseup=on_editor_mouseup
        >
            <textarea
                node_ref=textarea_ref
                style=textarea_style
                autocapitalize="off"
                autocomplete="off"
                spellcheck="false"
                on:keydown=on_keydown
                on:input=on_input
                on:compositionstart=on_composition_start
                on:compositionend=on_composition_end
                on:focus=on_focus
                on:blur=on_blur
            />

            // Scroll container — native browser scrolling with scrollbar
            <div
                node_ref=scroll_container_ref
                style="position: absolute; top: 0; left: 0; right: 0; bottom: 0; overflow-y: auto; overflow-x: hidden;"
                on:scroll=on_scroll
            >

            // CONTENT LAYER
            // Renders plain text instantly. Highlights visible lines first, then rest in background chunks.
            {move || {
                let _text_ver = text_version.get();
                let lang = language.get_untracked();

                let exec = executor_content.lock().unwrap();
                let text = exec.editor().get_text();
                let line_count = exec.editor().line_count();
                drop(exec);

                // Plain escaped text for instant render
                let plain_lines: Vec<String> = text.lines().map(highlight::html_escape).collect();

                let max_line_str = format!("{}", line_count);
                let gutter_template_width = max_line_str.len();

                let line_views: Vec<_> = (0..line_count)
                    .map(|i| {
                        let y = i as f64 * LINE_HEIGHT;
                        let line_num = i + 1;
                        let line_html = plain_lines.get(i).cloned().unwrap_or_default();
                        let line_num_str = format!("{:>width$}", line_num, width = gutter_template_width);

                        let line_style = format!(
                            "position: absolute; top: {}px; left: 0; right: 0; height: {}px; display: flex; align-items: center;",
                            y, LINE_HEIGHT
                        );

                        view! {
                            <div style=line_style>
                                <div class="kode-gutter" style=
                                    "text-align: right; padding: 0 24px 0 16px; \
                                     color: #565f89; user-select: none; flex-shrink: 0; white-space: pre;"
                                >{line_num_str}</div>
                                <div style=format!("flex: 1; white-space: pre; position: relative; min-height: {}px;", LINE_HEIGHT)>
                                    <span data-line=i inner_html=line_html style="position: relative; z-index: 1;" />
                                </div>
                            </div>
                        }
                    })
                    .collect();

                let content_height = line_count as f64 * LINE_HEIGHT;
                let content_style = format!(
                    "position: relative; width: 100%; height: {}px; min-height: 100%;",
                    content_height
                );

                // Async highlight: visible viewport first, then background chunks
                let text_for_hl = text.clone();
                let editor_ref_hl = editor_ref;
                let current_scroll = scroll_top.get_untracked();
                wasm_bindgen_futures::spawn_local(async move {
                    // Yield so the plain text DOM renders first
                    gloo_timers::future::TimeoutFuture::new(0).await;

                    // Highlight full document (sub-ms after first use)
                    let highlighted_html = highlight::highlight_text(&text_for_hl, lang);
                    let highlighted_lines: Vec<String> = highlighted_html
                        .split('\n')
                        .map(|s| s.to_string())
                        .collect();

                    let Some(editor) = editor_ref_hl.get() else { return };
                    let editor_el: &HtmlElement = editor.as_ref();

                    // Determine visible line range
                    let viewport_height = editor_el.client_height() as f64;
                    let first_visible = (current_scroll / LINE_HEIGHT).floor() as usize;
                    let last_visible = ((current_scroll + viewport_height) / LINE_HEIGHT).ceil() as usize;
                    let last_visible = last_visible.min(highlighted_lines.len());

                    // Phase 1: Apply visible lines immediately
                    for i in first_visible..last_visible {
                        if let Some(hl) = highlighted_lines.get(i) {
                            let selector = format!("[data-line=\"{}\"]", i);
                            if let Ok(Some(span)) = editor_el.query_selector(&selector) {
                                span.set_inner_html(hl);
                            }
                        }
                    }

                    // Phase 2: Apply remaining lines in chunks, yielding between each chunk
                    const CHUNK_SIZE: usize = 100;
                    let remaining: Vec<usize> = (0..highlighted_lines.len())
                        .filter(|i| *i < first_visible || *i >= last_visible)
                        .collect();

                    for chunk in remaining.chunks(CHUNK_SIZE) {
                        gloo_timers::future::TimeoutFuture::new(0).await;
                        for &i in chunk {
                            if let Some(hl) = highlighted_lines.get(i) {
                                let selector = format!("[data-line=\"{}\"]", i);
                                if let Ok(Some(span)) = editor_el.query_selector(&selector) {
                                    span.set_inner_html(hl);
                                }
                            }
                        }
                    }
                });

                view! {
                    <pre class="kode-content" style=content_style>
                        {line_views}
                    </pre>
                }
            }}

            // CURSOR + SELECTION OVERLAY
            {move || {
                let _cursor_ver = cursor_version.get();
                let _text_ver = text_version.get();
                let is_focused = focused.get();
                let is_visible = cursor_visible.get();

                let exec = executor_cursor.lock().unwrap();
                let cursor_pos = exec.editor().cursor_position();
                let selection = exec.editor().selection().cloned();
                let text = exec.editor().get_text();
                drop(exec);

                let editor = editor_ref.get();

                // Line highlight (no selection active)
                let line_highlight = if is_focused && selection.is_none() {
                    editor.as_ref().and_then(|el| {
                        let html_el: &HtmlElement = el.as_ref();
                        let (_, top) = measure_cursor_position(html_el, cursor_pos.line, 0)?;
                        let style = format!(
                            "position: absolute; top: {}px; left: 0; right: 0; height: {}px; \
                             background: rgba(255,255,255,0.03); pointer-events: none;",
                            top, LINE_HEIGHT
                        );
                        Some(view! { <div style=style /> })
                    })
                } else {
                    None
                };

                // Selection highlights (semi-transparent, rendered behind text via z-index)
                let sel_views: Vec<_> = if let Some(sel) = &selection {
                    let start = std::cmp::min(sel.start, sel.end);
                    let end = std::cmp::max(sel.start, sel.end);
                    if start == end {
                        vec![]
                    } else {
                        (start.line..=end.line)
                            .filter_map(|line| {
                                let el = editor.as_ref()?;
                                let html_el: &HtmlElement = el.as_ref();

                                let line_len =
                                    text.lines().nth(line).map(|l| l.len()).unwrap_or(0);
                                let sel_start = if line == start.line { start.column } else { 0 };
                                let sel_end = if line == end.line { end.column } else { line_len };

                                if sel_start >= sel_end {
                                    return None;
                                }

                                let s = measure_cursor_position(html_el, line, sel_start)?;
                                let e = measure_cursor_position(html_el, line, sel_end)?;
                                let width = (e.0 - s.0).max(0.0);

                                let style = format!(
                                    "position: absolute; top: {}px; left: {}px; width: {}px; height: {}px; \
                                     background: rgba(40, 52, 87, 0.6); pointer-events: none; z-index: 0;",
                                    s.1, s.0, width, LINE_HEIGHT
                                );
                                Some(view! { <span style=style /> })
                            })
                            .collect()
                    }
                } else {
                    vec![]
                };

                // Cursor caret (highest z-index)
                let cursor_view = if is_focused && is_visible {
                    editor.as_ref().and_then(|el| {
                        let html_el: &HtmlElement = el.as_ref();
                        let (left, top) =
                            measure_cursor_position(html_el, cursor_pos.line, cursor_pos.column)?;
                        let style = format!(
                            "position: absolute; top: {}px; left: {}px; width: 2px; height: {}px; \
                             background: #c0caf5; pointer-events: none; z-index: 2;",
                            top, left, LINE_HEIGHT
                        );
                        Some(view! { <span style=style /> })
                    })
                } else {
                    None
                };

                view! {
                    <div style="position: absolute; top: 0; left: 0; right: 0; bottom: 0; pointer-events: none;">
                        {line_highlight}
                        {sel_views}
                        {cursor_view}
                    </div>
                }
            }}

            </div> // close scroll container
        </div> // close editor
    }
}

/// Copy the current selection text into the hidden textarea for browser clipboard access.
fn copy_selection_to_textarea(
    executor: &Arc<Mutex<CommandExecutor>>,
    textarea_ref: &NodeRef<leptos::html::Textarea>,
) {
    let exec = executor.lock().unwrap();
    if let Some(sel) = exec.editor().selection() {
        let text = exec.editor().get_text();
        let start = std::cmp::min(sel.start, sel.end);
        let end = std::cmp::max(sel.start, sel.end);
        let start_offset = line_col_to_offset(&text, start.line, start.column);
        let end_offset = line_col_to_offset(&text, end.line, end.column);
        if start_offset < end_offset && end_offset <= text.len() {
            let selected = &text[start_offset..end_offset];
            if let Some(textarea) = textarea_ref.get() {
                let el: &HtmlTextAreaElement = textarea.as_ref();
                el.set_value(selected);
                el.select();
            }
        }
    }
}
