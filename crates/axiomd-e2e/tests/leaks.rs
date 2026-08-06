//! The backstop the gate ends with: nothing this run started is still running.
//!
//! The harness ends every launch it owns, and would still have left processes behind on
//! the paths where none of it runs — a worker killed mid-gate, a hook that exits on a
//! failing probe, a suite stopped by hand. The owner met the result of that: axiomd
//! processes in their session that nobody could account for (issue #44).
//!
//! What is tested here is the sweep itself, on a process that is deliberately orphaned
//! and looks exactly like a launch's: a sweep nobody has seen fail is a sweep that might
//! be finding nothing at all.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// The sweep the gate runs, found from this test rather than named twice, so moving it
/// breaks the test instead of silently unhooking the backstop.
fn sweep_script() -> PathBuf {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the repository root above crates/axiomd-e2e");
    let script = repo.join("scripts/leak-sweep.sh");
    assert!(script.is_file(), "the sweep is missing from {script:?}");
    script
}

/// One run of the sweep over one test's own fixtures.
///
/// Narrowed to `mark` because the whole suite runs at once: other targets are driving
/// real launches while this one is deciding what a leak is, and a sweep that ended those
/// would be this file causing the failures it exists to catch. What the gate runs is the
/// same sweep with the mark every launch carries.
fn run(command: &str, baseline: &Path, mark: &str) -> (bool, String) {
    let output = Command::new(sweep_script())
        .arg(command)
        .arg(baseline)
        .arg(mark)
        .output()
        .expect("run the leak sweep");
    (
        output.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        ),
    )
}

/// A process that looks exactly like something a launch left behind: it lives in a
/// harness scratch directory's world, which is the whole of what the sweep goes by.
fn orphan(label: &str) -> Child {
    let scratch = std::env::temp_dir().join(format!("{}orphan", mark(label)));
    Command::new("sleep")
        .arg("300")
        .env("AXIOMD_TEST_CONTROL", scratch.join("control.sock"))
        // Nothing of this test's own: a process left behind holding the suite's output
        // open keeps whoever ran it waiting for a run that has already finished — which
        // is the very complaint this file is about, arriving from the other direction.
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start an orphan for the sweep to find")
}

/// What this test's own fixtures are marked with, and nothing else on the machine is: a
/// harness scratch directory named after this test in this process.
fn mark(label: &str) -> String {
    format!("axiomd-e2e-leaks-{label}-{}-", std::process::id())
}

fn baseline_file(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("axiomd-leak-{label}-{}", std::process::id()))
}

/// The exit criterion: a run that leaves an axiomd of its own behind fails the gate.
#[test]
fn a_process_left_behind_by_a_run_fails_the_sweep() {
    let baseline = baseline_file("left-behind");
    let (took, _) = run("baseline", &baseline, &mark("left-behind"));
    assert!(took, "the sweep could not record what was already running");

    let mut left_behind = orphan("left-behind");
    let (passed, said) = run("sweep", &baseline, &mark("left-behind"));

    assert!(
        !passed,
        "the sweep passed a run that left a process of its own running: {said}",
    );
    assert!(
        said.contains(&left_behind.id().to_string()),
        "the sweep did not say which process it found: {said}",
    );

    // And it is gone: a sweep that reported the mess and left it would still be handing
    // the developer a session full of test copies.
    let ended = std::time::Instant::now();
    while ended.elapsed() < std::time::Duration::from_secs(5) {
        if left_behind.try_wait().expect("poll the orphan").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let still_running = left_behind.try_wait().expect("poll the orphan").is_none();
    let _ = left_behind.kill();
    let _ = left_behind.wait();
    let _ = std::fs::remove_file(&baseline);
    assert!(
        !still_running,
        "the sweep found the process this run left behind and left it running: {said}",
    );
}

/// And the other half, without which the test above proves only that the sweep always
/// fails: a run that cleans up after itself passes.
#[test]
fn a_run_that_left_nothing_passes_the_sweep() {
    let baseline = baseline_file("clean");
    assert!(run("baseline", &baseline, &mark("clean")).0);

    let (passed, said) = run("sweep", &baseline, &mark("clean"));
    let _ = std::fs::remove_file(&baseline);

    assert!(passed, "the sweep failed a run that left nothing: {said}");
}

/// What the sweep must never do: blame this run for something that was already running.
///
/// The person running the gate may well have their own axiomd open, and a second gate
/// run in another terminal is not this run's leak either. Only what appeared *after* the
/// baseline belongs to the run.
#[test]
fn the_sweep_leaves_alone_what_was_running_before_the_run() {
    let mut theirs = orphan("theirs");
    let baseline = baseline_file("theirs");
    assert!(run("baseline", &baseline, &mark("theirs")).0);

    let (passed, said) = run("sweep", &baseline, &mark("theirs"));
    let still_running = theirs.try_wait().expect("poll it").is_none();
    let _ = theirs.kill();
    let _ = theirs.wait();
    let _ = std::fs::remove_file(&baseline);

    assert!(
        passed,
        "the sweep blamed this run for a process that was already running: {said}",
    );
    assert!(
        still_running,
        "the sweep ended a process that was running before it took its baseline",
    );
}
