# UX decisions

UX decision record in FAQ form. Every entry here is an **intentional
decision — not a limitation, a gap, or a bug to be fixed.** Do not
"improve" or "fix" any of these without an explicit decision to change
direction.

## Why does opening a file take zero configuration?

`xdg-open README.md` must produce a rendered document with no dialogs, no
onboarding, no vault setup. The first-run experience is the document.

## Why do external links not open on render or hover?

A viewer that fetches remote content implicitly is a browser with extra
steps and a privacy leak. External URLs open in the default browser on
explicit click only. Remote images referenced by a document render as
placeholders unless the user opts in (per-document, visible affordance).

## Why is there one document per window?

Matches GNOME app conventions and keeps per-window resource accounting
honest (Apostrophe leaked shared class-attribute state across windows).
Tabs are a possible post-MVP decision, not an MVP gap.

## Why does the theme follow the system by default?

libadwaita style manager is the source of truth: light/dark/high-contrast
follow the desktop, with an optional in-app override. The rendered document
restyles live without a reload flash.
