---
name: __SUBSYSTEM__-invariants
description: Rules for any change under __SUBSYSTEM_PATHS__ — the authority documents, the invariants that may never be traded away, and the mandatory verification. Load before touching this subsystem.
---

<!--
  Copy to agent/skills/<subsystem>-invariants/SKILL.md and fill in the
  placeholders. Then run ./scripts/install-agent-files.sh.

  A domain skill exists for a subsystem where a quiet bug destroys user data,
  money, or trust long after it merges. Keep it to one page: a checklist with
  reasons, not a design document.
-->

# __SUBSYSTEM__ invariants

## The authority documents

__AUTHORITY_DOCS__ (e.g. an RFC that pins expected behavior per case) is the
authority for this subsystem's semantics.

- Code that deviates from a pinned behavior is a bug **in the code**. Fix the
  code; never edit the document's expected outcome to match what the code
  does.
- If you believe a pinned behavior is genuinely wrong, STOP and report with
  reasoning. Changing the document is the human's call.

## The invariants you may never trade away

<!-- List them by name, one line of consequence each. Examples of shape:
- **Convergence** — after N runs against a quiet counterparty, state is
  identical on both sides and another run is a no-op.
- **No silent divergence** — a version marker may never claim agreement
  while content differs; that freezes the record out of every future repair.
- **Crash windows converge** — any crash between steps leaves a state the
  next run drives to the same fixpoint, without duplicates or loss.
-->
- __INVARIANT_1__
- __INVARIANT_2__

## Mandatory verification for diffs in this subsystem

__DEEP_CHECKS__ (e.g. the property/fuzz suite at raised depth, an
integration suite against the strict fake). The quality gate runs these
automatically when it detects this scope — do not bypass it.

A failing deep check means **the product code is wrong until proven
otherwise**. Forbidden responses: shrinking case counts, reducing generator
coverage, loosening the model, or marking checks ignored.

## The test double mirrors the real system exactly

Any behavioral change to this subsystem's fake requires cited evidence: a
live probe result or official documentation, named in the report. Encoding a
guess corrupts every future test that runs against it.

## Off-limits without a human decision

__SETTLED_AREAS__ (behaviors documented as settled — list them so no task
"improves" one by accident).
