//! What the desktop draws when it draws axiomd, at the sizes it draws it (issue #46).
//!
//! The icon is a picture, so this suite reads pixels. Every assertion here goes
//! through `gdk-pixbuf` — the same librsvg the shell, the app grid, Files and the
//! about dialog reach an SVG through — rather than through the source of the SVG:
//! an icon is not correct because its markup says so, it is correct because the
//! thing that rasterises it produces a page with a blue sidebar on it.
//!
//! The expectations are written in the 128-unit grid the icon is drawn on, not in
//! pixels, so one composition is asserted at every size the icon theme asks for. That
//! is the whole point: an icon that reads at 128 and dissolves at 32 is an icon the
//! reader never actually sees, because 32 and 48 are the sizes a dock, a task list
//! and a file manager draw.
//!
//! Both backgrounds are checked. An icon is composited onto whatever is behind it,
//! and axiomd's is drawn on a bright app grid and on a dark dock; a shadow or a rim
//! that only works on white is a defect on half the desktops it lands on.

use gtk::gdk_pixbuf::Pixbuf;
use std::path::{Path, PathBuf};

const APP_ID: &str = "io.github.etf.axiomd";

/// White, and GNOME's darkest palette grey — the two extremes an icon is composited
/// onto in practice.
const LIGHT: (f32, f32, f32) = (255.0, 255.0, 255.0);
const DARK: (f32, f32, f32) = (36.0, 31.0, 49.0);

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository this test was built from")
}

fn scalable() -> PathBuf {
    repository().join(format!("data/icons/hicolor/scalable/apps/{APP_ID}.svg"))
}

fn symbolic() -> PathBuf {
    repository().join(format!(
        "data/icons/hicolor/symbolic/apps/{APP_ID}-symbolic.svg"
    ))
}

/// An icon as the desktop has it: rasterised at one size, already composited onto the
/// background it is being drawn on, and addressed in the units it was designed in.
struct Drawn {
    size: i32,
    /// The design grid this icon's coordinates are given in: 128 for the app icon,
    /// 16 for the symbolic one.
    grid: f32,
    /// Row-major RGBA, the colour already blended onto the background and the alpha
    /// kept, so a region can be asked for its colour and for how much of the icon is
    /// actually there.
    pixels: Vec<[f32; 4]>,
}

impl Drawn {
    fn new(path: &Path, size: i32, grid: f32, background: (f32, f32, f32)) -> Self {
        let picture = Pixbuf::from_file_at_size(path, size, size)
            .unwrap_or_else(|error| panic!("{} does not rasterise: {error}", path.display()));
        assert_eq!(
            (picture.width(), picture.height()),
            (size, size),
            "{} does not draw square at {size}",
            path.display(),
        );

        let channels = picture.n_channels() as usize;
        let stride = picture.rowstride() as usize;
        let bytes = picture.read_pixel_bytes();
        let mut pixels = Vec::with_capacity((size * size) as usize);
        for y in 0..size as usize {
            for x in 0..size as usize {
                let at = y * stride + x * channels;
                let alpha = if channels == 4 {
                    f32::from(bytes[at + 3]) / 255.0
                } else {
                    1.0
                };
                let blend = |channel: usize, behind: f32| {
                    f32::from(bytes[at + channel]) * alpha + behind * (1.0 - alpha)
                };
                pixels.push([
                    blend(0, background.0),
                    blend(1, background.1),
                    blend(2, background.2),
                    alpha,
                ]);
            }
        }

        Self { size, grid, pixels }
    }

    /// The mean colour of a rectangle of the design grid, and how much of the icon
    /// covers it. Never empty: a region smaller than a pixel is read as the pixel it
    /// falls in, which is exactly the question being asked at 16 and 32.
    fn region(&self, (x0, y0, x1, y1): (f32, f32, f32, f32)) -> [f32; 4] {
        let scale = self.size as f32 / self.grid;
        let to_pixel = |value: f32| (value * scale).round().clamp(0.0, self.size as f32) as i32;
        let (left, top) = (to_pixel(x0), to_pixel(y0));
        let right = to_pixel(x1).max(left + 1).min(self.size);
        let bottom = to_pixel(y1).max(top + 1).min(self.size);

        let mut total = [0.0_f32; 4];
        let mut counted = 0.0_f32;
        for y in top..bottom {
            for x in left..right {
                let pixel = self.pixels[(y * self.size + x) as usize];
                for channel in 0..4 {
                    total[channel] += pixel[channel];
                }
                counted += 1.0;
            }
        }
        total.map(|sum| sum / counted)
    }

