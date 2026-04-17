use std::cell::RefCell;

thread_local! {
    static HIGHLIGHTER: RefCell<arborium::Highlighter> = RefCell::new(arborium::Highlighter::new());
}

/// Supported languages for syntax highlighting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Language {
    Sql,
    Yaml,
    Markdown,
    JavaScript,
    TypeScript,
    Python,
    Rust,
    Go,
    Html,
    Css,
    Json,
    Bash,
    C,
    Cpp,
    Java,
    #[default]
    Plain,
}

impl Language {
    fn arborium_name(&self) -> Option<&'static str> {
        match self {
            Language::Sql => Some("sql"),
            Language::Yaml => Some("yaml"),
            Language::Markdown => Some("markdown"),
            Language::JavaScript => Some("javascript"),
            Language::TypeScript => Some("typescript"),
            Language::Python => Some("python"),
            Language::Rust => Some("rust"),
            Language::Go => Some("go"),
            Language::Html => Some("html"),
            Language::Css => Some("css"),
            Language::Json => Some("json"),
            Language::Bash => Some("bash"),
            Language::C => Some("c"),
            Language::Cpp => Some("cpp"),
            Language::Java => Some("java"),
            Language::Plain => None,
        }
    }
}

/// Highlight a single line of source code, returning HTML with styled spans.
///
/// **Note:** For multi-line context-dependent languages (SQL, Python, etc.),
/// prefer [`highlight_block`] which parses the full text and splits into lines.
/// Single-line highlighting may miss keywords that depend on surrounding context.
pub fn highlight_line(text: &str, language: Language) -> String {
    let lang_name = match language.arborium_name() {
        Some(name) => name,
        None => return html_escape(text),
    };

    HIGHLIGHTER.with(|h| {
        let mut highlighter = h.borrow_mut();
        match highlighter.highlight(lang_name, text) {
            Ok(result) => result.to_string(),
            Err(_) => html_escape(text),
        }
    })
}

/// Highlight a block of source code as a single document, returning per-line HTML.
///
/// Unlike [`highlight_line`], this gives tree-sitter the full multi-line context
/// so keywords like `FROM` and `WHERE` in SQL are correctly identified.
///
/// Returns a `Vec<String>` with one HTML string per input line.
pub fn highlight_block(lines: &[&str], language: Language) -> Vec<String> {
    let lang_name = match language.arborium_name() {
        Some(name) => name,
        None => return lines.iter().map(|l| html_escape(l)).collect(),
    };

    // Join lines for multi-line parsing
    let full_text = lines.join("\n");

    let full_html = HIGHLIGHTER.with(|h| {
        let mut highlighter = h.borrow_mut();
        match highlighter.highlight(lang_name, &full_text) {
            Ok(result) => result.to_string(),
            Err(_) => html_escape(&full_text),
        }
    });

    // Split the HTML output back into per-line chunks.
    // Arborium's HTML output preserves newlines in the source text, so we split on
    // literal '\n'. However, spans may wrap across newlines, so we need to track
    // open tags and re-open them on continuation lines.
    split_html_by_lines(&full_html, lines.len())
}

/// Split highlighted HTML into per-line chunks, carrying open tags across lines.
fn split_html_by_lines(html: &str, expected_lines: usize) -> Vec<String> {
    let mut result: Vec<String> = Vec::with_capacity(expected_lines);
    let mut current_line = String::new();
    // Stack of currently open tags (e.g., "<a-k>")
    let mut open_tags: Vec<String> = Vec::new();
    let mut chars = html.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\n' {
            // Close open tags for this line
            for tag in open_tags.iter().rev() {
                let close = closing_tag(tag);
                current_line.push_str(&close);
            }
            result.push(std::mem::take(&mut current_line));
            // Re-open tags on the next line
            for tag in &open_tags {
                current_line.push_str(tag);
            }
        } else if ch == '<' {
            // Collect the full tag
            let mut tag = String::from('<');
            for tc in chars.by_ref() {
                tag.push(tc);
                if tc == '>' {
                    break;
                }
            }
            if tag.starts_with("</") {
                // Closing tag — pop from open_tags
                open_tags.pop();
            } else if !tag.ends_with("/>") {
                // Opening tag (not self-closing) — push to open_tags
                open_tags.push(tag.clone());
            }
            current_line.push_str(&tag);
        } else {
            current_line.push(ch);
        }
    }

    // Push the last line (may be empty if input ended with '\n')
    if !current_line.is_empty() || result.len() < expected_lines {
        result.push(current_line);
    }

    // Arborium may strip trailing newlines, so the split can produce fewer lines
    // than expected. Pad with empty strings for trailing empty lines.
    result.truncate(expected_lines);
    while result.len() < expected_lines {
        result.push(String::new());
    }

    result
}

