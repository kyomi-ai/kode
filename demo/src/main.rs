use std::sync::Arc;

use kode_leptos::{
    CodeEditor, CompletionItem, CompletionKind, CompletionProviderConfig, EditorHandle, EditorMode,
    Language, Marker, MarkerSeverity, MarkdownEditorComponent, Position, Theme,
};
use leptos::prelude::*;

fn main() {
    console_error_panic_hook::set_once();
    let _ = console_log::init_with_level(log::Level::Debug);

    mount_to_body(App);
}

fn generate_large_sql(lines: usize) -> String {
    let mut s = String::new();
    s.push_str("-- Auto-generated SQL for performance testing\n");
    s.push_str("WITH base_data AS (\n");
    s.push_str("  SELECT\n");
    s.push_str("    u.id,\n");
    s.push_str("    u.name,\n");
    s.push_str("    u.email,\n");
    s.push_str("    u.created_at,\n");
    s.push_str("    u.status\n");
    s.push_str("  FROM users u\n");
    s.push_str("  WHERE u.status = 'active'\n");
    s.push_str("),\n");
    for i in 0..lines.saturating_sub(30) {
        s.push_str(&format!(
            "cte_{i} AS (\n  SELECT id, COUNT(*) AS cnt_{i}\n  FROM orders\n  WHERE region_id = {i}\n  GROUP BY id\n),\n"
        ));
    }
    s.push_str("final AS (\n");
    s.push_str("  SELECT * FROM base_data\n");
    s.push_str(")\n");
    s.push_str("SELECT * FROM final\n");
    s.push_str("ORDER BY id\n");
    s.push_str("LIMIT 1000;\n");
    s
}

