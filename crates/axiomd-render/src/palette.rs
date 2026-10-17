//! Which of the two palettes a document is read in, said by the application.
//!
//! The document's own stylesheet carries both palettes and asks the page which one is
//! wanted, with `prefers-color-scheme` ([`crate::stylesheet`]) — and the answer an
//! engine gives that query is libadwaita's, which is the source of truth for light and
//! dark (`ux_decisions.md`). WebKitGTK 2.54 stopped giving it: it reads the desktop's
//! colour scheme once, when a web process starts, and never again (WebKit bug 327072,
//! fixed upstream in 323359@main and in no release the GNOME 49 runtime carries). A
//! packaged window therefore followed the desktop into dark while the document inside
//! it stayed white — issue #60, and the same for a reader switching the theme by hand
//! while reading.
//!
//! So the application says which palette is in force rather than asking the page to
//! work it out, exactly as it already says `prefers-contrast` and
//! `prefers-reduced-motion` ([`crate::Reading::stylesheet`]). It is said in the
//! reader's own stylesheet, which is swapped live, so a palette change still costs no
//! parse, no render and no page load (invariant 9). It is said in *both* directions,
//! because a page whose answer is stuck is as readily stuck at dark as at light — a
//! window opened on a dark desktop and taken back to light is the other half of the
//! same defect.
//!
//! Four things carry a palette, because a reader sees all four:
//!
//!   * the custom properties the document is painted from — exactly the ones the dark
//!     palette repaints, each given the value this palette gives it;
//!   * the code palette, whose colours are rules rather than properties
//!     ([`crate::highlight`]);
//!   * `color-scheme`, which is what the engine paints the furniture it owns with:
//!     scrollbars, and the canvas under a page that has not drawn yet;
//!   * the width of the one box on the page a drawing plugin can watch. A page cannot
//!     listen for a stylesheet changing — a rendered document runs no event listener at
//!     all — but it can observe a box that resizes, and `mermaid-view.js` draws its
//!     diagrams in the palette that box reports.
//!
//! Every declaration is important. Under the cascade (CSS Cascading and Inheritance
//! Level 5, §6.2) a normal user declaration loses to the document's own author one, so
//! an unimportant palette would be a palette that does nothing. And all of it is
//! `screen`-only, like the `dark.css` block it stands in for: a print job and an
//! exported PDF are paginated from the very page on screen with this stylesheet
//! installed, and paper follows nobody's colour scheme.

use std::sync::OnceLock;

use crate::{Palette, highlight};

/// The light document: where every property's light value is declared.
const LIGHT_CSS: &str = include_str!("../assets/axiomd.css");

/// The dark palette: both *which* properties a palette consists of and their dark
/// values. A property added here is stated in both palettes from then on, with no
/// second list to keep in step.
const DARK_CSS: &str = include_str!("../assets/dark.css");

/// The one box on the page whose size says which palette is in force, as a drawing
/// plugin reads it (`assets/plugin/mermaid-view.js`).
const PROBE: &str = ".axiomd-scheme";

/// The stylesheet that puts `palette` in force over the document's own.
///
/// Built once per palette per process: it is two syntect themes' worth of CSS, and it
/// is installed again every time anything about the way the reader reads changes.
pub(crate) fn stated(palette: Palette) -> &'static str {
    static LIGHT: OnceLock<String> = OnceLock::new();
    static DARK: OnceLock<String> = OnceLock::new();
    match palette {
        Palette::Light => LIGHT.get_or_init(|| written(Palette::Light)),
        Palette::Dark => DARK.get_or_init(|| written(Palette::Dark)),
    }
}

/// `palette`, written out as the four things that carry it.
fn written(palette: Palette) -> String {
    let (scheme, probe) = match palette {
        Palette::Light => ("light", 1),
        Palette::Dark => ("dark", 2),
    };
    let mut css = format!("@media screen {{\n  :root {{\n    color-scheme: {scheme} !important;\n");
    for (property, value) in properties(palette) {
        css.push_str(&format!("    {property}: {value} !important;\n"));
    }
    css.push_str("  }\n\n");
    css.push_str(&important(&highlight::palette(palette)));
    css.push_str(&format!(
        "\n  {PROBE} {{ width: {probe}px !important; }}\n}}\n"
    ));
    css
}

/// What `palette` paints each of the document's colour properties.
///
/// The properties are the dark palette's, in the order it declares them, because that
/// is what "a palette" means here: the set of properties the two readings disagree
/// about. The light reading of each is the value the light document declares — so the
/// two statements cover exactly the same properties, and going back to light undoes
/// every declaration going dark made rather than most of them.
fn properties(palette: Palette) -> Vec<(String, String)> {
    let light = declared(LIGHT_CSS);
    declared(DARK_CSS)
        .into_iter()
        .map(|(property, dark)| match palette {
            Palette::Dark => (property, dark),
            Palette::Light => {
                let value = light
                    .iter()
                    .find(|(declared, _)| *declared == property)
                    .map(|(_, value)| value.clone())
                    .unwrap_or_else(|| {
                        panic!(
                            "the dark palette repaints {property}, which the light \
                             document does not declare, so there is no light reading \
                             of it to go back to"
                        )
                    });
                (property, value)
            }
        })
        .collect()
}

