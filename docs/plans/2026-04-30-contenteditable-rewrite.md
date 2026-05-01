# WYSIWYG Contenteditable Rewrite

Replace the hidden-textarea + manual-cursor rendering in the kode WYSIWYG editor with a `contenteditable`-based approach, matching how TipTap/ProseMirror works. The document model (kode-doc), extension trait, toolbar, themes, and code editor are unchanged.

## Problem

The current WYSIWYG editor uses a hidden `<textarea>` to capture input and renders a fake cursor `<div>` positioned via `getBoundingClientRect` DOM measurement. This pattern works for monospace code editors but is fundamentally broken for rich text:

- Cursor position goes stale on resize (no ResizeObserver)
- Manual pixel measurement fails with variable fonts, headings, and custom blocks
- Extension blocks break cursor positioning — the measurement system assumes text nodes
- Every edge case (gap cursors, vertical movement, block boundaries) must be hand-coded
- Selection highlighting is manually rendered with absolutely-positioned divs

## Solution

Use `contenteditable="true"` as the editing surface. The browser handles:

- Cursor rendering and blinking
- Selection highlighting
- Click-to-position mapping
- Resize adaptation
- Native clipboard behavior
- IME/composition input

The editor intercepts events, maps them to DocState operations, and patches the DOM to match.

## What stays unchanged

| Layer | Lines | Status |
|---|---|---|
| kode-core (code editor model) | 2,300 | Keep |
| kode-doc (document tree, editing, transforms) | 13,000 | Keep |
| kode-markdown (parser) | 2,600 | Keep |
| kode-leptos CodeEditor component | 1,000 | Keep |
| Extension trait | 200 | Keep |
| Theme system | 400 | Keep |
| Toolbar | 500 | Keep |
| Completion popup | 870 | Keep |
| Diagnostics | 860 | Keep |

## What gets rewritten

The `kode-leptos/src/wysiwyg/` directory (~3,500 lines):

| File | Current | New |
|---|---|---|
| `tree_editor.rs` (1,607 lines) | Hidden textarea, manual cursor | Contenteditable div, event interception |
| `cursor.rs` (540 lines) | DOM measurement, manual positioning | Removed — browser handles cursor |
| `click.rs` (130 lines) | caretRangeFromPoint mapping | Removed — browser handles clicks |
| `selection.rs` (165 lines) | Manual selection div rendering | Removed — browser renders selection |
| `doc_renderer.rs` (1,002 lines) | Renders tree to positioned elements | Simplified — renders into contenteditable |
| `clipboard.rs` (~200 lines) | Manual clipboard handling | Simplified — leverage contenteditable clipboard |
| `dom_helpers.rs` | Position attribute parsing | Keep subset for position mapping |

## Architecture

### Data flow

```
User interaction (keyboard, mouse, paste)
    ↓
Browser contenteditable handles it natively
    ↓
Event listener intercepts (beforeinput, keydown, selectionchange)
    ↓
Map browser Selection → DocState position (via data-pos-* attributes)
    ↓
Apply DocState operation (insert_text, backspace, split_block, etc.)
    ↓
Re-render: DocState → DOM (patch contenteditable innerHTML)
    ↓
Restore browser Selection from DocState cursor position
```

### Component structure

```rust
#[component]
pub fn TreeWysiwygEditor(...) -> impl IntoView {
    // DocState (source of truth)
    let doc_state = Arc::new(Mutex::new(DocState::from_markdown(&content)));
    
    // Contenteditable div
    view! {
        <div
            contenteditable="true"
            on:beforeinput=handle_input
            on:keydown=handle_keydown
            inner_html=move || render_doc_to_html(&doc_state)
        />
    }
}
```

### Key mechanisms

#### 1. Selection mapping: Browser ↔ DocState

The DOM has `data-pos-start` and `data-pos-end` attributes on block elements. To map the browser's `window.getSelection()` to a DocState position:

1. Get `Selection.focusNode` and `Selection.focusOffset`
2. Walk up from focusNode to find nearest ancestor with `data-pos-start`
3. Count characters from the element start to the selection point
4. DocState position = `data-pos-start + char_count`

To restore selection after re-render:

1. Find the DOM element containing the target DocState position (via `data-pos-*`)
2. Walk text nodes to find the right node and offset
3. Call `Selection.collapse(node, offset)`

#### 2. Event handling

**`beforeinput` event** — the modern standard for contenteditable input:
- `inputType: "insertText"` → `ds.insert_text(data)`
- `inputType: "insertParagraph"` → `ds.split_block()`
- `inputType: "deleteContentBackward"` → `ds.backspace()`
- `inputType: "deleteContentForward"` → `ds.delete_forward()`
- `inputType: "insertFromPaste"` → handle paste
- `inputType: "formatBold"` → `ds.toggle_mark(Strong)`

