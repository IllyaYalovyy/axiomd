---
name: verify
description: Run the project quality gate (./scripts/quality.sh) and triage its failures. Load before claiming any task complete, and before the final commit.
---

# The quality gate

The gate is **one command**:

```bash
./scripts/quality.sh
```

Never retype its individual checks from memory. A gate that lives as prose
gets retyped minus a flag — in the project this template distills, a
hand-typed lint command silently skipped ALL test code, and broken changes
merged green twice before anyone noticed. The script is the gate; there is no
weaker variant to drift toward.

## When

- Focused checks (one test file, one module) while iterating — fine.
- The full gate: before writing the completion report, and again if ANYTHING
  changed after the last green run. The tree you commit must be the tree the
  gate saw.

## Triage, by failure class

- **Formatting** → run the formatter, done.
- **Lint** → fix the code. Adding a suppression to silence it is capitulation
  unless the suppression carries a comment justifying why the lint is wrong
  *here* — and that justification goes in the completion report too.
- **Unit/integration test** → reproduce focused first, then fix the code.
  Never delete or weaken the failing assertion to pass (see the test-quality
  skill).
- **End-to-end / smoke** → the real application broke in a way mocked suites
  structurally cannot see. This is a product bug, not a harness flake.
  Investigate the application, not the harness.

## Hard rules

- Exit non-zero = task NOT complete. No exceptions, no "only formatting
  failed".
- If a check cannot run because the environment is broken, fix the
  environment or stop and report the blocker — do not skip the check.
- If the gate itself is wrong (a missing check, a wrong flag), do not work
  around it: stop and report, so it is fixed once, in the script, for
  everyone.
