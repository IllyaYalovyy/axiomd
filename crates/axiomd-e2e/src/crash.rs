//! An ending nobody asked for.
//!
//! The application under test may end in exactly one way: because this harness asked it
//! to, and cleanly. Every other ending is a failure of whatever test was running at the
//! time — and until issue #45 none of them was. `close()` polled the process, found it
//! already gone, threw the status away and went on to kill a process group that was no
//! longer there; a launch that had segfaulted and one that had quit when asked were the
//! same empty answer. Eleven core dumps of axiomd were written in one day underneath a
//! full day of green gates.
//!
//! So the status is kept rather than discarded, and the question it answers is asked at
//! every point a test can reach: [`Watch::check`] in every wait, so a launch that dies
//! mid-run fails the test it was running instead of taking thirty seconds to time out
//! on a condition nothing is left to satisfy, and [`Watch::end`] at teardown, so a
//! death between the last assertion and `close()` — the window in which a crash used to
//! be indistinguishable from a pass — is read before this harness's own kill can paint
//! over it.
//!
//! # Told apart from a kill of this harness's own
//!
//! A launch that will not go when asked is ended by force, and that ending is signalled
//! too. The two are told apart by order rather than by signal number: the process is
//! polled until it goes on its own, and only what is read *before* [`containment::end`]
//! runs is anybody's crash. After that, the signal is this harness's own and says
//! nothing about the application.
//!
//! # What a failure has to say
//!
//! Which signal, and where the dump is. A crash reported as "the process disappeared"
//! is a crash that gets investigated the next morning by hand, from `coredumpctl` and a
//! guess about which test was running — which is exactly what issue #45 cost. The
//! complaint therefore carries the signal by name and the path systemd-coredump stored
//! the core at, so the failing test output is the whole of the trail.

use std::os::unix::process::ExitStatusExt;
use std::process::{Child, ExitStatus};
use std::time::{Duration, Instant};

use crate::containment;

/// How long a launch is given to go once it has been asked to.
const GOES_WITHIN: Duration = Duration::from_secs(30);

/// How long systemd-coredump is given to have written the dump down. It is started by
/// the kernel as the process dies, so the death is visible here before the dump is:
/// asking once and reporting "no dump" would be a race that says the wrong thing about
/// half the crashes it catches.
const RECORDED_WITHIN: Duration = Duration::from_secs(5);

/// The application's process, watched for an ending nobody asked for.
pub(crate) struct Watch {
    axiomd: Child,
    /// The ending, once there is one — remembered rather than re-read, because a
    /// reaped child cannot be polled twice and because the answer must not change
    /// after this harness's own kill.
    ending: Option<ExitStatus>,
    /// Whether the signal that ended it was this harness's.
    ours: bool,
    /// Whether [`Watch::end`] has already run. A launch is ended once: a second pass
    /// would signal a process group this harness no longer owns, and the pid of a
    /// reaped child belongs to whoever the kernel hands it to next.
    over: bool,
}

impl Watch {
    pub(crate) fn over(axiomd: Child) -> Watch {
        Watch {
            axiomd,
            ending: None,
            ours: false,
            over: false,
        }
    }

    /// Whether the launch has ended without being asked to, and what to say about it.
    ///
    /// `None` while it is still running, which is every call in a passing test.
    pub(crate) fn check(&mut self) -> Option<String> {
        let ending = self.poll()?;
        Some(match ending.signal() {
            Some(signal) => self.died_of(signal),
            None => format!(
                "axiomd exited with {ending} on its own, before this test asked it to \
                 quit.\n  An application that ends by itself under test has failed \
                 whatever the test was doing at the time.",
            ),
        })
    }

