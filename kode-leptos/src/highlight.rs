use std::borrow::Cow;
use std::cell::RefCell;

thread_local! {
    static HIGHLIGHTER: RefCell<arborium::Highlighter> = RefCell::new(arborium::Highlighter::new());
}

/// A language identifier used to select a grammar for syntax highlighting.
///
/// This is a thin wrapper around a string tag that matches the name registered
/// with [`arborium::Highlighter`] (e.g. `"sql"`, `"markdown"`, `"python"`).
///
/// Which tags actually highlight at runtime depends on which `lang-*` features
/// the *consumer* has enabled on their own `arborium` dependency. `kode-leptos`
/// does not enable any language by default — consumers opt in to the grammars
/// they need and pay only for those in the final WASM.
///
/// Unknown or unregistered tags fall back to plain-text HTML escaping.
///
/// An empty name ([`Language::PLAIN`]) always renders as plain text.
///
/// # Construction
///
/// ```no_run
/// use kode_leptos::Language;
///
/// // Zero-cost from a string literal
/// let sql = Language::new_static("sql");
///
/// // From an owned String (e.g. a markdown fence info string)
/// let dynamic = Language::new(format!("{}", "markdown"));
///
/// // Plain text
/// let plain = Language::PLAIN;
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Language(Cow<'static, str>);

impl Language {
    /// Plain text — no highlighting is attempted.
    pub const PLAIN: Language = Language(Cow::Borrowed(""));

    /// Construct a language from a `'static` string tag.
    ///
    /// Prefer this for string literals — it is `const`, zero-cost, and never
    /// allocates.
    pub const fn new_static(name: &'static str) -> Self {
        Self(Cow::Borrowed(name))
    }

    /// Construct a language from a string tag, borrowed or owned.
    ///
    /// Use this when the tag comes from a dynamic source such as a markdown
    /// fence info string or a configuration file.
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self(name.into())
    }

    /// The grammar tag passed to the highlighter (e.g. `"sql"`).
    pub fn name(&self) -> &str {
        &self.0
    }

    /// Returns `true` if this represents plain text (no highlighting).
    pub fn is_plain(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<&'static str> for Language {
    fn from(s: &'static str) -> Self {
        Self::new_static(s)
    }
}

impl From<String> for Language {
    fn from(s: String) -> Self {
        Self(Cow::Owned(s))
    }
}

/// Highlight a single line of source code, returning HTML with styled spans.
///
/// **Note:** For multi-line context-dependent languages (SQL, Python, etc.),
/// prefer [`highlight_block`] which parses the full text and splits into lines.
/// Single-line highlighting may miss keywords that depend on surrounding context.
pub fn highlight_line(text: &str, language: &Language) -> String {
    if language.is_plain() {
        return html_escape(text);
    }

    HIGHLIGHTER.with(|h| {
        let mut highlighter = h.borrow_mut();
        match highlighter.highlight(language.name(), text) {
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
pub fn highlight_block(lines: &[&str], language: &Language) -> Vec<String> {
    if language.is_plain() {
        return lines.iter().map(|l| html_escape(l)).collect();
    }

    // Join lines for multi-line parsing
    let full_text = lines.join("\n");

    let full_html = HIGHLIGHTER.with(|h| {
        let mut highlighter = h.borrow_mut();
        match highlighter.highlight(language.name(), &full_text) {
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

/// Resolve a fenced code block info string to a [`Language`].
///
/// Canonicalizes common aliases (e.g. `"md"` → `"markdown"`, `"yml"` → `"yaml"`,
/// `"jsx"` → `"javascript"`) so that the returned tag matches the name
/// `arborium` registers a grammar under. Unknown names resolve to
/// [`Language::PLAIN`].
///
/// The set of aliases here is a superset of widely-recognized markdown fence
/// tags; it does *not* guarantee the resulting language is available at
/// runtime — that still depends on which `arborium` language features the
/// consumer enabled.
pub fn language_from_info_string(info: &str) -> Language {
    let lang = info.split_whitespace().next().unwrap_or("");
    let canonical: &'static str = match lang.to_lowercase().as_str() {
        "sql" => "sql",
        "yaml" | "yml" | "chartml" => "yaml",
        "markdown" | "md" => "markdown",
        "javascript" | "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "typescript" | "ts" | "tsx" | "mts" | "cts" => "typescript",
        "python" | "py" => "python",
        "rust" | "rs" => "rust",
        "go" | "golang" => "go",
        "html" | "htm" => "html",
        "css" => "css",
        "json" | "jsonc" => "json",
        "bash" | "sh" | "shell" | "zsh" => "bash",
        "c" | "h" => "c",
        "cpp" | "c++" | "cxx" | "hpp" => "cpp",
        "java" => "java",
        _ => return Language::PLAIN,
    };
    Language::new_static(canonical)
}

/// The markdown language tag — used internally by [`FenceTracker`] when
/// emitting fence delimiter lines and ambient text.
const LANG_MARKDOWN: Language = Language::new_static("markdown");

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
            fence_lang: LANG_MARKDOWN,
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
                return LANG_MARKDOWN; // fence line itself
            }
            LANG_MARKDOWN
        } else if is_fence_close(trimmed, self.fence_char, self.fence_count) {
            self.in_fence = false;
            LANG_MARKDOWN // closing fence
        } else {
            self.fence_lang.clone()
        }
    }
}

/// Compute the effective highlight language for each line of a markdown document.
///
/// When `base_language` is markdown (i.e. `base_language.name() == "markdown"`),
/// this detects fenced code blocks (lines starting with ``` or ~~~) and returns
/// the appropriate embedded language for lines inside those blocks. Fence lines
/// themselves stay as markdown.
///
/// For any other `base_language`, every line simply gets that language.
pub fn line_languages(lines: &[(usize, String)], base_language: &Language) -> Vec<Language> {
    if base_language.name() != "markdown" {
        return vec![base_language.clone(); lines.len()];
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
        Language::PLAIN
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

    fn lang(name: &'static str) -> Language {
        Language::new_static(name)
    }

    #[test]
    fn sql_keywords_highlighted_on_all_lines() {
        // Multi-line SQL: keywords on lines 2+ must be highlighted via block parsing
        let lines = vec!["SELECT *", "FROM users", "WHERE id = 1"];
        let highlighted = highlight_block(&lines, &lang("sql"));

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
        let highlighted = highlight_block(&lines, &lang("sql"));
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
        let highlighted = highlight_block(&lines, &lang("sql"));
        assert_eq!(highlighted.len(), 1);
        assert!(highlighted[0].contains("<a-"), "Should have highlight tags");
    }

    #[test]
    fn highlight_block_empty_lines() {
        let lines = vec!["SELECT *", "", "FROM users"];
        let highlighted = highlight_block(&lines, &lang("sql"));
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
        let langs = line_languages(&lines, &lang("markdown"));
        assert_eq!(langs[0], lang("markdown")); // heading
        assert_eq!(langs[1], lang("markdown")); // blank
        assert_eq!(langs[2], lang("markdown")); // fence open
        assert_eq!(langs[3], lang("yaml"));     // chartml content
        assert_eq!(langs[4], lang("yaml"));     // chartml content
        assert_eq!(langs[5], lang("markdown")); // fence close
        assert_eq!(langs[6], lang("markdown")); // blank
        assert_eq!(langs[7], lang("markdown")); // text
    }

    #[test]
    fn yaml_fence_gets_yaml_highlighting() {
        let lines = make_lines(&[
            "```yaml",
            "key: value",
            "```",
        ]);
        let langs = line_languages(&lines, &lang("markdown"));
        assert_eq!(langs[0], lang("markdown"));
        assert_eq!(langs[1], lang("yaml"));
        assert_eq!(langs[2], lang("markdown"));
    }

    #[test]
    fn sql_fence_gets_sql_highlighting() {
        let lines = make_lines(&[
            "```sql",
            "SELECT * FROM users",
            "```",
        ]);
        let langs = line_languages(&lines, &lang("markdown"));
        assert_eq!(langs[1], lang("sql"));
    }

    #[test]
    fn non_markdown_base_language_ignores_fences() {
        let lines = make_lines(&[
            "```chartml",
            "type: bar",
            "```",
        ]);
        let langs = line_languages(&lines, &lang("sql"));
        assert_eq!(langs[0], lang("sql"));
        assert_eq!(langs[1], lang("sql"));
        assert_eq!(langs[2], lang("sql"));
    }

    #[test]
    fn tilde_fence_works() {
        let lines = make_lines(&[
            "~~~chartml",
            "type: bar",
            "~~~",
        ]);
        let langs = line_languages(&lines, &lang("markdown"));
        assert_eq!(langs[1], lang("yaml"));
    }

    #[test]
    fn unknown_language_gets_plain() {
        let lines = make_lines(&[
            "```unknown",
            "some content",
            "```",
        ]);
        let langs = line_languages(&lines, &lang("markdown"));
        assert_eq!(langs[1], Language::PLAIN);
    }

    #[test]
    fn language_from_info_string_chartml() {
        assert_eq!(language_from_info_string("chartml"), lang("yaml"));
        assert_eq!(language_from_info_string("ChartML"), lang("yaml"));
        assert_eq!(language_from_info_string("CHARTML"), lang("yaml"));
        assert_eq!(language_from_info_string("yaml"), lang("yaml"));
        assert_eq!(language_from_info_string("sql"), lang("sql"));
        assert_eq!(language_from_info_string("unknown"), Language::PLAIN);
    }

    #[test]
    fn language_plain_is_plain() {
        assert!(Language::PLAIN.is_plain());
        assert_eq!(Language::PLAIN.name(), "");
    }

    #[test]
    fn language_from_string_owned() {
        let s = "sql".to_string();
        let l = Language::from(s);
        assert_eq!(l.name(), "sql");
        assert!(!l.is_plain());
    }

    #[test]
    fn language_from_static_str() {
        let l: Language = "markdown".into();
        assert_eq!(l.name(), "markdown");
    }
}
