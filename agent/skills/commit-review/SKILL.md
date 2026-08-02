---
name: commit-review
description: Self-review checklist before any commit — read the full staged diff, scope honesty, out-of-scope disclosure, decision authority. Load before every commit.
---

# Commit self-review

Run this AFTER the gate is green and BEFORE `git commit`. The reviewer is
you, but the standard is an outside reviewer's: what would they need to know?

## Read the actual diff

`git diff --cached` — every hunk, file by file. Not `--stat`. You are looking
for: hunks that don't belong to the task, debug leftovers, commented-out
code, accidental reverts of files you never meant to touch, and any AI
working files (agent state, prompts, context files, chat logs — never
staged; the pre-commit guard backstops this, but do not rely on it).

## Scope honesty

- Every hunk maps to the task. Unrelated cleanup: revert it, or name it in
  the commit message as deliberate and justify it in one line.
- **Production code changed on a test-only task must be named explicitly** in
  both the commit message and the completion report — file, function, and why
  the tests forced it. Correct work merged *silently* is still a process
  defect: the reviewer approved a different change than they read.
- If the diff contradicts a design doc, an accepted RFC, or a recorded
  decision: STOP. Do not commit. Report the contradiction — one of the two is
  wrong, and that call is not yours.

## Decision authority

Open design decisions belong to the **human**, never to an agent:

- If your task is blocked on an unratified decision, stop and report
  "blocked on <decision>" — do not implement your preferred option.
- Never edit an RFC or decision record to mark a decision ratified,
  accepted, or resolved. Your report may *recommend*, with reasoning; it may
  not *decide*.
- Claims about external-system behavior are resolved only by verified
  evidence (live probe, official docs), cited in the report — never by
  choosing a plausible answer.

## The commit itself

- Follow the repository's commit rules (`docs/COMMITS.md`); the subject
  states behavior, the body states risk and any disclosure from above.
- The committed tree is byte-identical to the tree the gate passed on. If
  anything changed after the green run — even a comment — re-run the gate.
- The report includes: commit hash, checks run, red-check outputs
  (test-quality skill), skipped checks with reasons, and residual risk.
