//! The engine boundary contract: what an engine advertises must be what it parses.
//!
//! Every test here runs over **every registered engine** (issue #17). The boundary
//! exists so that the view layer cannot tell which parser produced a document, and
//! that is only true if each engine independently satisfies the whole contract — so
//! the suite is parameterised rather than written against one of them. An engine added
//! to the registry is held to all of it without a line being added here.
//!
//! The one thing engines are allowed to differ on is which extensions they can parse,
//! and that difference is not free: `engine_parses_exactly_the_extensions_it_advertises`
//! pins each engine's capability report to what it observably does, in both directions.

mod support;

use axiomd_engine::{
    Alignment, ComrakEngine, EngineId, Event, Extension, Extensions, MarkdownEngine, Parsed,
    PulldownEngine, Tag, TagEnd,
};

/// The engines the contract is checked against.
fn engines() -> &'static [&'static dyn MarkdownEngine] {
    let engines = axiomd_engine::engines();
    assert!(
        engines.len() >= 2,
        "only {} engine(s) registered; the boundary is not being proved by a second \
         implementation (issue #17)",
        engines.len(),
    );
    engines
}

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

/// Every extension an engine advertises is really parsed, and nothing it advertises
/// leaks into strict CommonMark mode. Together these pin every capability report to
/// observable behaviour rather than to a hand-maintained list — which is what makes the
/// capability matrix in `designs/engine-comparison.md` evidence rather than a claim.
#[test]
fn engine_parses_exactly_the_extensions_it_advertises() {
    assert_eq!(
        PROBES.len(),
        Extension::ALL.len(),
        "every Extension needs a probe document"
    );

    for engine in engines() {
        let capabilities = engine.capabilities();
        for probe in PROBES {
            let advertised = capabilities.contains(probe.extension);
            let parsed = engine.parse(probe.markdown, Extensions::FULL);
            assert_eq!(
                (probe.marker)(&parsed),
                advertised,
                "{}: {:?}: capabilities() says {advertised} but parsing {:?} disagrees",
                engine.id(),
                probe.extension,
                probe.markdown
            );

            let strict = engine.parse(probe.markdown, Extensions::COMMONMARK);
            assert!(
                !(probe.marker)(&strict),
                "{}: {:?} was parsed even though only CommonMark was requested",
                engine.id(),
                probe.extension,
            );
        }
    }
}

/// Requesting an extension set narrower than the engine's capabilities parses only
/// that set.
#[test]
fn requesting_a_narrower_set_narrows_the_parse() {
    let markdown = "~~gone~~ and $x^2$\n";
    for engine in engines() {
        let gfm = engine.parse(markdown, Extensions::GFM);
        assert!(
            has_event(&gfm, |e| matches!(e, Event::Start(Tag::Strikethrough))),
            "{}: strikethrough is in the GFM set and was not parsed",
            engine.id(),
        );
        assert!(
            !has_event(&gfm, |e| matches!(e, Event::Math { .. })),
            "{}: math is not in the GFM set but was parsed anyway",
            engine.id(),
        );
    }
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
    let source = "---\ntitle: Notes\n---\n\n# Heading\n";
    for engine in engines() {
        let parsed = engine.parse(source, Extensions::FULL);

        assert_eq!(
            parsed.front_matter().map(str::trim_end),
            Some("---\ntitle: Notes\n---"),
            "{}: front matter is not the blob the document opened with",
            engine.id(),
        );
        assert!(
            !has_event(
                &parsed,
                |e| matches!(e, Event::Text(t) if t.contains("title"))
            ),
            "{}: front matter leaked into the event stream",
            engine.id(),
        );
        let heading = parsed
            .events()
            .iter()
            .find(|e| matches!(e.event, Event::Start(Tag::Heading { .. })))
            .unwrap_or_else(|| panic!("{}: no heading after the front matter", engine.id()));
        assert_eq!(
            source[heading.span.range.clone()].trim_end(),
            "# Heading",
            "{}: the heading's span does not slice the heading",
            engine.id(),
        );
    }
}

/// A document with no front matter reports none — the non-happy path for metadata.
#[test]
fn absent_front_matter_is_none() {
    for engine in engines() {
        assert_eq!(
            engine.parse("# Heading\n", Extensions::FULL).front_matter(),
            None,
            "{}: a document without front matter reported some",
            engine.id(),
        );
        assert_eq!(
            engine
                .parse("---\ntitle: x\n---\n", Extensions::COMMONMARK)
                .front_matter(),
            None,
            "{}: front matter was extracted without the extension being requested",
            engine.id(),
        );
    }
}

