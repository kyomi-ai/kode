use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use kode_core::{CompletionContext, CompletionItem, CompletionKind, CompletionTrigger, Editor, Position};
use leptos::prelude::*;
use leptos::tachys::view::any_view::AnyView;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::HtmlElement;

/// An async function that provides completion items given editor context.
pub type CompletionProviderFn = Arc<
    dyn Fn(CompletionContext) -> Pin<Box<dyn Future<Output = Vec<CompletionItem>>>> + Send + Sync,
>;

/// A function that renders a custom view for a completion item.
///
/// Receives the `CompletionItem` and whether it is currently selected.
/// Returns an `AnyView` to render inside the popup row.
///
/// When provided on a `CompletionProviderConfig`, items from that provider
/// use this renderer instead of the default kind+label+detail layout.
pub type CompletionItemRenderer =
    Arc<dyn Fn(&CompletionItem, bool) -> AnyView + Send + Sync>;

/// Configuration for a single completion provider.
#[derive(Clone)]
pub struct CompletionProviderConfig {
    /// The async completion function.
    pub provider: CompletionProviderFn,
    /// Characters that trigger an immediate (no debounce) call.
    pub trigger_characters: Vec<char>,
    /// If true, call this provider on any keystroke (debounced).
    pub activate_on_typing: bool,
    /// Optional custom renderer for items from this provider.
    ///
    /// When set, the popup uses this function to render each item instead
    /// of the default kind badge + label + detail layout. The renderer
    /// receives the item and a boolean indicating whether it is selected.
    pub render_item: Option<CompletionItemRenderer>,
}

/// State machine for the completion popup lifecycle.
#[derive(Clone)]
pub(crate) enum CompletionState {
    Idle,
    Active {
        /// Full unfiltered results from providers.
        items: Vec<CompletionItem>,
        /// Per-item custom renderer (parallel to `items`). `None` = default rendering.
        renderers: Vec<Option<CompletionItemRenderer>>,
        /// Filtered indices into `items`.
        filtered: Vec<usize>,
        /// Currently selected index in `filtered`.
        selected_index: usize,
        /// Text typed since `word_start`.
        prefix: String,
        /// Position where the completed word starts (for popup positioning and insertion).
        word_start: Position,
    },
}

impl CompletionState {
    /// Returns `true` if the completion popup is active.
    pub(crate) fn is_active(&self) -> bool {
        matches!(self, CompletionState::Active { .. })
    }

    /// Dismiss the completion popup, returning to Idle.
    pub(crate) fn dismiss(&mut self) {
        *self = CompletionState::Idle;
    }

    /// Returns the word start position, or `None` if idle.
    pub(crate) fn word_start(&self) -> Option<Position> {
        match self {
            CompletionState::Active { word_start, .. } => Some(*word_start),
            CompletionState::Idle => None,
        }
    }

    /// Returns the currently selected completion item, or `None` if idle.
    pub(crate) fn selected_item(&self) -> Option<&CompletionItem> {
        match self {
            CompletionState::Idle => None,
            CompletionState::Active {
                items,
                filtered,
                selected_index,
                ..
            } => filtered
                .get(*selected_index)
                .and_then(|&idx| items.get(idx)),
        }
    }

    /// Move the selection by `delta`, wrapping around. Only operates in Active state.
    pub(crate) fn move_selection(&mut self, delta: i32) {
        if let CompletionState::Active {
            filtered,
            selected_index,
            ..
        } = self
        {
            let len = filtered.len();
            if len == 0 {
                return;
            }
            let current = *selected_index as i32;
            let new_index = ((current + delta) % len as i32 + len as i32) % len as i32;
            *selected_index = new_index as usize;
        }
    }

