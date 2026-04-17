//! Editor theme definitions.
//!
//! A [`Theme`] bundles all UI colors (background, foreground, gutter, markers,
//! etc.) together with a [`SyntaxTheme`] that controls token-level highlighting.

/// Syntax highlighting theme selection.
///
/// Each variant maps to a builtin arborium-theme, except [`Custom`] which
/// accepts raw CSS.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyntaxTheme {
    TokyoNight,
    OneDark,
    Dracula,
    Nord,
    GithubLight,
    GruvboxDark,
    CatppuccinMocha,
    SolarizedDark,
    SolarizedLight,
    Custom(String),
}

/// Complete editor theme — UI chrome colors plus syntax highlighting.
///
/// All fields have sensible defaults via [`Default`] (Tokyo Night dark theme).
/// Use struct update syntax to override only what you need:
///
/// ```ignore
/// Theme {
///     accent: "#0366d6",
///     content_font_family: Some("DM Sans"),
///     ..Theme::dark()
/// }
/// ```
///
/// The `#[non_exhaustive]` attribute ensures new fields can be added without
/// breaking downstream consumers — the compiler enforces `..Default::default()`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Theme {
    // ── Core colors (required) ──────────────────────────────────────
    pub bg: &'static str,
    pub fg: &'static str,
    pub fg_bright: &'static str,
    pub fg_dim: &'static str,
    pub cursor: &'static str,
    pub selection: &'static str,
    pub current_line: &'static str,
    pub gutter_fg: &'static str,
    pub gutter_border: &'static str,
    pub border: &'static str,
    pub accent: &'static str,
    pub bg_highlight: &'static str,
    pub bg_hover: &'static str,
    pub marker_error: &'static str,
    pub marker_warning: &'static str,
    pub marker_info: &'static str,
    pub marker_hint: &'static str,
    pub code_fg: &'static str,
    pub link: &'static str,
    pub syntax: SyntaxTheme,

    // ── Typography (optional overrides) ─────────────────────────────
    /// Monospace font for the code editor. Default: JetBrains Mono stack.
    pub font_family: Option<&'static str>,
    /// Code editor font size. Default: 14px.
    pub font_size: Option<&'static str>,
    /// Prose font for WYSIWYG content. Default: system sans-serif stack.
    pub content_font_family: Option<&'static str>,
    /// Heading font family. Default: inherits from content_font_family.
    pub heading_font_family: Option<&'static str>,
    /// Code font for inline code and code blocks in WYSIWYG. Default: JetBrains Mono stack.
    pub code_font_family: Option<&'static str>,

    // ── Toolbar layout (optional overrides) ────────────────────────────
    /// Toolbar background. Default: var(--kode-bg).
    pub toolbar_bg: Option<&'static str>,
    /// Toolbar bottom border color. Default: var(--kode-border).
    pub toolbar_border_color: Option<&'static str>,
    /// Toolbar padding. Default: 6px 8px.
    pub toolbar_padding: Option<&'static str>,
    /// Toolbar gap between items. Default: 2px.
    pub toolbar_gap: Option<&'static str>,
    /// Toolbar flex-wrap. Default: wrap. Set to "nowrap" for horizontal scroll.
    pub toolbar_wrap: Option<&'static str>,
    /// Toolbar scrollbar color (for nowrap mode). Default: auto.
    pub toolbar_scrollbar_color: Option<&'static str>,

    // ── Toolbar buttons (optional overrides) ────────────────────────────
    /// Toolbar button height. Default: 28px.
    pub toolbar_button_height: Option<&'static str>,
    /// Toolbar button border-radius. Default: 4px.
    pub toolbar_button_border_radius: Option<&'static str>,
    /// Toolbar button padding. Default: 0 6px.
    pub toolbar_button_padding: Option<&'static str>,
    /// Toolbar button font-size. Default: 13px.
    pub toolbar_button_font_size: Option<&'static str>,
    /// Toolbar button gap between icon and label text. Default: 0.
    pub toolbar_button_gap: Option<&'static str>,
    /// Toolbar button border. Default: 1px solid transparent.
    pub toolbar_button_border: Option<&'static str>,

    // ── Toolbar button states (optional overrides) ────────────────────
    /// Toolbar button hover background. Default: var(--kode-bg-hover).
    pub toolbar_button_hover_bg: Option<&'static str>,
    /// Toolbar button selected/active background. Default: var(--kode-accent).
    pub toolbar_button_selected_bg: Option<&'static str>,
    /// Toolbar button selected/active text color. Default: var(--kode-bg).
    pub toolbar_button_selected_color: Option<&'static str>,

    // ── Toolbar separator (optional overrides) ───────────────────────
    /// Toolbar separator height. Default: 20px.
    pub toolbar_separator_height: Option<&'static str>,
    /// Toolbar separator margin. Default: 0 4px.
    pub toolbar_separator_margin: Option<&'static str>,

    // ── Content area (optional overrides) ───────────────────────────
    /// Outer .wysiwyg-container padding (around toolbar + content). Default: 0.
    pub container_padding: Option<&'static str>,
    /// Inner .tree-wysiwyg-scroll-container padding (content area only). Default: 1.5rem 2rem.
    pub content_padding: Option<&'static str>,
    /// WYSIWYG content max-width. Default: 800px.
    pub content_max_width: Option<&'static str>,
    /// WYSIWYG content line-height. Default: 1.7.
    pub content_line_height: Option<&'static str>,

    // ── Heading overrides (optional) ────────────────────────────────
    /// Heading font-weight. Default: 700.
    pub heading_font_weight: Option<&'static str>,
    /// H1 border-bottom width. Set to "0" to remove. Default: 1px.
    pub h1_border_width: Option<&'static str>,
    /// H2 border-bottom width. Set to "0" to remove. Default: 1px.
    pub h2_border_width: Option<&'static str>,
}