/// Convert an opening tag like "<a-k>" to its closing form "</a-k>".
fn closing_tag(open_tag: &str) -> String {
    // Extract tag name: "<a-k>" -> "a-k", "<span class='x'>" -> "span"
    let inner = open_tag.trim_start_matches('<').trim_end_matches('>');
    let name = inner.split_whitespace().next().unwrap_or(inner);
    format!("</{name}>")
}

/// Resolve a fenced code block info string to a `Language`.
///
/// Maps well-known names and aliases (e.g. "chartml" → Yaml) to the
/// corresponding `Language` variant. Unknown names resolve to `Plain`.
pub fn language_from_info_string(info: &str) -> Language {
    let lang = info.split_whitespace().next().unwrap_or("");
    match lang.to_lowercase().as_str() {
        "sql" => Language::Sql,
        "yaml" | "yml" | "chartml" => Language::Yaml,
        "markdown" | "md" => Language::Markdown,
        "javascript" | "js" | "jsx" | "mjs" | "cjs" => Language::JavaScript,
        "typescript" | "ts" | "tsx" | "mts" | "cts" => Language::TypeScript,
        "python" | "py" => Language::Python,
        "rust" | "rs" => Language::Rust,
        "go" | "golang" => Language::Go,
        "html" | "htm" => Language::Html,
        "css" => Language::Css,
        "json" | "jsonc" => Language::Json,
        "bash" | "sh" | "shell" | "zsh" => Language::Bash,
        "c" | "h" => Language::C,
        "cpp" | "c++" | "cxx" | "hpp" => Language::Cpp,
        "java" => Language::Java,
        _ => Language::Plain,
    }
}

/// Tracks fenced code block state while scanning a markdown document.
///
/// The editor renders a virtual viewport (not the full document), so we need to
/// scan lines from the top of the buffer to establish fence state, then produce
/// per-line languages for the visible range.
#[derive(Clone)]
pub struct FenceTracker {
    in_fence: bool,
    fence_lang: Language,
    fence_char: char,
    fence_count: usize,
}

impl Default for FenceTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl FenceTracker {
    /// Create a new tracker starting outside any fence.
    pub fn new() -> Self {
        Self {
            in_fence: false,
            fence_lang: Language::Markdown,
            fence_char: '`',
            fence_count: 0,
        }
    }

    /// Process one line and return the language for that line.
    pub fn process_line(&mut self, text: &str) -> Language {
        let trimmed = text.trim_start();

        if !self.in_fence {
            if let Some((fc, count, lang)) = detect_fence_open(trimmed) {
                self.in_fence = true;
                self.fence_char = fc;
                self.fence_count = count;
                self.fence_lang = lang;
                return Language::Markdown; // fence line itself
            }
            Language::Markdown
        } else {
            if is_fence_close(trimmed, self.fence_char, self.fence_count) {
                self.in_fence = false;
                Language::Markdown // closing fence
            } else {
                self.fence_lang
            }
        }
    }
}

/// Compute the effective highlight language for each line of a markdown document.
///
/// When `base_language` is `Language::Markdown`, this detects fenced code blocks
/// (lines starting with ``` or ~~~) and returns the appropriate embedded language
/// for lines inside those blocks. Fence lines themselves stay as Markdown.
///
/// For any other `base_language`, every line simply gets that language.
pub fn line_languages(lines: &[(usize, String)], base_language: Language) -> Vec<Language> {
    if base_language != Language::Markdown {
        return vec![base_language; lines.len()];
    }

    let mut tracker = FenceTracker::new();
    lines.iter().map(|(_, text)| tracker.process_line(text)).collect()
}

/// Detect a fenced code block opening. Returns (fence_char, fence_count, language).
fn detect_fence_open(trimmed: &str) -> Option<(char, usize, Language)> {
    let first = trimmed.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }

    let fence_count = trimmed.chars().take_while(|&c| c == first).count();
    if fence_count < 3 {
        return None;
    }

    let info = trimmed[fence_count..].trim();
    // Backtick fences cannot contain backticks in the info string
    if first == '`' && info.contains('`') {
        return None;
    }

    let lang = if info.is_empty() {
        Language::Plain
    } else {
        language_from_info_string(info)
    };

    Some((first, fence_count, lang))
}

