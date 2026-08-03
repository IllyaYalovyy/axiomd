//! The engine boundary contract: what an engine advertises must be what it parses.

mod support;

use axiomd_engine::{
    Alignment, ComrakEngine, Event, Extension, Extensions, MarkdownEngine, Parsed, Tag,
};

/// A document that only parses into `marker` when `extension` is recognised.
struct Probe {
    extension: Extension,
    markdown: &'static str,
    marker: fn(&Parsed<'_>) -> bool,
}

fn has_event(parsed: &Parsed<'_>, predicate: impl Fn(&Event<'_>) -> bool) -> bool {
    parsed.events().iter().any(|e| predicate(&e.event))
}

const PROBES: &[Probe] = &[
    Probe {
        extension: Extension::Tables,
        markdown: "| a | b |\n| - | - |\n| c | d |\n",
        marker: |p| has_event(p, |e| matches!(e, Event::Start(Tag::Table { .. }))),
    },
    Probe {
        extension: Extension::TaskLists,
        markdown: "- [x] done\n",
        marker: |p| {
            has_event(p, |e| {
                matches!(e, Event::Start(Tag::Item { task: Some(_) }))
            })
        },
    },
    Probe {
        extension: Extension::Strikethrough,
        markdown: "~~gone~~\n",
        marker: |p| has_event(p, |e| matches!(e, Event::Start(Tag::Strikethrough))),
    },
    Probe {
        extension: Extension::Autolinks,
        markdown: "see www.example.com now\n",
        marker: |p| has_event(p, |e| matches!(e, Event::Start(Tag::Link { .. }))),
    },
    Probe {
        extension: Extension::Footnotes,
        markdown: "text[^a]\n\n[^a]: the note\n",
        marker: |p| has_event(p, |e| matches!(e, Event::FootnoteReference(_))),
    },
    Probe {
        extension: Extension::Math,
        markdown: "the value $x^2$ here\n",
        marker: |p| has_event(p, |e| matches!(e, Event::Math { .. })),
    },
    Probe {
        extension: Extension::WikiLinks,
        markdown: "[[Target]]\n",
        marker: |p| has_event(p, |e| matches!(e, Event::Start(Tag::WikiLink { .. }))),
    },
    Probe {
        extension: Extension::Callouts,
        markdown: "> [!NOTE]\n> body\n",
        marker: |p| {
            has_event(p, |e| {
                matches!(e, Event::Start(Tag::BlockQuote { callout: Some(_) }))
            })
        },
    },
    Probe {
        extension: Extension::FrontMatter,
        markdown: "---\ntitle: x\n---\n\nbody\n",
        marker: |p| p.front_matter().is_some(),
    },
];

/// Every extension the engine advertises is really parsed, and nothing it advertises
/// leaks into strict CommonMark mode. Together these pin the capability report to
/// observable behaviour rather than to a hand-maintained list.
#[test]
fn engine_parses_exactly_the_extensions_it_advertises() {
    let engine = ComrakEngine::new();
    let capabilities = engine.capabilities();

    assert_eq!(
        PROBES.len(),
        Extension::ALL.len(),
        "every Extension needs a probe document"
    );

    for probe in PROBES {
        let advertised = capabilities.contains(probe.extension);
        let parsed = engine.parse(probe.markdown, Extensions::FULL);
        assert_eq!(
            (probe.marker)(&parsed),
            advertised,
            "{:?}: capabilities() says {advertised} but parsing {:?} disagrees",
            probe.extension,
            probe.markdown
        );

        let strict = engine.parse(probe.markdown, Extensions::COMMONMARK);
        assert!(
            !(probe.marker)(&strict),
            "{:?} was parsed even though only CommonMark was requested",
            probe.extension
        );
    }
}

/// Requesting an extension set narrower than the engine's capabilities parses only
/// that set.
#[test]
fn requesting_a_narrower_set_narrows_the_parse() {
    let engine = ComrakEngine::new();
    let markdown = "~~gone~~ and $x^2$\n";

    let gfm = engine.parse(markdown, Extensions::GFM);
    assert!(has_event(&gfm, |e| matches!(
        e,
        Event::Start(Tag::Strikethrough)
    )));
    assert!(
        !has_event(&gfm, |e| matches!(e, Event::Math { .. })),
        "math is not in the GFM set but was parsed anyway"
    );
}

/// Extension sets are ordinary sets, and their debug form names their members so a
/// failing conformance run is readable.
#[test]
fn extension_sets_compose() {
    assert!(Extensions::GFM.contains(Extension::Tables));
    assert!(!Extensions::GFM.contains(Extension::Math));
    assert!(Extensions::FULL.contains(Extension::Math));
    assert_eq!(Extensions::COMMONMARK.iter().count(), 0);
    assert_eq!(Extensions::FULL.iter().count(), Extension::ALL.len());
    assert_eq!(
        (Extensions::COMMONMARK | Extension::Math).intersection(Extensions::GFM),
        Extensions::COMMONMARK
    );
    assert_eq!(
        format!("{:?}", Extensions::from(Extension::Tables)),
        "{Tables}"
    );
}

