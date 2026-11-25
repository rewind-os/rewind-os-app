#!/usr/bin/env python3
"""Draws the Rewind OS app icon: a rewound cassette tape.

Flat shapes on a full-bleed rounded tile (viewBox 0 0 512 512, rx=112), in the app's own look:
a navy tile with a soft vertical gradient, a cassette drawn in thick white lines (the shell and
the tape path at its foot), and its two reels. The left reel holds all the tape, a large blue
disc; the right one is nearly empty, a small faint disc: the tape has been rewound to the
start. The lines are thick so the icon still reads at 16 px.

    python3 assets/app-icon/draw.py > assets/app-icon/rewind-os.svg
    cargo xtask icons

Standard library only. Original work for this project; licensed like the repository.
"""

TILE_TOP = "#2b2f45"
TILE_BOTTOM = "#1a1d2e"
TAPE_LIGHT = "#5fb0ff"
TAPE_DARK = "#1774d6"
LINE = "#f2f3f7"
HOLE = "#1d2033"

FULL_REEL = (190, 240, 82)  # x, y, radius of the tape wound on the left reel
EMPTY_REEL = (336, 240, 44)  # what is left on the right one
HUB, HUB_HOLE = 32, 15


def reel(x, y, tape, opacity):
    faint = f' opacity="{opacity}"' if opacity < 1 else ""
    return (
        f'<circle cx="{x}" cy="{y}" r="{tape}" fill="url(#blue)"{faint}/>'
        f'<circle cx="{x}" cy="{y}" r="{HUB}" fill="{LINE}"/>'
        f'<circle cx="{x}" cy="{y}" r="{HUB_HOLE}" fill="{HOLE}"/>'
    )


def main():
    print(
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512">'
        "<defs>"
        f'<linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{TILE_TOP}"/>'
        f'<stop offset="1" stop-color="{TILE_BOTTOM}"/></linearGradient>'
        f'<linearGradient id="blue" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="{TAPE_LIGHT}"/>'
        f'<stop offset="1" stop-color="{TAPE_DARK}"/></linearGradient>'
        "</defs>"
        '<rect width="512" height="512" rx="112" fill="url(#bg)"/>'
        f'<rect x="70" y="128" width="372" height="256" rx="40" fill="none" stroke="{LINE}" stroke-width="30"/>'
        + reel(*FULL_REEL, 1)
        + reel(*EMPTY_REEL, 0.55)
        + f'<path d="M150 384 L170 336 L342 336 L362 384" fill="none" stroke="{LINE}" '
        'stroke-width="28" stroke-linejoin="round"/>'
        "</svg>"
    )


if __name__ == "__main__":
    main()
