---
name: Task
about: A scoped unit of work with tests and exit criteria
title: ""
labels: []
---

## Goal

<!-- One paragraph: what should be true when this is done, and for whom. -->

## Scope

- <!-- In-scope bullets: the exact surface this task may touch. -->

Out of scope / frozen:

- <!-- Subsystems this task must NOT touch, and open choices the worker may
     not make. If implementation surfaces a reason to cross a line, STOP and
     flag it in the report — do not decide silently. -->

## Tests (red first)

- <!-- Each test named at the layer where the risk lives, demonstrated RED
     before the change and GREEN after. -->

## Exit criteria

- [ ] <!-- Observable, checkable statements. "Works" is not a criterion. -->
- [ ] Every behavior above is covered by an automated test (manual checks
      may supplement, never substitute); untestable behavior escalated per
      docs/TESTING.md, not shipped.
- [ ] `./scripts/quality.sh` green.
