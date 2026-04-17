use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use arborium_tree_sitter as ts;
use kode_core::{Diagnostic, DiagnosticSeverity, Marker, Position};
use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

/// Default debounce delay in milliseconds.
const DIAGNOSTIC_DEBOUNCE_MS: i32 = 300;

/// An async function that validates editor text and returns diagnostics.
///
/// Parameters:
/// - `String` — current editor text
/// - `u64` — monotonic text version (use to detect/discard stale results)
///
/// Returns `Vec<Diagnostic>` — the findings for this provider.
///
/// # Example
///
/// ```ignore
/// use kode_leptos::DiagnosticProvider;
/// use kode_core::{Diagnostic, DiagnosticSeverity, Position};
/// use std::sync::Arc;
///
/// let provider: DiagnosticProvider = Arc::new(|text, _version| {
///     Box::pin(async move {
///         // Validate and return diagnostics
///         vec![]
///     })
/// });
/// ```
pub type DiagnosticProvider = Arc<
    dyn Fn(String, u64) -> Pin<Box<dyn Future<Output = Vec<Diagnostic>>>> + Send + Sync,
>;

/// Spawn the reactive diagnostic pipeline.
///
/// Watches `text_version`, debounces, calls all providers in parallel,
/// merges results (per-provider slots) into the `markers` signal.
pub fn spawn_diagnostic_pipeline(
    providers: Signal<Vec<DiagnosticProvider>>,
    editor: Arc<std::sync::Mutex<kode_core::Editor>>,
    text_version: RwSignal<u64>,
    markers: RwSignal<Vec<Marker>>,
    marker_version: RwSignal<u64>,
    debounce_ms: Option<i32>,
) {
    let debounce = debounce_ms.unwrap_or(DIAGNOSTIC_DEBOUNCE_MS);
    let timer_handle: StoredValue<Option<i32>> = StoredValue::new(None);
    // Per-provider result slots, indexed by position in the providers vec.
    // When any provider responds, we merge all slots into the markers signal.
    let provider_results: StoredValue<Vec<Vec<Marker>>> = StoredValue::new(vec![]);

    Effect::new(move |_| {
        let version = text_version.get();
        let providers_list = providers.get();

        // No providers → clear any provider-sourced markers and bail
        if providers_list.is_empty() {
            provider_results.set_value(vec![]);
            return;
        }

        // Reset slots to match current provider count
        provider_results.set_value(vec![vec![]; providers_list.len()]);

        // Cancel any pending debounce timer
        if let Some(handle) = timer_handle.get_value() {
            if let Some(window) = web_sys::window() {
                window.clear_timeout_with_handle(handle);
            }
        }

        let editor = editor.clone();
        let cb = Closure::once(move || {
            let text = editor.lock().unwrap().text();

            for (idx, provider) in providers_list.into_iter().enumerate() {
                let text = text.clone();
                let fut = provider(text, version);
                leptos::task::spawn_local(async move {
                    let diagnostics = fut.await;

                    // Discard stale results — text has changed since we started
                    if text_version.get_untracked() != version {
                        return;
                    }

                    let new_markers: Vec<Marker> =
                        diagnostics.into_iter().map(Into::into).collect();

                    // Update this provider's slot
                    provider_results.update_value(|results| {
                        if idx < results.len() {
                            results[idx] = new_markers;
                        }
                    });

                    // Flatten all provider results into the markers signal
                    let merged: Vec<Marker> = provider_results
                        .with_value(|results| results.iter().flatten().cloned().collect());
                    markers.set(merged);
                    marker_version.update(|v| *v += 1);
                });
            }
        });

        let handle = web_sys::window()
            .and_then(|w| {
                w.set_timeout_with_callback_and_timeout_and_arguments_0(
                    cb.as_ref().unchecked_ref(),
                    debounce,
                )
                .ok()
            })
            .unwrap_or(0);
        timer_handle.set_value(Some(handle));
        cb.forget();
    });
}

// ── Built-in tree-sitter diagnostic provider ────────────────────────────

thread_local! {
    static TS_PARSER: RefCell<ts::Parser> = RefCell::new(ts::Parser::new());
}

/// Create a [`DiagnosticProvider`] that uses a tree-sitter grammar to detect
/// syntax errors. Works with **any** tree-sitter language — SQL, Python, JSON,
/// BigQuery SQL, etc.
///
/// The provider parses the full text on each (debounced) change and walks the
/// syntax tree for `ERROR` and `MISSING` nodes, converting each to a
/// [`Diagnostic`].
///
/// # Example
///
/// ```ignore
/// use kode_leptos::{CodeEditor, tree_sitter_provider};
///
/// // Use arborium's bundled SQL grammar
/// let sql_provider = tree_sitter_provider(arborium::lang_sql::language().into());
///
/// // Or any external tree-sitter grammar crate
/// // let bq_provider = tree_sitter_provider(tree_sitter_bigquery::language().into());
///
/// view! {
///     <CodeEditor
///         diagnostic_providers=Signal::stored(vec![sql_provider])
///     />
/// }
/// ```
pub fn tree_sitter_provider(language: ts::Language) -> DiagnosticProvider {
    Arc::new(move |text: String, _version: u64| {
        let language = language.clone();
        Box::pin(async move {
            tree_sitter_diagnose(&text, &language)
        })
    })
}

/// Parse `text` with the given tree-sitter language and collect all
/// `ERROR`/`MISSING` nodes as diagnostics.
fn tree_sitter_diagnose(text: &str, language: &ts::Language) -> Vec<Diagnostic> {
    TS_PARSER.with(|parser| {
        let mut parser = parser.borrow_mut();
        // set_language is cheap if unchanged (pointer comparison internally)
        if parser.set_language(language).is_err() {
            return vec![];
        }
        let tree = match parser.parse(text.as_bytes(), None) {
            Some(t) => t,
            None => return vec![],
        };
        let mut diagnostics = Vec::new();
        collect_errors(tree.root_node(), &mut diagnostics);
        diagnostics
    })
}

/// Recursively walk the tree, collecting ERROR and MISSING nodes.
/// Skips children of ERROR nodes to avoid duplicate/noisy diagnostics
/// for a single parse failure.
fn collect_errors(node: ts::Node<'_>, out: &mut Vec<Diagnostic>) {
    if node.is_error() {
        let start = node.start_position();
        let end = node.end_position();
        // For zero-width errors, extend to at least 1 char so the underline is visible
        let end_col = if start.row == end.row && start.column == end.column {
            end.column + 1
        } else {
            end.column
        };
        out.push(Diagnostic {
            start: Position::new(start.row, start.column),
            end: Position::new(end.row, end_col),
            severity: DiagnosticSeverity::Error,
            message: "Syntax error".into(),
            source: Some("tree-sitter".into()),
        });
        // Don't recurse into ERROR children — they produce noisy sub-errors
        return;
    }
    if node.is_missing() {
        let start = node.start_position();
        let kind = node.kind();
        out.push(Diagnostic {
            start: Position::new(start.row, start.column),
            end: Position::new(start.row, start.column + 1),
            severity: DiagnosticSeverity::Error,
            message: format!("Missing {kind}"),
            source: Some("tree-sitter".into()),
        });
        return;
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            collect_errors(child, out);
        }
    }
}
