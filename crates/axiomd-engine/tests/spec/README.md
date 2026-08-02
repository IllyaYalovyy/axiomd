# Vendored conformance suites

These files are verbatim copies of the official specifications. They are the
authority for rendering correctness disputes (VISION principle 1); do not edit
them, and do not "fix" a case to make a test pass.

| File | Source | Retrieved |
|---|---|---|
| `commonmark-0.31.2.spec.txt` | <https://raw.githubusercontent.com/commonmark/commonmark-spec/0.31.2/spec.txt> | 2026-08-02 |
| `gfm-0.29.spec.txt` | <https://raw.githubusercontent.com/github/cmark-gfm/master/test/spec.txt> (GFM spec 0.29) | 2026-08-02 |

Both specifications are licensed CC-BY-SA 4.0 by their respective authors
(John MacFarlane; GitHub, Inc.). They are included here as test fixtures only.

Example blocks are delimited by a run of 32 backticks followed by `example`,
with the Markdown input and expected HTML separated by a lone `.` line. Inside
those blocks `→` (U+2192) stands for a literal tab; the loader in
`tests/support/mod.rs` substitutes it.