    /// Update the filter prefix. Filters `items` by checking if `item.label` starts with
    /// `new_prefix` (case-insensitive). Clamps `selected_index`. If filtered becomes empty,
    /// transitions to Idle.
    pub(crate) fn update_filter(&mut self, new_prefix: &str) {
        let should_idle = if let CompletionState::Active {
            items,
            filtered,
            selected_index,
            prefix,
            ..
        } = self
        {
            let lower_prefix = new_prefix.to_lowercase();
            *filtered = items
                .iter()
                .enumerate()
                .filter(|(_, item)| item.label.to_lowercase().starts_with(&lower_prefix))
                .map(|(i, _)| i)
                .collect();
            *prefix = new_prefix.to_string();

            if filtered.is_empty() {
                true
            } else {
                if *selected_index >= filtered.len() {
                    *selected_index = filtered.len() - 1;
                }
                false
            }
        } else {
            false
        };

        if should_idle {
            *self = CompletionState::Idle;
        }
    }

    /// Create an Active state from provider results. Sorts items by `sort_order` then
    /// alphabetically by `label`. Runs initial filter with `prefix`. If nothing matches,
    /// returns Idle.
    ///
    /// `renderers` is a parallel vec to `items` — each entry is the custom renderer
    /// for that item (or `None` for default rendering).
    pub(crate) fn activate(
        items: Vec<CompletionItem>,
        renderers: Vec<Option<CompletionItemRenderer>>,
        prefix: &str,
        word_start: Position,
    ) -> Self {
        // Sort items and renderers together
        let mut pairs: Vec<(CompletionItem, Option<CompletionItemRenderer>)> =
            items.into_iter().zip(renderers).collect();
        pairs.sort_by(|(a, _), (b, _)| {
            a.sort_order
                .cmp(&b.sort_order)
                .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
        });

        let lower_prefix = prefix.to_lowercase();
        let filtered: Vec<usize> = pairs
            .iter()
            .enumerate()
            .filter(|(_, (item, _))| item.label.to_lowercase().starts_with(&lower_prefix))
            .map(|(i, _)| i)
            .collect();

        if filtered.is_empty() {
            return CompletionState::Idle;
        }

        let (items, renderers): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();

        CompletionState::Active {
            items,
            renderers,
            filtered,
            selected_index: 0,
            prefix: prefix.to_string(),
            word_start,
        }
    }
}

/// Default debounce delay for typing-triggered completions (milliseconds).
const COMPLETION_DEBOUNCE_MS: i32 = 100;