impl Default for Theme {
    fn default() -> Self {
        Self::tokyo_night()
    }
}

impl Theme {
    /// Pre-configured dark theme (Tokyo Night). Alias for `Default::default()`.
    ///
    /// Use as a starting point for dark-themed host apps:
    /// ```ignore
    /// Theme { accent: "#ff6b6b", ..Theme::dark() }
    /// ```
    pub fn dark() -> Self {
        Self::tokyo_night()
    }

    /// Pre-configured light theme (GitHub Light). Use as a starting point
    /// for light-themed host apps:
    /// ```ignore
    /// Theme { accent: "#0550ae", ..Theme::light() }
    /// ```
    pub fn light() -> Self {
        Self::github_light()
    }

    /// Tokyo Night — the default dark theme.
    ///
    /// Colors extracted from the current hardcoded values in editor.css,
    /// wysiwyg.css, and toolbar.rs.
    pub fn tokyo_night() -> Self {
        Self {
            bg: "#1a1b26",
            fg: "#a9b1d6",
            fg_bright: "#c0caf5",
            fg_dim: "#565f89",
            cursor: "#c0caf5",
            selection: "rgba(40, 52, 87, 0.6)",
            current_line: "rgba(255, 255, 255, 0.07)",
            gutter_fg: "#3b4261",
            gutter_border: "#252535",
            border: "#3b4261",
            accent: "#7aa2f7",
            bg_highlight: "#292e42",
            bg_hover: "#33467c",
            marker_error: "#e53e3e",
            marker_warning: "#d69e2e",
            marker_info: "#3182ce",
            marker_hint: "#718096",
            code_fg: "#9ece6a",
            link: "#7aa2f7",
            syntax: SyntaxTheme::TokyoNight,
            font_family: None,
            font_size: None,
            content_font_family: None,
            heading_font_family: None,
            code_font_family: None,
            toolbar_bg: None,
            toolbar_border_color: None,
            toolbar_padding: None,
            toolbar_gap: None,
            toolbar_wrap: None,
            toolbar_scrollbar_color: None,
            toolbar_button_height: None,
            toolbar_button_border_radius: None,
            toolbar_button_padding: None,
            toolbar_button_font_size: None,
            toolbar_button_gap: None,
            toolbar_button_border: None,
            toolbar_button_hover_bg: None,
            toolbar_button_selected_bg: None,
            toolbar_button_selected_color: None,
            toolbar_separator_height: None,
            toolbar_separator_margin: None,
            container_padding: None,
            content_padding: None,
            content_max_width: None,
            content_line_height: None,
            heading_font_weight: None,
            h1_border_width: None,
            h2_border_width: None,
        }
    }

    /// Atom One Dark palette.
    pub fn one_dark() -> Self {
        Self {
            bg: "#282c34",
            fg: "#abb2bf",
            fg_bright: "#d7dae0",
            fg_dim: "#5c6370",
            cursor: "#528bff",
            selection: "rgba(62, 68, 81, 0.6)",
            current_line: "rgba(255, 255, 255, 0.07)",
            gutter_fg: "#4b5263",
            gutter_border: "#3b3f4c",
            border: "#3b3f4c",
            accent: "#61afef",
            bg_highlight: "#2c313c",
            bg_hover: "#3e4451",
            marker_error: "#e06c75",
            marker_warning: "#e5c07b",
            marker_info: "#61afef",
            marker_hint: "#5c6370",
            code_fg: "#98c379",
            link: "#61afef",
            syntax: SyntaxTheme::OneDark,
            ..Self::tokyo_night()
        }
    }