#[component]
fn App() -> impl IntoView {
    // ── Tab state ────────────────────────────────────────────────
    let (active_tab, set_active_tab) = signal("markdown");

    // ── Theme state ───────────────────────────────────────────────
    let (theme, set_theme) = signal(Theme::tokyo_night());

    // ── Code editor state ────────────────────────────────────────
    let (language, set_language) = signal(Language::new_static("sql"));

    let sql_sample = "SELECT\n  u.id,\n  u.name,\n  u.email,\n  COUNT(o.id) AS order_count,\n  SUM(o.total) AS total_spent\nFROM users u\nLEFT JOIN orders o ON o.user_id = u.id\nWHERE u.created_at >= '2025-01-01'\n  AND u.status = 'active'\nGROUP BY u.id, u.name, u.email\nHAVING COUNT(o.id) > 0\nORDER BY total_spent DESC\nLIMIT 100;";

    let yaml_sample = "dashboard:\n  title: Monthly Revenue\n  description: Revenue breakdown by region\n  refresh: 5m\n\npanels:\n  - type: chart\n    title: Revenue Over Time\n    datasource: production-bq\n    query: |\n      SELECT date, SUM(amount) as revenue\n      FROM sales\n      GROUP BY date";

    let (code_content, set_code_content) = signal(sql_sample.to_string());
    let (editor_handle, set_editor_handle) = signal(None::<EditorHandle>);
    let (selection_text, set_selection_text) = signal(String::new());

    let on_lang_change = move |lang: Language| {
        let sample = match lang.name() {
            "sql" => sql_sample,
            "yaml" => yaml_sample,
            "markdown" => "# Markdown\n\nUse the **Markdown Editor** tab instead.",
            "javascript" => "function greet(name) {\n  console.log(`Hello, ${name}!`);\n}\n\ngreet('world');",
            "typescript" => "interface User {\n  name: string;\n  age: number;\n}\n\nfunction greet(user: User): string {\n  return `Hello, ${user.name}!`;\n}",
            "python" => "def fibonacci(n: int) -> list[int]:\n    a, b = 0, 1\n    result = []\n    for _ in range(n):\n        result.append(a)\n        a, b = b, a + b\n    return result\n\nprint(fibonacci(10))",
            "rust" => "fn main() {\n    let items: Vec<&str> = vec![\"hello\", \"world\"];\n    for item in &items {\n        println!(\"{item}\");\n    }\n}",
            "go" => "package main\n\nimport \"fmt\"\n\nfunc main() {\n\tfmt.Println(\"Hello, World!\")\n}",
            "html" => "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"UTF-8\">\n  <title>Hello</title>\n</head>\n<body>\n  <h1>Hello, World!</h1>\n</body>\n</html>",
            "css" => "body {\n  font-family: system-ui, sans-serif;\n  margin: 0;\n  padding: 2rem;\n  background: #1a1b26;\n  color: #a9b1d6;\n}",
            "json" => "{\n  \"name\": \"kode\",\n  \"version\": \"0.2.0\",\n  \"features\": [\"syntax-highlighting\", \"themes\", \"wysiwyg\"]\n}",
            "bash" => "#!/bin/bash\nset -euo pipefail\n\nfor file in *.rs; do\n  echo \"Checking $file\"\n  cargo check\ndone",
            "c" => "#include <stdio.h>\n\nint main(void) {\n    printf(\"Hello, World!\\n\");\n    return 0;\n}",
            "cpp" => "#include <iostream>\n#include <vector>\n\nint main() {\n    std::vector<int> v = {1, 2, 3};\n    for (auto x : v) {\n        std::cout << x << std::endl;\n    }\n}",
            "java" => "public class Hello {\n    public static void main(String[] args) {\n        System.out.println(\"Hello, World!\");\n    }\n}",
            _ => "Hello, world!",
        };
        set_language.set(lang);
        set_code_content.set(sample.to_string());
    };

    // ── Markdown editor state ────────────────────────────────────
    let markdown_sample = "# Dashboard Documentation\n\nThis dashboard tracks **monthly revenue** across all regions.\n\n## Data Sources\n\n- `production-bq`: BigQuery production dataset\n- `analytics-ch`: ClickHouse analytics cluster\n\n## Usage\n\n1. Select a date range from the picker\n2. Choose one or more regions\n3. Click **Apply** to refresh\n\n```sql\nSELECT region, SUM(revenue)\nFROM sales\nGROUP BY region\n```\n\n> Note: Data refreshes every 5 minutes.\n\n### Links\n\nSee [the docs](https://example.com) for more information.\n\n---\n\n*Last updated: March 2026*";

    let (md_content, set_md_content) = signal(markdown_sample.to_string());

    // ── SQL completion provider ──────────────────────────────────
    let sql_completion = CompletionProviderConfig {
        provider: Arc::new(|ctx| {
            Box::pin(async move {
                let keywords = [
                    "SELECT", "FROM", "WHERE", "JOIN", "LEFT", "RIGHT", "INNER",
                    "OUTER", "GROUP", "ORDER", "BY", "HAVING", "LIMIT", "OFFSET",
                    "INSERT", "UPDATE", "DELETE", "CREATE", "ALTER", "DROP",
                    "TABLE", "INDEX", "VIEW", "AS", "ON", "AND", "OR", "NOT",
                    "IN", "EXISTS", "BETWEEN", "LIKE", "IS", "NULL", "TRUE",
                    "FALSE", "CASE", "WHEN", "THEN", "ELSE", "END", "DISTINCT",
                    "COUNT", "SUM", "AVG", "MIN", "MAX", "UNION", "ALL",
                    "WITH",
                ];
                let line = ctx.text.lines().nth(ctx.cursor.line).unwrap_or("");
                let before_cursor = &line[..ctx.cursor.col.min(line.len())];
                let word_start = before_cursor
                    .rfind(|c: char| !c.is_alphanumeric() && c != '_')
                    .map(|i| i + 1)
                    .unwrap_or(0);
                let prefix = &before_cursor[word_start..];

                if prefix.is_empty() {
                    return vec![];
                }

                let prefix_lower = prefix.to_lowercase();
                keywords
                    .iter()
                    .filter(|k| k.to_lowercase().starts_with(&prefix_lower))
                    .map(|k| CompletionItem {
                        label: k.to_string(),
                        insert_text: None,
                        detail: Some("SQL keyword".to_string()),
                        sort_order: 0,
                        kind: CompletionKind::Keyword,
                    })
                    .collect()
            })
        }),
        trigger_characters: vec![],
        activate_on_typing: true,
        render_item: None,
    };
    let completion_providers = Signal::stored(vec![sql_completion]);

    // ── Shared styles ────────────────────────────────────────────
    let pill_style = move |active: bool| {
        let t = theme.get();
        format!(
            "padding:6px 16px;border-radius:6px;border:1px solid {};background:{};color:{};cursor:pointer;font-size:14px;",
            if active { t.accent } else { t.border },
            if active { t.accent } else { "transparent" },
            if active { t.bg } else { t.fg_dim },
        )
    };

    let small_btn_style = move || {
        let t = theme.get();
        format!(
            "padding:6px 14px;border-radius:6px;border:1px solid {};\
             background:transparent;color:{};cursor:pointer;font-size:13px;",
            t.border, t.fg_dim,
        )
    };

    let tab_style = move |tab: &'static str| {
        move || {
            let t = theme.get();
            if active_tab.get() == tab {
                format!(
                    "padding:8px 20px;border:none;border-bottom:2px solid {};background:transparent;\
                     color:{};cursor:pointer;font-size:14px;font-family:inherit;",
                    t.accent, t.fg_bright,
                )
            } else {
                format!(
                    "padding:8px 20px;border:none;border-bottom:2px solid transparent;background:transparent;\
                     color:{};cursor:pointer;font-size:14px;font-family:inherit;",
                    t.fg_dim,
                )
            }
        }
    };

    view! {
        <div style=move || {
            let t = theme.get();
            format!(
                "max-width:1000px;margin:40px auto;font-family:system-ui,sans-serif;\
                 background:{};color:{};min-height:100vh;padding:0 16px;",
                t.bg, t.fg,
            )
        }>
            <h1 style=move || format!("margin-bottom:4px;color:{};", theme.get().fg_bright)>"kode"</h1>
            <p style=move || format!("color:{};margin-bottom:20px;font-size:13px;", theme.get().fg_dim)>
                "A native Rust editor for Leptos. Code editing + Markdown WYSIWYG."
            </p>

            // ── Theme switcher ──────────────────────────────────────────
            <div style="display:flex;gap:8px;margin-bottom:12px;">
                {[
                    ("Tokyo Night", Theme::tokyo_night()),
                    ("One Dark", Theme::one_dark()),
                    ("GitHub Light", Theme::github_light()),
                ].into_iter().map(|(label, t)| {
                    let t_clone = t.clone();
                    let is_active = {
                        let t_cmp = t;
                        move || theme.get() == t_cmp
                    };
                    view! {
                        <button
                            on:click=move |_| set_theme.set(t_clone.clone())
                            style=move || pill_style(is_active())
                        >
                            {label}
                        </button>
                    }
                }).collect::<Vec<_>>()}
            </div>

            // ── Demo tabs ──────────────────────────────────────────────
            <div style=move || format!("display:flex;gap:0;border-bottom:1px solid {};margin-bottom:16px;", theme.get().bg_highlight)>
                <button style=tab_style("markdown") on:click=move |_| set_active_tab.set("markdown")>
                    "Markdown Editor"
                </button>
                <button style=tab_style("code") on:click=move |_| set_active_tab.set("code")>
                    "Code Editor"
                </button>
            </div>

            // ── Markdown editor demo ───────────────────────────────────
            {move || {
                if active_tab.get() == "markdown" {
                    view! {
                        <div>
                            <p style=move || format!("color:{};font-size:12px;margin-bottom:12px;", theme.get().fg_dim)>
                                "Switch between Source and WYSIWYG modes. Toolbar available in WYSIWYG mode. Ctrl+B/I for bold/italic."
                            </p>
                            <div style=move || format!("height:600px;border:1px solid {};border-radius:8px;overflow:hidden;", theme.get().border)>
                                <MarkdownEditorComponent
                                    content=md_content
                                    on_change=Arc::new(move |text: String| {
                                        set_md_content.set(text);
                                    })
                                    initial_mode=EditorMode::Wysiwyg
                                    theme=theme
                                />
                            </div>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <div>
                            <div style="display:flex;gap:8px;margin-bottom:12px;flex-wrap:wrap;">
                                {[
                                    ("SQL", Language::new_static("sql")),
                                    ("YAML", Language::new_static("yaml")),
                                    ("JS", Language::new_static("javascript")),
                                    ("TS", Language::new_static("typescript")),
                                    ("Python", Language::new_static("python")),
                                    ("Rust", Language::new_static("rust")),
                                    ("Go", Language::new_static("go")),
                                    ("HTML", Language::new_static("html")),
                                    ("CSS", Language::new_static("css")),
                                    ("JSON", Language::new_static("json")),
                                    ("Bash", Language::new_static("bash")),
                                    ("C", Language::new_static("c")),
                                    ("C++", Language::new_static("cpp")),
                                    ("Java", Language::new_static("java")),
                                    ("Plain", Language::PLAIN),
                                ].into_iter().map(|(label, lang)| {
                                    let lang_for_active = lang.clone();
                                    let is_active = move || language.get() == lang_for_active;
                                    let on_click = move |_| on_lang_change(lang.clone());
                                    view! {
                                        <button
                                            on:click=on_click
                                            style=move || pill_style(is_active())
                                        >
                                            {label}
                                        </button>
                                    }
                                }).collect::<Vec<_>>()}

                                <div style=move || format!("border-left:1px solid {};margin:0 4px;", theme.get().border) />

                                {[100, 500, 1000, 5000].into_iter().map(|n| {
                                    let on_click = move |_| {
                                        set_language.set(Language::new_static("sql"));
                                        set_code_content.set(generate_large_sql(n));
                                    };
                                    view! {
                                        <button on:click=on_click
                                            style=move || {
                                                let t = theme.get();
                                                format!(
                                                    "padding:6px 12px;border-radius:6px;border:1px solid {};\
                                                     background:transparent;color:{};cursor:pointer;font-size:12px;",
                                                    t.border, t.fg_dim,
                                                )
                                            }>
                                            {format!("{}L", n)}
                                        </button>
                                    }
                                }).collect::<Vec<_>>()}
                            </div>

                            <div style=move || format!("height:500px;border:1px solid {};border-radius:8px;overflow:hidden;", theme.get().border)>
                                <CodeEditor
                                    language=language
                                    content=code_content
                                    theme=theme
                                    completion_providers=completion_providers
                                    placeholder="// Clear the buffer to see this placeholder — try pressing ⌘A then Delete"
                                    on_change=Arc::new(move |text: String| {
                                        log::debug!("Content changed: {} chars", text.len());
                                    })
                                    on_ready=Arc::new(move |handle: EditorHandle| {
                                        set_editor_handle.set(Some(handle));
                                    })
                                />
                            </div>

                            // ── API demo controls ─────────────────────────────
                            <div style="display:flex;gap:8px;margin-top:12px;flex-wrap:wrap;align-items:center;">
                                <button
                                    style=small_btn_style
                                    on:click=move |_| {
                                        if let Some(handle) = editor_handle.get() {
                                            handle.insert_at_cursor(" /* inserted */ ");
                                        }
                                    }
                                >
                                    "Insert at Cursor"
                                </button>

                                <button
                                    style=small_btn_style
                                    on:click=move |_| {
                                        if let Some(handle) = editor_handle.get() {
                                            let text = handle.selected_text().unwrap_or_default();
                                            set_selection_text.set(text);
                                        }
                                    }
                                >
                                    "Read Selection"
                                </button>

                                <button
                                    style=move || {
                                        let t = theme.get();
                                        format!(
                                            "padding:6px 14px;border-radius:6px;border:1px solid {};\
                                             background:transparent;color:{};cursor:pointer;font-size:13px;",
                                            t.marker_error, t.marker_error,
                                        )
                                    }
                                    on:click=move |_| {
                                        if let Some(handle) = editor_handle.get() {
                                            handle.set_markers(vec![
                                                Marker {
                                                    start: Position { line: 0, col: 0 },
                                                    end: Position { line: 0, col: 6 },
                                                    message: "Consider using SELECT DISTINCT".to_string(),
                                                    severity: MarkerSeverity::Warning,
                                                },
                                                Marker {
                                                    start: Position { line: 8, col: 28 },
                                                    end: Position { line: 8, col: 40 },
                                                    message: "Column 'created_at' is ambiguous".to_string(),
                                                    severity: MarkerSeverity::Error,
                                                },
                                            ]);
                                        }
                                    }
                                >
                                    "Set Error Markers"
                                </button>

                                <button
                                    style=small_btn_style
                                    on:click=move |_| {
                                        if let Some(handle) = editor_handle.get() {
                                            handle.clear_markers();
                                        }
                                    }
                                >
                                    "Clear Markers"
                                </button>
                            </div>

                            // ── Selection readout ─────────────────────────────
                            <div style=move || {
                                let t = theme.get();
                                format!(
                                    "margin-top:8px;padding:8px 12px;border-radius:6px;border:1px solid {};\
                                     background:{};color:{};font-size:12px;font-family:monospace;min-height:20px;",
                                    t.bg_highlight, t.bg, t.fg_dim,
                                )
                            }>
                                {move || {
                                    let text = selection_text.get();
                                    if text.is_empty() {
                                        "Selection: (none)".to_string()
                                    } else {
                                        format!("Selection: \"{}\"", text)
                                    }
                                }}
                            </div>
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}