/// Spawn the reactive completion pipeline.
///
/// Watches `trigger_signal` — when it transitions from `None` to `Some(trigger)`,
/// invokes matching providers (immediately for `TriggerCharacter`/`Invoke`, debounced
/// for `Typing`), then activates the completion state with the merged results.
pub(crate) fn spawn_completion_pipeline(
    providers: Signal<Vec<CompletionProviderConfig>>,
    editor: Arc<std::sync::Mutex<Editor>>,
    text_version: RwSignal<u64>,
    cursor_version: RwSignal<u64>,
    completion_state: RwSignal<CompletionState>,
    trigger_signal: RwSignal<Option<CompletionTrigger>>,
) {
    let timer_handle: StoredValue<Option<i32>> = StoredValue::new(None);
    // Per-provider result slots (mirrors the diagnostic pipeline pattern).
    // Each provider's results are stored in its slot; when any provider responds,
    // all slots are merged into a single activation.
    // Per-provider slots: (items, renderer) pairs.
    let provider_results: StoredValue<Vec<(Vec<CompletionItem>, Option<CompletionItemRenderer>)>> =
        StoredValue::new(vec![]);

    Effect::new(move |_| {
        let trigger = trigger_signal.get();
        // Track cursor_version so the effect re-runs when cursor moves
        let _cursor_ver = cursor_version.get();

        let trigger = match trigger {
            Some(t) => t,
            None => return,
        };

        let providers_list = providers.get();
        if providers_list.is_empty() {
            trigger_signal.set(None);
            provider_results.set_value(vec![]);
            return;
        }

        // Cancel any pending debounce timer
        if let Some(handle) = timer_handle.get_value() {
            if let Some(window) = web_sys::window() {
                window.clear_timeout_with_handle(handle);
            }
            timer_handle.set_value(None);
        }

        let needs_debounce = trigger == CompletionTrigger::Typing;
        let editor = editor.clone();

        let fire = move || {
            let (text, cursor, word_start, version) = {
                let ed = editor.lock().unwrap();
                let text = ed.text();
                let cursor = ed.cursor();
                let word_start = ed.word_start_before_cursor();
                let version = text_version.get_untracked();
                (text, cursor, word_start, version)
            };

            // Compute the prefix: text from word_start to cursor on the current line
            let prefix = {
                let line_text = text.lines().nth(cursor.line).unwrap_or("");
                let start_col = word_start.col.min(line_text.len());
                let end_col = cursor.col.min(line_text.len());
                line_text[start_col..end_col].to_string()
            };

            // Filter providers based on trigger type, tracking original index + renderer
            let matching: Vec<(usize, CompletionProviderFn, Option<CompletionItemRenderer>)> =
                providers_list
                    .iter()
                    .enumerate()
                    .filter(|(_, cfg)| match &trigger {
                        CompletionTrigger::TriggerCharacter(ch) => {
                            cfg.trigger_characters.contains(ch)
                        }
                        CompletionTrigger::Typing => cfg.activate_on_typing,
                        CompletionTrigger::Invoke => true,
                    })
                    .map(|(idx, cfg)| (idx, cfg.provider.clone(), cfg.render_item.clone()))
                    .collect();

            if matching.is_empty() {
                trigger_signal.set(None);
                return;
            }

            // Reset per-provider slots to match current provider count
            provider_results.set_value(
                providers_list.iter().map(|_| (vec![], None)).collect(),
            );

            let context = CompletionContext {
                text,
                cursor,
                version,
                trigger: trigger.clone(),
            };

            for (idx, provider, renderer) in matching {
                let ctx = context.clone();
                let prefix = prefix.clone();
                leptos::task::spawn_local(async move {
                    let items = provider(ctx).await;

                    // Discard stale results — text has changed since we started
                    if text_version.get_untracked() != version {
                        return;
                    }

                    // Update this provider's slot
                    provider_results.update_value(|results| {
                        if idx < results.len() {
                            results[idx] = (items, renderer.clone());
                        }
                    });

                    // Merge all provider slots into items + renderers vecs
                    let (merged_items, merged_renderers) = provider_results.with_value(|results| {
                        let mut items = Vec::new();
                        let mut renderers = Vec::new();
                        for (slot_items, slot_renderer) in results {
                            for item in slot_items {
                                items.push(item.clone());
                                renderers.push(slot_renderer.clone());
                            }
                        }
                        (items, renderers)
                    });

                    let new_state =
                        CompletionState::activate(merged_items, merged_renderers, &prefix, word_start);
                    completion_state.set(new_state);
                });
            }

            trigger_signal.set(None);
        };

        if needs_debounce {
            let cb = Closure::once(fire);
            let handle = web_sys::window()
                .and_then(|w| {
                    w.set_timeout_with_callback_and_timeout_and_arguments_0(
                        cb.as_ref().unchecked_ref(),
                        COMPLETION_DEBOUNCE_MS,
                    )
                    .ok()
                })
                .unwrap_or(0);
            timer_handle.set_value(Some(handle));
            cb.forget();
        } else {
            fire();
        }
    });
}

// ── Keyboard intercept ──────────────────────────────────────────────────

/// Result of a completion key handler.
pub(crate) enum CompletionKeyResult {
    /// Completion handled the key — don't pass to editor.
    Consumed,
    /// Completion didn't handle it — pass through normally.
    Ignored,
}

