//! The pictures a software centre shows, taken from the running application
//! (issue #33).
//!
//! A store listing is a picture of the app, and the only honest way to get one is to
//! take it off the app: this launches the real axiomd on the real compositor the rest
//! of the e2e suite uses, sizes a window the way a listing wants it, and writes the
//! window — header bar, outline and document together — into `data/screenshots/`,
//! which is what the metainfo publishes.
//!
//! # Why this is `#[ignore]`d and what runs instead
//!
//! It writes into the source tree, so it is not something a quality-gate run may do as
//! a side effect: a gate that rewrote the published pictures would leave every run
//! with a dirty tree and a picture nobody looked at. Regenerating them is a decision,
//! taken when the reading view changes, and the new pictures are looked at before they
//! are committed.
//!
//! What the gate holds instead is `packaging.rs`: the metainfo's screenshot URLs name
//! files this repository really has, at the size they declare, and the picture the
//! metainfo calls the dark one really is dark. That is the part a change can break
//! without anyone noticing.
//!
//! Regenerate with:
//!
//! ```text
//! cargo test -p axiomd-app --test screenshots -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};

use axiomd_e2e::Preferences;

/// The size the window is taken at: 16:9, the shape AppStream asks screenshots to be,
/// and small enough to fit the harness's compositor (1400x1000) without being resized
/// to fit it — a picture whose size came from the display rather than from the request
/// would not match what the metainfo declares.
const SIZE: (i32, i32) = (1280, 720);

/// The page a reader reads on under the light palette, and under the dark one — the
/// colours `appearance.rs` and `preferences.rs` hold the rendered document to.
const PAGE_LIGHT: (u8, u8, u8) = (255, 255, 255);
const PAGE_DARK: (u8, u8, u8) = (29, 29, 32);

/// The document in the picture.
///
/// A page of a real project's notes rather than filler, and written so that the things
/// the app is good at are the things on screen: headings for the outline to list, prose
/// with a link and inline code, a list, a highlighted code block and a table, all above
/// the fold at the size the picture is taken at. What is left below it — the quote and
/// the ordered list — is there so the document does not end where the picture does.
const DOCUMENT: &str = "\
# Wayfinder 2.0

Turns a route into directions a person can follow while they are driving.

## What is new

- **Streaming routes** — `Route::between` yields turns as it computes them.
- **Offline first** — nothing is fetched while you are moving.

## How it performs

| Route         | Cold  | Warm  | Memory |
| ------------- | ----- | ----- | ------ |
| City block    | 4 ms  | 1 ms  | 2 MB   |
| Cross-country | 61 ms | 12 ms | 9 MB   |

Measured on the [reference corpus](https://example.com/corpus), cache warm.

## Getting started

```rust
use wayfinder::{Point, Route};

let home = Point::new(47.61, -122.33);
let market = Point::new(47.65, -122.31);

for step in Route::between(home, market)?.directions() {
    println!(\"{} in {}\", step.instruction, step.distance);
}
```

## How it is written

> Directions are read at a glance, by someone who cannot afford to look twice.
> When a phrasing and a shorter phrasing both work, take the shorter one.

## Next

1. Lane guidance on motorway exits.
2. A voice that says street names the way the street says them.
3. Route sharing that survives a dead battery.
";

/// The repository this test was built from — where the published pictures live.
fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository this test was built from")
}

/// Takes one picture: axiomd reading `DOCUMENT` under `theme`, written where the
/// metainfo publishes it.
fn capture(theme: &str, name: &str) -> axiomd_e2e::Screenshot {
    // The theme is set before the launch rather than turned in the preferences dialog:
    // a picture of the app is a picture of a reader reading, never of a dialog they
    // left open.
    let reader = Preferences::with(
        &format!("screenshot-{theme}"),
        "theme",
        &format!("'{theme}'"),
    );
    let app = axiomd_e2e::launch_without_document_with(&reader);

    // Sized before the document arrives, not after: a window resized while a document is
    // in it has a page laid out at the old width until the web process catches up, and a
    // picture taken in between is the document at a size the window no longer is. This
    // way the document is laid out once, at the size the picture is taken at.
    app.resize(SIZE.0, SIZE.1);
    app.wait_for(&format!("the window to be {}x{}", SIZE.0, SIZE.1), || {
        let window = app.layout().window;
        (window.width, window.height) == SIZE
    });

    // Out of the launch's own documents folder, so the window says where it is from the
    // way it would on a reader's desktop — `~/Documents`, and not a scratch path with
    // this run's process id in it, which is what the picture would otherwise publish.
    let document = app.documents_dir().join("wayfinder.md");
    std::fs::write(&document, DOCUMENT)
        .unwrap_or_else(|error| panic!("write {}: {error}", document.display()));
    app.open_here(&document);

    let published = repository()
        .join("data/screenshots")
        .join(format!("{name}.png"));
    let picture = app.capture_window(&published);

    assert!(
        app.close().is_empty(),
        "the launch the picture was taken from left processes behind",
    );
    picture
}

/// The light and dark pictures the metainfo points at, taken from the app and left
/// where they are published.
///
/// Both are the same document at the same place, which is what their captions say: the
/// pair exists so a software centre can show the app the way the reader's desktop is
/// set, not to show two different things.
#[test]
#[ignore = "writes the published pictures into data/screenshots; run when the reading view changes"]
fn the_published_pictures_are_taken_from_the_running_application() {
    let light = capture("light", "reading-light");
    let dark = capture("dark", "reading-dark");

    for (picture, name) in [(&light, "reading-light"), (&dark, "reading-dark")] {
        assert_eq!(
            picture.size(),
            (SIZE.0 as u32, SIZE.1 as u32),
            "{name} is not the size the metainfo declares",
        );
        assert!(
            !picture.is_blank(),
            "{name} is a picture of a window that never drew",
        );
    }

    // Each picture is in the palette it is published as. Not "the two differ": two
    // launches of the same theme differ by a pixel of text placement anyway, so that
    // assertion passed with both pictures light (checked by taking the dark one on a
    // light theme). The page the reader reads on is the thing that changes, and it is
    // three quarters of the picture.
    let surface = (SIZE.0 * SIZE.1) as usize / 2;
    assert!(
        light.pixels_coloured(PAGE_LIGHT) > surface,
        "the light picture is not mostly the light page the reader reads on",
    );
    assert!(
        dark.pixels_coloured(PAGE_DARK) > surface,
        "the dark picture is not mostly the dark page: the theme did not reach the document",
    );
}