/// The custom properties the first `:root` rule of `css` declares, in order.
///
/// The first `:root` is the palette of every stylesheet here: `axiomd.css` declares
/// the light document's and `dark.css` the dark one's, both before anything else they
/// say. Comments go first, so prose about a colour is never read as one.
fn declared(css: &str) -> Vec<(String, String)> {
    let css = without_comments(css);
    let at = css.find(":root").expect("a stylesheet declares a palette");
    let open = css[at..].find('{').expect("a `:root` rule with a body") + at + 1;
    let close = css[open..]
        .find('}')
        .expect("a `:root` rule that is closed")
        + open;
    css[open..close]
        .split(';')
        .filter_map(|declaration| {
            let (property, value) = declaration.split_once(':')?;
            let property = property.trim();
            property
                .starts_with("--")
                .then(|| (property.to_owned(), value.trim().to_owned()))
        })
        .collect()
}

/// `css` with every declaration in it made important, and nothing else about it
/// changed.
///
/// For the code palette, which is generated from a syntect theme and so cannot simply
/// be written important in the first place. Comments are left exactly as they are: a
/// declaration inside one is prose, and making prose important would put `!important`
/// into the middle of a sentence.
fn important(css: &str) -> String {
    let mut written = String::with_capacity(css.len() + css.matches(';').count() * 11);
    let mut rest = css;
    let (mut depth, mut pending) = (0usize, false);
    while let Some(next) = rest.chars().next() {
        if let Some(comment) = rest.strip_prefix("/*") {
            let end = comment.find("*/").map_or(comment.len(), |at| at + 2);
            written.push_str(&rest[..end + 2]);
            rest = &comment[end..];
            continue;
        }
        match next {
            '{' => {
                depth += 1;
                pending = false;
            }
            // A declaration the generator left without a closing semicolon still has
            // to be made important, or it is a colour the reader never sees. It ends
            // where its last word does, not where the line does.
            '}' => {
                if pending {
                    let ended = written.trim_end().len();
                    let after = written.split_off(ended);
                    written.push_str(" !important");
                    written.push_str(&after);
                }
                depth = depth.saturating_sub(1);
                pending = false;
            }
            ';' if depth > 0 => {
                written.push_str(" !important");
                pending = false;
            }
            character if depth > 0 && !character.is_whitespace() => pending = true,
            _ => {}
        }
        written.push(next);
        rest = &rest[next.len_utf8()..];
    }
    written
}

/// `css` with its comments taken out.
fn without_comments(css: &str) -> String {
    let mut written = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(at) = rest.find("/*") {
        written.push_str(&rest[..at]);
        let comment = &rest[at + 2..];
        rest = comment.find("*/").map_or("", |end| &comment[end + 2..]);
    }
    written.push_str(rest);
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rewriter the code palette goes through, over the shapes a generated
    /// stylesheet comes in: several declarations to a rule, a rule's last declaration
    /// left without its semicolon, and a comment that must come out the other side as
    /// the sentence it was.
    #[test]
    fn every_declaration_comes_out_important_and_nothing_else_changes() {
        let rewritten = important(
            "/* a theme; generated */\n.sy-code {\n color: #111;\n background-color: #222;\n}\n\
             .sy-keyword {\n color: #333\n}\n",
        );

        assert_eq!(
            rewritten,
            "/* a theme; generated */\n.sy-code {\n color: #111 !important;\n \
             background-color: #222 !important;\n}\n.sy-keyword {\n color: #333 !important\n}\n",
        );
    }

    /// A selector is not a declaration, and neither is the prelude of an at-rule: a
    /// `!important` outside a rule's body would make the stylesheet itself invalid.
    #[test]
    fn nothing_outside_a_rule_is_made_important() {
        let rewritten = important("@media screen {\n.sy-code {\n color: #111;\n}\n}\n");

        assert_eq!(
            rewritten,
            "@media screen {\n.sy-code {\n color: #111 !important;\n}\n}\n",
        );
    }

    /// What a palette consists of: the properties the dark reading repaints, and
    /// nothing a reader chose for themselves. The measure in particular is the
    /// reader's own preference, and a palette that restated it would pin every
    /// document to the default width.
    #[test]
    fn a_palette_is_colours_and_never_the_readers_own_choices() {
        let dark = declared(DARK_CSS);
        assert!(
            dark.len() >= 20,
            "the dark palette has gone missing: {dark:?}"
        );
        for (property, _) in &dark {
            assert!(
                property.starts_with("--axiomd-"),
                "{property} is not one of the document's own properties",
            );
            assert_ne!(
                property, "--axiomd-reading-width",
                "a palette repaints the measure the reader chose",
            );
        }
    }
}
