#!/usr/bin/env python3
"""Turns `Cargo.lock` into the sources a flatpak build downloads instead of cargo.

A flatpak build has no network (`flatpak-builder` runs every build command in a
sandbox without one), so cargo cannot fetch a single crate while it runs. What it
gets instead is every crate already unpacked beside it and a cargo configuration
pointing at them — which is what this writes: one `archive` source per crate in
the lock file, each with the checksum the lock already records, plus the
`.cargo-checksum.json` cargo wants beside an unpacked crate and the
`[source.vendored-sources]` stanza that replaces crates.io with the directory
they land in.

The shape is `flatpak-cargo-generator.py`'s from flatpak-builder-tools (read at
`master`, 2026-08-03), because that is what `flatpak-builder` and cargo have
agreed on: crates under `cargo/vendor/<name>-<version>`, `CARGO_HOME=cargo`.
This is a rewrite of the parts of it this project uses rather than a copy of it,
for three reasons, all of which matter to the quality gate:

* it needs no network. Upstream is built around `aiohttp` because it resolves
  git dependencies by fetching them; every dependency here comes from crates.io,
  where the lock file's own `checksum` field *is* the sha256 of the `.crate`, so
  nothing has to be downloaded to write the manifest. The gate can therefore
  check that the generated file still matches `Cargo.lock` offline, on every run.
* it needs nothing installed. `tomllib` and `json` are in the standard library.
* a git dependency is refused loudly here rather than resolved. Adding one is a
  dependency decision this project makes deliberately (`CONTRIBUTING.md`), and
  it would need the generator to grow the machinery upstream has; failing is the
  honest response until someone makes that decision.

Usage:

    build-aux/flatpak/cargo-sources.py Cargo.lock            # writes to stdout
    build-aux/flatpak/cargo-sources.py Cargo.lock -o out.json
"""

import argparse
import json
import sys
import tomllib
from pathlib import Path

#: Where a `.crate` is downloaded from. crates.io's own CDN, and the only host
#: this file ever names.
CRATES_IO = "https://static.crates.io/crates"

#: `CARGO_HOME` inside the build directory, and the vendor directory under it.
#: The manifest sets `CARGO_HOME` to the same relative path, which is what makes
#: the generated configuration the one cargo reads.
CARGO_HOME = "cargo"
VENDOR = f"{CARGO_HOME}/vendor"

#: The registry a crate has to come from to be vendored this way.
REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"


def sources(lock: dict) -> list[dict]:
    """Every flatpak source the packages in `lock` need, in lock-file order.

    Workspace members have no `source` and are skipped: they arrive with the
    repository itself, not from a registry.
    """
    generated: list[dict] = []
    for package in lock.get("package", []):
        source = package.get("source")
        if source is None:
            continue
        name, version = package["name"], package["version"]
        if source.startswith("git+"):
            raise SystemExit(
                f"{name} {version} is a git dependency, which this generator does "
                "not vendor. Adding one is a dependency decision (CONTRIBUTING.md); "
                "make it, then teach this generator to resolve git sources the way "
                "flatpak-builder-tools does."
            )
        if source != REGISTRY:
            raise SystemExit(f"{name} {version} comes from an unknown registry: {source}")
        checksum = package.get("checksum")
        if checksum is None:
            raise SystemExit(
                f"{name} {version} has no checksum in the lock file, so its download "
                "could not be verified. Refusing to generate an unverified source."
            )
        crate = f"{VENDOR}/{name}-{version}"
        generated.append(
            {
                "type": "archive",
                "archive-type": "tar-gzip",
                "url": f"{CRATES_IO}/{name}/{name}-{version}.crate",
                "sha256": checksum,
                "dest": crate,
            }
        )
        generated.append(
            {
                "type": "inline",
                # What cargo reads to believe an unpacked crate is the one the
                # lock file names. `files` is empty by the same convention
                # `cargo vendor` follows for a registry crate.
                "contents": json.dumps({"package": checksum, "files": {}}),
                "dest": crate,
                "dest-filename": ".cargo-checksum.json",
            }
        )

    generated.append(
        {
            "type": "inline",
            "contents": (
                "[source.crates-io]\n"
                f'replace-with = "vendored-sources"\n'
                "\n"
                "[source.vendored-sources]\n"
                f'directory = "{VENDOR}"\n'
            ),
            "dest": CARGO_HOME,
            "dest-filename": "config.toml",
        }
    )
    return generated


def generate(cargo_lock: Path) -> str:
    """The contents of the sources file for `cargo_lock`, newline-terminated."""
    with cargo_lock.open("rb") as lock:
        return json.dumps(sources(tomllib.load(lock)), indent=4) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cargo_lock", type=Path, help="the Cargo.lock to read")
    parser.add_argument(
        "-o",
        "--output",
        type=Path,
        help="where to write the sources (default: standard output)",
    )
    arguments = parser.parse_args()

    generated = generate(arguments.cargo_lock)
    if arguments.output is None:
        sys.stdout.write(generated)
    else:
        arguments.output.write_text(generated)


if __name__ == "__main__":
    main()
