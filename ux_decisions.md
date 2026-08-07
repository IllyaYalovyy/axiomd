# UX decisions

UX decision record in FAQ form. Every entry here is an **intentional
decision — not a limitation, a gap, or a bug to be fixed.** Do not
"improve" or "fix" any of these without an explicit decision to change
direction.

## Why does opening a file take zero configuration?

`xdg-open README.md` must produce a rendered document with no dialogs, no
onboarding, no vault setup. The first-run experience is the document.

## When is a modal dialog acceptable?

Owner ruling (2026-08-02, refined): the line is the **open/view path**.
Opening a file renders best-effort, immediately — the app NEVER interrupts
opening, rendering, or reading with a blocking question (Apostrophe's
preview-security modal is the named anti-pattern). Anything degraded
(unrenderable block, unloaded remote image, failed plugin) shows inline
with a one-click affordance.

Modal dialogs ARE acceptable for important, explicit, user-initiated
actions: Save As, preferences, about, print, close-with-unsaved-changes,
and similar. The test: did the user just ask for something that needs
input or confirmation? Fine. Is the app volunteering a question while the
user just wants to see or read a document? Design defect.

## How are external file changes handled?

Best effort, no questions (owner ruling 2026-08-02): if the buffer is
clean, the app silently reloads the changed file and preserves the reading
position whenever possible. Only a dirty buffer meeting an external change
surfaces anything — an inline banner with one-click choices, never a
blocking dialog. With autosave enabled, dirty windows are rare by
construction.

## Why autosave?

Owner decision (2026-08-02): edits should be autosaved — as an optional,
clearly-toggleable behavior. Losing work to a crash is worse than any
autosave surprise; atomic writes and the clean-buffer reload rule make
autosave safe.

## Why do external links not open on render or hover?

An app that fetches remote content implicitly is a browser with extra
steps and a privacy leak. External URLs open in the default browser on
explicit click only. Remote images render as placeholder cards that ARE
the one-click load button (per image), with a single inline "load all"
affordance per document — never a modal, never a setting hunt.

## Why is there one document per window?

Owner ruling (2026-08-02): windows, not tabs. Matches GNOME app
conventions and keeps per-window resource accounting honest (Apostrophe
leaked shared class-attribute state across windows). Tabs are not planned.

## What happens on a bare launch, and how are files opened?

Launching axiomd with no file (owner ruling 2026-08-02) opens a NEW
UNTITLED document in edit mode — same as Ctrl+N; the first Ctrl+S runs
Save As. Opening an existing file always starts in READ mode. axiomd
handles Markdown files only (`.md`/`.markdown`); it does not register for
or open other formats.

## Why read/edit toggle instead of a split view?

Owner ruling (2026-08-02): two modes initially — read and edit (Ctrl+E) —
no split view with scroll sync in MVP. The architecture keeps the door
open (view container and document model must not assume a single visible
surface), but no split implementation ships until decided. Switching modes
preserves the reading/editing position in both directions via the
span/anchor map.

## What does editor syntax highlighting look like?

Owner ruling (2026-08-03): minimal, fast, reliable source highlighting —
GtkSourceView's stock Markdown definition with the Adwaita schemes,
nothing more. Obsidian-style live-preview styling (markup rendering in
place while editing) is explicitly rejected, not merely deferred.
Highlighting may never noticeably degrade typing latency; the perf
budgets police this.

## Where does search live?

Owner ruling (2026-08-03): the search bar belongs to the document pane —
compact, overlaid on the document view only. It never spans or disturbs
the sidebar (issue #26).

## How does the chrome separate from the document?

Owner ruling (2026-08-06), from a side-by-side with Apostrophe on the same
document: with Adwaita's raised shadow, never a hard hairline. Every top bar
the reader can see uses `AdwToolbarView`'s raised style — the header, the
external-change banner in the same box beside it, and the find bar over the
document — so there is one separation language and one shadow at each
boundary the reader meets. High contrast renders that shadow as a solid rule;
that is Adwaita's own high-contrast stylesheet and it stays, because a reader
who asked for contrast must not be the one reader who cannot see where the
header ends. The scroll-driven flat-until-scrolled treatment GNOME Text
Editor uses was considered and NOT taken: the separation is persistent
(issue #47).

## How is the chapter the reader is in marked in the outline?

Owner ruling clarified (2026-08-06): **the accent is a line, never a fill.**
"Use the accent colour" always meant use it *as an accent*; the solid blue
lozenge with white text that shipped on 2026-08-05 was a misreading of that
instruction by the task spec, not a change of direction, and no later task
may "restore" it. The current chapter is a neutral grey wash — the language
`.navigation-sidebar` itself says "this one" in — with the accent spent on a
thin outline around the pill and the row's text left in the ordinary
foreground (issue #48).

Of the two accent shapes the ruling allows, a leading-edge bar or an outline,
the outline is what ships: it is the shape libadwaita gives a row it singles
out (`row:drop(active)`), it needs no side of its own so it reads the same in
both text directions, and it is the one that stays visible when the wash
under it is all a high-contrast desktop will allow. The state ladder is the
acceptance bar and is measured, not eyeballed: hover is washed at most half
as deep as the chapter being read, and the chapter being read is the only one
that carries an outline. Both differences are luminance, so a reader who
cannot tell the accent from the wash still sees which row is theirs.

Issue #42's guarantee is untouched by this and stands: the mark belongs to
the section being read and the pointer can never take it.

## Is the sidebar resizable?

Yes (owner ruling 2026-08-03): drag the divider; width persists across
sessions as window state, not a preferences entry (issue #27).

## Why is the app name lowercase "axiomd"?

Intentional branding, matching the owner's project naming (axiotask). The
HIG preference for header-capitalized names ("Axiomd") is knowingly not
followed; the desktop entry, metainfo, and About dialog all use "axiomd".
Recorded 2026-08-03 after UX review flagged it — this is a decision, not
an oversight.

## Do controls reflect state?

A control that toggles state shows where it takes you: the read/edit
toggle changes icon and tooltip with the mode (issue #28). The general
rule: a stateful affordance whose presentation never changes is a defect.

## How is a control named?

One rule, everywhere (issue #32): the tooltip is `Action Name (Key)` and
the accessible label is the action name alone, header-capitalized. GTK
announces the accelerator itself, so a name carrying it would say it
twice. The key is never written beside the words — it is looked up from
the shortcut table in `chrome.rs` by the action the control fires, so a
tooltip cannot promise a key the keyboard does not install. A control on
no key is simply named. Applied through `chrome::name` and swept by
`crates/axiomd-app/tests/naming.rs`.

## Does an untitled window open with its outline sidebar?

No (issue #32). A window with no document has no headings and no prospect
of any until the reader writes one, so "No headings" beside an empty page
is noise where their first line should be. The sidebar appears at the
first heading they write, at F9, or when a document is opened into the
window — never on save, which names the page rather than opening one.
A document that was *opened* with no headings keeps the "No headings"
empty state: that is the right answer for a document that has none.

## Is YAML frontmatter rendered?

No — out of scope (owner ruling 2026-08-02). Frontmatter is parsed as
metadata (used for things like the export title) and hidden from the
rendered view.

## Is there a vault?

No (owner ruling 2026-08-02): the document's folder is the root. Wikilinks
resolve within the document's directory tree; there is no vault detection
and no vault configuration.

## Why does the theme follow the system by default?

libadwaita style manager is the source of truth: light/dark/high-contrast
follow the desktop, with an optional in-app override. The rendered document
restyles live without a reload flash.
