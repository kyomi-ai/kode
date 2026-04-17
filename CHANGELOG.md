# Changelog

All notable changes to this project are documented here. Format based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); this project follows
[SemVer](https://semver.org/).

## [Unreleased]

## [0.2.0] — 2026-04-17

### Breaking changes

#### `kode-leptos`

- The `Language` enum is replaced with a newtype wrapping `Cow<'static, str>`.
  Consumers construct languages by string tag — the same tag `arborium`
  registers the grammar under.
  - `Language::Sql` → `Language::new_static("sql")`
  - `Language::Markdown` → `Language::new_static("markdown")`
  - `Language::Plain` → `Language::PLAIN`
  - …and similarly for the other 12 former variants.
- `highlight_line`, `highlight_block`, and `line_languages` now take
  `&Language` instead of `Language` by value (the newtype is not `Copy`).
- `kode-leptos` no longer enables *any* language features on its `arborium`
  dependency. Consumers must now opt in to the grammars they need via their
  own `arborium` dep:

  ```toml
  arborium = { version = "2.16", default-features = false, features = ["lang-sql", "lang-markdown"] }
  ```

  Cargo feature unification means `arborium::Highlighter` inside `kode-leptos`
  sees exactly those grammars. A downstream WASM consumer that needs only
  SQL + markdown saves ~14 MB of tree-sitter parser tables (SQL alone is
  ~11 MB compiled).

### Why

Previously every downstream consumer shipped all 15 tree-sitter grammars
whether they used them or not, bloating WASM bundles by ~14 MB. Making
grammar selection a consumer concern lets each app pay only for what it
actually highlights.

## [0.1.0] — 2026-04-17

Initial release.

### `kode-core`
- Text buffer on `ropey` with position/selection primitives, undo/redo,
  transactional edits, and completion-trigger types.

### `kode-doc`
- Tree-based structured document model: `Node`, `Fragment`, `Mark`, `Slice`,
  `Step`, `Transform`, token-based `ResolvedPos`.
- Markdown parse ↔ serialize round-tripping.
- `DocState` wiring editing, formatting, selection, undo, and clipboard.

### `kode-markdown`
- Commands for common markdown formatting actions.
- Input rules for live markdown shortcuts (`#` → heading, `-` → bullet, etc.).

### `kode-leptos`
- `CodeEditor` component with syntax highlighting for SQL, YAML, Markdown,
  Rust, Python, JS/TS, HTML, CSS, JSON, and Bash.
- `MarkdownEditorComponent` with toggleable WYSIWYG / source mode.
- `TreeWysiwygEditor` — WYSIWYG markdown editor built on `kode-doc`.
- Diagnostic provider API (`DiagnosticProvider`, `tree_sitter_provider`, opt-in
  `json_schema_provider` behind `schema` feature).
- Completion provider API with keyword triggers, typing triggers, and custom
  renderers.
- `placeholder` prop — ghost text for empty buffers.
- `EditorHandle` imperative API for insert/selection/markers.
- Theming via CSS variables; built-in `tokyo_night`, `one_dark`, `github_light`
  themes.
- Toolbar with default + custom buttons and command injection.

[Unreleased]: https://github.com/kyomi-ai/kode/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/kyomi-ai/kode/releases/tag/v0.2.0
[0.1.0]: https://github.com/kyomi-ai/kode/releases/tag/v0.1.0
