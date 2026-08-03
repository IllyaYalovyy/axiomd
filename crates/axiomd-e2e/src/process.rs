//! Finding what a launch left behind.
//!
//! Closing a window must free everything it owned, and closing the application must
//! leave nothing running: not axiomd, not the web process that rendered its
//! documents, not the network process beside it. Asserting that needs a way to name
//! *this* launch's processes and no others, on a machine where the developer may well
//! have their own axiomd open.
//!
//! The name is the control socket. It is unique to one launch — it lives in that
//! launch's own scratch directory — and it is in the environment of the application
//! and, by inheritance, of every process the application starts. So the processes
//! belonging to a launch are exactly the ones whose environment mentions its socket,
//! whatever they are called and however they were started.

use std::path::Path;

/// The processes still alive that were started by the launch owning `socket`.
///
/// Reads only this user's own processes; a process that disappears while the
/// directory is being read is simply gone, which is the answer being looked for.
pub(crate) fn launched_with(socket: &Path) -> Vec<u32> {
    let needle = format!("AXIOMD_TEST_CONTROL={}", socket.display());
    let Ok(entries) = std::fs::read_dir("/proc") else {
        panic!("this harness needs /proc to tell whether a launch left anything behind");
    };

    let mut alive = Vec::new();
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(environment) = std::fs::read(entry.path().join("environ")) else {
            continue;
        };
        if environment
            .split(|byte| *byte == 0)
            .any(|variable| variable == needle.as_bytes())
        {
            alive.push(pid);
        }
    }
    alive.sort_unstable();
    alive
}
