# AI Agent Instructions

These instructions apply to AI coding agents working in this repository.

## Operating Principles

- Read existing code and docs before proposing architecture.
- Prefer the repository's established patterns over new abstractions.
- Keep changes scoped to the task. Do not perform unrelated cleanup.
- Preserve user changes. Never revert files you did not intentionally modify.
- Use fast search tools such as `rg` before slower recursive commands.
- Testability is a MUST: no automated test, not done. Untestable behavior
  is escalated with a candidate testing approach or drop proposal — never
  shipped on a manual check (see docs/TESTING.md).
- Run `./scripts/quality.sh` before declaring implementation complete when
  practical. The gate is that one command — never retype its checks from
  memory; a prose gate loses flags in retelling.

## Skills

Focused rule sets live in `.claude/skills/` (installed from the tracked
`agent/skills/` by `./scripts/install-agent-files.sh` — run it if the
directory is missing). Load each at its step; they are the standard you are
held to:

- `verify` — before claiming completion: gate usage and failure triage.
- `test-quality` — before writing or changing ANY test: state assertions,
  mandatory red-check, forbidden fake-coverage patterns, anti-capitulation.
- `commit-review` — before every commit: full-diff self-review, scope
  honesty, out-of-scope disclosure, decision authority.
- Domain skills (`<subsystem>-invariants`) — before touching the subsystem
  they name.

## Decision Authority

Open design decisions belong to the human, never to an agent:

- Blocked on an unratified decision → stop and report "blocked on
  <decision>". Do not implement your preferred option.
- Never mark a decision ratified, accepted, or resolved in an RFC or
  decision record. Reports may recommend, with reasoning; they may not
  decide.
- Claims about external-system behavior are settled only by verified
  evidence (live probe, official documentation), cited in the report —
  never by encoding a plausible guess, least of all into a test double.

## Standard Workflow

1. **Understand** - read the request, relevant docs, existing code, tests, and
   open design records.
2. **Classify** - decide whether this is a small task, bug fix, RFC-required
   change, review, or commit-prep request.
3. **Plan** - for non-trivial work, state the concrete steps and test strategy.
4. **Implement** - keep edits scoped and preserve unrelated user changes.
5. **Verify** - run focused checks first, then `./scripts/quality.sh` when
   practical.
6. **Handoff** - summarize changed behavior, files, checks, skipped checks, and
   residual risks.

## Planning and Design

Use an RFC before implementation when the change:

- Touches multiple subsystems
- Adds or replaces dependencies
- Changes persistence, API, protocol, auth, or public behavior
- Is difficult to reverse

Use `designs/RFC-000-template.md` and keep the development plan updated as steps
complete.

Design review rules live in `docs/DESIGN-REVIEW.md`.

## Implementation Rules

- Keep commits and patches reviewable.
- Do not hard-code local paths, usernames, hostnames, secrets, or tokens.
- Do not commit agent working directories, task-runner state, prompts, context
  files, scratchpads, or chat logs. These are local-only and must not leak to
  the remote repository.
- Make failures explicit. Prefer errors with context over silent fallback.
- For UI work, verify real behavior, not only component existence.

## Commit Rules

- Read `docs/COMMITS.md` before preparing a commit.
- Verify `git config user.name` and `git config user.email`.
- Keep the local AI-file pre-commit guard installed by running
  `./scripts/install-git-hooks.sh` after cloning or reinitializing the project.
- Inspect the staged diff before committing.
- Keep commits coherent and reversible.
- Do not push unless explicitly instructed.

## Prompt Templates

Reusable prompts live in `docs/prompts/`:

- `task.md` - clarify and execute a task
- `rfc.md` - draft or revise an RFC
- `implement.md` - implement accepted work
- `review.md` - review a diff or branch
- `commit.md` - prepare a commit

## Review Before Handoff

Before handing work back:

- Summarize changed files and behavior.
- State which tests or checks were run.
- State any checks that were skipped and why.
- Call out remaining risks or follow-up work.