    /// GitHub Light palette.
    pub fn github_light() -> Self {
        Self {
            bg: "#ffffff",
            fg: "#24292e",
            fg_bright: "#000000",
            fg_dim: "#6a737d",
            cursor: "#044289",
            selection: "rgba(4, 66, 137, 0.15)",
            current_line: "rgba(0, 0, 0, 0.04)",
            gutter_fg: "#babbbd",
            gutter_border: "#e1e4e8",
            border: "#e1e4e8",
            accent: "#0366d6",
            bg_highlight: "#f6f8fa",
            bg_hover: "#e1e4e8",
            marker_error: "#cb2431",
            marker_warning: "#b08800",
            marker_info: "#0366d6",
            marker_hint: "#6a737d",
            code_fg: "#22863a",
            link: "#0366d6",
            syntax: SyntaxTheme::GithubLight,
            ..Self::tokyo_night()
        }
    }

    /// Produce an inline CSS variable string for all UI colors.
    ///
    /// The returned string is meant to be set as the `style` attribute on the
    /// editor's root element so that child CSS rules can reference the
    /// `--kode-*` custom properties.
    pub fn to_css_vars(&self) -> String {
        let mut css = format!(
            "--kode-bg:{};--kode-fg:{};--kode-fg-bright:{};--kode-fg-dim:{};\
             --kode-cursor:{};--kode-selection:{};--kode-current-line:{};\
             --kode-gutter-fg:{};--kode-gutter-border:{};--kode-border:{};\
             --kode-accent:{};--kode-bg-highlight:{};--kode-bg-hover:{};\
             --kode-marker-error:{};--kode-marker-warning:{};--kode-marker-info:{};\
             --kode-marker-hint:{};--kode-code-fg:{};--kode-link:{}",
            self.bg,
            self.fg,
            self.fg_bright,
            self.fg_dim,
            self.cursor,
            self.selection,
            self.current_line,
            self.gutter_fg,
            self.gutter_border,
            self.border,
            self.accent,
            self.bg_highlight,
            self.bg_hover,
            self.marker_error,
            self.marker_warning,
            self.marker_info,
            self.marker_hint,
            self.code_fg,
            self.link,
        );

        // Append optional component-level overrides
        let optionals: &[(&str, Option<&str>)] = &[
            ("--kode-font-family", self.font_family),
            ("--kode-font-size", self.font_size),
            ("--kode-content-font-family", self.content_font_family),
            ("--kode-heading-font-family", self.heading_font_family),
            ("--kode-code-font-family", self.code_font_family),
            ("--kode-toolbar-bg", self.toolbar_bg),
            ("--kode-toolbar-border-color", self.toolbar_border_color),
            ("--kode-toolbar-padding", self.toolbar_padding),
            ("--kode-toolbar-gap", self.toolbar_gap),
            ("--kode-toolbar-wrap", self.toolbar_wrap),
            ("--kode-toolbar-scrollbar-color", self.toolbar_scrollbar_color),
            ("--kode-toolbar-button-height", self.toolbar_button_height),
            ("--kode-toolbar-button-border-radius", self.toolbar_button_border_radius),
            ("--kode-toolbar-button-padding", self.toolbar_button_padding),
            ("--kode-toolbar-button-font-size", self.toolbar_button_font_size),
            ("--kode-toolbar-button-gap", self.toolbar_button_gap),
            ("--kode-toolbar-button-border", self.toolbar_button_border),
            ("--kode-toolbar-button-hover-bg", self.toolbar_button_hover_bg),
            ("--kode-toolbar-button-selected-bg", self.toolbar_button_selected_bg),
            ("--kode-toolbar-button-selected-color", self.toolbar_button_selected_color),
            ("--kode-toolbar-separator-height", self.toolbar_separator_height),
            ("--kode-toolbar-separator-margin", self.toolbar_separator_margin),
            ("--kode-container-padding", self.container_padding),
            ("--kode-content-padding", self.content_padding),
            ("--kode-content-max-width", self.content_max_width),
            ("--kode-content-line-height", self.content_line_height),
            ("--kode-heading-font-weight", self.heading_font_weight),
            ("--kode-h1-border-width", self.h1_border_width),
            ("--kode-h2-border-width", self.h2_border_width),
        ];
        for (name, value) in optionals {
            if let Some(v) = value {
                css.push_str(&format!(";{}:{}", name, v));
            }
        }

        css
    }

