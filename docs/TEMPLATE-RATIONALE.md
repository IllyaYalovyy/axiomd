# Template Rationale

This template distills working practices from two active AI-assisted projects:
`axiotask` and `rttx`.

## Practices Carried Forward

- **Explicit contributor rules** - identity, secrets, agent files, and push policy
  are documented at the top level instead of relying on memory.
- **RFC-driven design** - broad or hard-to-reverse changes require a written
  decision record before implementation.
- **User-task inventory** - user workflows are written as preconditions, flows,
  outcomes, and expected regression coverage.
- **Risk-based testing** - tests are selected by the failure mode they protect,
  not by a raw coverage target.
- **Local quality gate** - one command should run the checks expected before
  review, with project-specific hooks added as the stack becomes concrete.
- **Review discipline** - reviews lead with correctness, tests, privacy, and
  maintainability before style details.
- **Agent hygiene** - AI working files stay out of version control, and agent
  handoffs must state changes, checks, skipped checks, and residual risks.
- **Executable gate, not prose** - the quality gate is one script, never a
  list of commands in a prompt. A prose gate got retyped minus one flag
  (`--all-targets`), lint silently skipped all test code, and broken changes
  merged green twice before anyone noticed. A script is fixed once, for
  everyone, permanently.
- **Skills, not longer prompts** - quality rules load at the step where they
  apply (writing a test, preparing a commit, claiming completion). Every rule
  added to an always-on prompt dilutes the rest; a skill arrives with full
  attention at exactly the right moment.
- **Mandatory red-check** - every new test is demonstrated failing before it
  is shown passing, failure output in the report. A test never seen red may
  be asserting nothing; entire mocked suites have passed against broken
  production code.
- **Validate mocks against reality** - test doubles encode only verified
  behavior, probed live or cited from official docs. An unvalidated mock let
  a feature fail 100% of the time in production (a missing `Content-Length`
  header) while every mocked test stayed green.
- **Decision authority is human** - autonomous agents *recommend*; a human
  ratifies. In practice agents instructed to wait for ratification will
  ratify design decisions themselves unless the rule is explicit, loaded at
  commit time, and checked in review.
- **Conflict-matrix RFCs** - for reconciliation-style subsystems, enumerate
  every local×remote permutation with an expected outcome and a status tag
  (settled / untested / gap / decide / probe). Untagged behavior is behavior
  that exists only as an accident of implementation.

## Practices Kept Generic

The reference projects are Rust desktop applications, but this template is
stack-neutral. Rust, Node, Python, shell, and custom hooks are detected by
`scripts/quality.sh`; project-specific CI and packaging gates should be added
once the new repository chooses its technology.
