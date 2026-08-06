# Crashes

Every core dump of axiomd that has been found on a development machine, what was
in it, and what was done about it. A crash whose cause is not written down is a
crash that gets rediscovered.

The rule that keeps this list honest is in the harness and in the gate, not in
this file: an application that ends under test without being asked to fails the
test it was running (`crates/axiomd-e2e/src/crash.rs`), and a run that dumped
core fails the gate whatever its tests said (`scripts/coredump-sweep.sh`). Both
landed with issue #45, which is where this record starts.

How to read a dump on this machine:

```bash
coredumpctl list --json=short          # everything, machine-readable
coredumpctl info <pid>                 # the stack it died on, per thread
```

The stack of the thread that died is the one whose thread id equals the process
id. `target/debug/axiomd` is rebuilt constantly, so `coredumpctl debug` will
usually have no symbols for a dump older than the last build; the stack
`coredumpctl info` prints was resolved when the dump was taken and is the one to
read.

## 2026-08-05 — fifteen dumps of axiomd, under a day of green gates (issue #45)

Fifteen dumps of `target/debug/axiomd` were in the journal when issue #45 was
filed (the issue counts eleven; the two of 2026-08-03 and two more were found in
the same sweep). They fall into three groups by backtrace. Two of the three were
defects in work that was in progress at the time and were fixed before that work
landed — which is *why* the gates were green, and is no comfort at all: nothing
in the gate would have caught them if they had not been.

### A. Ten dumps, 07:58–08:01 — the outline sidebar's row widget (ours, fixed)

| when | pids | signal |
|---|---|---|
| 07:58:05–08:00:00 | 959367–960339 | 8 × SIGSEGV |
| 08:00:44 | 960555 | SIGTRAP |
| 08:01:38 | 962508 | SIGSEGV |

Provoked by the outline suite — the scratch directories name the tests:
`outline-hover-tracking`, `outline-hover-nowhere`, `outline-keyboard`,
`outline-lists`, plus two from a hand-run probe on `/tmp/probe-run/guide.md`.

The stack is the same in all ten:

```
gtk_list_item_base_update            (libgtk-4.so.1)   <- SIGSEGV here
gtk_list_item_manager_ensure_items
gtk_list_item_manager_model_items_changed_cb
...
gtk_tree_list_row_set_expanded
axiomd::outline::Outline::unfold
axiomd::outline::Outline::show
axiomd::window::DocumentWindow::present_page
```

The SIGTRAP of 08:00:44 is the same path one frame earlier: it died in
`gtk_accessible_update_state` inside `gtk_list_item_manager_ensure_items`, on a
`g_log` that the run had made fatal.

**Root cause — ours.** The work in progress at the time (issue #42, drawing the
reader's place in the sidebar) marked the row from the list factory's *bind*
handler, which meant reaching for the `GtkListItemWidget` GTK owns while GTK was
in the middle of binding it. Doing that segfaults inside `gtk_list_item_base_update`
on GTK 4.20.4.

**Fixed** in `6a380e6`, which moved every mark off the row GTK owns and on to the
`GtkTreeExpander` the panel puts inside it. The commit message records the probe;
`crates/axiomd-app/src/outline.rs` records the constraint at `Here`, so the next
person to reach for the row node meets it first.

**Verified still fixed:** 10 runs of the whole outline suite plus 20 runs of the
crashing tests under parallel load, 2026-08-05, no dump (`scripts/coredump-sweep.sh`).

### B. Three dumps, 17:15–17:16 — a panic inside a GTK callback (ours, fixed)

| when | pids | signal |
|---|---|---|
| 17:15:46–17:16:30 | 1210507, 1210845, 1211399 | 3 × SIGABRT |

Provoked by the print probe (`axiomd-e2e-probe-print-*/report.md`). The stack:

```
abort
std::panicking::panic_with_hook
core::panicking::panic_cannot_unwind
gtk::functions::enumerate_printers::func_func   <- a Rust panic, inside a C callback
gtk_enumerate_printers
axiomd::export::print
axiomd::window::DocumentWindow::print
```

**Root cause — ours.** A Rust panic inside a callback GTK calls from C. There is
no unwinding across that boundary, so the panic becomes `panic_cannot_unwind` and
then `abort`: whatever the panic was, the process dies. The work in progress at
the time (issue #43) walked the printer list synchronously with
`gtk_enumerate_printers`.

**Fixed** in `5ecad0e`, which asks through `gtk::PrintDialog`'s own asynchronous
setup instead; `gtk_enumerate_printers` is not called anywhere in the tree and
never was on a commit. The class of defect is not fixed and cannot be: every
gtk-rs signal handler is a Rust closure called from C, and a panic in one of them
aborts. Panicking in a handler is therefore a crash, not a failed assertion.

### C. Two dumps, 2026-08-03 23:32 — libspelling's region assertion (upstream, open)

| when | pids | signal |
|---|---|---|
| 23:32:03, 23:32:45 | 1957487, 1958035 | 2 × SIGABRT |

Provoked by the editor suite's `editor-spelling-long` fixture — the test that
opens a 900 KB document, edits it, and replaces it with a 1.1 MB one and back.

```
abort
g_assertion_message_expr
_cjh_text_region_remove              (libspelling-1.so.2)
_cjh_text_region_replace
spelling_engine_job_finished
g_task_return_now / complete_in_idle_cb
```

**Root cause — upstream's, on this evidence.** The assertion is
`length <= region->length` at `../lib/cjhtextregion.c:1110` in
`_cjh_text_region_remove` (read out of libspelling-0.4.9-1.fc43.x86_64 at the
faulting address, 2026-08-05). A spell-checking job completes on an idle and asks
the region to replace a run longer than the region now is — that is, the buffer
shrank underneath a check that was already in flight. Replacing a text buffer's
contents is an ordinary thing for an application to do, and the library asserts
on it rather than coping.

**Not fixed, and not reproduced.** 6 runs of the editor suite, 20 runs of the
provoking test under parallel load, and 25 rounds of a probe that shrinks an
800 KB buffer to 2 KB immediately after a keystroke, all on 2026-08-05, all
clean. The editor's spell-checking code is byte-identical to the code that
crashed (nothing has touched `crates/axiomd-app/src/editor.rs` since `e961630`),
so this is live and rare rather than gone.

A candidate workaround exists and is deliberately not implemented: disabling the
adapter *before* the buffer is refilled rather than reacting to the refill from
`GtkTextBuffer::changed`. Without a reproducer it cannot be shown to fix
anything, and a change that cannot be demonstrated red first does not belong in
the tree. **Open decision for the owner.** Meanwhile the next occurrence fails
the test that provoked it, by name and with the dump's path — which is what was
missing the first two times.

### Not ours: WebKit's web process

`/usr/libexec/webkitgtk-6.0/WebKitWebProcess` dumps core on this machine
routinely — around thirty times on 2026-08-05 alone, against fifteen of ours in
total, and under every WebKitGTK application rather than only this one. They are
upstream's and are deliberately outside what the gate's sweep watches
(`scripts/coredump-sweep.sh` explains the rule and why a gate that failed on them
would be a gate that never passes). If one of these ever turns out to be provoked
by something axiomd does, it belongs in this file with the evidence.
