# Engine comparison: comrak and pulldown-cmark

Evidence for **D5 — which markdown engine is axiomd's default**. Produced by
`crates/axiomd-engine/tests/comparison.rs`, which regenerates the measured
section below and fails the quality gate when this file stops matching what
the engines do.

**This document recommends. It does not decide.** D5 is the project owner's
(`design_decisions.md`, `AGENTS.md`), and comrak remains the default until
they rule. Nothing in issue #17 changed it.

Measured on 2026-08-03, on the machine in that task's report:
Linux 7.1.4 x86_64, release build, comrak 0.54, pulldown-cmark 0.13.4.

## What is measured, and how

Both engines are the same kind of thing to axiomd: they implement
`MarkdownEngine` and produce the boundary's typed events with source spans.
Everything below is asked of them through that trait, so nothing here depends
on either parser's own API.

- **Capabilities** — each engine's `capabilities()` report.
  `boundary.rs::engine_parses_exactly_the_extensions_it_advertises` holds it
  to what the engine observably does, in both directions, so a `yes` here is
  a document that really parses and a `no` is one that really does not.
- **Conformance** — the vendored `commonmark-0.31.2.spec.txt` and the
  `(extension)` sections of `gfm-0.29.spec.txt`, serialised to HTML by the
  suites' own minimal serialiser and compared byte for byte with the text the
  specification prints. Rendering disputes are settled by the spec, never by
  what looks plausible.
- **Golden corpus** — every fixture the render pipeline is pinned against,
  parsed by each engine and serialised the same way. comrak is the shipping
  default, so it is what the others are compared with.
- **Span quality** — the properties `spans.rs` asserts over all 1324 spec
  documents, plus how finely each engine anchors a document. Spans carry
  outline, scroll sync, search and live-reload anchoring (invariant 3).
- **Throughput** — parse time on the three documents the performance budgets
  are measured on (`axiomd_e2e::corpus`), in a release build.

<!-- measured:begin -->
### Capabilities

What each engine reports it can parse, held to what it
observably does by `boundary.rs`.

| Extension | comrak | pulldown-cmark |
| --- | --- | --- |
| Tables | yes | yes |
| TaskLists | yes | yes |
| Strikethrough | yes | yes |
| Autolinks | yes | no |
| Footnotes | yes | yes |
| Math | yes | yes |
| WikiLinks | yes | yes |
| Callouts | yes | yes |
| FrontMatter | yes | yes |

### Conformance

Examples whose serialised HTML is byte-for-byte what the
specification prints.

| Suite | comrak | pulldown-cmark |
| --- | --- | --- |
| CommonMark 0.31.2 (652 examples) | 652/652 (100.0%) | 652/652 (100.0%) |
| GFM extensions (24 examples) | 24/24 (100.0%) | 13/24 (54.2%) |

### Golden corpus (12 fixtures)

Every document the render pipeline is pinned against, parsed by each
engine and serialised the same way. `comrak` is the shipping default, so what
the others are compared against is its parse.

| Engine | agrees with the default | differs on |
| --- | --- | --- |
| comrak | 12/12 | — |
| pulldown-cmark | 9/12 | footnotes.md, inline.md, math.md |

### Span quality (1324 spec documents)

Spans are load-bearing: outline, scroll sync, search and live-reload
anchoring all map through them (invariant 3). The first three rows are
properties `spans.rs` asserts; the last is how finely an engine anchors a
document, which is what scroll-sync granularity is.

| Measure | comrak | pulldown-cmark |
| --- | --- | --- |
| spanned events | 8691 | 8616 |
| block spans inside their parent | 100.0% | 100.0% |
| top-level blocks whose span re-parses to itself | 100.0% | 100.0% |
| non-blank lines carrying a block anchor | 62.5% | 62.5% |

<!-- measured:end -->

## Throughput

Parse only — no rendering, no highlighting — on the perf fixtures, release
build, one engine at a time. Reproduce with `./scripts/quality.d/30-engines.sh`.

