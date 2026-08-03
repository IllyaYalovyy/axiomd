//! What a callout kind means: what it is called, what colour it is, and its icon.
//!
//! Obsidian's vocabulary is open — an author may write `> [!anything]` — so the parser
//! carries the kind through as written (`axiomd_engine::Callout`) and this module is
//! the whole of what axiomd knows about it. Three answers, from one table:
//!
//! * **A class**, so the stylesheet colours it. Aliases collapse onto the kind they
//!   are an alias of, and a kind nothing here knows falls back to `note` — the issue
//!   #12 rule, and the only way an unknown kind can look like anything at all.
//! * **A title**, when the author wrote none: the kind as they wrote it, capitalised,
//!   which is both what Obsidian shows for a known kind and what "the literal kind as
//!   title" means for an unknown one. One rule covers both.
//! * **An icon**, one of a bundled Lucide subset (`assets/icon`, ISC — see the LICENSE
//!   beside them). They reach the document through the stylesheet rather than the
//!   markup, so a document carries no inline SVG and the sanitiser's tag allowlist
//!   does not have to grow to admit one.

use crate::plugin::Asset;

/// The path bundled icons are served under, below `axiomd://assets`.
const ICON_PREFIX: &str = "/icon/";

/// One callout kind axiomd knows: what it is called in the stylesheet, and what it
/// looks like.
struct Known {
    /// The canonical name — the class the document carries, and the icon's file name
    /// without its extension.
    name: &'static str,
    /// The kinds an author may write for it, canonical name included.
    spellings: &'static [&'static str],
    /// The Lucide icon, by file name.
    icon: &'static str,
}

/// Obsidian's callout vocabulary, in the order the stylesheet lists it.
///
/// The spellings are Obsidian's own aliases: `> [!tldr]` and `> [!summary]` are the
/// same callout as `> [!abstract]`, and only the *title* tells them apart.
const KNOWN: &[Known] = &[
    Known {
        name: "note",
        spellings: &["note"],
        icon: "pencil",
    },
    Known {
        name: "abstract",
        spellings: &["abstract", "summary", "tldr"],
        icon: "clipboard-list",
    },
    Known {
        name: "info",
        spellings: &["info"],
        icon: "info",
    },
    Known {
        name: "todo",
        spellings: &["todo"],
        icon: "circle-check",
    },
    Known {
        name: "tip",
        spellings: &["tip", "hint", "important"],
        icon: "flame",
    },
    Known {
        name: "success",
        spellings: &["success", "check", "done"],
        icon: "check",
    },
    Known {
        name: "question",
        spellings: &["question", "help", "faq"],
        icon: "circle-question-mark",
    },
    Known {
        name: "warning",
        spellings: &["warning", "caution", "attention"],
        icon: "triangle-alert",
    },
    Known {
        name: "failure",
        spellings: &["failure", "fail", "missing"],
        icon: "x",
    },
    Known {
        name: "danger",
        spellings: &["danger", "error"],
        icon: "zap",
    },
    Known {
        name: "bug",
        spellings: &["bug"],
        icon: "bug",
    },
    Known {
        name: "example",
        spellings: &["example"],
        icon: "list",
    },
    Known {
        name: "quote",
        spellings: &["quote", "cite"],
        icon: "quote",
    },
];

/// Every icon compiled into the pipeline, by file name.
///
/// The fold arrow is here beside the callout icons because it is one too: a foldable
/// callout is a `<details>`, and the marker a browser would draw for it is replaced so
/// that folding looks the same on every platform.
const ICONS: &[Asset] = &[
    icon("pencil", include_bytes!("../assets/icon/pencil.svg")),
    icon(
        "clipboard-list",
        include_bytes!("../assets/icon/clipboard-list.svg"),
    ),
    icon("info", include_bytes!("../assets/icon/info.svg")),
    icon(
        "circle-check",
        include_bytes!("../assets/icon/circle-check.svg"),
    ),
    icon("flame", include_bytes!("../assets/icon/flame.svg")),
    icon("check", include_bytes!("../assets/icon/check.svg")),
    icon(
        "circle-question-mark",
        include_bytes!("../assets/icon/circle-question-mark.svg"),
    ),
    icon(
        "triangle-alert",
        include_bytes!("../assets/icon/triangle-alert.svg"),
    ),
    icon("x", include_bytes!("../assets/icon/x.svg")),
    icon("zap", include_bytes!("../assets/icon/zap.svg")),
    icon("bug", include_bytes!("../assets/icon/bug.svg")),
    icon("list", include_bytes!("../assets/icon/list.svg")),
    icon("quote", include_bytes!("../assets/icon/quote.svg")),
    icon(
        "chevron-right",
        include_bytes!("../assets/icon/chevron-right.svg"),
    ),
];

const fn icon(name: &'static str, bytes: &'static [u8]) -> Asset {
    Asset {
        name,
        content_type: "image/svg+xml",
        bytes,
    }
}

/// The class a callout of `kind` carries — its canonical name, or `note` for a kind
/// axiomd does not know.
pub(crate) fn class_of(kind: &str) -> &'static str {
    KNOWN
        .iter()
        .find(|known| known.spellings.contains(&kind))
        .map(|known| known.name)
        .unwrap_or("note")
}

/// What a callout of `kind` is called when its author gave it no title: the kind,
/// capitalised, whether or not axiomd has ever heard of it.
pub(crate) fn title_of(kind: &str) -> String {
    let mut characters = kind.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
        None => String::new(),
    }
}

