# UX decisions

UX decision record in FAQ form. Every entry here is an **intentional
decision — not a limitation, a gap, or a bug to be fixed.** Do not
"improve" or "fix" any of these without an explicit decision to change
direction.

## Why does opening a file take zero configuration?

`xdg-open README.md` must produce a rendered document with no dialogs, no
onboarding, no vault setup. The first-run experience is the document.

## Why are there no modal questions, ever?

Owner decision (2026-08-02): every document renders best-effort,
immediately. The app never interrupts opening or rendering with a blocking
dialog — Apostrophe's preview-security modal is the named anti-pattern.
Anything degraded (unrenderable block, unloaded remote image, failed
plugin) shows inline with a one-click affordance to resolve it. If a
feature seems to need a modal question, the design is wrong.

## Why do external links not open on render or hover?

An app that fetches remote content implicitly is a browser with extra
steps and a privacy leak. External URLs open in the default browser on
explicit click only. Remote images render as placeholder cards that ARE
the one-click load button (per image), with a single inline "load all"
affordance per document — never a modal, never a setting hunt.

## Why is there one document per window?

Matches GNOME app conventions and keeps per-window resource accounting
honest (Apostrophe leaked shared class-attribute state across windows).
Tabs are a possible post-MVP decision, not an MVP gap.

## Why does the theme follow the system by default?

libadwaita style manager is the source of truth: light/dark/high-contrast
follow the desktop, with an optional in-app override. The rendered document
restyles live without a reload flash.