/// A `---`-fenced block that is *not* at the front of the document is not front
/// matter, and every line of it still reaches the reader.
///
/// The failure this guards is silent content loss: pulldown-cmark recognises a
/// metadata block anywhere in a document, so a note holding a YAML-looking block
/// halfway down would simply stop being displayed.
#[test]
fn a_fenced_block_that_is_not_at_the_front_is_content() {
    let source = "Intro paragraph.\n\n---\nkey: value\n---\n";
    for engine in engines() {
        let parsed = engine.parse(source, Extensions::FULL);
        assert_eq!(
            parsed.front_matter(),
            None,
            "{}: a block halfway down the document was taken for front matter",
            engine.id(),
        );
        assert!(
            has_event(
                &parsed,
                |e| matches!(e, Event::Text(t) if t.contains("key: value"))
            ),
            "{}: the block's own lines are not in the document: {:?}",
            engine.id(),
            parsed.events(),
        );
    }
}

/// Callout kind, author title and fold marker survive the boundary — for the kinds
/// GitHub knows and for the ones only Obsidian does, which is the whole point of
/// carrying the kind as the author wrote it (issue #12).
#[test]
fn callouts_carry_kind_title_and_fold() {
    for engine in engines() {
        let callout = |source: &str| {
            let parsed = engine.parse(source, Extensions::FULL);
            let Some(Event::Start(Tag::BlockQuote {
                callout: Some(callout),
            })) = parsed.events().first().map(|e| &e.event)
            else {
                panic!(
                    "{}: expected a callout block quote, got {:?}",
                    engine.id(),
                    parsed.events()
                );
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
            "{}",
            engine.id(),
        );
        assert_eq!(
            callout("> [!bug]\n> body\n"),
            ("bug".to_owned(), None, None),
            "{}",
            engine.id(),
        );
        assert_eq!(
            callout("> [!tldr]- Folded away\n> body\n"),
            (
                "tldr".to_owned(),
                Some("Folded away".to_owned()),
                Some(false)
            ),
            "{}",
            engine.id(),
        );
        assert_eq!(
            callout("> [!question]+ Open\n> body\n"),
            ("question".to_owned(), Some("Open".to_owned()), Some(true)),
            "{}",
            engine.id(),
        );
    }
}

/// The marker is not the quote's first sentence: a reader must not be shown
/// `[!note]` above the body, and the body itself must survive intact.
#[test]
fn a_callout_marker_leaves_the_quotes_own_prose_alone() {
    for engine in engines() {
        let parsed = engine.parse("> [!note] Titled\n> body text\n", Extensions::FULL);
        let text: Vec<String> = parsed
            .events()
            .iter()
            .filter_map(|e| match &e.event {
                Event::Text(text) => Some(text.to_string()),
                _ => None,
            })
            .collect();

        assert_eq!(
            text,
            ["body text"],
            "{}: {:?}",
            engine.id(),
            parsed.events()
        );
    }
}

/// A block quote that merely looks like a callout is still a block quote, and every
/// word of it is still there.
#[test]
fn a_quote_that_is_not_a_callout_keeps_its_brackets() {
    for engine in engines() {
        let parsed = engine.parse("> [!note]x not a marker\n", Extensions::FULL);
        let Some(Event::Start(Tag::BlockQuote { callout: None })) =
            parsed.events().first().map(|e| &e.event)
        else {
            panic!(
                "{}: expected a plain block quote, got {:?}",
                engine.id(),
                parsed.events()
            );
        };
        assert!(
            parsed.events().iter().any(|e| matches!(&e.event,
                Event::Text(text) if text.contains("[!note]x not a marker"))),
            "{}: {:?}",
            engine.id(),
            parsed.events(),
        );
    }
}

/// A callout inside a callout is two callouts. Obsidian nests them, and recognising
/// the marker on the finished stream is what makes that need no special case.
#[test]
fn callouts_nest() {
    for engine in engines() {
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

        assert_eq!(kinds, ["note", "tip"], "{}", engine.id());
    }
}

/// Where a task's box lives in the source, which is what makes it something a reader
/// can press: two identical items are two different offsets, and each one names the
/// character between its own brackets.
#[test]
fn task_items_carry_the_source_offset_of_their_own_box() {
    let source = "- [ ] same\n- [x] same\n  - [ ] nested\n";
    for engine in engines() {
        let parsed = engine.parse(source, Extensions::FULL);
        let tasks: Vec<(bool, usize)> = parsed
            .events()
            .iter()
            .filter_map(|e| match &e.event {
                Event::Start(Tag::Item { task: Some(task) }) => Some((task.checked, task.marker)),
                _ => None,
            })
            .collect();

        assert_eq!(tasks.len(), 3, "{}: {:?}", engine.id(), parsed.events());
        for (checked, marker) in &tasks {
            assert_eq!(
                &source[marker - 1..marker + 2],
                if *checked { "[x]" } else { "[ ]" },
                "{}: the offset {marker} does not name a checkbox",
                engine.id(),
            );
        }
        assert_eq!(tasks[0], (false, 3), "{}", engine.id());
        assert_eq!(tasks[1], (true, 14), "{}", engine.id());
    }
}

/// An embed is a reference to something axiomd does not transclude (issue #12). It
/// reaches the boundary as a wikilink that says what it is, at a span that slices the
/// source it came from.
#[test]
fn an_embed_is_a_wikilink_that_says_it_is_one() {
    let source = "See ![[diagram.png]] and [[guide]].\n";
    for engine in engines() {
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
            "{}",
            engine.id(),
        );
    }
}