| Document | comrak | pulldown-cmark |
| --- | --- | --- |
| typical (50 KB) | 2.4 ms (20.7 MB/s) | 0.9 ms (53.9 MB/s) |
| 10 MB | 503.4 ms (19.9 MB/s) | 189.4 ms (52.8 MB/s) |
| pathological (200 deep) | 0.5 ms (79.6 MB/s) | 0.2 ms (183.8 MB/s) |

pulldown-cmark is about 2.6× faster on every shape, and neither engine goes
quadratic on the pathological document. Both produce identical event counts
on all three (6247, 1 219 213 and 1209), so this is the same work done faster
rather than less work done.

For scale: parsing is not where a 10 MB document costs its time. Issue #9
measured that render at 9.1 s, of which 0.4 s was the parse and 8.3 s the
syntax highlighter. Swapping engines would take roughly 0.3 s off a 9-second
render — real, and not the bottleneck.

## Where the two engines differ, and why

### GFM extended autolinks

pulldown-cmark 0.13.4 has no option for them; a bare `www.example.com`,
`http://…` or `foo@bar.baz` in prose stays prose. This is the whole of the
GFM conformance gap above — all 11 failing examples are in
"Autolinks (extension)", and every other GFM extension example passes — and
it is the whole of the `inline.md` golden difference. It is declared honestly:
`Extension::Autolinks` is not in pulldown-cmark's capability report, so the
renderer sees a document without those links rather than a promise that was
not kept.

CommonMark's own `<https://example.com>` autolinks work in both engines.

### Footnote definitions

comrak collects footnote definitions, orders them by first reference and
drops the ones nothing refers to — GFM's behaviour. pulldown-cmark emits each
definition where it was written and keeps unreferenced ones. This is the
`footnotes.md` golden difference: the same content, in a different order,
plus one definition comrak removes.

### Whitespace inside a multi-line math span

comrak normalises a line break inside a `$…$` span to a space;
pulldown-cmark carries the source verbatim, newline included. This is the
`math.md` golden difference. Both are whitespace to LaTeX, so the typeset
output is the same — but the two engines' `Event::Math` payloads are not
byte-identical, and the golden corpus is pinned byte for byte.

### What is the same

Everything else. Both reach 100% on CommonMark 0.31.2. Both satisfy every
span property, on every one of the 1324 spec documents, at the same anchor
granularity. Callouts and `![[embeds]]` are recognised by the shared
`obsidian.rs` transform on the finished event stream, so both engines read
Obsidian's whole open vocabulary identically — including the fold marker,
which neither parser's own alert extension preserves. Tight and loose lists,
task-marker source offsets, table alignments, front matter, wikilinks, raw
HTML blocks and code-fence info strings all agree.

## Recommendation

**Keep comrak as the default; keep pulldown-cmark selectable.** Reasoning:

1. **GFM autolinks are the deciding difference.** Bare `www.` and bare URL
   links are ordinary in READMEs and notes, which is most of what axiomd is
   opened on. A reader who switched engines and found half the links in their
   document had become plain text would experience it as a bug, not as a
   setting. axiomd's own README relies on them.
2. **Footnote ordering is GFM's, and comrak implements it.** A document whose
   footnotes reorder when the engine changes is the same kind of surprise.
3. **The speed difference is real but is not the bottleneck.** pulldown-cmark
   is 2.6× faster at parsing, and parsing is under 5% of what a large
   document costs (issue #9). Trading correctness for it would be trading the
   thing readers notice for the thing they do not.

pulldown-cmark earns its place regardless: it is the second implementation
that proves the boundary is a boundary, it is measurably faster, it is the
engine to reach for on a document where parse time dominates, and it is what
the parameterised suites hold both engines to. If pulldown-cmark gains
extended autolinks, this recommendation should be measured again — the two
engines would then differ only on footnote ordering.

**Blocked on D5.** The default engine is the project owner's ruling.
