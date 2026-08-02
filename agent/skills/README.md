# Agent skills

Skills are focused instruction sets an AI coding agent loads **at the step
where they apply**, instead of one ever-growing system prompt. A long prompt
dilutes every rule in it; a skill arrives exactly when the agent is writing a
test, preparing a commit, or claiming completion.

This directory is the **tracked source of truth**. Agents read skills from
`.claude/skills/`, which is deliberately gitignored (AI working files never
enter version control). Install / refresh the local copies with:

```bash
./scripts/install-agent-files.sh
```

`init-project.sh` runs it automatically for new projects. Re-run it after
editing anything under `agent/`.

## The core skills

- `verify` — the quality gate is ONE command; when to run it and how to
  triage each failure class. Never retyped from memory.
- `test-quality` — what a test must prove: state assertions, a mandatory
  red-check, forbidden fake-coverage patterns, anti-capitulation.
- `commit-review` — pre-commit self-review: read the whole staged diff,
  scope honesty, disclosure of out-of-scope changes, and decision authority
  (agents recommend; humans ratify).

## Domain skills

When a subsystem carries invariants that must never be traded away (a sync
engine, a billing pipeline, a crypto layer), give it its own skill so any
agent touching that code loads the rules first. Copy
`../domain-skill-example.md` into `agent/skills/<name>/SKILL.md` and fill in
the placeholders. Keep one skill per subsystem, and keep it short — a skill
is a checklist with reasons, not a design document.
