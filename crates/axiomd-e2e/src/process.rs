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
//!
//! # And how much they are using
//!
//! The same naming answers the memory budgets (issue #9). "A window's memory" is not
//! one process: axiomd renders in a web process of WebKit's and fetches through a
//! network process beside it, and a budget that read only the application's own
//! resident set would report a fraction of what the machine is actually carrying.
//! [`footprint`] therefore adds up every process the launch is made of.

use std::path::Path;

/// What a launch is using right now.
///
/// One value rather than two numbers because the second is meaningless without the
/// first: memory read while a window's processes are still going away is memory that
/// is about to be freed, and the count is how a test waits for that to have happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Footprint {
    /// How many processes this launch is: the application, the web process its
    /// documents are rendered in, and the network process beside them.
    pub processes: usize,
    /// The resident memory of all of them together, in bytes.
    pub bytes: u64,
}

/// What the launch owning `socket` is using now.
pub(crate) fn footprint(socket: &Path) -> Footprint {
    let alive = launched_with(socket);
    Footprint {
        processes: alive.len(),
        bytes: alive.iter().filter_map(|pid| resident_bytes(*pid)).sum(),
    }
}

/// The resident set of one process in bytes, or `None` if it has exited while being
/// read — which is not a failure, it is the answer.
fn resident_bytes(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = status
        .lines()
        .find(|line| line.starts_with("VmRSS:"))?
        .strip_prefix("VmRSS:")?;
    // `VmRSS:\t    7276 kB` — the kernel writes this field in kibibytes whatever the
    // page size is (probed on this machine, Linux 7.1.4, against `/proc/self/status`).
    let kibibytes: u64 = line.split_whitespace().next()?.parse().ok()?;
    Some(kibibytes * 1024)
}

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