    /// Generate syntax-highlighting CSS scoped to `selector`.
    ///
    /// For [`SyntaxTheme::Custom`] the raw CSS string is returned as-is.
    /// For builtin themes, token-level rules (`a-k`, `a-s`, etc.) are
    /// generated directly from the arborium [`Theme`](arborium_theme::Theme)
    /// struct's public `styles` array — **without** calling `to_css()`.
    ///
    /// This ensures only token colors are emitted. Base styles (`background`,
    /// `color`, CSS custom properties) that arborium's `to_css()` normally
    /// includes are never generated, because those are controlled by
    /// [`Theme::to_css_vars()`].
    ///
    /// Additionally, `a-p` (punctuation) and `a-tl` (text literal) are
    /// overridden with `var(--kode-fg-dim)` and `var(--kode-code-fg)` so
    /// markdown structure characters (`#`, `*`, `` ` ``) are visually
    /// distinct.
    pub fn syntax_css(&self, selector: &str) -> String {
        if let SyntaxTheme::Custom(css) = &self.syntax {
            return css.clone();
        }

        let arb_theme = self.syntax.arborium_theme();
        token_only_css(&arb_theme, selector)
    }
}

impl SyntaxTheme {
    /// Resolve to the arborium theme struct.
    fn arborium_theme(&self) -> arborium_theme::Theme {
        match self {
            SyntaxTheme::TokyoNight => arborium_theme::builtin::tokyo_night(),
            SyntaxTheme::OneDark => arborium_theme::builtin::one_dark(),
            SyntaxTheme::Dracula => arborium_theme::builtin::dracula(),
            SyntaxTheme::Nord => arborium_theme::builtin::nord(),
            SyntaxTheme::GithubLight => arborium_theme::builtin::github_light(),
            SyntaxTheme::GruvboxDark => arborium_theme::builtin::gruvbox_dark(),
            SyntaxTheme::CatppuccinMocha => arborium_theme::builtin::catppuccin_mocha(),
            SyntaxTheme::SolarizedDark => arborium_theme::builtin::solarized_dark(),
            SyntaxTheme::SolarizedLight => arborium_theme::builtin::solarized_light(),
            SyntaxTheme::Custom(_) => unreachable!("Custom handled before arborium_theme()"),
        }
    }
}

