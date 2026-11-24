#!/usr/bin/env python3
"""Draws the Rewind OS app icon: a cassette tape, seen from the front with a little depth.

Flat shapes on a full-bleed rounded tile (viewBox 0 0 512 512, rx=112): a cream shell with four
screws, a four-colour label, the window with both reels and the tape between them, and the
trapezoid at the bottom with its holes. The palette is warm and retro; the shapes are large so
the cassette still reads at 16 px.

    python3 assets/app-icon/draw.py > assets/app-icon/rewind-os.svg
    cargo xtask icons

Standard library only. Original work for this project; licensed like the repository.
"""

import math

FIELD = "#21170f"   # background
SHADOW = "#120c08"
SHELL = "#efe6d6"   # cassette body
SHELL_SIDE = "#c9bca6"
INK = "#4a2c22"     # outlines, screws, holes
CREAM = "#f6efe2"
SAND = "#e3b96b"
MUSTARD = "#d39a3a"
RUST = "#b4573a"
TAPE = "#2b1a14"


def rect(x, y, w, h, rx, fill, extra=""):
    return f'<rect x="{x:.1f}" y="{y:.1f}" width="{w:.1f}" height="{h:.1f}" rx="{rx:.1f}" fill="{fill}"{extra}/>'


def circle(cx, cy, r, fill, extra=""):
    return f'<circle cx="{cx:.1f}" cy="{cy:.1f}" r="{r:.1f}" fill="{fill}"{extra}/>'


def reel(cx, cy):
    """A take-up reel: a sand ring, a dark hub and six teeth."""
    parts = [
        circle(cx, cy, 30, SAND, f' stroke="{INK}" stroke-width="6"'),
        circle(cx, cy, 17, TAPE),
    ]
    for i in range(6):
        a = math.tau * i / 6
        x, y = cx + 15 * math.cos(a), cy + 15 * math.sin(a)
        parts.append(
            f'<rect x="{x - 4:.1f}" y="{y - 4:.1f}" width="8" height="8" rx="1.5" fill="{SAND}" '
            f'transform="rotate({math.degrees(a):.1f} {x:.1f} {y:.1f})"/>'
        )
    parts.append(circle(cx, cy, 6, SAND))
    return "".join(parts)


def main():
    # The shell, its depth (offset down-left) and a soft floor shadow.
    x, y, w, h = 70, 118, 372, 248
    out = [
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512">',
        '<defs><clipPath id="t"><rect width="512" height="512" rx="112"/></clipPath>',
        f'<clipPath id="label"><rect x="{x + 34}" y="{y + 30}" width="{w - 68}" height="{h - 92}" rx="18"/></clipPath>',
        "</defs>",
        '<g clip-path="url(#t)">',
        rect(0, 0, 512, 512, 0, FIELD),
        # Larger than drawn, so the cassette fills the tile at small sizes.
        '<g transform="translate(256 262) scale(1.14) translate(-256 -250)">',
        f'<ellipse cx="{x + w / 2 + 6}" cy="{y + h + 34}" rx="{w / 2 + 10}" ry="18" fill="{SHADOW}" opacity="0.8"/>',
        rect(x - 12, y + 14, w, h, 26, SHELL_SIDE),
        rect(x, y, w, h, 26, SHELL),
    ]
    # Screws in the corners.
    for sx, sy in [(x + 18, y + 18), (x + w - 18, y + 18), (x + 18, y + h - 18), (x + w - 18, y + h - 18)]:
        out.append(circle(sx, sy, 7.5, INK))
    # The label in four colours, outlined.
    lx, ly, lw, lh = x + 34, y + 30, w - 68, h - 92
    mx, my = lx + lw / 2, ly + lh / 2
    out.append('<g clip-path="url(#label)">')
    out.append(rect(lx, ly, mx - lx, my - ly, 0, CREAM))
    out.append(rect(mx, ly, lw / 2, my - ly, 0, SAND))
    out.append(rect(lx, my, mx - lx, lh / 2, 0, MUSTARD))
    out.append(rect(mx, my, lw / 2, lh / 2, 0, RUST))
    out.append("</g>")
    out.append(rect(lx, ly, lw, lh, 18, "none", f' stroke="{INK}" stroke-width="8"'))
    # The window: a cream pill with both reels and the tape between them.
    wx, wy, ww, wh = lx + 34, my - 38, lw - 68, 76
    out.append(rect(wx, wy, ww, wh, wh / 2, SHELL, f' stroke="{INK}" stroke-width="7"'))
    left, right = wx + 52, wx + ww - 52
    out.append(rect(left + 36, my - 20, right - left - 72, 40, 6, TAPE))
    out.append(rect(left + 52, my - 20, 18, 40, 4, INK))
    out.append(reel(left, my))
    out.append(reel(right, my))
    # The trapezoid at the bottom with its holes.
    ty = y + h - 52
    out.append(
        f'<path d="M{x + 92} {y + h} L{x + 108} {ty} L{x + w - 108} {ty} L{x + w - 92} {y + h} Z" '
        f'fill="{SHELL}" stroke="{INK}" stroke-width="5" stroke-linejoin="round"/>'
    )
    for hx, kind in [(x + 140, "c"), (x + 178, "s"), (x + w - 178, "s"), (x + w - 140, "c")]:
        hy = ty + 28
        out.append(circle(hx, hy, 8, INK) if kind == "c" else rect(hx - 6, hy - 6, 12, 12, 2, INK))
    out.append("</g></g></svg>")
    print("".join(out))


if __name__ == "__main__":
    main()