    /// How bright a region is, on the 0..255 scale the pixels came in on, weighted the
    /// way an eye weights the channels.
    fn brightness(&self, region: (f32, f32, f32, f32)) -> f32 {
        let colour = self.region(region);
        0.2126 * colour[0] + 0.7152 * colour[1] + 0.0722 * colour[2]
    }

    /// How much of the icon is there at all, over the whole canvas.
    fn coverage(&self) -> f32 {
        self.region((0.0, 0.0, self.grid, self.grid))[3]
    }
}

/// A region is blue when the blue channel leads the red one by a margin no grey, no
/// white and no shade of the page can produce.
fn assert_blue(what: &str, drawn: &Drawn, region: (f32, f32, f32, f32), on: &str) {
    let colour = drawn.region(region);
    assert!(
        colour[2] > 130.0 && colour[2] - colour[0] > 55.0,
        "{what} is not blue at {}px on {on}: rgb({:.0}, {:.0}, {:.0})",
        drawn.size,
        colour[0],
        colour[1],
        colour[2],
    );
}

// ---------------------------------------------------------------------------
// The composition, at every size the shell draws it
// ---------------------------------------------------------------------------

/// The sizes that matter: 128 is the about dialog and the software centre, 64 and 48
/// the app grid and the file manager, 32 the dock, the alt-tab list and the window
/// list. The concept is only worth shipping if it survives all of them.
const SHELL_SIZES: [i32; 4] = [128, 64, 48, 32];

/// The parts of the icon, in the 128-unit grid: the sidebar below its last dot, the
/// bare page between the heading and the first line of text, the heading bar, the
/// first line of body text, the checkbox, the line of text beside it, and the badge.
const SIDEBAR: (f32, f32, f32, f32) = (22.0, 84.0, 38.0, 104.0);
const BARE_PAGE: (f32, f32, f32, f32) = (52.0, 38.0, 96.0, 46.0);
const HEADING: (f32, f32, f32, f32) = (52.0, 24.0, 96.0, 32.0);
const FIRST_LINE: (f32, f32, f32, f32) = (52.0, 48.0, 96.0, 56.0);
const CHECKBOX: (f32, f32, f32, f32) = (49.0, 85.0, 63.0, 99.0);
const TASK_LINE: (f32, f32, f32, f32) = (69.0, 88.0, 79.0, 96.0);
/// The badge below its glyph, which is the part of it that is only ever the disc.
const BADGE: (f32, f32, f32, f32) = (92.0, 108.0, 104.0, 112.0);
/// The rim around the badge, directly under it.
const BADGE_RIM: (f32, f32, f32, f32) = (94.0, 114.0, 102.0, 118.0);

/// The whole ruling of issue #46 in one test: at every size the shell draws it, the
/// icon still reads as a page with a blue sidebar, a heading, text, and a checked
/// task — on a bright background and on a dark one.
///
/// The thresholds are the legibility bar, not a fingerprint of the drawing: text has
/// to stay a *visible* fraction darker than the page it sits on rather than the wash
/// it was at 3.2 units and 38% opacity, and the blue parts have to stay recognisably
/// blue rather than the pale smear a 1-pixel-wide bar of gradient becomes.
#[test]
fn the_icon_reads_as_a_page_with_a_blue_sidebar_at_every_size_the_shell_draws_it() {
    for (background, on) in [(LIGHT, "white"), (DARK, "a dark dock")] {
        for size in SHELL_SIZES {
            let drawn = Drawn::new(&scalable(), size, 128.0, background);

            assert_blue("the outline sidebar", &drawn, SIDEBAR, on);
            assert_blue("the heading bar", &drawn, HEADING, on);
            assert_blue("the checked task's box", &drawn, CHECKBOX, on);

            let page = drawn.brightness(BARE_PAGE);
            assert!(
                page > 215.0,
                "the page is not a bright page at {size}px on {on}: {page:.0}/255",
            );

            for (what, line) in [
                ("the first line of text", FIRST_LINE),
                ("the task's text", TASK_LINE),
            ] {
                let ink = drawn.brightness(line);
                assert!(
                    page - ink > 30.0,
                    "{what} is invisible against the page at {size}px on {on}: \
                     {ink:.0} against {page:.0}",
                );
            }

            let badge = drawn.brightness(BADGE);
            assert!(
                badge < 70.0,
                "the markdown badge is not a dark mark at {size}px on {on}: {badge:.0}/255",
            );
        }
    }
}

