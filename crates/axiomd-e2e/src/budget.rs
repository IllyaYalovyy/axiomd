//! A measured number, printed, held to a ceiling, and shown against the number the
//! project is aiming at.
//!
//! VISION states axiomd's performance as numbers rather than as aspirations, and
//! issue #9 makes each of them a test. This module is what a budget test is written
//! with, and it exists to make four things true of every budget in the project at
//! once.
//!
//! # Every budget prints what it measured
//!
//! A budget that only passes or fails tells nobody how close it came, and a ceiling
//! nobody can see the distance to is a ceiling nobody can ratchet. So every budget
//! prints a `perf:` line whether it passes or fails, and the gate's perf hook runs the
//! suite with `--nocapture` so those lines reach the person who ran it. The table in a
//! task report is that output.
//!
//! # There are two numbers, and they are not the same number
//!
//! A [`Budget`] carries a **ceiling** — what the gate holds a measurement to — and
//! what the project is **aiming at**, which is VISION's own figure. They start apart,
//! because a budget's first honest measurement is what axiomd costs today and VISION's
//! figure is what it is meant to cost. Printing both means every run says how far
//! there is left to go, rather than saying "green" and hiding it.
//!
//! # A ceiling only ever comes down
//!
//! Lowering a ceiling is ordinary work: something got faster, so the budget gets
//! tighter, and the gap to the target shrinks. **Raising one is a decision for the
//! project owner** (issue #9) — it means axiomd got slower and somebody accepted
//! that. A failure here says so, because the tempting fix for a failing budget is the
//! forbidden one.
//!
//! # A budget is not a flaky test
//!
//! One wall-clock reading of anything on a machine also running a compositor, a
//! browser engine and a test suite is noise. [`time`] therefore takes the measurement
//! [`SAMPLES`] times and holds the *middle* one to the ceiling, and prints the spread
//! beside it so a budget drifting towards its ceiling is visible long before it fails.
//! Memory is a level rather than a race and is read once.
//!
//! # And it is not measured in a debug build
//!
//! An unoptimised comrak is an order of magnitude off an optimised one, so a budget
//! measured in a debug build measures nothing anybody ships. Every entry point here
//! refuses to run in one rather than reporting a number that means nothing.

use std::time::Duration;

/// How many times [`time`] measures before believing an answer. Odd, so the middle one
/// is a reading that really happened rather than an average of two.
const SAMPLES: usize = 5;

/// Set to ask for the budgets that take minutes rather than seconds.
const SOAK: &str = "AXIOMD_PERF_SOAK";

/// One budget: what a measurement is held to, and what it is meant to become.
///
/// Both numbers are kept in the smallest unit the measurement has — microseconds,
/// bytes — and written in the unit a person reads, so a budget can never be compared
/// against a number that was in the other one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// The number the gate enforces. Comes down, never up (see the module note).
    held_to: u64,
    /// VISION's figure for the same thing — where the ceiling is being walked to.
    aiming_at: u64,
    unit: Unit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    Time,
    Memory,
}

impl Budget {
    /// A budget on how long something may take, written in milliseconds.
    pub const fn millis(held_to: u64, aiming_at: u64) -> Budget {
        Budget {
            held_to: held_to * 1_000,
            aiming_at: aiming_at * 1_000,
            unit: Unit::Time,
        }
    }

    /// A budget on how much memory something may use, written in mebibytes.
    pub const fn megabytes(held_to: u64, aiming_at: u64) -> Budget {
        Budget {
            held_to: held_to * 1024 * 1024,
            aiming_at: aiming_at * 1024 * 1024,
            unit: Unit::Memory,
        }
    }

    fn show(&self, amount: u64) -> String {
        match self.unit {
            Unit::Time => format!("{:.1} ms", amount as f64 / 1_000.0),
            Unit::Memory => format!("{:.0} MB", amount as f64 / (1024.0 * 1024.0)),
        }
    }
}

/// Measures `each_time` [`SAMPLES`] times, prints what it saw, and fails if the middle
/// answer is over `budget`'s ceiling.
///
/// `what` is the budget's name and belongs in a report table, so it reads as a thing
/// rather than as a test: `cold start to a typical document served`.
pub fn time(what: &str, budget: Budget, mut each_time: impl FnMut() -> Duration) {
    assert_eq!(
        budget.unit,
        Unit::Time,
        "{what} is measured in time and its budget is not",
    );
    only_in_a_release_build(what);

    let mut samples: Vec<u64> = (0..SAMPLES)
        .map(|_| each_time().as_micros() as u64)
        .collect();
    samples.sort_unstable();

    held_to(
        what,
        samples[SAMPLES / 2],
        budget,
        &format!(
            "{SAMPLES} samples, {} to {}",
            budget.show(samples[0]),
            budget.show(samples[SAMPLES - 1]),
        ),
    );
}

/// Prints how much memory `what` uses, in bytes, and fails if it is over `budget`'s
/// ceiling.
pub fn memory(what: &str, budget: Budget, measured: u64) {
    assert_eq!(
        budget.unit,
        Unit::Memory,
        "{what} is measured in memory and its budget is not",
    );
    only_in_a_release_build(what);

    held_to(what, measured, budget, "measured once");
}

/// Whether the budgets that take minutes are being measured this run.
///
/// Ten megabytes of Markdown measured five times is minutes of machine, which is too
/// long to stand between somebody and every commit. The gate's perf hook runs the
/// quick budgets; a person who wants the whole picture — before a release, or when a
/// change could touch the heavy path — sets `AXIOMD_PERF_SOAK=1` and gets all of them.
///
/// Says `what` was left out when they are not, so a budget that did not run can never
/// be mistaken for one that passed.
pub fn measuring_the_long_ones(what: &str) -> bool {
    if std::env::var_os(SOAK).is_some() {
        return true;
    }
    println!(
        "perf: {what:.<58} {:>12}  (set {SOAK}=1 to measure it)",
        "not run"
    );
    false
}

/// The one line a budget leaves behind, and the assertion that gives the gate its
/// answer.
fn held_to(what: &str, measured: u64, budget: Budget, note: &str) {
    let distance = if measured <= budget.aiming_at {
        "at target".to_owned()
    } else {
        format!("target {}", budget.show(budget.aiming_at))
    };
    println!(
        "perf: {what:.<58} {:>12}  (ceiling {}; {distance}; {note})",
        budget.show(measured),
        budget.show(budget.held_to),
    );
    assert!(
        measured <= budget.held_to,
        "\nthe budget for {what} is not met: {} against a ceiling of {}.\n\
         \n\
         This ceiling only ever comes down. Raising it means axiomd got slower and\n\
         somebody decided that is acceptable, which is a decision for the person who\n\
         owns the project and not for whoever is making this change (issue #9).\n\
         Make it fit, or stop and take the number to them.\n",
        budget.show(measured),
        budget.show(budget.held_to),
    );
}

/// Refuses to report a number from a build nobody ships.
fn only_in_a_release_build(what: &str) {
    if cfg!(debug_assertions) {
        panic!(
            "{what} was measured in a debug build, where the answer means nothing.\n\
             The perf budgets run in release: ./scripts/quality.d/20-perf.sh",
        );
    }
}