/// Apply a completion acceptance: replace the prefix with the completion text.
///
/// Sets a selection from `word_start` to the current cursor, then inserts
/// `insert_text`, which replaces the selection.
pub(crate) fn accept_completion(
    editor: &mut Editor,
    word_start: Position,
    insert_text: &str,
) {
    let cursor = editor.cursor();
    editor.set_selection(word_start, cursor);
    editor.insert(insert_text);
}

/// Handle a keydown event while the completion system is present.
///
/// Takes extracted key info (`key`, `ctrl`, `shift`) rather than a `KeyboardEvent`
/// directly so it can be unit-tested without WASM.
pub(crate) fn handle_completion_keydown(
    state: &mut CompletionState,
    editor: &mut Editor,
    key: &str,
    ctrl: bool,
    _shift: bool,
    trigger_signal: &RwSignal<Option<CompletionTrigger>>,
    providers: &[CompletionProviderConfig],
) -> CompletionKeyResult {
    match state {
        CompletionState::Idle => {
            // Ctrl+Space → invoke completions
            if ctrl && key == " " {
                trigger_signal.set(Some(CompletionTrigger::Invoke));
                return CompletionKeyResult::Consumed;
            }

            // Single printable character (not ctrl): check for trigger chars
            if key.len() == 1 && !ctrl {
                let ch = key.chars().next().unwrap();
                let is_trigger = providers
                    .iter()
                    .any(|cfg| cfg.trigger_characters.contains(&ch));
                if is_trigger {
                    // Let the character be inserted by normal handling;
                    // the input handler will fire the trigger.
                    return CompletionKeyResult::Ignored;
                }
            }

            CompletionKeyResult::Ignored
        }
        CompletionState::Active { .. } => {
            match key {
                "ArrowUp" => {
                    state.move_selection(-1);
                    CompletionKeyResult::Consumed
                }
                "ArrowDown" => {
                    state.move_selection(1);
                    CompletionKeyResult::Consumed
                }
                "Enter" | "Tab" => {
                    // Accept the selected item
                    let insert_text = state.selected_item().map(|item| {
                        item.insert_text
                            .as_deref()
                            .unwrap_or(&item.label)
                            .to_string()
                    });
                    let ws = state.word_start();
                    if let (Some(text), Some(ws)) = (insert_text, ws) {
                        accept_completion(editor, ws, &text);
                    }
                    state.dismiss();
                    CompletionKeyResult::Consumed
                }
                "Escape" => {
                    state.dismiss();
                    CompletionKeyResult::Consumed
                }
                "Backspace" => {
                    // Let backspace pass through — the input handler will
                    // re-check the prefix and dismiss if cursor goes before word_start.
                    CompletionKeyResult::Ignored
                }
                _ if key.len() == 1 && !ctrl && key.chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_') => {
                    // Printable alphanumeric: let through without dismissing.
                    // The on_input handler will update the filter.
                    CompletionKeyResult::Ignored
                }
                _ => {
                    // Non-printable / structural key: dismiss and pass through
                    state.dismiss();
                    CompletionKeyResult::Ignored
                }
            }
        }
    }
}

// ── Popup component ─────────────────────────────────────────────────

fn kind_label(kind: &CompletionKind) -> &'static str {
    match kind {
        CompletionKind::Text => "T",
        CompletionKind::Keyword => "K",
        CompletionKind::Variable => "V",
        CompletionKind::Function => "F",
        CompletionKind::Field => "f",
        CompletionKind::Property => "P",
        CompletionKind::Method => "M",
        CompletionKind::Module => "m",
        CompletionKind::Snippet => "S",
        CompletionKind::Other => "?",
    }
}

