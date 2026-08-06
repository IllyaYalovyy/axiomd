//! What a test run is allowed to do to the session it runs in: nothing (issue #44).
//!
//! The owner found 1,026 `app-flatpak-io.github.etf.axiomd-*.scope` units started in
//! their own desktop session in a day, every one of them a launch from a gate run, and
//! ruled that a test must never be visible as an instance of the application. Three ways
//! out of a launch had to be closed for that, and each one is asserted here from the
//! running application's own answer about itself:
//!
//! * the **display** it draws on is this launch's compositor, and no other compositor is
//!   reachable from a sandbox at all;
//! * the **session bus** it registered on is this launch's own, so it can neither see the
//!   copy of axiomd the reader has open nor be seen by it;
//! * the **session unit** it runs under is not one named after the application, which is
//!   what makes a launch count as a running app on the desktop.
//!
//! # The desktop nothing may reach
//!
//! Half of these tests run beside a [`Desktop`](axiomd_e2e::Desktop): a stand-in for the
//! developer's own — a session bus with an axiomd already registered on it, and the
//! compositor that axiomd draws on. It is what a launch would find if containment were
//! missing, and it is *watchable*: a document forwarded into it shows up as a window in
//! that axiomd, where a test can assert it, instead of on the desktop of whoever is
//! running the gate.
//!
//! The fourth way of launching — the copy `scripts/install.sh` leaves in a prefix — is
//! asserted where that prefix already exists, in `packaging.rs`.
//!
//! Every launch is already held to this at the moment it connects — the harness refuses a
//! launch that got out — so a containment defect fails these tests twice over: once in
//! what they assert, and once in the launch never being handed over at all.

use axiomd_e2e::{Desktop, Fixture};

/// The application id, which is also the bus name a copy claims and the name a session
/// scope is called after.
const APP_ID: &str = "io.github.etf.axiomd";

/// The whole of what containment means, asserted of one launch however it was started.
///
/// `sealed` says whether this launch lives in a world of its own — true of a sandbox,
/// which has a filesystem namespace and must hold no other compositor at all; false of a
/// launch of the binary itself, which runs in the developer's own filesystem and can see
/// their session's socket sitting there whether it connects to it or not.
fn is_contained(app: &axiomd_e2e::App, how: &str, sealed: bool) {
    let where_it_is = app.whereabouts();

    assert_eq!(
        where_it_is.backend, "GdkWaylandDisplay",
        "{how} did not open a Wayland display, so nothing else here is a statement \
         about the compositor this test started: {where_it_is:?}",
    );
    assert!(
        where_it_is.display.starts_with(std::env::temp_dir()),
        "{how} is drawing on {}, which is not a compositor this test started",
        where_it_is.display.display(),
    );
    assert!(
        !where_it_is.scope.contains(APP_ID),
        "{how} runs under the session unit {}, so the desktop is counting this test as \
         a running application — which is the defect issue #44 reports",
        where_it_is.scope,
    );
    if sealed {
        assert_eq!(
            where_it_is.strays,
            Vec::<std::path::PathBuf>::new(),
            "{how} can reach a compositor that is not the one this test started, so one \
             lost WAYLAND_DISPLAY would put its window on the developer's screen",
        );
    }
}

/// A launch of the binary this test was built beside — the one every other suite drives.
#[test]
fn a_launch_draws_on_the_compositor_this_test_started_and_registers_on_no_bus() {
    let fixture = Fixture::new("contained-native");
    let document = fixture.write("note.md", "# Contained\n\nA paragraph.\n");

    let app = axiomd_e2e::launch(&document);

    is_contained(&app, "a launch of the built binary", false);
    assert_eq!(
        app.whereabouts().bus,
        None,
        "a launch of the built binary registered on a session bus; with one to reach, a \
         single-instance application hands its document to the copy the reader has open",
    );
    assert!(app.close().is_empty(), "the launch left something running");
}

