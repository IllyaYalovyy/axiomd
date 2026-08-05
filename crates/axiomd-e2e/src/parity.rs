//! The packaged axiomd measured against the native one (issue #36).
//!
//! The flatpak is a supported distribution (`design_decisions.md`), and a supported
//! distribution that is quietly slower than the recommended one is a defect nobody has
//! a number for. This module is how it gets one: every metric here is measured on both
//! forms, in the same run, on the same machine, and printed as one line with the
//! overhead between them.
//!
//! # Both forms, alternately
//!
//! A machine's mood drifts over the minutes a suite takes, so measuring every native
//! sample and then every packaged one compares the moods as much as the forms. [`time`]
//! therefore alternates: native, packaged, native, packaged, and holds the middle of
//! each to its own answer.
//!
//! # The clock starts before the sandbox exists
//!
//! A launch is timed with [`App::launched_in`](crate::App::launched_in) rather than with
//! the application's own [`App::startup`](crate::App::startup), and that is the whole
//! point of the exercise: building a sandbox happens before there is an application to
//! ask, so the number axiomd reports about itself is exactly the number that cannot see
//! the overhead being measured.
//!
//! # One ceiling, and what it aims at
//!
//! Only the packaged form is held to anything. The native form is already pinned by the
//! perf budgets (`crates/axiomd-app/tests/perf.rs`) and pinning it twice would fail this
//! run for a native measurement that has nothing to do with the package. So a
//! [`Metric`]'s budget is the packaged ceiling, and what it *aims at* is the native
//! budget it is the parity partner of: parity is reached when the packaged form fits in
//! the native ceiling, and every run says how far there is left to go.
//!
//! Like every ceiling in this project, it only ever comes down.

use std::path::Path;
use std::time::Duration;

use crate::App;
use crate::budget::{self, Budget, Unit};

/// Which axiomd a measurement was made on.
///
/// Carries how to start one, so a metric is written once and measured twice: the test
/// says what it wants a launch to do, and the form says what a launch *is*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// The binary built beside this test — what `scripts/install.sh` installs, what the
    /// owner runs, and what the perf budgets are measured on.
    Native,
    /// The flatpak installed on this machine, in its own sandbox.
    Packaged,
}

impl Form {
    /// Starts this form showing `document`, and returns once it is on screen.
    ///
    /// The route a reader takes who already has the file: the application is given a
    /// path it can read.
    pub fn launch(self, document: &Path) -> App {
        match self {
            Form::Native => crate::launch(document),
            Form::Packaged => crate::launch_installed_flatpak(document),
        }
    }

    /// The same, launched the way the desktop launches it — a double-click in Files.
    ///
    /// The two forms differ here and the difference is the metric: the native build is
    /// handed the document's own path, while the package is handed a name the document
    /// portal invented for it, on a fuse filesystem, after a round trip to that portal
    /// (issue #22). What the reader waits for is the same document either way.
    pub fn from_the_desktop(self, document: &Path) -> App {
        match self {
            Form::Native => crate::launch(document),
            Form::Packaged => crate::launch_installed_flatpak_from_the_desktop(document),
        }
    }

    /// What a parity line and the committed table call it.
    fn name(self) -> &'static str {
        match self {
            Form::Native => "native",
            Form::Packaged => "flatpak",
        }
    }
}

/// One row of the parity table: a thing a reader waits for, measured on both forms.
#[derive(Debug, Clone, Copy)]
pub struct Metric {
    /// Its name, which a parity line and the committed table both print.
    pub what: &'static str,
    /// What it measures, in the words the table explains it to a reader in.
    pub measures: &'static str,
    /// The ceiling the packaged form is held to, aiming at the native budget this is
    /// the parity partner of. See the module note.
    pub packaged: Budget,
}

/// Measures `sample` on both forms and holds the packaged one to `metric`'s ceiling.
///
/// Each form is sampled [`budget::SAMPLES`] times, alternately, and the middle answer of
/// each is what is reported — one wall-clock reading of an application launching on a
/// machine that is also running a compositor and a browser engine is noise.
pub fn time(metric: &Metric, mut sample: impl FnMut(Form) -> Duration) {
    assert_eq!(
        metric.packaged.unit,
        Unit::Time,
        "{} is measured in time and its budget is not",
        metric.what,
    );
    budget::only_in_a_release_build(metric.what);

    let mut native = Vec::new();
    let mut packaged = Vec::new();
    for _ in 0..budget::SAMPLES {
        native.push(sample(Form::Native).as_micros() as u64);
        packaged.push(sample(Form::Packaged).as_micros() as u64);
    }

    let note = format!("{} samples each", budget::SAMPLES);
    report(metric, middle(native), middle(packaged), &note);
}

/// The same for a level rather than a race: each form is read once, because memory is
/// not a thing that happens faster or slower.
pub fn memory(metric: &Metric, mut read: impl FnMut(Form) -> u64) {
    assert_eq!(
        metric.packaged.unit,
        Unit::Memory,
        "{} is measured in memory and its budget is not",
        metric.what,
    );
    budget::only_in_a_release_build(metric.what);

    let native = read(Form::Native);
    let packaged = read(Form::Packaged);
    report(metric, native, packaged, "read once each");
}

/// The parity table as the committed evidence holds it — generated from the metrics the
/// harness pins, so a ceiling that moves without the evidence moving fails the gate.
pub fn table(metrics: &[Metric]) -> String {
    let mut out = String::from(
        "| Metric | What it measures | Flatpak ceiling | Parity is |\n\
         | --- | --- | --- | --- |\n",
    );
    for metric in metrics {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            metric.what,
            metric.measures,
            metric.packaged.show(metric.packaged.held_to),
            metric.packaged.show(metric.packaged.aiming_at),
        ));
    }
    out
}

/// The one line a parity metric leaves behind, and the assertion the gate reads.
///
/// Both numbers and the multiple between them, because the multiple is the thing being
/// worked down and a reader of the gate's output should not have to divide.
fn report(metric: &Metric, native: u64, packaged: u64, note: &str) {
    let budget = metric.packaged;
    let overhead = match native {
        0 => "immeasurable".to_owned(),
        _ => format!("×{:.2}", packaged as f64 / native as f64),
    };
    let distance = match packaged <= budget.aiming_at {
        true => "at parity".to_owned(),
        false => format!("parity {}", budget.show(budget.aiming_at)),
    };
    println!(
        "parity: {what:.<48} {native_form} {:>10}  {packaged_form} {:>10}  {overhead:>7}  \
         (ceiling {}; {distance}; {note})",
        budget.show(native),
        budget.show(packaged),
        budget.show(budget.held_to),
        what = metric.what,
        native_form = Form::Native.name(),
        packaged_form = Form::Packaged.name(),
    );
    budget::enforce(metric.what, packaged, budget);
}

/// The middle of what was measured — a reading that really happened, rather than an
/// average two outliers can move.
fn middle(mut samples: Vec<u64>) -> u64 {
    samples.sort_unstable();
    samples[samples.len() / 2]
}
