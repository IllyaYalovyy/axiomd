# Screenshot goldens

Each PNG here is a rendered surface a human looked at once and approved. From
then on it is the specification: every run captures the same surface and diffs
against it, so a visual change nobody approved fails the suite.

## Pinning is a human act

The harness will not write a file in this directory unless `AXIOMD_PIN_GOLDENS=1`
is set in the environment. That variable belongs to the person reviewing the
picture, in the same way the quality gate's skip variables do:

- **An agent may never set it.** Re-pinning to make a failing visual test pass
  is the one thing this scheme exists to prevent.
- **The quality gate refuses to run with it set at all**
  (`scripts/quality.d/10-e2e.sh`), and fails if anything in this directory
  changed during a run.

## Approving a picture

1. Run the golden test. It fails and names two files under
   `target/debug/e2e-artifacts/`: what was captured, and a map of what moved
   (changed pixels in blue, the rest faded).
2. Look at them. If the new picture is right — and only then — re-run that test
   alone with `AXIOMD_PIN_GOLDENS=1` set.
3. Commit the changed PNG. The commit message records the approval.
