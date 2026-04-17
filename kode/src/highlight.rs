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
    /// Returns the arborium language identifier.
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

/// Highlight source code, returning HTML with styled spans.
/// Uses a thread-local arborium::Highlighter for performance.
pub fn highlight_text(text: &str, language: Language) -> String {
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

/// Get the CSS for the current theme.
pub fn theme_css() -> String {
    arborium_theme::builtin::tokyo_night().to_css("pre.kode-content")
}

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