/// Check if a line closes the current fence.
fn is_fence_close(trimmed: &str, fence_char: char, min_count: usize) -> bool {
    let count = trimmed.chars().take_while(|&c| c == fence_char).count();
    if count < min_count {
        return false;
    }
    // Closing fence must have only whitespace after the fence chars
    trimmed[count..].trim().is_empty()
}

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_lines(texts: &[&str]) -> Vec<(usize, String)> {
        texts.iter().enumerate().map(|(i, t)| (i, t.to_string())).collect()
    }

    #[test]
    fn sql_keywords_highlighted_on_all_lines() {
        // Multi-line SQL: keywords on lines 2+ must be highlighted via block parsing
        let lines = vec!["SELECT *", "FROM users", "WHERE id = 1"];
        let highlighted = highlight_block(&lines, Language::Sql);

        assert_eq!(highlighted.len(), 3);

        // Line 1 should have SELECT highlighted (contains an arborium tag)
        assert!(
            highlighted[0].contains("<a-"),
            "Line 1 'SELECT *' should have highlight tags, got: {}",
            highlighted[0]
        );
        // Line 2 should have FROM highlighted
        assert!(
            highlighted[1].contains("<a-"),
            "Line 2 'FROM users' should have highlight tags, got: {}",
            highlighted[1]
        );
        // Line 3 should have WHERE highlighted
        assert!(
            highlighted[2].contains("<a-"),
            "Line 3 'WHERE id = 1' should have highlight tags, got: {}",
            highlighted[2]
        );
    }

    #[test]
    fn highlight_block_with_trailing_empty_line() {
        // 4 lines where the last is empty — this is common when the buffer
        // ends with a newline (ropey represents it as an extra empty line)
        let lines = vec!["SELECT *", "FROM users", "WHERE id = 1", ""];
        let highlighted = highlight_block(&lines, Language::Sql);
        assert_eq!(
            highlighted.len(),
            4,
            "Expected 4 lines, got {}: {:?}",
            highlighted.len(),
            highlighted
        );
    }

    #[test]
    fn highlight_block_single_line() {
        let lines = vec!["SELECT * FROM users"];
        let highlighted = highlight_block(&lines, Language::Sql);
        assert_eq!(highlighted.len(), 1);
        assert!(highlighted[0].contains("<a-"), "Should have highlight tags");
    }

    #[test]
    fn highlight_block_empty_lines() {
        let lines = vec!["SELECT *", "", "FROM users"];
        let highlighted = highlight_block(&lines, Language::Sql);
        assert_eq!(highlighted.len(), 3);
        assert_eq!(highlighted[1], "", "Empty line should produce empty string");
    }

    #[test]
    fn chartml_fence_gets_yaml_highlighting() {
        let lines = make_lines(&[
            "# My Dashboard",
            "",
            "```chartml",
            "type: bar",
            "title: Sales",
            "```",
            "",
            "Some text",
        ]);
        let langs = line_languages(&lines, Language::Markdown);
        assert_eq!(langs[0], Language::Markdown); // heading
        assert_eq!(langs[1], Language::Markdown); // blank
        assert_eq!(langs[2], Language::Markdown); // fence open
        assert_eq!(langs[3], Language::Yaml);     // chartml content
        assert_eq!(langs[4], Language::Yaml);     // chartml content
        assert_eq!(langs[5], Language::Markdown); // fence close
        assert_eq!(langs[6], Language::Markdown); // blank
        assert_eq!(langs[7], Language::Markdown); // text
    }

    #[test]
    fn yaml_fence_gets_yaml_highlighting() {
        let lines = make_lines(&[
            "```yaml",
            "key: value",
            "```",
        ]);
        let langs = line_languages(&lines, Language::Markdown);
        assert_eq!(langs[0], Language::Markdown);
        assert_eq!(langs[1], Language::Yaml);
        assert_eq!(langs[2], Language::Markdown);
    }

    #[test]
    fn sql_fence_gets_sql_highlighting() {
        let lines = make_lines(&[
            "```sql",
            "SELECT * FROM users",
            "```",
        ]);
        let langs = line_languages(&lines, Language::Markdown);
        assert_eq!(langs[1], Language::Sql);
    }

    #[test]
    fn non_markdown_base_language_ignores_fences() {
        let lines = make_lines(&[
            "```chartml",
            "type: bar",
            "```",
        ]);
        let langs = line_languages(&lines, Language::Sql);
        assert_eq!(langs[0], Language::Sql);
        assert_eq!(langs[1], Language::Sql);
        assert_eq!(langs[2], Language::Sql);
    }

    #[test]
    fn tilde_fence_works() {
        let lines = make_lines(&[
            "~~~chartml",
            "type: bar",
            "~~~",
        ]);
        let langs = line_languages(&lines, Language::Markdown);
        assert_eq!(langs[1], Language::Yaml);
    }

    #[test]
    fn unknown_language_gets_plain() {
        let lines = make_lines(&[
            "```unknown",
            "some content",
            "```",
        ]);
        let langs = line_languages(&lines, Language::Markdown);
        assert_eq!(langs[1], Language::Plain);
    }

    #[test]
    fn language_from_info_string_chartml() {
        assert_eq!(language_from_info_string("chartml"), Language::Yaml);
        assert_eq!(language_from_info_string("ChartML"), Language::Yaml);
        assert_eq!(language_from_info_string("CHARTML"), Language::Yaml);
        assert_eq!(language_from_info_string("yaml"), Language::Yaml);
        assert_eq!(language_from_info_string("sql"), Language::Sql);
        assert_eq!(language_from_info_string("unknown"), Language::Plain);
    }
}
