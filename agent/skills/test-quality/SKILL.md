---
name: test-quality
description: Standards every new or modified test must meet — state assertions, mandatory red-check, forbidden fake-coverage patterns, anti-capitulation. Load before writing or changing any test.
---

# Test quality bar

A test exists to fail when the behavior it protects breaks. Every rule below
comes from a fake-coverage pattern that was actually merged somewhere and
later had to be purged.

## Assert STATE, not plumbing

Assert what the user or the system can observe: the row appears in the view,
the record holds the value, the fake server ends in the pushed state, the
panel stays open. Asserting that an internal call was *invoked* is not
coverage — it passes against a no-op implementation.

Quick self-test: **would this test still pass if the implementation were
replaced by a stub that does nothing?** If yes, it is not a test.

## Forbidden patterns

- Styling/class-name assertions as a proxy for behavior.
- Source-grep tests (asserting the code *contains* something).
- Reimplementing the production logic inside the test body and asserting the
  reimplementation agrees with itself.
- Asserting mock state that only the test itself set up.
- Duplicate tests of the same behavior at the same layer.

## Red-check (mandatory, no exceptions)

Every new test must be **demonstrated failing** against the unfixed or
stubbed code before it is shown passing, with the failure output in the
completion report. A test never seen red proves nothing — it may be asserting
vacuously. For a pure refactor, red-check the moved tests by stubbing the
extracted code once.

## Every task includes a non-happy path

At least one test per task exercises an edge: the record has children, the
field is empty, the input arrives on the second page, the response is an
error status, the value was inherited rather than set.

## Mocks mirror reality

A test double must reproduce the real system's verified behavior — no
stricter, no looser. Never encode a guess into a fake to make a test pass;
verify against the real system or its documentation first, and cite which in
the report. A permissive fake lets the whole suite pass while production is
broken.

## Anti-capitulation (load-bearing)

Never make a test pass by: deleting or weakening assertions, widening
tolerances, shrinking a generator's coverage, quarantining, or adding lint
suppressions. If you believe the TEST is wrong, say so in the report with
reasoning and stop. If the CODE is wrong, fix the code.

## Flakes are failures

Deterministic RNG with fixed seeds, no sleeps, no wall-clock time in
assertions. A flake means: stop, find the nondeterminism, fix it, re-run
enough times to prove it gone, and document the root cause in the report.