    /// Sees the launch out — it has been asked to quit — and says whether it went the
    /// way it was asked.
    ///
    /// Waits for it to go by itself first, so the ending that is read is its own. Only
    /// a launch that ignores the request is ended by force, and its process group with
    /// it: a launch is a sandbox around a bus daemon around an application around the
    /// web process that renders its documents, and ending only the process this harness
    /// holds leaves the rest of that tree running in the developer's session (issue
    /// #44).
    pub(crate) fn end(&mut self) -> Option<String> {
        if self.over {
            return None;
        }
        self.over = true;
        let deadline = Instant::now() + GOES_WITHIN;
        while self.poll().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        // Whatever it is going to be, the ending is decided by now: anything after this
        // line is this harness ending what would not end.
        self.ours = self.ending.is_none();
        containment::end(&mut self.axiomd);
        if self.ours {
            return None;
        }

        let ending = self.ending?;
        match ending.signal() {
            Some(signal) => Some(self.died_of(signal)),
            None if ending.success() => None,
            None => Some(format!(
                "axiomd exited with {ending} when it was asked to quit.\n  \
                 Quitting is not a failure and must not be reported as one.",
            )),
        }
    }

    /// The process this launch is, for the harness's own bookkeeping.
    pub(crate) fn pid(&self) -> u32 {
        self.axiomd.id()
    }

    /// The ending, read once and then remembered.
    fn poll(&mut self) -> Option<ExitStatus> {
        if self.ending.is_none() {
            self.ending = self.axiomd.try_wait().expect("poll axiomd");
        }
        self.ending
    }

    /// The whole of what a crash has to say for it to be worth anything afterwards.
    fn died_of(&self, signal: i32) -> String {
        format!(
            "axiomd died of {} — it was not asked to end and it did not end itself.\n  \
             {}\n  A crash under test is a failed test: this is the defect, not the \
             assertion that noticed it.",
            named(signal),
            dump_of(self.pid()),
        )
    }
}

/// The signal by the name the reader will search for, and never only a number.
fn named(signal: i32) -> String {
    let name = match signal {
        libc::SIGHUP => "SIGHUP",
        libc::SIGINT => "SIGINT",
        libc::SIGQUIT => "SIGQUIT",
        libc::SIGILL => "SIGILL",
        libc::SIGTRAP => "SIGTRAP",
        libc::SIGABRT => "SIGABRT",
        libc::SIGBUS => "SIGBUS",
        libc::SIGFPE => "SIGFPE",
        libc::SIGKILL => "SIGKILL",
        libc::SIGSEGV => "SIGSEGV",
        libc::SIGPIPE => "SIGPIPE",
        libc::SIGTERM => "SIGTERM",
        libc::SIGXCPU => "SIGXCPU",
        libc::SIGXFSZ => "SIGXFSZ",
        libc::SIGSYS => "SIGSYS",
        _ => return format!("signal {signal}"),
    };
    format!("{name} ({signal})")
}

