//! Command-line behavior of the `axiomd` binary that can be asserted without a
//! display, so the quality gate stays headless.
//!
//! The on-screen window is asserted by the e2e harness (issue #15); these tests
//! cover what a terminal user observes.

use std::process::{Child, Command, Output, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_axiomd");

/// A command whose display connection cannot succeed, whatever the machine
/// running the suite happens to have.
///
/// `WAYLAND_DISPLAY` is an absolute path, so libwayland uses it verbatim instead
/// of falling back to the default `wayland-0` socket in `XDG_RUNTIME_DIR`, and
/// pinning `GDK_BACKEND` stops GDK from trying X11 next.
///
/// The session bus is taken away as well. axiomd is a single-instance application:
/// with a bus to reach, a second copy hands its arguments to the first and exits
/// happily — so a developer who happens to have axiomd open would see this suite
/// fail for no reason. Without a bus there is no first copy to hand anything to.
fn headless_axiomd() -> Command {
    let mut command = Command::new(BIN);
    command
        .env_remove("DISPLAY")
        .env("GDK_BACKEND", "wayland")
        .env("WAYLAND_DISPLAY", "/nonexistent/axiomd-tests-no-display")
        .env("DBUS_SESSION_BUS_ADDRESS", "disabled:")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

/// Waits for a child, turning a hang into a test failure instead of a hung gate.
fn wait_without_hanging(mut child: Child) -> Output {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match child.try_wait().expect("poll axiomd") {
            Some(_) => return child.wait_with_output().expect("collect axiomd output"),
            None if Instant::now() < deadline => sleep(Duration::from_millis(50)),
            None => {
                let _ = child.kill();
                panic!("axiomd did not exit on its own");
            }
        }
    }
}

#[test]
fn prints_its_version_without_needing_a_display() {
    let output = headless_axiomd()
        .arg("--version")
        .output()
        .expect("run axiomd --version");

    assert!(
        output.status.success(),
        "axiomd --version exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8(output.stdout).expect("axiomd --version prints utf-8");
    let reported = stdout.trim_end();
    let version = reported
        .strip_prefix("axiomd ")
        .unwrap_or_else(|| panic!("expected `axiomd <version>`, got {reported:?}"));

    let components: Vec<&str> = version.split('.').collect();
    assert!(
        components.len() == 3
            && components
                .iter()
                .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit())),
        "expected a three-part numeric version, got {version:?}",
    );
}

#[test]
fn reports_the_unusable_display_instead_of_starting_or_hanging() {
    let child = headless_axiomd().spawn().expect("spawn axiomd");
    let output = wait_without_hanging(child);

    assert!(
        !output.status.success(),
        "axiomd claimed success with no usable display; stdout: {}",
        String::from_utf8_lossy(&output.stdout),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.to_lowercase().contains("display"),
        "expected the failure to name the display; stderr: {stderr}",
    );
}

/// `--engine` naming an engine this build has not got says so, names the ones it has,
/// and stops — without a display, exactly as `--version` answers without one.
///
/// The failure this guards is the shape the flag would have had if it were not
/// declared as a command-line option: an application that HANDLES_OPEN takes every
/// argument for a file, so `--engine nonsense` would have become a document that does
/// not exist, and the reader would have got a window explaining that instead of an
/// answer about their command line.
#[test]
fn an_unknown_engine_is_reported_without_needing_a_display() {
    let output = headless_axiomd()
        .arg("--engine=no-such-engine")
        .output()
        .expect("run axiomd --engine");

    assert!(
        !output.status.success(),
        "axiomd claimed success with an engine it does not have; stdout: {}",
        String::from_utf8_lossy(&output.stdout),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no-such-engine"),
        "the failure does not name the engine that was asked for; stderr: {stderr}",
    );
    assert!(
        stderr.contains("comrak") && stderr.contains("pulldown-cmark"),
        "the failure does not say which engines this build has; stderr: {stderr}",
    );
    // Not the failure a broken display is: this is a command line that is wrong, and a
    // reader who mistyped an engine must not be told their display is at fault.
    assert!(
        !stderr.to_lowercase().contains("display"),
        "a wrong engine name was reported as a display problem; stderr: {stderr}",
    );
}
