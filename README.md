# Kode

A fast, embeddable code editor for [Leptos](https://leptos.dev/) applications. Kode
provides a syntax-highlighted code editor component, a WYSIWYG Markdown editor, and
a toolkit of building blocks for editor experiences that feel native to the web.

> **Status:** `0.2.0-alpha.1`. APIs may change before 1.0.

## Features

- **Code editor** with syntax highlighting for SQL, YAML, Markdown, Rust, Python,
  JavaScript/TypeScript, HTML, CSS, JSON, Bash, and plain text (powered by
  tree-sitter via [`arborium`](https://crates.io/crates/arborium)).
- **WYSIWYG Markdown editor** that round-trips to source.
- **Diagnostics API** — plug in linters/validators that render squiggly underlines.
- **Completions API** — autocomplete with keyword triggers, typing triggers, and
  custom item renderers.
- **Theming** via CSS variables; three built-in themes (`tokyo_night`, `one_dark`,
  `github_light`) and full custom theme support.
- **Placeholder text**, IME composition, multi-cursor selection, undo/redo,
  find-and-replace primitives, virtualized rendering for large files.
- **Imperative handle** for programmatic control (insert text, read selection, set
  markers).

## Workspace layout

| Crate | Purpose |
|---|---|
| `kode-core` | Text buffer, selection, editing primitives (no UI, no wasm deps). |
| `kode-leptos` | Leptos components: `CodeEditor`, `MarkdownEditorComponent`, `TreeWysiwygEditor`, toolbar, completion popup. |
| `kode-markdown` | Markdown parser and formatting utilities. |
| `kode-doc` | Structured document model (used by the WYSIWYG tree editor). |
| `kode-demo` | Trunk-built SPA showing the full API in action. |

## Installation

```toml
# In your Cargo.toml
[dependencies]
kode-leptos = "0.1"
leptos = { version = "0.8", features = ["csr"] }
```

For active local development, use a path dependency:

```toml
kode-leptos = { path = "../kode/kode-leptos" }
```

## Quick start

```rust
use leptos::prelude::*;
use kode_leptos::{CodeEditor, Language, Theme};
use std::sync::Arc;

#[component]
fn App() -> impl IntoView {
    let content = RwSignal::new(String::from("SELECT * FROM users;"));

    view! {
        <div style="height:400px;">
            <CodeEditor
                language=Signal::stored(Language::Sql)
                content=content.read_only()
                theme=Signal::stored(Theme::tokyo_night())
                placeholder="-- Start typing, or paste a query"
                on_change=Arc::new(move |text: String| {
                    content.set(text);
                })
            />
        </div>
    }
}
```

## `CodeEditor` props

| Prop | Type | Default | Description |
|---|---|---|---|
| `language` | `Signal<Language>` | `Language::Plain` | Syntax highlighting language. Reactive — changes re-highlight immediately. |
| `content` | `Signal<String>` | `""` | Editor text. When the signal changes, the editor replaces its buffer. |
| `theme` | `Signal<Theme>` | `Theme::default()` | Color theme. Set via CSS variables on the editor root. |
| `on_change` | `Option<Arc<dyn Fn(String) + Send + Sync>>` | `None` | Called after every text edit with the full new text. |
| `on_ready` | `Option<Arc<dyn Fn(EditorHandle) + Send + Sync>>` | `None` | Called once with an imperative handle (see below). |
| `diagnostic_providers` | `Signal<Vec<DiagnosticProvider>>` | `vec![]` | Debounced providers that return markers. See [Diagnostics](#diagnostics). |
| `diagnostic_debounce_ms` | `Option<i32>` | `300` | Debounce delay for diagnostic providers. |
| `completion_providers` | `Signal<Vec<CompletionProviderConfig>>` | `vec![]` | Autocomplete providers. See [Completions](#completions). |
| `placeholder` | `Signal<String>` | `""` | Ghost text shown at line 1 col 1 when the buffer is empty. Hidden on first keystroke. |

### Supported languages

```rust
Language::Plain       // no highlighting
Language::Sql
Language::Yaml
Language::Markdown    // recognizes fenced code blocks
Language::Python
Language::Rust
Language::Html
Language::Css
Language::Json
Language::Bash
```

## Imperative handle

Receive an `EditorHandle` via `on_ready` to interact with the editor outside the
props flow:

```rust
use kode_leptos::{CodeEditor, EditorHandle, Marker, MarkerSeverity, Position};

let (handle, set_handle) = signal::<Option<EditorHandle>>(None);

view! {
    <CodeEditor
        content=content.read_only()
        on_ready=Arc::new(move |h: EditorHandle| set_handle.set(Some(h)))
    />
    <button on:click=move |_| {
        if let Some(h) = handle.get() {
            h.insert_at_cursor("-- injected comment\n");
        }
    }>"Insert"</button>
}
```

Available methods:

```rust
handle.insert_at_cursor("text");      // replace selection / insert at cursor
handle.selected_text();               // Option<String>
handle.cursor();                      // Position { line, col }
handle.selection();                   // Selection { anchor, head }
handle.set_markers(vec![
    Marker {
        start: Position::new(0, 0),
        end: Position::new(0, 5),
        severity: MarkerSeverity::Error,
        message: "syntax error".into(),
    },
]);
handle.clear_markers();
```

## Theming

Pick a built-in theme:

```rust
Theme::tokyo_night()
Theme::one_dark()
Theme::github_light()
```

Or customize by starting from a builtin and overriding fields. The theme drives a
set of `--kode-*` CSS custom properties applied to the editor root. You can
override any of them in your own CSS:

```css
.my-editor {
    --kode-bg: #1a1a1a;
    --kode-fg: #e0e0e0;
    --kode-cursor: #ff8800;
    --kode-selection: rgba(255, 136, 0, 0.25);
    --kode-gutter-fg: #555;
    --kode-fg-dim: #888;  /* used for placeholder ghost text */
}
```

Key variables: `--kode-bg`, `--kode-fg`, `--kode-fg-dim`, `--kode-cursor`,
`--kode-selection`, `--kode-current-line`, `--kode-gutter-fg`,
`--kode-gutter-border`, `--kode-accent`, `--kode-marker-error`,
`--kode-marker-warning`, `--kode-marker-info`.

## Diagnostics

Register one or more providers. Each provider receives the current buffer and
returns markers; results are merged and rendered as wavy underlines.

```rust
use kode_leptos::{DiagnosticProvider, tree_sitter_provider, Language};

let providers = Signal::stored(vec![
    tree_sitter_provider(Language::Sql),
]);

view! {
    <CodeEditor
        language=Signal::stored(Language::Sql)
        content=content.read_only()
        diagnostic_providers=providers
        diagnostic_debounce_ms=Some(500)
    />
}
```

Write a custom provider by constructing a `DiagnosticProvider` from a closure that
takes the text and returns `Vec<Marker>`.

With the optional `schema` feature:

```toml
kode-leptos = { git = "...", features = ["schema"] }
```

```rust
use kode_leptos::json_schema_provider;

let schema = serde_json::json!({ "type": "object", "required": ["name"] });
let providers = Signal::stored(vec![json_schema_provider(schema)]);
```

## Completions

```rust
use kode_leptos::{CompletionProviderConfig, CompletionItem, CompletionKind, CompletionContext};

let sql_keywords = CompletionProviderConfig {
    activate_on_typing: true,
    trigger_characters: vec![],
    provider: Arc::new(|ctx: CompletionContext| {
        vec![
            CompletionItem {
                label: "SELECT".into(),
                kind: CompletionKind::Keyword,
                detail: None,
                insert_text: "SELECT ".into(),
            },
            CompletionItem {
                label: "FROM".into(),
                kind: CompletionKind::Keyword,
                detail: None,
                insert_text: "FROM ".into(),
            },
        ]
    }),
    render: None,
};

let providers = Signal::stored(vec![sql_keywords]);

view! {
    <CodeEditor
        language=Signal::stored(Language::Sql)
        content=content.read_only()
        completion_providers=providers
    />
}
```

- `activate_on_typing: true` — popup opens as the user types identifiers.
- `trigger_characters: vec!['.', ':']` — popup opens when any listed character is
  typed (good for member access).
- `render: Some(renderer)` — customize popup item rendering.

Keyboard: ↑/↓ navigate, Enter/Tab accept, Esc dismiss.

## WYSIWYG Markdown editor

```rust
use kode_leptos::{MarkdownEditorComponent, EditorMode};

let content = RwSignal::new(String::from("# Hello\n\nSome **bold** text."));
let mode = RwSignal::new(EditorMode::Wysiwyg);

view! {
    <MarkdownEditorComponent
        content=content.read_only()
        mode=mode.read_only()
        on_change=Arc::new(move |text| content.set(text))
    />
}
```

Toggle `mode` between `EditorMode::Wysiwyg` and `EditorMode::Source` to switch
between the rendered and markdown-source views.

## Running the demo

The `demo/` crate is a Trunk-built SPA that exercises every feature.

```bash
# Install Trunk if you don't have it
cargo install trunk

# Build and serve (listens on 0.0.0.0:8090)
cd demo
trunk serve --address 0.0.0.0 --port 8090
```

Open http://localhost:8090/ and try the language selector, theme switcher,
completions, and imperative-handle controls.

## Development

```bash
# Native check (all crates)
cargo check --workspace

# WASM check (what the editor actually targets)
cargo check -p kode-leptos --target wasm32-unknown-unknown

# Lints
cargo clippy --workspace -- -D warnings

# Unit tests
cargo test -p kode-leptos --lib
cargo test -p kode-core --lib
```

Prerequisites:

- Rust `stable` (see `rust-toolchain.toml`).
- `wasm32-unknown-unknown` target: `rustup target add wasm32-unknown-unknown`.
- [Trunk](https://trunkrs.dev/) for the demo: `cargo install trunk`.

## Project layout for contributors

```
kode/
├── kode-core/           # editing primitives (no-UI)
├── kode-leptos/         # Leptos components + CSS + highlighting
│   ├── src/editor.rs    # CodeEditor component
│   ├── src/wysiwyg/     # WYSIWYG markdown tree editor
│   └── src/completion.rs
├── kode-markdown/       # markdown parser
├── kode-doc/            # document tree model
└── demo/                # Trunk SPA demo
```

## License

MIT — see [LICENSE](./LICENSE).