/// The developer's own copy, and a launch beside it: the launch has to be invisible to
/// it, and it has to be invisible to the launch.
///
/// This is the scenario in the report — a gate run on a machine where axiomd is open —
/// with the developer's session stood in for so the assertion can be made at all.
#[test]
fn a_launch_beside_the_readers_own_axiomd_neither_reaches_it_nor_is_reached_by_it() {
    let fixture = Fixture::new("contained-beside");
    let reading = fixture.write("reading.md", "# What the reader has open\n\nTheirs.\n");
    let probe = fixture.write("probe.md", "# What the test opened\n\nThe test's.\n");

    let desktop = Desktop::with_axiomd_open(&reading);
    assert_eq!(
        desktop.axiomd().window_count(),
        1,
        "the reader's own axiomd did not open their document",
    );

    let app = axiomd_e2e::launch(&probe);

    // What the test opened is in the test's own copy…
    assert_eq!(
        app.dom_text("h1"),
        "What the test opened",
        "the launch is not showing the document it was given",
    );
    // …and the reader's copy is untouched: no second window, and still their document.
    assert_eq!(
        desktop.axiomd().window_count(),
        1,
        "the test's document opened as a window in the reader's own axiomd",
    );
    assert_eq!(
        desktop.axiomd().dom_text("h1"),
        "What the reader has open",
        "the reader's own axiomd is showing the test's document",
    );
    // And nothing of the launch is on the desktop's compositor either.
    assert_ne!(
        app.whereabouts().display,
        desktop.axiomd().whereabouts().display,
        "the launch drew on the compositor the reader's own axiomd draws on",
    );

    assert!(app.close().is_empty(), "the launch left something running");
}

/// The same, for the packaged application — the launch the report is actually about.
///
/// A `flatpak run` is where every one of the three ways out was open: it asks the
/// session's own service manager for a scope named after the application, flatpak proxies
/// whatever session bus is there whatever the environment says, and `--socket=wayland`
/// mounts the session's compositor into the sandbox beside the harness's.
#[test]
#[ignore = "drives the installed flatpak; run by scripts/quality.d/40-flatpak.sh"]
fn the_packaged_launch_is_contained_beside_the_readers_own_axiomd() {
    let fixture = Fixture::new("contained-flatpak");
    let reading = fixture.write("reading.md", "# What the reader has open\n\nTheirs.\n");
    let probe = fixture.write("probe.md", "# What the test opened\n\nThe test's.\n");

    let desktop = Desktop::with_axiomd_open(&reading);
    let app = axiomd_e2e::launch_installed_flatpak(&probe);

    is_contained(&app, "the packaged launch", true);
    assert_eq!(
        app.dom_text("h1"),
        "What the test opened",
        "the packaged launch is not showing the document it was given",
    );
    assert_eq!(
        desktop.axiomd().window_count(),
        1,
        "the packaged launch handed its document to the reader's own axiomd",
    );
    assert_eq!(
        desktop.axiomd().dom_text("h1"),
        "What the reader has open",
        "the reader's own axiomd is showing the packaged launch's document",
    );

    assert!(
        app.close().is_empty(),
        "the packaged launch left something running",
    );
}

/// And the route a double-click in Files takes into the package, which needs a document
/// portal and therefore a session — this one has a session of its own, portal and all.
#[test]
#[ignore = "drives the installed flatpak; run by scripts/quality.d/40-flatpak.sh"]
fn the_packaged_launch_through_the_portal_is_contained_too() {
    let fixture = Fixture::new("contained-portal");
    let reading = fixture.write("reading.md", "# What the reader has open\n\nTheirs.\n");
    let probe = fixture.write("probe.md", "# Through the portal\n\nThe test's.\n");

    let desktop = Desktop::with_axiomd_open(&reading);
    let app = axiomd_e2e::launch_installed_flatpak_from_the_desktop(&probe);

    is_contained(&app, "the packaged launch through the portal", true);
    assert_eq!(
        app.dom_text("h1"),
        "Through the portal",
        "the launch through the portal is not showing the document it was given",
    );
    assert_eq!(
        desktop.axiomd().window_count(),
        1,
        "the launch through the portal handed its document to the reader's own axiomd",
    );

    assert!(
        app.close().is_empty(),
        "the launch through the portal left something running",
    );
}