/// A wikilink with a label shows the label and points at the target.
#[test]
fn a_wikilink_may_be_labelled() {
    let source = "[[Target|Label]]\n";
    for engine in engines() {
        let parsed = engine.parse(source, Extensions::FULL);
        let events = parsed.events();
        let at = events
            .iter()
            .position(|e| matches!(e.event, Event::Start(Tag::WikiLink { .. })))
            .unwrap_or_else(|| panic!("{}: no wikilink in {:?}", engine.id(), events));
        let Event::Start(Tag::WikiLink { target, .. }) = &events[at].event else {
            unreachable!()
        };
        assert_eq!(target.as_ref(), "Target", "{}", engine.id());
        assert!(
            matches!(&events[at + 1].event, Event::Text(text) if text.as_ref() == "Label"),
            "{}: the label is not what the reader is shown: {:?}",
            engine.id(),
            events[at + 1],
        );
    }
}

/// Table column alignments reach the cells that need them.
#[test]
fn table_cells_carry_their_column_alignment() {
    for engine in engines() {
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
            ],
            "{}",
            engine.id(),
        );
    }
}

/// Code fences carry the language and the rest of the info string separately, which
/// is what the plugin layer's fence handlers key on.
#[test]
fn code_fences_split_language_from_meta() {
    for engine in engines() {
        let parsed = engine.parse("```rust ignore,no_run\ncode\n```\n", Extensions::FULL);
        let Some(Event::Start(Tag::CodeBlock {
            language,
            meta,
            fenced,
        })) = parsed.events().first().map(|e| &e.event)
        else {
            panic!(
                "{}: expected a code block, got {:?}",
                engine.id(),
                parsed.events()
            );
        };
        assert_eq!(language.as_deref(), Some("rust"), "{}", engine.id());
        assert_eq!(meta.as_deref(), Some("ignore,no_run"), "{}", engine.id());
        assert!(fenced, "{}", engine.id());

        let indented = engine.parse("    code\n", Extensions::FULL);
        let Some(Event::Start(Tag::CodeBlock {
            language,
            meta,
            fenced,
        })) = indented.events().first().map(|e| &e.event)
        else {
            panic!(
                "{}: expected an indented code block, got {:?}",
                engine.id(),
                indented.events()
            );
        };
        assert_eq!(language.as_deref(), None, "{}", engine.id());
        assert_eq!(meta.as_deref(), None, "{}", engine.id());
        assert!(!fenced, "{}", engine.id());
    }
}