/// Front matter is metadata: it is exposed separately and never appears as content.
#[test]
fn front_matter_is_metadata_not_content() {
    let engine = ComrakEngine::new();
    let source = "---\ntitle: Notes\n---\n\n# Heading\n";
    let parsed = engine.parse(source, Extensions::FULL);

    assert_eq!(parsed.front_matter(), Some("---\ntitle: Notes\n---"));
    assert!(
        !has_event(
            &parsed,
            |e| matches!(e, Event::Text(t) if t.contains("title"))
        ),
        "front matter leaked into the event stream"
    );
    let heading = parsed
        .events()
        .iter()
        .find(|e| matches!(e.event, Event::Start(Tag::Heading { .. })))
        .expect("heading after front matter");
    assert_eq!(&source[heading.span.range.clone()], "# Heading");
}

/// A document with no front matter reports none — the non-happy path for metadata.
#[test]
fn absent_front_matter_is_none() {
    let engine = ComrakEngine::new();
    assert_eq!(
        engine.parse("# Heading\n", Extensions::FULL).front_matter(),
        None
    );
    assert_eq!(
        engine
            .parse("---\ntitle: x\n---\n", Extensions::COMMONMARK)
            .front_matter(),
        None,
        "front matter was extracted without the extension being requested"
    );
}

/// Callout kind, author title and fold marker survive the boundary — for the kinds
/// GitHub knows and for the ones only Obsidian does, which is the whole point of
/// carrying the kind as the author wrote it (issue #12).
#[test]
fn callouts_carry_kind_title_and_fold() {
    let engine = ComrakEngine::new();
    let callout = |source: &str| {
        let parsed = engine.parse(source, Extensions::FULL);
        let Some(Event::Start(Tag::BlockQuote {
            callout: Some(callout),
        })) = parsed.events().first().map(|e| &e.event)
        else {
            panic!("expected a callout block quote, got {:?}", parsed.events());
        };
        (
            callout.kind.to_string(),
            callout.title.as_deref().map(str::to_owned),
            callout.fold,
        )
    };

    assert_eq!(
        callout("> [!WARNING] Mind the gap\n> body\n"),
        ("warning".to_owned(), Some("Mind the gap".to_owned()), None),
    );
    assert_eq!(
        callout("> [!bug]\n> body\n"),
        ("bug".to_owned(), None, None),
    );
    assert_eq!(
        callout("> [!tldr]- Folded away\n> body\n"),
        (
            "tldr".to_owned(),
            Some("Folded away".to_owned()),
            Some(false)
        ),
    );
    assert_eq!(
        callout("> [!question]+ Open\n> body\n"),
        ("question".to_owned(), Some("Open".to_owned()), Some(true)),
    );
}

/// The marker is not the quote's first sentence: a reader must not be shown
/// `[!note]` above the body, and the body itself must survive intact.
#[test]
fn a_callout_marker_leaves_the_quotes_own_prose_alone() {
    let engine = ComrakEngine::new();
    let parsed = engine.parse("> [!note] Titled\n> body text\n", Extensions::FULL);
    let text: Vec<String> = parsed
        .events()
        .iter()
        .filter_map(|e| match &e.event {
            Event::Text(text) => Some(text.to_string()),
            _ => None,
        })
        .collect();

    assert_eq!(text, ["body text"], "{:?}", parsed.events());
}

/// A block quote that merely looks like a callout is still a block quote, and every
/// word of it is still there.
#[test]
fn a_quote_that_is_not_a_callout_keeps_its_brackets() {
    let engine = ComrakEngine::new();
    let parsed = engine.parse("> [!note]x not a marker\n", Extensions::FULL);
    let Some(Event::Start(Tag::BlockQuote { callout: None })) =
        parsed.events().first().map(|e| &e.event)
    else {
        panic!("expected a plain block quote, got {:?}", parsed.events());
    };
    assert!(
        parsed.events().iter().any(|e| matches!(&e.event,
            Event::Text(text) if text.contains("[!note]x not a marker"))),
        "{:?}",
        parsed.events(),
    );
}

/// A callout inside a callout is two callouts. Obsidian nests them, and recognising
/// the marker on the finished stream is what makes that need no special case.
#[test]
fn callouts_nest() {
    let engine = ComrakEngine::new();
    let parsed = engine.parse(
        "> [!note] Outer\n> text\n>\n> > [!tip] Inner\n> > deep\n",
        Extensions::FULL,
    );
    let kinds: Vec<String> = parsed
        .events()
        .iter()
        .filter_map(|e| match &e.event {
            Event::Start(Tag::BlockQuote {
                callout: Some(callout),
            }) => Some(callout.kind.to_string()),
            _ => None,
        })
        .collect();

    assert_eq!(kinds, ["note", "tip"]);
}

