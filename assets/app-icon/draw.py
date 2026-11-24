#!/usr/bin/env python3
"""Draws the Rewind OS app icon: a disk gauge around a rewind symbol.

Flat shapes on a full-bleed rounded tile (viewBox 0 0 512 512, rx=112): a graphite tile with a
soft vertical gradient, a gauge ring (a dim track and a blue arc, like a drive's usage chart)
and, inside it, two white triangles pointing left (rewind). The ring and the triangles are
thick so the icon still reads at 16 px.

    python3 assets/app-icon/draw.py > assets/app-icon/rewind-os.svg
    cargo xtask icons

Standard library only. Original work for this project; licensed like the repository.
"""

import math

TILE_TOP = "#1c2433"
TILE_BOTTOM = "#0c1018"
TRACK = "#2a3446"
ARC_START = "#3b82f6"
ARC_END = "#7cb4ff"
MARK = "#f4f7fb"

CX, CY, R, WIDTH = 256, 256, 150, 50
SWEEP = 285  # degrees of the arc, clockwise from the top


def point(angle_deg):
    a = math.radians(angle_deg - 90)
    return CX + R * math.cos(a), CY + R * math.sin(a)


def triangle(tip_x, half_h, width):
    """A triangle pointing left with its tip at (tip_x, CY), rounded corners."""
    return (
        f'<path d="M{tip_x:.1f} {CY} L{tip_x + width:.1f} {CY - half_h:.1f} '
        f'L{tip_x + width:.1f} {CY + half_h:.1f} Z" fill="{MARK}" stroke="{MARK}" '
        f'stroke-width="18" stroke-linejoin="round"/>'
    )


def main():
    x0, y0 = point(0)
    x1, y1 = point(SWEEP)
    large = 1 if SWEEP > 180 else 0
    width, half_h, overlap = 78, 62, 18
    left = CX - width + overlap / 2 - 4
    out = [
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512">',
        "<defs>",
        f'<linearGradient id="tile" x1="0" y1="0" x2="0" y2="1">'
        f'<stop offset="0" stop-color="{TILE_TOP}"/><stop offset="1" stop-color="{TILE_BOTTOM}"/>'
        "</linearGradient>",
        f'<linearGradient id="arc" x1="0" y1="1" x2="1" y2="0">'
        f'<stop offset="0" stop-color="{ARC_START}"/><stop offset="1" stop-color="{ARC_END}"/>'
        "</linearGradient>",
        "</defs>",
        '<rect width="512" height="512" rx="112" fill="url(#tile)"/>',
        f'<circle cx="{CX}" cy="{CY}" r="{R}" fill="none" stroke="{TRACK}" stroke-width="{WIDTH}"/>',
        f'<path d="M{x0:.1f} {y0:.1f} A{R} {R} 0 {large} 1 {x1:.1f} {y1:.1f}" fill="none" '
        f'stroke="url(#arc)" stroke-width="{WIDTH}" stroke-linecap="round"/>',
        triangle(left, half_h, width),
        triangle(left + width - overlap, half_h, width),
        "</svg>",
    ]
    print("".join(out))


if __name__ == "__main__":
    main()
