#!/usr/bin/env python3
"""Draw the contact sheet the owner pins the application icon from (issue #46).

Every tile on the sheet is the icon as the desktop draws it: rasterised through
gdk-pixbuf, which is the librsvg the shell, the app grid, Files and the about dialog
all reach an SVG through, at its true pixel size and never scaled afterwards. A sheet
made by scaling one big render would show a picture nobody will ever see.

Three things are on it, each on a bright background and on a dark one, because an icon
is composited onto whatever is behind it:

  * the shipped icon, at the sizes the desktop draws (128, 64, 48, 32, 16);
  * the same drawing with the Markdown badge left off, at the small sizes where the
    badge is only a mark - the one variant issue #46 sanctions, for the pin to decide;
  * the shipped symbolic icon at 16, 24 and 32, in the ink a symbolic icon is
    recoloured to on each background.

Run it from the repository root; it writes designs/icon/contact-sheet.png.
Needs python3-gobject (GdkPixbuf, Pango) and python3-cairo.
"""

import sys
from pathlib import Path

import cairo
import gi

gi.require_version("GdkPixbuf", "2.0")
gi.require_version("Pango", "1.0")
gi.require_version("PangoCairo", "1.0")
from gi.repository import GdkPixbuf, Pango, PangoCairo  # noqa: E402

REPOSITORY = Path(__file__).resolve().parent.parent
SHIPPED = REPOSITORY / "data/icons/hicolor/scalable/apps/io.github.etf.axiomd.svg"
SYMBOLIC = REPOSITORY / "data/icons/hicolor/symbolic/apps/io.github.etf.axiomd-symbolic.svg"
NO_BADGE = REPOSITORY / "designs/icon/axiomd-reader-layout-no-badge.svg"
SHEET = REPOSITORY / "designs/icon/contact-sheet.png"

# The two backgrounds an icon lands on: a bright app grid, and a dark dock.
LIGHT = (0.965, 0.961, 0.957)
DARK = (0.141, 0.121, 0.192)
# What a symbolic icon is recoloured to on each of them.
LIGHT_INK = (0.145, 0.125, 0.192)
DARK_INK = (0.937, 0.933, 0.925)

# Each tile is a size and how many times it is magnified. The magnified tiles are
# there because the sizes that decide this icon are the ones too small to look at:
# 32 blown up four times shows which pixels the dock is actually given, and it is
# nearest-neighbour, so it shows those pixels and not a smoother lie about them.
SHELL_TILES = ((128, 1), (64, 1), (48, 1), (32, 1), (16, 1), (32, 4))
SMALL_TILES = ((48, 1), (32, 1), (16, 1), (32, 4))
SYMBOLIC_TILES = ((32, 1), (24, 1), (16, 1), (16, 8))

MARGIN = 32
COLUMN = 152
LABEL = 22
CAPTION = 26


def sheet_width():
    return MARGIN * 2 + COLUMN * max(len(SHELL_TILES), len(SYMBOLIC_TILES))


def rasterise(path, size):
    """The icon at exactly `size` pixels, as a cairo surface, straight from librsvg."""
    picture = GdkPixbuf.Pixbuf.new_from_file_at_size(str(path), size, size)
    surface = cairo.ImageSurface(cairo.FORMAT_ARGB32, size, size)
    pixels = picture.get_pixels()
    stride = picture.get_rowstride()
    channels = picture.get_n_channels()
    data = surface.get_data()
    for y in range(size):
        for x in range(size):
            at = y * stride + x * channels
            alpha = pixels[at + 3] if channels == 4 else 255
            # cairo wants premultiplied BGRA in native byte order.
            out = y * surface.get_stride() + x * 4
            for offset, channel in ((0, 2), (1, 1), (2, 0)):
                data[out + offset] = pixels[at + channel] * alpha // 255
            data[out + 3] = alpha
    surface.mark_dirty()
    return surface


def text(context, x, y, words, colour, size=11, bold=False):
    layout = PangoCairo.create_layout(context)
    layout.set_font_description(
        Pango.FontDescription(f"Cantarell {'Bold ' if bold else ''}{size}")
    )
    layout.set_text(words, -1)
    context.set_source_rgb(*colour)
    context.move_to(x, y)
    PangoCairo.show_layout(context, layout)
    return layout.get_pixel_size().height


def row(context, top, path, tiles, background, caption, ink=None):
    """One background strip: the icon at each tile, labelled, on one background."""
    band = max(size * zoom for size, zoom in tiles)
    height = CAPTION + band + LABEL + MARGIN // 2
    width = sheet_width()
    context.set_source_rgb(*background)
    context.rectangle(0, top, width, height)
    context.fill()

    foreground = ink if ink else (DARK_INK if background == DARK else LIGHT_INK)
    text(context, MARGIN, top + 4, caption, foreground, size=10, bold=True)

    baseline = top + CAPTION + band
    for column, (size, zoom) in enumerate(tiles):
        drawn = size * zoom
        # Centred in its column and standing on the strip's baseline, so the same
        # drawing at every size can be read along one line.
        left = MARGIN + column * COLUMN + (COLUMN - drawn) // 2
        icon = rasterise(path, size)
        context.save()
        context.translate(left, baseline - drawn)
        context.scale(zoom, zoom)
        pattern = cairo.SurfacePattern(icon)
        pattern.set_filter(cairo.FILTER_NEAREST)
        if ink:
            # A symbolic icon is drawn in the ink of whatever is showing it: use its
            # alpha as a mask over the colour, exactly as GTK's recolouring does.
            context.set_source_rgb(*ink)
            context.mask(pattern)
        else:
            context.set_source(pattern)
            context.paint()
        context.restore()
        label = f"{size}px" if zoom == 1 else f"{size}px ×{zoom}"
        text(context, left, baseline + 4, label, foreground, size=9)

    return height


def main():
    for path in (SHIPPED, SYMBOLIC, NO_BADGE):
        if not path.is_file():
            sys.exit(f"missing: {path}")

    strips = [
        (SHIPPED, SHELL_TILES, LIGHT, "shipped icon, bright background", None),
        (SHIPPED, SHELL_TILES, DARK, "shipped icon, dark background", None),
        (NO_BADGE, SMALL_TILES, LIGHT, "variant: no badge, bright background", None),
        (NO_BADGE, SMALL_TILES, DARK, "variant: no badge, dark background", None),
        (SYMBOLIC, SYMBOLIC_TILES, LIGHT, "symbolic, bright background", LIGHT_INK),
        (SYMBOLIC, SYMBOLIC_TILES, DARK, "symbolic, dark background", DARK_INK),
    ]

    width = sheet_width()
    height = MARGIN * 2 + sum(
        CAPTION + max(size * zoom for size, zoom in tiles) + LABEL + MARGIN // 2
        for _, tiles, _, _, _ in strips
    )
    surface = cairo.ImageSurface(cairo.FORMAT_RGB24, width, height)
    context = cairo.Context(surface)
    context.set_source_rgb(1, 1, 1)
    context.paint()

    text(
        context,
        MARGIN,
        8,
        "axiomd application icon — rendered by librsvg at the size shown, never scaled",
        LIGHT_INK,
        size=12,
        bold=True,
    )

    top = MARGIN + 8
    for path, sizes, background, caption, ink in strips:
        top += row(context, top, path, sizes, background, caption, ink)

    surface.write_to_png(str(SHEET))
    print(f"wrote {SHEET.relative_to(REPOSITORY)} ({width}x{height})")


if __name__ == "__main__":
    main()
