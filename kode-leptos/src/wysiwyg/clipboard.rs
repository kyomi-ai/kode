//! Clipboard helpers for the tree-based WYSIWYG editor.
//!
//! Provides HTML escaping for embedding markdown in clipboard HTML payloads,
//! and extraction of markdown from our custom `<pre data-kode-md>` wrapper.

/// Minimal HTML escaping for embedding markdown in an HTML attribute/element.
pub(crate) fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Extract the markdown content from our custom `<pre data-kode-md>` wrapper.
///
/// Returns `None` if the marker is not found. Reverses the HTML escaping
/// applied by [`html_escape`].
pub(crate) fn extract_kode_markdown(html: &str) -> Option<String> {
    // Find the content between <pre data-kode-md> and </pre>.
    let start_marker = "data-kode-md>";
    let start_idx = html.find(start_marker)? + start_marker.len();
    let end_idx = html[start_idx..].find("</pre>").map(|i| start_idx + i)?;
    let escaped = &html[start_idx..end_idx];
    // Reverse HTML escaping.
    Some(
        escaped
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&amp;", "&"),
    )
}
