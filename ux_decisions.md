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

## What does the editor offer for a misspelled word?

Issue #59 (2026-09-13): libspelling's own context-menu section, **as
shipped** — no menu of axiomd's. Right-click, Shift+F10 or the Menu key
on a misspelled word in edit mode shows, below GTK's Cut/Copy/Paste
section: up to five corrections (libspelling's `MAX_CORRECTIONS`), Add to
Dictionary, Ignore, Check Spelling, and a Languages submenu. The rows are
always about the word the menu opened on: a right click first moves the
caret to where it landed (unless inside a selection), and both paths ask
libspelling to update its corrections before the menu is built. On a
correctly spelled word, with spell checking off, and over a document too
long to check (`LARGEST_CHECKED_DOCUMENT`), there are no corrections and
no Add or Ignore. Reading mode has no source view, so no such menu.

- **A correction** is one edit: one undo step, the document unsaved, the
  page re-rendered like typing. libspelling does not act on a selection.
- **Add to Dictionary** is permanent, in enchant's personal dictionary:
  `$XDG_CONFIG_HOME/enchant/<language>.dic` for a host install, and the
  sandbox's own `~/.var/app/io.github.etf.axiomd/config/enchant` for the
  flatpak — never the host's, consistent with `host:ro` (#23).
- **Ignore** lasts until axiomd quits (enchant's session list). It is
  process-wide, not per window, since windows share enchant's dictionary.
- **Languages** changes the checker of that window only (each window has
  its own checker) and is not persisted: a relaunch goes back to the
  locale's language. Persisting it is a separate owner decision.
- **Check Spelling** in that menu is libspelling's own toggle, part of
  the section as shipped. It does not change the Check Spelling
  preference (#20), and axiomd applies the preference again on the next
  edit, so the underlines it removes come back at the next keystroke
  (observed 2026-09-13). How the row should relate to the preference is
  an **open owner decision**, not settled here.

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

## What is marked while the reader is above the first heading?

The title row (owner report, issue #50, 2026-08-06). The outline is never
markerless: a document opened at its top used to mark nothing at all, which
is honest — the reader is in no section — but reads as broken, and it is the
state every first open starts in. A reader above the first heading is at the
document's title, so the title row over the list (issue #35) carries the
mark until they read into a section, and takes it back when they scroll
above it again. Marking the *first section* instead was considered and is
refused: it would say the reader is somewhere they are not.

Presentation only. Where the reader is is still "no section" everywhere it
is asked — the tracking bridge's zero report, its cadence, and the span map
are untouched — and the mark is drawn in the same grammar issue #48 ruled
for sections: the grey wash, the accent as an outline and never a fill. It
follows the row's own shape, which for a full-width title row is the row's
edges rather than a pill. A document with no headings at all keeps the mark
on its title row for as long as it is open.

## Is the sidebar resizable?

Yes (owner ruling 2026-08-03): drag the divider; width persists across
sessions as window state, not a preferences entry (issue #27).

## Why is the app name lowercase "axiomd"?

Intentional branding, matching the owner's project naming (axiotask). The
HIG preference for header-capitalized names ("Axiomd") is knowingly not
followed; the desktop entry, metainfo, and About dialog all use "axiomd".
Recorded 2026-08-03 after UX review flagged it — this is a decision, not
an oversight.

## Where does a reader find how to support axiomd?

In two places, both opening the metainfo's `<url type="donation">` and
nothing else. Issue #55 put a "Support axiomd" link row in the About
dialog only, under a "nowhere else in the application" constraint. The
owner lifted that for the primary menu (owner ruling 2026-09-13, issue
#58): "The 'donation' option is hidden so DEEP inside the menu tree that
nobody would ever see it. It has to be at the top of the home menu." So
"Support axiomd" is the first row of the primary menu, in a section of
its own, above everything, the narrow-window history included, at every
width. It is a plain menu row: no icon, no styling. The About link row
stays as it was.

Still no nagging: no banners, startup prompts, notification dots, toasts,
first-run screens, preferences rows, header buttons or shortcuts. Nothing
is fetched until the reader activates the row, and then the address goes
to the system browser.

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

## Does a document open where the reader left it?

Yes, by default (issue #51) — the Evince/Papers behaviour a GNOME reader
expects. The place is a span anchor, never a pixel offset: the source line
of the block that was at the top of the page, restored through the very
machinery a mode switch and a live reload use (invariant 5). It is applied
before the reader is shown the document at all, so there is no top-then-jump.

Three rulings go with it:

- **Never a question.** A document that lost the anchored line between
  visits lands on the nearest surviving block; a store that is missing or
  damaged opens the document at the top. Silently, both times — the open
  path is never interrupted (VISION principle 6, invariant 12).
- **Last write wins.** Several windows on one document each write down
  where their reader was as they close, and the last window to close is
  the place the document opens at next time. Nothing is merged: two
  windows are two readings, and the most recent one is the answer.
- **Off keeps what is already written.** "Remember Reading Position" off
  stops both halves — nothing restored, nothing written — and leaves the
  places already written down alone, so turning it back on resumes rather
  than starts over.

## Do documents read in axiomd appear in the desktop's recent files?

Yes (issue #52, tier 1). Every real open — from Files, from the command
line, from `Ctrl+O`, and by following a relative link or a `[[wikilink]]` —
registers the document with `GtkRecentManager`, so it turns up in
Files → Recent, in the file chooser's Recent view and in Shell search like
a document opened in any other GNOME application. An untitled page joins
when the reader first gives it a name; a file axiomd *wrote* — an exported
PDF or page — never does, because the reader made it rather than read it.

What is registered is the path the reader keeps the document at, never the
`/run/user/…/doc/…` path the document portal handed over: the desktop mints
that afresh for every open, so an entry spelled with it would name nothing
at all by the time the reader pressed it.

**An in-app recent surface was deliberately not in that tier** — it was a
later owner decision, taken in issue #54 and recorded in the entry below.
Tier 1 feeds the desktop and keeps no list of its own; tier 2 reads that
same list back and still keeps none.

## How does the reader get back to a document they were reading?

Owner ruling (2026-08-11, issue #54): from the primary menu, under
**Open Recent**, directly below *Open…* — the last **ten** documents they
read in axiomd, most recently read first, each opening where they are.

**Why the menu and not a split Open button.** The header bar's Open button
is one of the three controls a narrow window gives up to keep the
document's name readable (issue #30), so a recent list hanging off it
would be a way back that disappears exactly when the window is small. The
primary menu is in every window at every width, it is on `F10` for a
reader with no pointer, and it leaves the frozen chrome of #29/#30 exactly
as it was. GNOME Text Editor's header surface — the precedent the issue
named — is a popover with a search entry, a list view and an empty state
of its own (read out of `/usr/bin/gnome-text-editor` on this machine:
`EditorOpenPopover`, "Search documents", "No Recent Documents"), which is
most of what #54 rules out of scope; a `GMenu` submenu is the same idea in
the form this application already builds menus in.

**Why ten.** The list is a popover of its own, so its length costs the
menu it hangs off nothing. A reader opening it is looking for a document
they had open earlier in the day, and a session of reading is easily eight
or nine documents — a shorter list would drop exactly the one they are
looking for. Ten is still a glance rather than a search, which is what
keeps the search box (out of scope) from being missed.

**What is in it.** Every document axiomd registered with the desktop
(issue #52) that is still on the disk, newest first, **including the one
the window is showing**: it is one rule rather than one rule per window,
the entry is a real way back for the reader who has that document open in
*another* window, and a reader who presses it where they already are is
simply shown the window they are already in.

**What it says.** The document's own name. Two documents on the list that
share a name are told apart by the folder they are in — `notes.md
(wayfinder)` — and, when the folders share a name too, by as much of the
path as it takes; nothing is added to a name that is already unlike every
other on the list.

**Empty is empty.** A reader with nothing to come back to — a first run,
a desktop with *File History* off, a list whose documents have all been
moved away — gets no *Open Recent* in their menu at all: never an error,
never a dialog, and never an item that is there and can never be pressed.
The menu already has a section that appears only when it has something in
it — the history a narrow window takes out of the header bar (issue #30) —
so this is that idiom and not a new one.

**Still one store.** There is no axiomd recent list. The menu is a view of
`GtkRecentManager`'s own, filtered to what axiomd registered, worked out at
the moment the menu opens — which is the only moment at which it can be
right, since a document deleted behind the desktop's back changes nothing
the recent manager would announce.

## Why is there no File History switch in axiomd's preferences?

Because the desktop already has one, and it is the reader's answer for
every application at once (issue #52). GNOME's Settings → Privacy → File
History reaches GTK as `Gtk/RecentFilesEnabled`, and with it off GTK writes
nothing to the recent list at all — probed on GTK 4.20.4: the file is not
so much as created. A switch of axiomd's own could therefore only disagree
with the desktop about a decision the reader has already made there.

This is the one recorded exception to invariant 14 ("every behavior knob
lives in preferences and applies live"): there is no axiomd knob to put in
preferences, because the knob is the platform's. Ageing and capping the
list are the desktop's too — axiomd writes entries and nothing else.

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
