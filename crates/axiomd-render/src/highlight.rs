//! Code-fence highlighting, class-based.
//!
//! Highlighting emits classes only — never an inline colour — so that a theme
//! change restyles the document without touching the parser or the DOM. Both
//! palettes ship in the stylesheet: the light one at the top level, the dark one in
//! a `prefers-color-scheme` block that overrides it.
//!
//! The syntax set is several megabytes of dumped Sublime grammars, so it is loaded
//! on first use rather than at startup, and only a fenced block naming a language
//! ever triggers that load.

use std::sync::OnceLock;

use syntect::html::{ClassStyle, ClassedHTMLGenerator, css_for_theme_with_class_style};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
use two_face::theme::EmbeddedThemeName;

/// Every class this module emits starts with this, so document classes and palette
/// classes cannot collide.
const CLASS_STYLE: ClassStyle = ClassStyle::SpacedPrefixed { prefix: "sy-" };

/// The light palette. InspiredGitHub reads as the familiar GitHub/Obsidian light
/// code block.
const LIGHT: EmbeddedThemeName = EmbeddedThemeName::InspiredGithub;
/// The dark palette, chosen to sit on Adwaita's dark view background.
const DARK: EmbeddedThemeName = EmbeddedThemeName::OneHalfDark;

/// Highlights `code` as `language`, or returns `None` when no grammar claims that
/// language — an unknown fence is rendered as plain text, never as an error.
pub(crate) fn highlight(language: &str, code: &str) -> Option<String> {
    let syntaxes = syntaxes();
    let syntax = syntaxes.find_syntax_by_token(language)?;
    let mut generator = ClassedHTMLGenerator::new_with_class_style(syntax, syntaxes, CLASS_STYLE);
    for line in LinesWithEndings::from(code) {
        // A grammar that fails mid-document would leave half-highlighted markup, so
        // the whole block falls back to plain text instead.
        generator
            .parse_html_for_line_which_includes_newline(line)
            .ok()?;
    }
    Some(generator.finalize())
}

/// The CSS both palettes need, light first and dark inside the media query that
/// overrides it.
pub(crate) fn palettes() -> String {
    let themes = two_face::theme::extra();
    let mut css = css_for_theme_with_class_style(themes.get(LIGHT), CLASS_STYLE)
        .expect("the light palette is a bundled theme");
    let dark = css_for_theme_with_class_style(themes.get(DARK), CLASS_STYLE)
        .expect("the dark palette is a bundled theme");
    css.push_str("\n@media (prefers-color-scheme: dark) {\n");
    css.push_str(&dark);
    css.push_str("}\n");
    css
}

fn syntaxes() -> &'static SyntaxSet {
    static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAXES.get_or_init(two_face::syntax::extra_newlines)
}
