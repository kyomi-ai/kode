# Kode Coding Standards

Rules learned from code reviews. Each rule includes rationale and a correct example.

## Position Attributes in Extension Blocks

Extension blocks rendered via `doc_to_html` must use `start` and `start + node.node_size()` for `data-pos-start`/`data-pos-end` attributes — NOT `content_start`/`content_end`. The Leptos view path and the HTML string path must produce identical positions.

**Why:** `mount_extension_views` reads these attributes and passes them to `render_code_block`. A 1-token offset breaks any extension keyed on block positions.

```rust
// Correct — matches the Leptos view renderer
format!("data-pos-start=\"{}\" data-pos-end=\"{}\"", start, start + node.node_size())

// Wrong — off by 1 token on each end
format!("data-pos-start=\"{}\" data-pos-end=\"{}\"", content_start, content_end)
```

## Arc Clones in Effects

When multiple closures inside a single `Effect` need the same `Arc<T>`, clone once outside and move into each closure — don't create separate named clones per closure.

**Why:** Multiple clones with different names create a confusing rename chain for identical data.

```rust
// Correct — one clone, moved into closures
let exts = Arc::clone(&extensions);
Effect::new(move |_| {
    let exts_for_mount = Arc::clone(&exts);
    // ...
});
```

## mark_atoms() After Markdown Parsing

Any `DocState` method that introduces `CodeBlock` nodes from markdown parsing must call `mark_atoms()` afterward. This includes `from_doc_with_atoms`, `set_from_markdown`, and `insert_from_markdown`.

**Why:** Without `mark_atoms()`, pasted atomic blocks (chartml, etc.) lose their `atom: true` flag and become editable, breaking extension rendering.