/// Where the core of `pid` was stored, said as a sentence a failing test can be read
/// straight out of.
///
/// Asked of `coredumpctl`, which is where the dumps on this machine go — the kernel
/// hands the core to systemd-coredump through `kernel.core_pattern`, so there is no
/// file beside the test to point at. A launch ended by a signal that leaves no core —
/// `SIGKILL` above all — says so rather than leaving the reader looking for one.
fn dump_of(pid: u32) -> String {
    let deadline = Instant::now() + RECORDED_WITHIN;
    loop {
        match std::process::Command::new("coredumpctl")
            .args(["info", &pid.to_string()])
            .output()
        {
            Ok(found) if found.status.success() => {
                let said = String::from_utf8_lossy(&found.stdout);
                if let Some(storage) = said
                    .lines()
                    .find_map(|line| line.trim().strip_prefix("Storage: "))
                {
                    return format!(
                        "Its core dump is at {}.\n  Read it with: coredumpctl info \
                         {pid}",
                        storage.trim(),
                    );
                }
                return format!("coredumpctl has a record of {pid} but stored no core.");
            }
            Ok(_) => {}
            Err(error) => {
                return format!("No dump could be looked up: coredumpctl did not run ({error}).",);
            }
        }
        if Instant::now() >= deadline {
            return format!(
                "No core dump was recorded for {pid}. A signal that terminates without \
                 dumping — SIGKILL — leaves none; for any other, core dumping is off \
                 on this machine.",
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    //! What a crash has to produce, proven on a process that really crashes.
    //!
    //! The fixture is a command that sits still until it is signalled, and then is sent
    //! the signals the day of issue #45 was made of. It is deliberately not axiomd: the
    //! gate's own coredump sweep watches for dumps of that name, so a test that crashed
    //! one on purpose would fail the gate it runs in.

    use super::*;
    use std::process::{Command, Stdio};

    /// A process that will sit still until it is signalled.
    fn waiting() -> Watch {
        let child = Command::new("/usr/bin/sleep")
            .arg("120")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start the fixture");
        Watch::over(child)
    }

    fn signal(watch: &Watch, signal: i32) {
        // SAFETY: a signal to a process this test started itself.
        assert_eq!(
            unsafe { libc::kill(watch.pid() as i32, signal) },
            0,
            "could not signal the fixture",
        );
    }

    /// Waits until the fixture has really gone, so what is asserted is the ending and
    /// not the race to it.
    fn ended(watch: &mut Watch) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while watch.poll().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(watch.poll().is_some(), "the fixture never ended");
    }

    /// The exit criterion, in the shape the 08:00 batch of issue #45 had: the process
    /// crashes, and what comes back names the signal and the dump.
    #[test]
    fn a_launch_that_crashes_is_reported_with_its_signal_and_its_dump() {
        let mut watch = waiting();
        let pid = watch.pid();
        signal(&watch, libc::SIGABRT);
        ended(&mut watch);

        let said = watch.check().expect("a crash has to be a complaint");
        let at_teardown = watch.end().expect("and still one at teardown");

        assert!(
            said.contains("SIGABRT"),
            "the complaint did not name the signal that killed it: {said}",
        );
        assert!(
            said.contains("/var/lib/systemd/coredump/"),
            "the complaint did not say where the dump is: {said}",
        );
        assert!(
            said.contains(&pid.to_string()),
            "the complaint did not say which process to look up: {said}",
        );
        // The window issue #45 is about: a death that has already happened when
        // teardown runs must not be painted over by teardown's own kill.
        assert!(
            at_teardown.contains("SIGABRT"),
            "closing a launch that had already crashed reported no crash: {at_teardown}",
        );
    }

    /// And the one that leaves nothing behind to point at. It is still a crash, and the
    /// complaint has to say what it is instead of a path that does not exist.
    #[test]
    fn a_launch_killed_outright_is_a_crash_that_says_it_left_no_dump() {
        let mut watch = waiting();
        signal(&watch, libc::SIGKILL);
        ended(&mut watch);

        let said = watch
            .check()
            .expect("a killed launch has to be a complaint");

        assert!(
            said.contains("SIGKILL"),
            "the complaint did not name the signal that killed it: {said}",
        );
        assert!(
            said.contains("No core dump was recorded"),
            "the complaint did not say that there is no dump to read: {said}",
        );
    }

    /// A watch that complains about everything would pass every test above and be
    /// worthless: the ending a launch is asked for is not a failure.
    #[test]
    fn a_launch_that_goes_when_it_is_asked_is_no_complaint_at_all() {
        let mut watch = waiting();
        assert_eq!(watch.check(), None, "a running launch was called an ending");

        // What quitting looks like from out here: it ends by itself, cleanly, before
        // teardown has to do anything about it.
        signal(&watch, libc::SIGTERM);
        // `sleep` has no handler for it, so it dies of the signal — which is exactly
        // what must be reported. The clean ending is the one below.
        ended(&mut watch);
        assert!(watch.check().is_some());

        let quiet = Command::new("/usr/bin/true")
            .spawn()
            .expect("a process that exits cleanly");
        let mut asked = Watch::over(quiet);
        assert_eq!(
            asked.end(),
            None,
            "a launch that exited cleanly when asked was reported as a failure",
        );
    }

    /// And the other kind of ending nobody asked for: no signal, no crash, the
    /// application simply stopping in the middle of a test.
    #[test]
    fn a_launch_that_exits_by_itself_mid_test_is_a_failure_too() {
        let stopping = Command::new("/usr/bin/false")
            .spawn()
            .expect("a process that stops by itself");
        let mut watch = Watch::over(stopping);
        ended(&mut watch);

        let said = watch
            .check()
            .expect("an exit mid-test has to be a complaint");
        assert!(
            said.contains("before this test asked it to quit"),
            "the complaint did not say that nobody asked it to end: {said}",
        );
    }
}
