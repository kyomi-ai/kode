# Atomic Extension Blocks

Extension-rendered code blocks (charts, images, embeds) must be indivisible units in the WYSIWYG editor. The cursor can rest before or after an atomic block but never inside it. All editing operations respect atomicity at the document model level.

## Context

The `Extension` trait (`kode-leptos/src/extension.rs`) lets consumers register custom renderers for fenced code blocks by language. A ChartML extension, for example, renders `chartml` code blocks as interactive charts instead of syntax-highlighted code.

Currently, the only protection is a click bypass — clicks inside `data-kode-extension` elements don't reposition the cursor. But keyboard navigation can move the cursor into the code block content, and typing/backspace/delete corrupt the underlying YAML.

## Design

### Atomicity on the Node

Add an `atom: bool` field to `Node` (kode-doc). Default `false`. Atomic nodes are blocks whose content is opaque to the cursor and editing operations.

`node_size()` is unchanged — atomic CodeBlocks still have `size = 1 + content.size() + 1`. The content is stored and serialized normally. Atomicity is enforced by cursor movement and editing operations, not by hiding content from the position space.

Atomicity is a runtime property, not a document property. The markdown format has no concept of "atomic" — it's determined by which extensions are registered.

### Configuration on DocState

`DocState` holds a `HashSet<String>` of atomic languages. When the document tree is built from markdown, any `CodeBlock` whose `language` attribute matches the set gets `atom: true`.

Flow:
1. Tree editor collects `code_block_languages()` from all registered extensions
2. Passes this set to `DocState::new(markdown, atomic_languages)`
3. DocState marks matching CodeBlocks during tree construction
4. `set_from_markdown()` re-applies the same marking
5. Editing operations that create/modify CodeBlocks preserve the flag

### Gap Cursor Positions

Currently, `adjust_into_textblock()` nudges between-block positions into the nearest textblock. With atomic blocks this fails — the nearest textblock might be inside an atomic block.

Change: if a between-block position has an atomic block on either side, it is a valid **gap cursor** position. The cursor can rest there.

A gap position is any position `p` where `resolve(p).parent()` is a non-textblock container (e.g., Doc) AND at least one of `node_before`/`node_after` is atomic.

### Editing Behavior

| Operation | At gap before atomic block | At gap after atomic block |
|---|---|---|
| Type text | Insert new Paragraph before block, type there | Insert new Paragraph after block, type there |
| Enter | Insert empty Paragraph before block | Insert empty Paragraph after block |
| Backspace | Delete the atomic block whole | No-op |
| Delete | No-op | Delete the atomic block whole |
| Arrow Left | Move to previous textblock end or gap | Move to gap before this block |
| Arrow Right | Move to gap after this block | Move to next textblock start or gap |

Cursor can never enter an atomic block. Backspace deletes backward, Delete deletes forward — matching standard editor behavior for atomic elements.

### Selection

Range selection includes entire atomic blocks. If a selection starts or ends inside an atomic block, it expands to include the whole block. Deleting a selection that spans an atomic block deletes the block.

### Rendering

The tree editor renders the cursor at gap positions using the bounding rects of the adjacent blocks — vertical bar at the content left edge, vertically centered in the gap between blocks.

## Changes by Layer

### kode-doc

1. `Node` — add `atom: bool` field, `is_atom()` method
2. `DocState` — add `atomic_languages: HashSet<String>`
3. `DocState::new()` — accept atomic languages, mark matching CodeBlocks
4. `DocState::set_from_markdown()` — re-mark after rebuild
5. `adjust_into_textblock()` — skip atomic blocks, allow gap positions
6. `backspace()` — if previous node is atomic, delete it whole
7. `delete_forward()` — if next node is atomic, delete it whole
8. `insert_text()` at gap — create Paragraph, insert text, splice into doc
9. `split_block()` at gap — insert empty Paragraph
10. Selection — expand to include full atomic blocks if partially selected

### kode-leptos

1. `next_text_pos()` — skip atomic blocks (check `node.is_atom()` alongside `is_textblock()`)
2. Tree editor init — collect atomic languages from extensions, pass to DocState
3. Gap cursor rendering — position cursor between blocks at gap positions
4. Vertical cursor movement — respect atomicity when probing

### kode-markdown

No changes. Atomicity is applied by DocState after parsing.
