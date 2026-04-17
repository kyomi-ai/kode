use std::sync::Arc;

use leptos::prelude::*;

use crate::editor::CodeEditor;
use crate::extension::Extension;
use crate::highlight::Language;
use crate::theme::Theme;
use crate::wysiwyg::tree_editor::TreeWysiwygEditor;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EditorMode {
    Source,
    Wysiwyg,
}

#[component]
pub fn MarkdownEditorComponent(
    #[prop(into, default = Signal::stored(String::new()))]
    content: Signal<String>,
    #[prop(optional)]
    on_change: Option<Arc<dyn Fn(String) + Send + Sync>>,
    #[prop(default = EditorMode::Wysiwyg)]
    initial_mode: EditorMode,
    #[prop(into, default = Signal::stored(Theme::default()))]
    theme: Signal<Theme>,
    /// Editor extensions for custom code block rendering, toolbar buttons,
    /// and keyboard shortcuts. Passed through to the WYSIWYG editor.
    #[prop(default = vec![])]
    extensions: Vec<Arc<dyn Extension>>,
) -> impl IntoView {
    let mode = RwSignal::new(initial_mode);

    // Provide a no-op default so we always have a callback to pass down
    let on_change: Arc<dyn Fn(String) + Send + Sync> = on_change
        .unwrap_or_else(|| Arc::new(|_: String| {}));

    let on_change_source = on_change.clone();
    let on_change_wysiwyg = on_change;
    let extensions = StoredValue::new(extensions);

    view! {
        <div style=move || format!("display:flex;flex-direction:column;height:100%;{}", theme.get().to_css_vars())>
            // ── Mode toggle bar ──────────────────────────────────────────
            <div class="kode-mode-toggle">
                <button
                    class=move || if mode.get() == EditorMode::Source {
                        "kode-mode-toggle-button active"
                    } else {
                        "kode-mode-toggle-button"
                    }
                    on:click=move |_| mode.set(EditorMode::Source)
                >
                    "Source"
                </button>
                <button
                    class=move || if mode.get() == EditorMode::Wysiwyg {
                        "kode-mode-toggle-button active"
                    } else {
                        "kode-mode-toggle-button"
                    }
                    on:click=move |_| mode.set(EditorMode::Wysiwyg)
                >
                    "WYSIWYG"
                </button>
            </div>

            // ── Editor area ──────────────────────────────────────────────
            <div style="flex:1;overflow:hidden;">
                {move || {
                    match mode.get() {
                        EditorMode::Source => {
                            let cb = on_change_source.clone();
                            view! {
                                <CodeEditor
                                    language=Signal::stored(Language::new_static("markdown"))
                                    content=content
                                    on_change=cb
                                    theme=theme
                                />
                            }.into_any()
                        }
                        EditorMode::Wysiwyg => {
                            let cb = on_change_wysiwyg.clone();
                            let exts = extensions.get_value();
                            view! {
                                <TreeWysiwygEditor
                                    content=content
                                    on_change=cb
                                    show_toolbar=true
                                    theme=theme
                                    extensions=exts
                                />
                            }.into_any()
                        }
                    }
                }}
            </div>
        </div>
    }
}