/// The badge is the part that cannot keep its glyph at 32px, so what is asserted of it
/// there is its silhouette: a round dark mark, separated from the page by a rim, in the
/// corner. It has to be that on a dark background too — a dark disc with no rim on a
/// dark dock is a bite out of the page, not a badge.
#[test]
fn the_badge_keeps_a_clean_silhouette_when_its_glyph_is_gone() {
    for (background, on) in [(LIGHT, "white"), (DARK, "a dark dock")] {
        for size in SHELL_SIZES {
            let drawn = Drawn::new(&scalable(), size, 128.0, background);

            // Inside the disc: dark. On the rim just outside it: bright. Both read
            // below the glyph, where the disc is only ever the disc.
            let core = drawn.brightness(BADGE);
            let rim = drawn.brightness(BADGE_RIM);
            assert!(
                rim - core > 90.0,
                "the badge has no rim to tell it from what is behind it at {size}px \
                 on {on}: rim {rim:.0} against core {core:.0}",
            );
        }
    }
}

/// Every size in the hicolor theme, plus the 256 a software centre asks for: the SVG
/// has to rasterise to exactly the square requested, with the icon actually in it.
///
/// The coverage bounds are the two ways this fails silently — an icon that renders
/// empty (a broken reference, a filter the renderer dropped) and one that renders as
/// a full-bleed block (the page lost, only a backdrop left).
#[test]
fn both_icons_rasterise_at_every_size_the_icon_theme_asks_for() {
    for (path, grid) in [(scalable(), 128.0), (symbolic(), 16.0)] {
        for size in [16, 22, 24, 32, 48, 64, 96, 128, 256] {
            let drawn = Drawn::new(&path, size, grid, LIGHT);
            let coverage = drawn.coverage();
            assert!(
                (0.05..0.95).contains(&coverage),
                "{} covers {:.0}% of its canvas at {size}px",
                path.display(),
                coverage * 100.0,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// The symbolic icon
// ---------------------------------------------------------------------------

/// The symbolic icon is the same silhouette in one colour on the 16-unit grid: the
/// page's outline, the sidebar filled, the heading bar. What is asserted is where the
/// ink is and where it is not — a symbolic icon that fills its page is a blob, and a
/// symbolic icon whose sidebar is empty is a rectangle.
#[test]
fn the_symbolic_icon_reads_as_a_page_with_a_sidebar_at_sixteen_pixels() {
    for size in [16, 32, 48] {
        let drawn = Drawn::new(&symbolic(), size, 16.0, LIGHT);

        for (what, region) in [
            ("the sidebar", (3.5_f32, 7.0_f32, 5.5_f32, 13.0_f32)),
            ("the heading bar", (7.5, 4.25, 11.5, 5.75)),
            ("the page's right edge", (13.25, 7.0, 13.75, 11.0)),
        ] {
            let ink = drawn.region(region)[3];
            assert!(
                ink > 0.75,
                "{what} is missing from the symbolic icon at {size}px: {:.0}% ink",
                ink * 100.0,
            );
        }

        let blank = drawn.region((7.5, 8.0, 12.0, 12.5))[3];
        assert!(
            blank < 0.2,
            "the symbolic icon's page is filled in rather than drawn at {size}px: \
             {:.0}% ink",
            blank * 100.0,
        );
    }
}
