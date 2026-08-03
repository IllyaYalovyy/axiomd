//! The gate hook's half of the golden contract.
//!
//! `golden.rs` proves the harness never writes a picture nobody approved. This
//! proves the other half: a quality-gate run cannot be the thing that approves one.
//! Both halves matter — the first stops an accidental re-pin, the second stops a
//! deliberate one dressed up as "the gate did it".

use std::path::{Path, PathBuf};
use std::process::Command;

/// The hook the quality gate runs, found from this test rather than hard-coded, so
/// moving it breaks the test instead of silently unhooking the check.
fn hook() -> PathBuf {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the repository root above crates/axiomd-e2e");
    let hook = repo.join("scripts/quality.d/10-e2e.sh");
    assert!(
        hook.is_file(),
        "the gate hook is missing from {}",
        hook.display(),
    );
    hook
}

/// A run of the hook, with `AXIOMD_PIN_GOLDENS` set to `pin` when given.
fn run(pin: Option<&str>) -> (bool, String) {
    let mut command = Command::new(hook());
    match pin {
        Some(value) => command.env("AXIOMD_PIN_GOLDENS", value),
        None => command.env_remove("AXIOMD_PIN_GOLDENS"),
    };
    let output = command.output().expect("run the gate hook");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    (output.status.success(), said)
}

/// The exit criterion: with the human's variable set, the gate does not run. An
/// agent that sets it to re-pin a golden gets a failed gate, not a new golden.
#[test]
fn the_gate_refuses_to_run_while_goldens_could_be_re_pinned() {
    for value in ["1", "0", "yes", " "] {
        let (passed, said) = run(Some(value));

        assert!(
            !passed,
            "the gate ran with AXIOMD_PIN_GOLDENS={value:?} set, so a run could re-pin \
             a golden: {said}",
        );
        assert!(
            said.contains("AXIOMD_PIN_GOLDENS"),
            "the refusal did not say which variable stopped it: {said}",
        );
        assert!(
            said.contains("human"),
            "the refusal did not say that pinning is a human decision: {said}",
        );
        // The refusal has to come before the hook does any work: a gate that ran its
        // checks first could already have re-pinned a golden by the time it objects.
        assert!(
            !said.contains("harness contract intact"),
            "the hook finished its checks before refusing: {said}",
        );
    }
}

/// A hook the gate never runs is not a check, and nothing about it looks wrong: the
/// file is there, the gate reports success, and no one finds out until something it
/// was supposed to catch ships.
///
/// This is not hypothetical. The gate discovered hooks with `find -perm -111`, which
/// demands the execute bit for *everyone*; git records only the owner's (mode
/// 100755), so a checkout under the usual umask 027 produced `rwxr-x---` and the
/// hook was skipped without a word. So: the bit git carries is the bit that has to
/// be set, and the gate has to look for that one.
#[test]
fn the_gate_hook_is_executable_by_the_user_git_hands_it_to() {
    use std::os::unix::fs::PermissionsExt;

    let mode = std::fs::metadata(hook())
        .expect("read the gate hook's permissions")
        .permissions()
        .mode();

    assert!(
        mode & 0o100 != 0,
        "the gate hook is not executable by its owner (mode {:o}), so the quality \
         gate will pass without ever running it",
        mode & 0o777,
    );
}

/// A guard that refuses every run is not a guard, and the tests above would prove
/// nothing about it. Without the variable the hook has to get past that first check
/// and go on to the next one.
///
/// It is stopped at that next check rather than allowed to finish, because finishing
/// means shelling out to cargo — which would block on the `target/` lock this very
/// test is holding. So the hook is run with a PATH holding only what it needs to
/// reach its compositor check and not the compositor itself: it must then complain
/// about the compositor, which is only reachable by passing the pin check first.
#[test]
fn the_hook_is_not_simply_always_refusing() {
    let scratch = std::env::temp_dir().join(format!("axiomd-gate-{}", std::process::id()));
    let bin = scratch.join("bin");
    std::fs::create_dir_all(&bin).expect("create a scratch PATH");
    // Everything the hook needs to reach its compositor check — the interpreter its
    // shebang names, and the one external command it runs first — and nothing else.
    for command in ["bash", "dirname"] {
        let _ = std::fs::remove_file(bin.join(command));
        std::os::unix::fs::symlink(which(command), bin.join(command))
            .unwrap_or_else(|error| panic!("link {command}: {error}"));
    }

    let output = Command::new(hook())
        .env_remove("AXIOMD_PIN_GOLDENS")
        .env("PATH", &bin)
        .output()
        .expect("run the gate hook");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let _ = std::fs::remove_dir_all(&scratch);

    assert!(
        !said.contains("AXIOMD_PIN_GOLDENS"),
        "the hook refused a run that never set the variable: {said}",
    );
    assert!(
        said.contains("weston"),
        "the hook did not reach its compositor check, so what it did instead is \
         unknown: {said}",
    );
}

/// Where a command actually is, so the test links the real one rather than assuming
/// a directory it might not be in on another distribution.
fn which(command: &str) -> PathBuf {
    let found = Command::new("/usr/bin/env")
        .args(["sh", "-c", &format!("command -v {command}")])
        .output()
        .expect("look up a command");
    let path = String::from_utf8_lossy(&found.stdout).trim().to_owned();
    assert!(!path.is_empty(), "{command} is not on PATH");
    PathBuf::from(path)
}