/// A tight list and a loose one are told apart, and a paragraph is inside an item
/// either way.
///
/// The renderer decides whether to wrap an item's prose in `<p>` from the list's
/// `tight` flag, so an engine that reported every list loose would put a blank line
/// between every bullet the reader sees.
#[test]
fn lists_say_whether_they_are_tight() {
    for engine in engines() {
        for (source, tight) in [("- a\n- b\n", true), ("- a\n\n- b\n", false)] {
            let parsed = engine.parse(source, Extensions::FULL);
            let Some(Event::Start(Tag::List {
                tight: reported, ..
            })) = parsed.events().first().map(|e| &e.event)
            else {
                panic!(
                    "{}: expected a list, got {:?}",
                    engine.id(),
                    parsed.events()
                );
            };
            assert_eq!(
                *reported,
                tight,
                "{}: {source:?} is {} and was reported otherwise",
                engine.id(),
                if tight { "tight" } else { "loose" },
            );
            let paragraphs = parsed
                .events()
                .iter()
                .filter(|e| matches!(e.event, Event::Start(Tag::Paragraph)))
                .count();
            assert_eq!(
                paragraphs,
                2,
                "{}: {source:?} has two items and {paragraphs} paragraphs: {:?}",
                engine.id(),
                parsed.events(),
            );
        }
    }
}

/// An ordered list carries the ordinal it starts at, and says it is ordered when it
/// closes.
#[test]
fn ordered_lists_carry_their_first_ordinal() {
    for engine in engines() {
        let parsed = engine.parse("3. a\n4. b\n", Extensions::FULL);
        assert!(
            matches!(
                parsed.events().first().map(|e| &e.event),
                Some(Event::Start(Tag::List { start: Some(3), .. }))
            ),
            "{}: {:?}",
            engine.id(),
            parsed.events(),
        );
        assert!(
            has_event(&parsed, |e| matches!(
                e,
                Event::End(TagEnd::List { ordered: true })
            )),
            "{}: the list did not close as an ordered one",
            engine.id(),
        );
    }
}

/// A raw HTML block reaches the boundary as one literal, whole.
///
/// Sanitisation is the renderer's job and it judges a block at a time; an engine that
/// handed over one event per line would give it fragments to judge.
#[test]
fn a_raw_html_block_arrives_whole() {
    for engine in engines() {
        let parsed = engine.parse("<div class=\"x\">\ninside\n</div>\n", Extensions::FULL);
        let blocks: Vec<String> = parsed
            .events()
            .iter()
            .filter_map(|e| match &e.event {
                Event::HtmlBlock(html) => Some(html.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(blocks.len(), 1, "{}: {:?}", engine.id(), parsed.events());
        assert_eq!(
            blocks[0].trim_end(),
            "<div class=\"x\">\ninside\n</div>",
            "{}",
            engine.id(),
        );
    }
}

/// Every engine identifies itself, and no two engines answer to the same name — the
/// name is what a preference, a view-menu choice and `--engine` all store.
#[test]
fn every_engine_identifies_itself_uniquely() {
    let mut seen: Vec<EngineId> = Vec::new();
    for engine in engines() {
        let id = engine.id();
        assert!(
            !id.as_str().is_empty(),
            "an engine with no name cannot be chosen",
        );
        assert!(!seen.contains(&id), "two engines both answer to {id}");
        assert_eq!(
            axiomd_engine::engine(id.as_str()).map(|found| found.id()),
            Some(id),
            "{id} is registered and cannot be looked up by name",
        );
        seen.push(id);
    }

    assert_eq!(ComrakEngine::new().id(), ComrakEngine::ID);
    assert_eq!(ComrakEngine::ID.as_str(), "comrak");
    assert_eq!(ComrakEngine::ID.to_string(), "comrak");
    assert_eq!(PulldownEngine::new().id(), PulldownEngine::ID);
    assert_eq!(PulldownEngine::ID.as_str(), "pulldown-cmark");

    // A name this build has never heard of is answered as such, rather than with
    // whichever engine happens to be first.
    assert!(axiomd_engine::engine("no-such-engine").is_none());
}

/// The engine a document is read with when nothing has chosen otherwise. comrak until
/// the owner rules on D5 (`design_decisions.md`); an agent may not change this.
#[test]
fn the_first_registered_engine_is_the_default() {
    assert_eq!(engines()[0].id(), ComrakEngine::ID);
}

/// The boundary is usable through a trait object, which is what the registry, the
/// preference and per-document selection all need.
#[test]
fn engines_are_usable_as_trait_objects() {
    for engine in engines() {
        let parsed = engine.parse("# Title\n", Extensions::FULL);
        assert!(
            matches!(
                parsed.events().first().map(|e| &e.event),
                Some(Event::Start(Tag::Heading { level: 1 }))
            ),
            "{}: {:?}",
            engine.id(),
            parsed.events(),
        );
    }
}