#[component]
pub(crate) fn CompletionPopup(
    completion_state: RwSignal<CompletionState>,
    /// ID of the editor root element — needed for measure_cursor_x
    editor_root_id: String,
    scroll_top: RwSignal<f64>,
    /// Editor mutex — needed for click-to-accept
    editor: Arc<std::sync::Mutex<Editor>>,
    /// Called after text changes from accepting a completion
    on_text_change: Arc<dyn Fn() + Send + Sync>,
) -> impl IntoView {
    move || {
        let state = completion_state.get();
        match state {
            CompletionState::Idle => {
                view! { <div style="display:none;"></div> }.into_any()
            }
            CompletionState::Active {
                ref items,
                ref renderers,
                ref filtered,
                selected_index,
                word_start,
                ..
            } => {
                let document = web_sys::window().and_then(|w| w.document());
                let editor_el = document.as_ref().and_then(|d| d.get_element_by_id(&editor_root_id));

                let x = editor_el
                    .as_ref()
                    .and_then(|el| {
                        let el: &HtmlElement = el.unchecked_ref();
                        crate::editor::measure_cursor_x(el, word_start.line, word_start.col)
                    })
                    .unwrap_or(0.0);

                let line_height = crate::editor::LINE_HEIGHT;
                let st = scroll_top.get_untracked();
                let mut y = (word_start.line + 1) as f64 * line_height - st;

                // Flip above if popup would go below viewport
                let viewport_h = editor_el
                    .as_ref()
                    .map(|el| {
                        let el: &HtmlElement = el.unchecked_ref();
                        el.client_height() as f64
                    })
                    .unwrap_or(500.0);

                let popup_max_height = 200.0;
                if y + popup_max_height > viewport_h {
                    y = word_start.line as f64 * line_height - st - popup_max_height;
                }

                let item_views: Vec<_> = filtered
                    .iter()
                    .enumerate()
                    .map(|(fi, &item_idx)| {
                        let item = &items[item_idx];
                        let renderer = renderers.get(item_idx).and_then(|r| r.as_ref());
                        let is_selected = fi == selected_index;
                        let class_str = if is_selected {
                            "kode-completion-item kode-completion-item--selected"
                        } else {
                            "kode-completion-item"
                        };
                        let insert_text = item
                            .insert_text
                            .clone()
                            .unwrap_or_else(|| item.label.clone());

                        // Click-to-accept handler
                        let editor_click = editor.clone();
                        let on_text_change_click = on_text_change.clone();
                        let on_mousedown = move |ev: web_sys::MouseEvent| {
                            ev.prevent_default();
                            ev.stop_propagation();
                            let mut ed = editor_click.lock().unwrap();
                            accept_completion(&mut ed, word_start, &insert_text);
                            drop(ed);
                            completion_state.set(CompletionState::Idle);
                            on_text_change_click();
                        };

                        // Use custom renderer if provided, otherwise default layout
                        if let Some(render_fn) = renderer {
                            let custom_view = render_fn(item, is_selected);
                            view! {
                                <div class=class_str on:mousedown=on_mousedown>
                                    {custom_view}
                                </div>
                            }.into_any()
                        } else {
                            let kind = kind_label(&item.kind).to_string();
                            let label = item.label.clone();
                            let detail = item.detail.clone();
                            view! {
                                <div class=class_str on:mousedown=on_mousedown>
                                    <span class="kode-completion-kind">{kind}</span>
                                    <span class="kode-completion-label">{label}</span>
                                    {detail.map(|d| view! {
                                        <span class="kode-completion-detail">{d}</span>
                                    })}
                                </div>
                            }.into_any()
                        }
                    })
                    .collect();

                let style = format!(
                    "left:{x}px;top:{y}px;"
                );

                view! {
                    <div class="kode-completion-popup" style=style>
                        {item_views}
                    </div>
                }
                .into_any()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kode_core::CompletionKind;

    fn make_item(label: &str, sort_order: i32, kind: CompletionKind) -> CompletionItem {
        CompletionItem {
            label: label.to_string(),
            insert_text: None,
            detail: None,
            sort_order,
            kind,
        }
    }

    /// Test helper: activate with no custom renderers.
    fn activate(items: Vec<CompletionItem>, prefix: &str, word_start: Position) -> CompletionState {
        let n = items.len();
        CompletionState::activate(items, vec![None; n], prefix, word_start)
    }

    fn sample_items() -> Vec<CompletionItem> {
        vec![
            make_item("zebra", 0, CompletionKind::Variable),
            make_item("apple", 1, CompletionKind::Function),
            make_item("banana", 0, CompletionKind::Keyword),
            make_item("avocado", 0, CompletionKind::Field),
            make_item("fig", 1, CompletionKind::Method),
        ]
    }

    #[test]
    fn activate_sorts_by_sort_order_then_alphabetically() {
        let state = activate(sample_items(), "", Position::new(0, 0));
        if let CompletionState::Active { items, filtered, .. } = &state {
            let labels: Vec<&str> = filtered.iter().map(|&i| items[i].label.as_str()).collect();
            // sort_order 0: avocado, banana, zebra; sort_order 1: apple, fig
            assert_eq!(labels, vec!["avocado", "banana", "zebra", "apple", "fig"]);
        } else {
            panic!("Expected Active state");
        }
    }

    #[test]
    fn activate_with_prefix_filters_case_insensitive() {
        let items = vec![
            make_item("Foo", 0, CompletionKind::Text),
            make_item("fooBar", 0, CompletionKind::Text),
            make_item("bar", 0, CompletionKind::Text),
            make_item("FOOD", 0, CompletionKind::Text),
            make_item("baz", 0, CompletionKind::Text),
        ];
        let state = activate(items, "fo", Position::new(0, 0));
        if let CompletionState::Active { items, filtered, .. } = &state {
            let labels: Vec<&str> = filtered.iter().map(|&i| items[i].label.as_str()).collect();
            // Sorted case-insensitively: "foo" < "foobar" < "food" (lexicographic: 'b' < 'd')
            assert_eq!(labels, vec!["Foo", "fooBar", "FOOD"]);
        } else {
            panic!("Expected Active state");
        }
    }

    #[test]
    fn activate_with_no_matches_returns_idle() {
        let items = vec![
            make_item("apple", 0, CompletionKind::Text),
            make_item("banana", 0, CompletionKind::Text),
        ];
        let state = activate(items, "zzz", Position::new(0, 0));
        assert!(!state.is_active());
    }

    #[test]
    fn update_filter_narrows_results() {
        let mut state = activate(
            vec![
                make_item("format", 0, CompletionKind::Function),
                make_item("foreach", 0, CompletionKind::Function),
                make_item("find", 0, CompletionKind::Function),
                make_item("force", 0, CompletionKind::Function),
            ],
            "fo",
            Position::new(0, 0),
        );

        // Initial: format, force, foreach (all start with "fo")
        if let CompletionState::Active { filtered, .. } = &state {
            assert_eq!(filtered.len(), 3);
        }

        state.update_filter("for");
        if let CompletionState::Active { items, filtered, .. } = &state {
            let labels: Vec<&str> = filtered.iter().map(|&i| items[i].label.as_str()).collect();
            assert_eq!(labels, vec!["force", "foreach", "format"]);
        } else {
            panic!("Expected Active state after 'for' filter");
        }

        state.update_filter("form");
        if let CompletionState::Active { items, filtered, .. } = &state {
            let labels: Vec<&str> = filtered.iter().map(|&i| items[i].label.as_str()).collect();
            assert_eq!(labels, vec!["format"]);
        } else {
            panic!("Expected Active state after 'form' filter");
        }
    }

    #[test]
    fn update_filter_to_empty_transitions_to_idle() {
        let mut state = activate(
            vec![make_item("apple", 0, CompletionKind::Text)],
            "",
            Position::new(0, 0),
        );
        assert!(state.is_active());

        state.update_filter("zzz");
        assert!(!state.is_active());
    }

    #[test]
    fn move_selection_increments_and_wraps_at_end() {
        let mut state = activate(
            vec![
                make_item("a", 0, CompletionKind::Text),
                make_item("b", 0, CompletionKind::Text),
                make_item("c", 0, CompletionKind::Text),
            ],
            "",
            Position::new(0, 0),
        );

        if let CompletionState::Active { selected_index, .. } = &state {
            assert_eq!(*selected_index, 0);
        }

        state.move_selection(1);
        if let CompletionState::Active { selected_index, .. } = &state {
            assert_eq!(*selected_index, 1);
        }

        state.move_selection(1);
        if let CompletionState::Active { selected_index, .. } = &state {
            assert_eq!(*selected_index, 2);
        }

        // Wrap around to 0
        state.move_selection(1);
        if let CompletionState::Active { selected_index, .. } = &state {
            assert_eq!(*selected_index, 0);
        }
    }

    #[test]
    fn move_selection_decrements_and_wraps_at_start() {
        let mut state = activate(
            vec![
                make_item("a", 0, CompletionKind::Text),
                make_item("b", 0, CompletionKind::Text),
                make_item("c", 0, CompletionKind::Text),
            ],
            "",
            Position::new(0, 0),
        );

        if let CompletionState::Active { selected_index, .. } = &state {
            assert_eq!(*selected_index, 0);
        }

        // Wrap from 0 to last
        state.move_selection(-1);
        if let CompletionState::Active { selected_index, .. } = &state {
            assert_eq!(*selected_index, 2);
        }

        state.move_selection(-1);
        if let CompletionState::Active { selected_index, .. } = &state {
            assert_eq!(*selected_index, 1);
        }
    }

    #[test]
    fn selected_item_returns_correct_item() {
        let state = activate(
            vec![
                make_item("alpha", 0, CompletionKind::Variable),
                make_item("beta", 0, CompletionKind::Function),
            ],
            "",
            Position::new(0, 0),
        );

        let item = state.selected_item().expect("should have selected item");
        assert_eq!(item.label, "alpha");
    }

    #[test]
    fn dismiss_transitions_to_idle() {
        let mut state = activate(
            vec![make_item("test", 0, CompletionKind::Text)],
            "",
            Position::new(0, 0),
        );
        assert!(state.is_active());

        state.dismiss();
        assert!(!state.is_active());
        assert!(state.selected_item().is_none());
    }

    #[test]
    fn is_active_returns_correct_values() {
        let idle = CompletionState::Idle;
        assert!(!idle.is_active());

        let active = activate(
            vec![make_item("x", 0, CompletionKind::Text)],
            "",
            Position::new(0, 0),
        );
        assert!(active.is_active());
    }

    // ── accept_completion tests ─────────────────────────────────────────

    #[test]
    fn accept_replaces_partial_word_at_start_of_line() {
        let mut editor = Editor::empty();
        editor.insert("fo");
        // Cursor is at (0, 2), word_start is (0, 0)
        assert_eq!(editor.cursor(), Position::new(0, 2));

        accept_completion(&mut editor, Position::new(0, 0), "format");
        assert_eq!(editor.text(), "format");
        assert_eq!(editor.cursor(), Position::new(0, 6));
    }

    #[test]
    fn accept_replaces_partial_word_mid_line() {
        let mut editor = Editor::empty();
        editor.insert("hello fo");
        // Cursor is at (0, 8), word_start for "fo" is at (0, 6)
        assert_eq!(editor.cursor(), Position::new(0, 8));

        accept_completion(&mut editor, Position::new(0, 6), "format");
        assert_eq!(editor.text(), "hello format");
        assert_eq!(editor.cursor(), Position::new(0, 12));
    }

    #[test]
    fn accept_uses_insert_text_over_label() {
        let mut editor = Editor::empty();
        editor.insert("pr");
        // insert_text "println!()" differs from label "println"
        accept_completion(&mut editor, Position::new(0, 0), "println!()");
        assert_eq!(editor.text(), "println!()");
        assert_eq!(editor.cursor(), Position::new(0, 10));
    }
}