/// Generate token-only CSS from an arborium theme, scoped to `selector`.
///
/// Mirrors the token-rule generation logic from arborium's `to_css()` but
/// intentionally omits `background`, `color`, and CSS custom properties
/// (`--bg`, `--fg`, `--surface`, `--accent`, `--muted`).
///
/// Appends kode-specific overrides for `a-p` and `a-tl`.
fn token_only_css(theme: &arborium_theme::Theme, selector: &str) -> String {
    use arborium_theme::HIGHLIGHTS;
    use std::collections::{HashMap, HashSet};
    use std::fmt::Write;

    let mut css = String::new();
    writeln!(css, "{selector} {{").unwrap();

    // Build tag→style map for parent fallback (same logic as arborium's to_css)
    let mut tag_to_style: HashMap<&str, &arborium_theme::Style> = HashMap::new();
    for (i, def) in HIGHLIGHTS.iter().enumerate() {
        if !def.tag.is_empty() && !theme.styles[i].is_empty() {
            tag_to_style.insert(def.tag, &theme.styles[i]);
        }
    }

    let mut emitted: HashSet<&str> = HashSet::new();
    for (i, def) in HIGHLIGHTS.iter().enumerate() {
        if def.tag.is_empty() || emitted.contains(def.tag) {
            continue;
        }

        // Own style or parent fallback
        let style = if !theme.styles[i].is_empty() {
            &theme.styles[i]
        } else if !def.parent_tag.is_empty() {
            match tag_to_style.get(def.parent_tag) {
                Some(s) => s,
                None => continue,
            }
        } else {
            continue;
        };

        if style.is_empty() {
            continue;
        }

        emitted.insert(def.tag);

        write!(css, "  a-{} {{", def.tag).unwrap();
        if let Some(fg) = &style.fg {
            write!(css, " color: {};", fg.to_hex()).unwrap();
        }
        if let Some(bg) = &style.bg {
            write!(css, " background: {};", bg.to_hex()).unwrap();
        }
        let mut deco = Vec::new();
        if style.modifiers.underline {
            deco.push("underline");
        }
        if style.modifiers.strikethrough {
            deco.push("line-through");
        }
        if !deco.is_empty() {
            write!(css, " text-decoration: {};", deco.join(" ")).unwrap();
        }
        if style.modifiers.bold {
            write!(css, " font-weight: bold;").unwrap();
        }
        if style.modifiers.italic {
            write!(css, " font-style: italic;").unwrap();
        }
        writeln!(css, " }}").unwrap();
    }

    // Kode-specific overrides: punctuation and text.literal
    writeln!(css, "  a-p {{ color: var(--kode-fg-dim); }}").unwrap();
    writeln!(css, "  a-tl {{ color: var(--kode-code-fg); }}").unwrap();
    writeln!(css, "}}").unwrap();

    css
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_only_css_has_no_base_styles() {
        let arb = arborium_theme::builtin::tokyo_night();
        let css = token_only_css(&arb, "pre.test");

        for line in css.lines() {
            let t = line.trim();
            // No bare background, color, or CSS custom properties at root level
            assert!(
                !t.starts_with("background:"),
                "Found leaked base background: {t}"
            );
            assert!(
                !t.starts_with("color:"),
                "Found leaked base color: {t}"
            );
            assert!(
                !t.starts_with("--"),
                "Found leaked CSS custom property: {t}"
            );
        }
    }

    #[test]
    fn token_only_css_contains_token_rules() {
        let arb = arborium_theme::builtin::one_dark();
        let css = token_only_css(&arb, "pre.test");

        // Token rules present
        assert!(css.contains("a-k {"), "Missing keyword rule");
        assert!(css.contains("a-s {"), "Missing string rule");
        assert!(css.contains("a-f {"), "Missing function rule");
        // Selector wrapper
        assert!(css.starts_with("pre.test {"));
    }

    #[test]
    fn token_only_css_matches_arborium_token_output() {
        // The token rules in token_only_css should produce the same color
        // values as arborium's to_css for each a-* selector.
        let arb = arborium_theme::builtin::tokyo_night();
        let ours = token_only_css(&arb, "pre");
        let theirs = arb.to_css("pre");

        // Extract all "a-XX { ... }" lines from both
        let our_tokens: Vec<&str> = ours
            .lines()
            .filter(|l| l.trim().starts_with("a-"))
            .collect();
        let their_tokens: Vec<&str> = theirs
            .lines()
            .filter(|l| l.trim().starts_with("a-"))
            .collect();

        // Our output should have all of arborium's token rules (plus our overrides)
        for rule in &their_tokens {
            let tag = rule.trim().split_whitespace().next().unwrap();
            // Skip a-p and a-tl since we override them
            if tag == "a-p" || tag == "a-tl" {
                continue;
            }
            assert!(
                our_tokens.iter().any(|r| r.trim() == rule.trim()),
                "Missing token rule from arborium: {rule}"
            );
        }
    }

    #[test]
    fn syntax_css_includes_punctuation_and_literal_overrides() {
        let theme = Theme::tokyo_night();
        let css = theme.syntax_css("pre.test");
        assert!(
            css.contains("a-p { color: var(--kode-fg-dim); }"),
            "Missing punctuation override"
        );
        assert!(
            css.contains("a-tl { color: var(--kode-code-fg); }"),
            "Missing text.literal override"
        );
    }

    #[test]
    fn syntax_css_custom_passes_through_unchanged() {
        let custom_css = "pre { a-k { color: red; } }".to_string();
        let theme = Theme {
            syntax: SyntaxTheme::Custom(custom_css.clone()),
            ..Theme::tokyo_night()
        };
        assert_eq!(theme.syntax_css("pre"), custom_css);
    }

    #[test]
    fn all_builtin_themes_produce_token_css() {
        let themes = [
            SyntaxTheme::TokyoNight,
            SyntaxTheme::OneDark,
            SyntaxTheme::Dracula,
            SyntaxTheme::Nord,
            SyntaxTheme::GithubLight,
            SyntaxTheme::GruvboxDark,
            SyntaxTheme::CatppuccinMocha,
            SyntaxTheme::SolarizedDark,
            SyntaxTheme::SolarizedLight,
        ];
        for st in themes {
            let theme = Theme { syntax: st.clone(), ..Theme::tokyo_night() };
            let css = theme.syntax_css("pre");
            assert!(css.contains("a-k {"), "{st:?} missing keyword rule");
            // No leaked base styles
            for line in css.lines() {
                let t = line.trim();
                assert!(!t.starts_with("background:"), "{st:?} leaked background");
                assert!(!t.starts_with("color:"), "{st:?} leaked color");
                assert!(!t.starts_with("--"), "{st:?} leaked custom property");
            }
        }
    }
}