/// Where a task's box lives in the source, which is what makes it something a reader
/// can press: two identical items are two different offsets, and each one names the
/// character between its own brackets.
#[test]
fn task_items_carry_the_source_offset_of_their_own_box() {
    let engine = ComrakEngine::new();
    let source = "- [ ] same\n- [x] same\n  - [ ] nested\n";
    let parsed = engine.parse(source, Extensions::FULL);
    let tasks: Vec<(bool, usize)> = parsed
        .events()
        .iter()
        .filter_map(|e| match &e.event {
            Event::Start(Tag::Item { task: Some(task) }) => Some((task.checked, task.marker)),
            _ => None,
        })
        .collect();

    assert_eq!(tasks.len(), 3, "{:?}", parsed.events());
    for (checked, marker) in &tasks {
        assert_eq!(
            &source[marker - 1..marker + 2],
            if *checked { "[x]" } else { "[ ]" },
            "the offset {marker} does not name a checkbox",
        );
    }
    assert_eq!(tasks[0].1, 3);
    assert_eq!(tasks[1].1, 14);
}

/// An embed is a reference to something axiomd does not transclude (issue #12), and a
/// parser leaves it as literal text. It reaches the boundary as a wikilink that says
/// what it is, at a span that slices the source it came from.
#[test]
fn an_embed_is_a_wikilink_that_says_it_is_one() {
    let engine = ComrakEngine::new();
    let source = "See ![[diagram.png]] and [[guide]].\n";
    let parsed = engine.parse(source, Extensions::FULL);
    let links: Vec<(String, bool, String)> = parsed
        .events()
        .iter()
        .filter_map(|e| match &e.event {
            Event::Start(Tag::WikiLink { target, embed }) => Some((
                target.to_string(),
                *embed,
                source[e.span.range.clone()].to_owned(),
            )),
            _ => None,
        })
        .collect();

    assert_eq!(
        links,
        [
            (
                "diagram.png".to_owned(),
                true,
                "![[diagram.png]]".to_owned()
            ),
            ("guide".to_owned(), false, "[[guide]]".to_owned()),
        ],
    );
}

/// Table column alignments reach the cells that need them.
#[test]
fn table_cells_carry_their_column_alignment() {
    let engine = ComrakEngine::new();
    let parsed = engine.parse(
        "| a | b | c |\n|:--|:-:|--:|\n| 1 | 2 | 3 |\n",
        Extensions::GFM,
    );
    let alignments: Vec<Alignment> = parsed
        .events()
        .iter()
        .filter_map(|e| match &e.event {
            Event::Start(Tag::TableCell { alignment }) => Some(*alignment),
            _ => None,
        })
        .collect();
    assert_eq!(
        alignments,
        vec![
            Alignment::Left,
            Alignment::Center,
            Alignment::Right,
            Alignment::Left,
            Alignment::Center,
            Alignment::Right,
        ]
    );
}

/// Code fences carry the language and the rest of the info string separately, which
/// is what the plugin layer's fence handlers key on.
#[test]
fn code_fences_split_language_from_meta() {
    let engine = ComrakEngine::new();
    let parsed = engine.parse("```rust ignore,no_run\ncode\n```\n", Extensions::FULL);
    let Some(Event::Start(Tag::CodeBlock {
        language,
        meta,
        fenced,
    })) = parsed.events().first().map(|e| &e.event)
    else {
        panic!("expected a code block, got {:?}", parsed.events());
    };
    assert_eq!(language.as_deref(), Some("rust"));
    assert_eq!(meta.as_deref(), Some("ignore,no_run"));
    assert!(fenced);

    let indented = engine.parse("    code\n", Extensions::FULL);
    let Some(Event::Start(Tag::CodeBlock {
        language,
        meta,
        fenced,
    })) = indented.events().first().map(|e| &e.event)
    else {
        panic!(
            "expected an indented code block, got {:?}",
            indented.events()
        );
    };
    assert_eq!(language.as_deref(), None);
    assert_eq!(meta.as_deref(), None);
    assert!(!fenced);
}

/// The engine identifies itself, and the identifier is what selection persists.
#[test]
fn engine_identifies_itself() {
    let engine = ComrakEngine::new();
    assert_eq!(engine.id(), ComrakEngine::ID);
    assert_eq!(engine.id().as_str(), "comrak");
    assert_eq!(engine.id().to_string(), "comrak");
}

/// The boundary is usable through a trait object, which is what an engine registry
/// and per-document engine selection need.
#[test]
fn engine_is_usable_as_a_trait_object() {
    let engines: Vec<Box<dyn MarkdownEngine>> = vec![Box::new(ComrakEngine::new())];
    for engine in &engines {
        let parsed = engine.parse("# Title\n", Extensions::FULL);
        assert!(matches!(
            parsed.events().first().map(|e| &e.event),
            Some(Event::Start(Tag::Heading { level: 1 }))
        ));
    }
}