All handled via `preventDefault()` + DocState operation + re-render.

**`keydown` event** — for keyboard shortcuts not covered by beforeinput:
- `Ctrl+B/I/U` → toggle marks
- `Ctrl+Z/Shift+Z` → undo/redo
- `Tab` → list indent
- Arrow keys → let browser handle natively (no interception needed)

**`selectionchange` event** — sync browser selection to DocState:
- Map browser selection to DocState position
- Update DocState selection (for toolbar state, formatting detection)
- Do NOT re-render — this is read-only sync

#### 3. DOM rendering

Render DocState tree to HTML with `data-pos-*` attributes, same as current `doc_renderer.rs` but simplified:

- No `inner_html` for inline content — render as actual DOM elements so contenteditable can interact with them
- Block elements get `data-pos-start`/`data-pos-end`
- Extension blocks get `contenteditable="false"` wrapper — browser treats them as atomic natively

#### 4. Extension blocks (atom node views)

Extension-rendered blocks (ChartML, images, embeds) are wrapped in:

```html
<div contenteditable="false" data-kode-extension="chartml" data-pos-start="N" data-pos-end="M">
    <!-- extension's custom rendered content -->
</div>
```

The `contenteditable="false"` attribute tells the browser:
- Cursor cannot enter this block
- Selection treats it as a single unit
- Click on it selects the whole node
- Arrow keys skip over it
- Delete/Backspace at boundary removes the whole block

This replaces the entire atom/gap-cursor system we built in the document model. The browser does it for free.

#### 5. Re-render strategy

After each DocState operation:

1. Serialize DocState to HTML
2. Set `innerHTML` on the contenteditable div
3. Restore browser selection from DocState cursor position

This is the simple approach. If performance becomes an issue (large documents), we can add incremental DOM patching — but start simple.

## Migration plan

### Phase 1: Core contenteditable component

Replace `tree_editor.rs` with a contenteditable-based component:
- Render DocState → HTML into contenteditable div
- Handle `beforeinput` for text insertion, paragraph splits, deletion
- Handle `keydown` for formatting shortcuts and undo/redo
- Selection mapping (browser ↔ DocState) via `data-pos-*` attributes
- Selection restore after re-render

Validate: cursor movement, resize, selection, basic typing all work natively.

### Phase 2: Extension blocks

Add `contenteditable="false"` wrappers for extension-rendered blocks:
- Extension trait unchanged — same `render_code_block()` interface
- Wrapper div prevents cursor entry
- Browser handles atomic behavior natively
- Delete/Backspace at boundary removes whole block

Validate: ChartML blocks render, cursor skips them, delete removes them.

### Phase 3: Toolbar and formatting

Wire up the existing toolbar to the new component:
- Formatting state detection via `selectionchange` → DocState
- Toolbar button actions → DocState mark toggles
- Active state highlighting

### Phase 4: Cleanup

- Remove `cursor.rs`, `click.rs`, `selection.rs`
- Remove manual cursor/selection rendering from `tree_editor.rs`
- Remove `find_element_for_pos`, `measure_char_offset_position`, `vertical_cursor_move`
- Keep `dom_helpers.rs` subset (position attribute parsing)
- Update all tests

## Risks

1. **Browser inconsistencies**: contenteditable behavior varies across browsers. Chrome, Firefox, Safari handle `beforeinput` differently. Mitigation: `preventDefault()` on all input types and handle through DocState — never let the browser modify the DOM directly.

2. **Re-render flicker**: Setting `innerHTML` on every keystroke might cause visible flicker. Mitigation: use `requestAnimationFrame` for batching; if needed, switch to incremental DOM diffing later.

3. **IME input**: Composition input (CJK, etc.) needs special handling — don't interrupt composition with re-renders. Mitigation: track `compositionstart`/`compositionend` events, defer re-render until composition completes.

4. **Code block editing**: Code blocks inside contenteditable need special handling — Tab should indent, Enter should insert newline not split block. Mitigation: detect when cursor is inside a code block and adjust behavior.

## Success criteria

- Cursor moves smoothly through all block types without jumping or flickering
- Browser resize does not break cursor or selection positioning
- Extension blocks are naturally atomic — cursor skips, delete removes whole block
- Selection highlighting is native and works across block boundaries
- All existing Playwright tests pass (or have updated equivalents)
- No manual cursor div, no getBoundingClientRect measurement, no position calculation