/// The icon file `path` names under `axiomd://assets`, or `None` for a path that names
/// none.
pub(crate) fn asset(path: &str) -> Option<Asset> {
    let name = path.strip_prefix(ICON_PREFIX)?;
    ICONS.iter().find(|icon| icon.name == name).copied()
}

/// The styling that puts an icon on every callout, written against `uri` — which is
/// how the same rules serve a document on screen (where an icon is a file the app
/// answers for) and one in a file (where it has to travel inside the document).
///
/// Generated rather than written out, so the kinds this module knows and the kinds the
/// stylesheet draws cannot drift apart, and so an icon nothing references cannot be
/// shipped.
pub(crate) fn icon_styling(uri: &dyn Fn(&Asset) -> String) -> String {
    let mut styling = String::new();
    for known in KNOWN {
        let Some(asset) = ICONS.iter().find(|icon| icon.name == known.icon) else {
            continue;
        };
        styling.push_str(&mask_rule(
            &format!(".markdown .callout-{} > .callout-title::before", known.name),
            &uri(asset),
        ));
    }
    if let Some(arrow) = ICONS.iter().find(|icon| icon.name == "chevron-right") {
        styling.push_str(&mask_rule(
            ".markdown details.callout > .callout-title::after",
            &uri(arrow),
        ));
    }
    styling
}

/// One icon, as the alpha of a shape filled with the text colour — so an icon is the
/// colour of the callout it belongs to, in light, in dark and in high contrast alike,
/// without a second copy of the file per palette.
fn mask_rule(selector: &str, uri: &str) -> String {
    format!(
        "{selector} {{\n  \
         -webkit-mask-image: url(\"{uri}\");\n  \
         mask-image: url(\"{uri}\");\n\
         }}\n"
    )
}

/// The URI the app's own scheme answers for one bundled icon.
pub(crate) fn icon_uri(asset: &Asset) -> String {
    format!("axiomd://assets{ICON_PREFIX}{name}", name = asset.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every kind in the issue #12 list, and the rule that makes an unknown one
    /// render at all.
    #[test]
    fn every_obsidian_kind_has_a_class_and_an_unknown_one_falls_back_to_note() {
        assert_eq!(class_of("note"), "note");
        assert_eq!(class_of("tldr"), "abstract");
        assert_eq!(class_of("summary"), "abstract");
        assert_eq!(class_of("hint"), "tip");
        assert_eq!(class_of("important"), "tip");
        assert_eq!(class_of("done"), "success");
        assert_eq!(class_of("faq"), "question");
        assert_eq!(class_of("attention"), "warning");
        assert_eq!(class_of("missing"), "failure");
        assert_eq!(class_of("error"), "danger");
        assert_eq!(class_of("cite"), "quote");
        assert_eq!(class_of("bug"), "bug");
        assert_eq!(class_of("example"), "example");

        assert_eq!(class_of("nonsense"), "note", "an unknown kind lost its box");
        assert_eq!(class_of(""), "note");
    }

    /// The default title is the kind the author wrote — which is also what "the
    /// literal kind as title" means for one nobody knows.
    #[test]
    fn a_callout_with_no_title_is_called_after_its_kind() {
        assert_eq!(title_of("note"), "Note");
        assert_eq!(title_of("tldr"), "Tldr");
        assert_eq!(title_of("bug"), "Bug");
        assert_eq!(title_of("nonsense"), "Nonsense");
        assert_eq!(title_of(""), "");
    }

    /// Every icon the styling references is a file that shipped, and every file that
    /// shipped is referenced — a missing one is a callout with a hole where its icon
    /// should be, and a spare one is dead weight in the binary.
    #[test]
    fn every_kind_has_a_bundled_icon_and_no_icon_is_unused() {
        for known in KNOWN {
            assert!(
                ICONS.iter().any(|icon| icon.name == known.icon),
                "{} has no bundled icon called {}",
                known.name,
                known.icon,
            );
        }
        for icon in ICONS {
            let used =
                KNOWN.iter().any(|known| known.icon == icon.name) || icon.name == "chevron-right";
            assert!(used, "{} is bundled and nothing uses it", icon.name);
            assert!(
                icon.bytes.starts_with(b"<svg"),
                "{} is not an SVG",
                icon.name,
            );
        }
    }

    /// A kind is spelled once. Two entries claiming the same spelling would make
    /// which one wins depend on table order rather than on anything anybody decided.
    #[test]
    fn no_two_kinds_claim_the_same_spelling() {
        let mut seen: Vec<&str> = Vec::new();
        for known in KNOWN {
            assert!(
                known.spellings.contains(&known.name),
                "{} cannot be written as itself",
                known.name,
            );
            for spelling in known.spellings {
                assert!(!seen.contains(spelling), "{spelling} is claimed twice");
                seen.push(spelling);
            }
        }
    }

    /// The icons a document names are the ones the app can answer for.
    #[test]
    fn every_icon_the_styling_names_is_served_under_its_own_uri() {
        let styling = icon_styling(&icon_uri);
        for icon in ICONS {
            let uri = icon_uri(icon);
            assert!(styling.contains(&uri), "{uri} is in no rule");
            let path = uri.strip_prefix("axiomd://assets").expect("an asset path");
            assert_eq!(
                asset(path).map(|served| served.bytes),
                Some(icon.bytes),
                "{path} is named by the stylesheet and served by nothing",
            );
        }
        assert_eq!(asset("/icon/nothing"), None);
        assert_eq!(asset("/plugin/mermaid/mermaid.css"), None);
    }
}
