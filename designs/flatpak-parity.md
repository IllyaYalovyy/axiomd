# Flatpak parity: what the package costs over the build

Issue #36. The owner reported on 2026-08-03 that the flatpak feels slower than the
native build. The flatpak is a supported distribution (`design_decisions.md`), so the
gap is measured, explained, and reduced where the sandbox allows — and what cannot be
reduced is written down here as a known, quantified cost rather than left as a feeling.

Every metric below is measured on **both forms in the same run**, alternately, so the
comparison is of the two forms rather than of the machine's mood twenty minutes apart.
The harness is `crates/axiomd-app/tests/parity.rs` over `axiomd_e2e::parity`; the gate
runs it with:

```bash
./scripts/quality.d/40-flatpak.sh        # builds and installs the package
./scripts/quality.d/50-flatpak-perf.sh   # measures it against the build
```

A launch is timed from the moment the process is started, not from the moment axiomd's
own clock starts: the sandbox is built before there is an application to ask, so the
number the application reports about itself is exactly the one that cannot see the
overhead in question.

## What is pinned

The ceilings the gate holds the package to. They were measured honestly first and only
ever come down — raising one means the package got slower and somebody accepted that,
which is the project owner's decision and nobody else's (issue #9). **Parity is** the
native form's own measured figure: the package is at parity when it fits in it.

Only the package is held to anything here. The native budgets are pinned in
`crates/axiomd-app/tests/perf.rs` and are untouched by this table.

This block is generated from the harness by
`the_committed_table_is_what_this_run_pins`; a ceiling that moves without it moving
fails the gate.

<!-- pinned:begin -->
| Metric | What it measures | Flatpak ceiling | Parity is |
| --- | --- | --- | --- |
| cold start to a typical document served | the process starting to the document on screen, sandbox and all | 1200.0 ms | 700.0 ms |
| the application's own share of a cold start | the same launch with everything before axiomd's first instruction taken off it, ending when the document's bytes leave the handler | 750.0 ms | 430.0 ms |
| a document opened from the desktop | a launch with the document handed over the way Files hands it — through the document portal for the package | 1200.0 ms | 700.0 ms |
| a second document opened into a running application | a window and its document with the application already up — a launch with no launch in it | 540.0 ms | 430.0 ms |
| a changed typical file on screen | a file changing on disk to the reader seeing it, including the 150 ms a burst of writes is coalesced over | 320.0 ms | 240.0 ms |
| one window on a typical document | every process the launch is made of, resident | 600 MB | 570 MB |
<!-- pinned:end -->

## Measured

2026-08-05, on the machine the ceilings were set from: Fedora 43, Linux 7.1.5, 16 cores,
30 GB; GTK 4.20.4, libadwaita 1.8.6, WebKitGTK 2.52.5; flatpak 1.16.6, runtime
`org.gnome.Platform//49`; headless weston 14.0.2 with the pixman renderer and
`GSK_RENDERER=cairo`, which is how every budget in this project is measured. Each figure
is the middle of five samples; the three full runs made that day agreed to within a few
milliseconds, and the spread is in the task report.

| Metric | Native | Flatpak | Overhead |
| --- | --- | --- | --- |
| cold start to a typical document served | 697 ms | 863 ms | ×1.24 (+166 ms) |
| the application's own share of a cold start | 423 ms | 538 ms | ×1.27 (+115 ms) |
| a document opened from the desktop | 699 ms | 886 ms | ×1.27 (+187 ms) |
| a second document opened into a running application | 427 ms | 397 ms | ×0.93 (−30 ms) |
| a changed typical file on screen | 232 ms | 230 ms | ×0.99 (−2 ms) |
| one window on a typical document | 558 MB | 445 MB | ×0.80 (−113 MB) |

The reader waits about a sixth of a second longer for a packaged launch. Nothing after
the launch costs them anything: a second document, a changed file on disk and the memory
a window holds are all at parity or better in the package.

## Where the overhead goes

Measured, not guessed. Every number below came from a probe run on 2026-08-05 and named
with the command that produced it.

**cold start to a typical document served** — +166 ms, of which **+130 ms is the sandbox
being built, before axiomd's first instruction**. Measured two ways that agree:
`flatpak run --command=/bin/sh io.github.etf.axiomd -c 'date +%s.%N'` against the host's
own `/bin/sh` puts exec-to-first-instruction at 133 ms against 2.6 ms; reading the
process start times out of `/proc/<pid>/stat` for a real launch puts axiomd's own process
110–140 ms after the spawn. This is flatpak's own machinery — the client, the session
helper and `bwrap` — and the application has no say in any of it. It is the whole of the
difference a reader feels, and it is not reducible from inside the package.

**the application's own share of a cold start** — +115 ms. Timed on the same launches as
the metric above, so the three parts of a launch add up: +130 ms before axiomd starts,
+115 ms while it works, and −70 ms afterwards while WebKit lays the page out, which is
faster in the package. The +115 ms is inside WebKitGTK's web-process startup, after the
process has been exec'd: `/proc` start times put the web process 240 ms after axiomd in
the package against 270–410 ms natively, so it is not slower to *launch* — it is slower
to ask for its first page. Five candidate causes were tested and none of them is it (see
below). Going further needs instrumentation inside WebKitGTK, which this project does not
build; it is recorded as an open follow-up rather than guessed at.

One structural difference was observed in the process tree and is worth recording beside
it: natively, WebKit puts its web process inside a `bwrap` sandbox of its own and starts
an `xdg-dbus-proxy` beside it; in the package it does neither, because the flatpak
sandbox is already there. That is consistent with the package being cheaper in memory and
in the page-layout phase, and it is the one thing that differs; it is not a proven cause
of either.

**a document opened from the desktop** — +187 ms, which is the +166 ms above plus about
**+11 ms of document portal**. The package is handed a name the portal invented for the
document, on a fuse filesystem, after a round trip to that portal (issue #22); the native
build is handed the reader's own path. Measured as the difference between this metric and
the cold-start metric for the package (886 − 863 ms with 11 ms of it the portal and the
rest run-to-run spread), against 2 ms for the same difference natively.

**a second document opened into a running application** — the package is 30 ms *faster*.
Nothing of the sandbox is built on this path: it is a window and a web process in an
application that is already up. This is the metric that says the overhead is a launch
cost and not a running cost.

**a changed typical file on screen** — at parity within 2 ms. The document is watched by
inotify on a directory the sandbox was granted; a bind mount delivers those events at the
same speed as the host filesystem does, which is what this measures.

**one window on a typical document** — the package uses **113 MB less**. Sampling
`/proc/<pid>` during a measured launch shows where: the native launch's web process was
resident in 269 MB inside WebKit's own `bwrap`, and the package's in 94 MB without one,
with the application process itself 40 MB smaller too. The package is cheaper here and
the ceiling is set from what it costs, not from what the native build costs.

## What was ruled out

Each of these was one of issue #36's named suspects, and each was tested rather than
argued about:

* **Missing GPU in the sandbox.** `devices=dri` is pinned, and it works:
  `flatpak run --command=sh io.github.etf.axiomd -c 'ls /dev/dri'` lists `card0`,
  `card1`, `renderD128` and `renderD129`. Measuring the launch again with the device
  taken away (`--nodevice=dri`) moved the application's own share by 7 ms, inside the
  run-to-run spread — so on this machine the device costs nothing at startup and its
  absence would cost nothing either.
* **Cold caches inside the sandbox.** A cold fontconfig cache costs 13–22 ms in both
  forms (`fc-match sans` with a fresh `<cachedir>`), although the runtime carries more
  fonts than the host does (850 against 745). Not a source of the gap.
* **Slower library loading in the sandbox.** `axiomd --help` — which loads every library
  the binary needs and exits — takes 60–70 ms natively and 180–190 ms in the package,
  and 130 ms of that is the sandbox: the loading itself is the same. `WebKitWebProcess`
  started on its own measures the same way (50 ms against 170 ms).
* **A different engine in the runtime.** It is the same one. The host and the runtime
  both carry `libwebkitgtk-6.0.so.4.16.9` (WebKitGTK 2.52.5) and GTK 4.20.4, and the
  package is built by the same `rustc 1.97.1` with the same `--release` profile as the
  native build.
* **Portal round trips on every open.** They happen once, at launch, and cost 11 ms —
  measured above, not assumed.

## What is not reducible, and what is open

**Not reducible within the pinned permissions:** the 130 ms of sandbox construction. It
happens before the application exists, it is flatpak's own, and no `finish-args` this
project is allowed to write shortens it. Widening `build-aux/flatpak/permissions.pinned`
is a decision for the project owner and nothing here proposes one — nothing measured
above would be helped by a wider sandbox in any case.

**Open, and deliberately not guessed at:** the +115 ms inside WebKitGTK's web-process
startup. The candidate way to attribute it is WebKit's own tracing in a debug build of
the runtime, which is a larger piece of work than issue #36 asks for. Until somebody does
it, the ceiling above holds the package to what it costs today, and it only comes down.

**Not measured here:** a genuinely cold page cache. Dropping the kernel's caches needs
root, which the gate does not have and must not want, so every launch measured above has
the runtime's files already in memory. The first packaged launch after a reboot will be
slower than this table says — for both forms, and more so for the package, which reads a
runtime the host session is not already using.
