//! The other backstop the gate ends with: nothing this run started crashed.
//!
//! The harness fails the test whose application dies (`crash.rs`), which covers every
//! death a test is watching for. This covers the rest — a web process that goes down
//! after the test that opened it finished, a hook that shells out rather than driving a
//! launch — by asking the machine what it dumped since the run began.
//!
//! What is tested here is the sweep itself, on a dump that is deliberately planted: a
//! sweep nobody has seen fail is a sweep that might be finding nothing at all. That was
//! not hypothetical on 2026-08-05, when eleven core dumps of axiomd were written in one
//! day under gates that all reported success (issue #45).

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The sweep the gate runs, found from this test rather than named twice, so moving it
/// breaks the test instead of silently unhooking the backstop.
fn sweep_script() -> PathBuf {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the repository root above crates/axiomd-e2e");
    let script = repo.join("scripts/coredump-sweep.sh");
    assert!(script.is_file(), "the sweep is missing from {script:?}");
    script
}

/// One run of the sweep, over this test's own dumps and nobody else's.
///
/// Narrowed to `pattern` because the whole suite runs at once: other targets are driving
/// real launches beside this one, and a sweep that answered for those would be this file
/// reporting failures it did not cause — or worse, passing because somebody else's dump
/// arrived first. What the gate runs is the same sweep with the pattern that names every
/// axiomd.
fn run(command: &str, baseline: &Path, pattern: &str) -> (bool, String) {
    let output = Command::new(sweep_script())
        .arg(command)
        .arg(baseline)
        .arg(pattern)
        .output()
        .expect("run the coredump sweep");
    (
        output.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        ),
    )
}

/// What this test's own dumps are recognised by, and nothing else on the machine is.
fn pattern(label: &str) -> String {
    format!("/crashes-{label}-{}$", std::process::id())
}

fn baseline_file(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("axiomd-coredump-{label}-{}", std::process::id()))
}

/// A process that will dump core the moment it is told to, under a name of this test's
/// own — deliberately not `axiomd`, so the gate's own sweep running over this very test
/// does not find it and fail the run.
fn about_to_crash(label: &str) -> (Child, PathBuf) {
    let scratch = std::env::temp_dir().join(format!("axiomd-coredumps-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("a place for the fixture");
    // Copied rather than linked: what `coredumpctl` records is the path the process was
    // started from, and it has to be a path this test alone can be blamed for.
    let fixture = scratch.join(format!("crashes-{label}-{}", std::process::id()));
    std::fs::copy("/usr/bin/sleep", &fixture).expect("a fixture to crash");

    // `ETXTBSY` and nothing else: this process writes the fixture on one thread while
    // another may be forking, and a child that inherits the still-open descriptor cannot
    // exec the file it points at. It is a race in `fork`+`exec` rather than in the
    // fixture, it clears as soon as the writing thread's descriptor is closed, and
    // waiting it out is the documented remedy.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match Command::new(&fixture)
            .arg("120")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => return (child, fixture),
            Err(error)
                if error.kind() == std::io::ErrorKind::ExecutableFileBusy
                    && Instant::now() < deadline => {}
            Err(error) => panic!("start the fixture: {error}"),
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Crashes it, and returns once the dump has been recorded — so what the sweep is asked
/// about is a dump that exists rather than one on its way.
fn crash(child: &mut Child) {
    let pid = child.id();
    // SAFETY: a signal to a process this test started itself. `SIGABRT` because it is
    // one of the signals that leaves a core behind, which is what is being planted.
    unsafe {
        libc::kill(pid as i32, libc::SIGABRT);
    }
    let _ = child.wait();

    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        let recorded = Command::new("coredumpctl")
            .args(["info", &pid.to_string()])
            .output()
            .expect("ask coredumpctl about the fixture");
        if recorded.status.success() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("the fixture crashed but no dump was ever recorded for {pid}");
}

/// The exit criterion: a dump written during a run fails that run.
#[test]
fn a_dump_written_during_a_run_fails_the_sweep() {
    let pattern = pattern("planted");
    let baseline = baseline_file("planted");
    let (took, _) = run("baseline", &baseline, &pattern);
    assert!(took, "the sweep could not record when the run began");

    let (mut fixture, path) = about_to_crash("planted");
    let pid = fixture.id();
    crash(&mut fixture);
    let (passed, said) = run("sweep", &baseline, &pattern);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&baseline);

    assert!(
        !passed,
        "the sweep passed a run that dumped core during it: {said}",
    );
    assert!(
        said.contains(&pid.to_string()),
        "the sweep did not say which process crashed: {said}",
    );
    // The backtrace summary, so the failure is the start of the investigation rather
    // than an instruction to go and start one.
    assert!(
        said.contains("Stack trace of thread"),
        "the sweep reported a crash without any of the stack it died on: {said}",
    );
}

/// And the other half, without which the test above proves only that the sweep always
/// fails: a run that crashed nothing passes.
#[test]
fn a_run_that_crashed_nothing_passes_the_sweep() {
    let pattern = pattern("clean");
    let baseline = baseline_file("clean");
    assert!(run("baseline", &baseline, &pattern).0);

    let (passed, said) = run("sweep", &baseline, &pattern);
    let _ = std::fs::remove_file(&baseline);

    assert!(
        passed,
        "the sweep failed a run that crashed nothing: {said}"
    );
}

/// A sweep the gate never runs is not a backstop, and nothing about it looks wrong: the
/// script is there, its own tests pass, and the gate reports success over every dump.
/// That is the shape issue #45 arrived in, so the wiring is asserted rather than assumed
/// — the gate has to take a baseline before it runs anything and sweep against it after.
#[test]
fn the_quality_gate_takes_a_baseline_and_sweeps_against_it() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the repository root above crates/axiomd-e2e");
    let gate = std::fs::read_to_string(repo.join("scripts/quality.sh")).expect("read the gate");

    let sweep = sweep_script();
    let name = sweep
        .strip_prefix(repo)
        .expect("the sweep lives in the repository")
        .display()
        .to_string();
    for command in ["baseline", "sweep"] {
        assert!(
            gate.contains(&format!("{name} {command}")),
            "the quality gate never runs `{name} {command}`, so a run can dump core \
             and still be green — which is the defect issue #45 reports",
        );
    }
    // Before anything runs, or the run's own crashes are indistinguishable from the
    // ones that were already on the machine.
    let baseline = gate.find(&format!("{name} baseline")).expect("a baseline");
    let checks = gate
        .find("run_shell_syntax_checks\n")
        .expect("the first check");
    assert!(
        baseline < checks,
        "the gate takes its coredump baseline after it has started running checks",
    );
}

/// What the sweep must never do: blame this run for a crash that happened before it.
///
/// The machine keeps its dumps for days, and the developer's own axiomd may well have
/// crashed yesterday. Only what was dumped after the baseline belongs to the run — which
/// is also what stops the sweep from failing every gate for ever after the first crash
/// it catches.
#[test]
fn the_sweep_leaves_alone_a_crash_from_before_the_run() {
    let pattern = pattern("earlier");
    let (mut fixture, path) = about_to_crash("earlier");
    crash(&mut fixture);

    let baseline = baseline_file("earlier");
    assert!(run("baseline", &baseline, &pattern).0);
    let (passed, said) = run("sweep", &baseline, &pattern);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&baseline);

    assert!(
        passed,
        "the sweep blamed this run for a crash that happened before it began: {said}",
    );
}
