//! Math: `$inline$` and `$$display$$` LaTeX typeset in the document (issue #11).
//!
//! The plugin that proves a capability can be *complete in Rust*: the equation the
//! reader looks at is MathML written here, laid out by WebKitGTK's own maths engine.
//! Nothing is fetched, nothing is drawn by a script, and the page a document with
//! twenty equations in it is displayed as is exactly as inert as one with none — which
//! is the whole reason MathML was chosen over a JavaScript typesetter
//! (`design_decisions.md`: zero implicit network, and a document that cannot run code).
//!
//! # What the reader sees when the LaTeX is wrong
//!
//! Their own source, marked as an error, with the reason beside it — never a blank
//! space, never a lost sentence, never a dialog (invariant 12). That is also what
//! Obsidian's MathJax does with an unreadable formula, and matching what Obsidian
//! shows is the point of the extension surface (`VISION.md`).
//!
//! # The source travels with the equation
//!
//! Every equation carries its LaTeX in the `<annotation encoding="application/x-tex">`
//! the MathML specification keeps for exactly that, which is what KaTeX, MathJax and
//! therefore Obsidian all write. It is not shown — the user-agent stylesheet hides
//! every child of `<semantics>` but the first — and it is why an equation in a heading
//! still has words in the outline, and why the search never finds the same formula
//! twice.
//!
//! # The library
//!
//! `pulldown-latex` 0.8, MIT, one dependency of its own (`bumpalo`), renders straight
//! to MathML Core. The issue named it first and it was found sufficient for the whole
//! corpus in `tests/golden/math.md`, so the alternative it named — `math-core` — was
//! not needed. It would also have been the heavier of the two: seven transitive crates
//! including a proc-macro and a second renderer crate, against one. The comparison is
//! recorded in this task's report.
//!
//! The font is STIX Two Math 2.13 b171 (SIL Open Font License 1.1, the licence copied
//! beside it), converted from the release OTF to WOFF2. OFL was preferred to the GUST
//! licence Latin Modern Math carries, as the issue asks.

use std::borrow::Cow;

use axiomd_engine::Event;
use axiomd_i18n::{gettext, gettext_noop};
use pulldown_latex::config::DisplayMode;
use pulldown_latex::{Parser, ParserError, RenderConfig, Storage, push_mathml};

use super::{Asset, Manifest, PLUGIN_API, Plugin, STYLESHEET};
use crate::body::escape_text;

/// The face every equation is set in, carried rather than named: see `math.css`.
const FONT: Asset = Asset {
    name: "stix-two-math.woff2",
    content_type: "font/woff2",
    bytes: include_bytes!("../../assets/plugin/stix-two-math.woff2"),
};

/// How an equation sits in the page, and what the renderer's tables need to line up.
const STYLE: Asset = Asset {
    name: "math.css",
    content_type: STYLESHEET,
    bytes: include_bytes!("../../assets/plugin/math.css"),
};

const MANIFEST: Manifest = Manifest {
    api: PLUGIN_API,
    id: "math",
    name: gettext_noop("Math"),
    description: gettext_noop("Typeset $inline$ and $$display$$ LaTeX, bundled and offline."),
    fences: &[],
    assets: &[STYLE, FONT],
};

/// The plugin itself, which holds nothing: the library and the font are compiled in.
pub(super) struct Math;

impl Plugin for Math {
    fn manifest(&self) -> &'static Manifest {
        &MANIFEST
    }

    /// Turns one math span into the equation it means, and leaves everything else
    /// alone.
    ///
    /// The events are written where the original was, so an equation cannot move a
    /// block or cost it its line however long the markup it becomes is (invariant 3).
    fn rewrite<'a>(&self, event: &Event<'a>) -> Option<Vec<Event<'a>>> {
        let Event::Math { display, latex } = event else {
            return None;
        };
        Some(typeset(*display, latex))
    }
}

/// One equation, as the events that replace it.
///
/// Always three of them, and the middle one is always the author's LaTeX as *text*:
/// either the annotation nobody sees, or — when the LaTeX could not be read — the
/// source itself, shown. Writing it as text rather than into the markup is what keeps
/// it in the outline of a heading and in the alt text of a picture, exactly as it was
/// before an equation was anything more than its source.
fn typeset<'a>(display: bool, latex: &str) -> Vec<Event<'a>> {
    let (before, after) = match mathml(display, latex) {
        // The `<mrow>` is not decoration: `<semantics>` draws its *first* child and
        // hides the rest, which is how the annotation stays out of sight — so the
        // equation has to arrive as one element. Without it a formula of more than one
        // top-level term is drawn down to its first term and the reader loses the rest
        // (seen on WebKitGTK 2.52.5: an integral rendered as its integral sign alone).
        // It is also what MathJax and KaTeX write, for the same reason.
        Ok((open, inner)) => (
            format!(
                "{open}<semantics><mrow>{inner}</mrow>\
                 <annotation encoding=\"application/x-tex\">"
            ),
            "</annotation></semantics></math>".to_owned(),
        ),
        Err(reason) => (
            format!(
                "<span class=\"math-error math-error-{kind}\"><span class=\"math-error-source\">",
                kind = if display { "display" } else { "inline" },
            ),
            format!(
                "</span><span class=\"math-error-reason\">{}</span></span>",
                escape_text(&reason),
            ),
        ),
    };
    vec![
        Event::InlineHtml(Cow::Owned(before)),
        Event::Text(Cow::Owned(latex.to_owned())),
        Event::InlineHtml(Cow::Owned(after)),
    ]
}

/// The equation as `(opening tag, everything inside it)`, or the one-line reason it
/// could not be read.
///
/// Split rather than handed back whole because the annotation goes between them, and
/// the split is a parse of the renderer's own output rather than of anything a
/// document wrote: markup that is not the `<math>…</math>` this library documents
/// itself as writing is treated as a failure, so a change in it degrades to the
/// source instead of reaching the page half-formed.
fn mathml(display: bool, latex: &str) -> Result<(String, String), String> {
    let storage = Storage::new();
    let events = Parser::new(latex, &storage)
        .collect::<Result<Vec<_>, ParserError>>()
        .map_err(one_line)?;
    let config = RenderConfig {
        display_mode: match display {
            true => DisplayMode::Block,
            false => DisplayMode::Inline,
        },
        ..RenderConfig::default()
    };
    let mut markup = String::new();
    push_mathml(
        &mut markup,
        events.into_iter().map(Ok::<_, ParserError>),
        config,
    )
    .map_err(|error| error.to_string())?;
    let (open, rest) = markup
        .split_once('>')
        .ok_or_else(|| gettext("the equation could not be typeset"))?;
    let inner = rest
        .strip_suffix("</math>")
        .ok_or_else(|| gettext("the equation could not be typeset"))?;
    Ok((format!("{open}>"), inner.to_owned()))
}

/// A parser failure as one sentence the reader can act on.
///
/// The library's own message is a paragraph: a summary line, then a drawing of where
/// in the source it stopped. The drawing is for a terminal, and the badge beside an
/// equation is not one, so only the summary is kept.
fn one_line(error: ParserError) -> String {
    let whole = error.to_string();
    let first = whole.lines().next().unwrap_or_default();
    first
        .strip_prefix("parsing error: ")
        .unwrap_or(first)
        .to_owned()
}
