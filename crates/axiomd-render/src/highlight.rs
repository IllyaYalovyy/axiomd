//! Code-fence highlighting, class-based.
//!
//! Highlighting emits classes only — never an inline colour — so that a theme
//! change restyles the document without touching the parser or the DOM. Both
//! palettes ship in the stylesheet: the light one at the top level, the dark one in
//! a `prefers-color-scheme` block that overrides it — on screen only, so that code
//! is as light on paper and in an exported file as the rest of the document.
//!
//! Which of the two a reader is reading in is not left to that query, though: the
//! application states it, and states this module's palette for it, because an engine
//! that answers the query once and never again would leave a dark reader with light
//! code (`palette.rs`, issue #60).
//!
//! The syntax set is several megabytes of dumped Sublime grammars, so it is loaded
//! on first use rather than at startup, and only a fenced block naming a language
//! ever triggers that load.

use std::sync::OnceLock;

use syntect::html::{ClassStyle, ClassedHTMLGenerator, css_for_theme_with_class_style};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
use two_face::theme::EmbeddedThemeName;

use crate::Palette;

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

/// The CSS both palettes need, light first and dark inside the screen-only media
/// query that overrides it.
pub(crate) fn palettes() -> String {
    let mut css = palette(Palette::Light);
    css.push_str("\n@media screen and (prefers-color-scheme: dark) {\n");
    css.push_str(&palette(Palette::Dark));
    css.push_str("}\n");
    css
}

/// One palette alone: the light one is the whole of what a printed or exported
/// document needs, since neither of those follows anybody's colour scheme, and either
/// of them is what the application states while a reader is reading in it
/// ([`crate::Reading::stylesheet`]).
pub(crate) fn palette(palette: Palette) -> String {
    let theme = match palette {
        Palette::Light => LIGHT,
        Palette::Dark => DARK,
    };
    css_for_theme_with_class_style(two_face::theme::extra().get(theme), CLASS_STYLE)
        .expect("a palette is a bundled theme")
}

fn syntaxes() -> &'static SyntaxSet {
    static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAXES.get_or_init(two_face::syntax::extra_newlines)
}
