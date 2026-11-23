#!/usr/bin/env python3
"""Draws the Rewind OS app icon: a raccoon, frontal head-and-shoulders, as an engraving.

Three colours on a full-bleed rounded tile (viewBox 0 0 512 512, rx=112): the figure in paper,
ink line work and contour, and the app colour as the field. The engraving is generated: a tone
map (light muzzle and brows, black mask, grey fur) decides how thick each tapered ink stroke is,
and the strokes follow the direction fur grows in. A fixed seed keeps the output stable.

    python3 assets/app-icon/draw.py > assets/app-icon/rewind-os.svg
    cargo xtask icons

Standard library only. Original work for this project; licensed like the repository.
"""

import math
import random

INK = "#0c0c0e"
PAPER = "#f0eadf"
FIELD = "#1a9e8c"  # Rewind OS teal

CX = 256.0
rng = random.Random(1987)


# ---- geometry helpers -------------------------------------------------------------------------

def smoothstep(e0, e1, x):
    t = max(0.0, min(1.0, (x - e0) / (e1 - e0)))
    return t * t * (3 - 2 * t)


def ellipse_sdf(px, py, cx, cy, rx, ry, angle=0.0):
    """Approximate signed distance to a rotated ellipse (negative inside)."""
    ca, sa = math.cos(-angle), math.sin(-angle)
    dx, dy = px - cx, py - cy
    x, y = dx * ca - dy * sa, dx * sa + dy * ca
    k = math.hypot(x / rx, y / ry)
    return (k - 1.0) * min(rx, ry)


def point_in_poly(x, y, poly):
    inside = False
    n = len(poly)
    j = n - 1
    for i in range(n):
        xi, yi = poly[i]
        xj, yj = poly[j]
        if (yi > y) != (yj > y) and x < (xj - xi) * (y - yi) / (yj - yi + 1e-12) + xi:
            inside = not inside
        j = i
    return inside


def path_from_points(points, close=True):
    d = "M" + " L".join(f"{x:.1f} {y:.1f}" for x, y in points)
    return d + (" Z" if close else "")


# ---- the figure -------------------------------------------------------------------------------

HEAD_C = (CX, 270.0)


def head_radius(theta):
    """Head outline in polar form around HEAD_C; theta 0 = right, pi/2 = down."""
    keys = [  # (degrees, radius); mirrored for the left side
        (-90, 148), (-60, 152), (-30, 164), (0, 178), (20, 200), (36, 182), (58, 146), (90, 126),
    ]
    deg = math.degrees(theta)
    deg = ((deg + 180) % 360) - 180
    a = abs(deg) if abs(deg) <= 90 else 180 - abs(deg)
    a = a if deg >= -90 and deg <= 90 else a
    # fold to the right half, keeping up/down
    up = deg < 0 if abs(deg) <= 90 else deg < 0
    signed = -a if up else a
    for (d0, r0), (d1, r1) in zip(keys, keys[1:]):
        if d0 <= signed <= d1:
            t = (signed - d0) / (d1 - d0)
            t = t * t * (3 - 2 * t)
            return r0 + (r1 - r0) * t
    return keys[-1][1]


def head_outline():
    pts = []
    n = 220
    for i in range(n):
        th = -math.pi + 2 * math.pi * i / n
        r = head_radius(th)
        # Fur tufts: jagged along the cheeks, finer on the crown.
        side = abs(math.cos(th))
        lower = max(0.0, math.sin(th))
        amp = 2.0 + 11.0 * side * (0.3 + lower) + 3.0 * (1 - side)
        period = 7
        phase = (i % period) / period
        tooth = (1 - abs(2 * phase - 1)) ** 2.2
        r += amp * tooth * (0.55 + 0.45 * rng.random())
        # Tufts sweep down and out on the cheeks.
        sweep = 6.0 * tooth * side * lower
        pts.append((HEAD_C[0] + r * math.cos(th), HEAD_C[1] + r * math.sin(th) + sweep))
    return pts


def body_outline():
    pts = []
    # Shoulders fall from under the cheeks to the bottom edge, with fur tufts along them.
    for side in (1, -1):
        seg = []
        for i in range(40):
            t = i / 39
            x = CX + side * (95 + 150 * t ** 0.9)
            y = 380 + 150 * t ** 1.4
            jag = 5 * (0.5 + 0.5 * math.cos(i * 2.1)) * rng.random()
            seg.append((x + side * jag, y))
        pts.append(seg)
    right, left = pts
    return [(CX, 390)] + right + [(CX + 300, 560), (CX - 300, 560)] + left[::-1]


EARS = [  # (cx, cy, rx, ry, angle)
    (152, 122, 46, 60, -0.42),
    (360, 122, 46, 60, 0.42),
]


def inside_figure(x, y, head, body):
    if point_in_poly(x, y, head) or point_in_poly(x, y, body):
        return True
    return any(ellipse_sdf(x, y, *e) < 0 for e in EARS)


# ---- tone (0 = paper, 1 = ink) ----------------------------------------------------------------

MASK = [  # bandit mask lobes around the eyes, drooping outwards
    (192, 266, 76, 37, 0.30),
    (320, 266, 76, 37, -0.30),
]
EYES = [(196, 262), (316, 262)]


def mask_sdf(x, y):
    lobes = min(ellipse_sdf(x, y, *m) for m in MASK)
    bridge = ellipse_sdf(x, y, CX, 254, 30, 22)
    return min(lobes, bridge)


def stripe_tone(x, y):
    """The dark stripe from the mask up the forehead, narrowing towards the crown."""
    if y > 250 or y < 110:
        return 0.0
    half = 6 + 12 * smoothstep(120, 245, y)
    return smoothstep(half + 6, half - 2, abs(x - CX)) * smoothstep(105, 150, y)


def tone(x, y):
    t = 0.27
    # Ears: dark inside, pale rim.
    for (ex, ey, rx, ry, a) in EARS:
        d = ellipse_sdf(x, y, ex, ey, rx, ry, a)
        if d < 0:
            inner = ellipse_sdf(x, y, ex + (0 if a == 0 else math.copysign(4, a)), ey + 10, rx * 0.58, ry * 0.62, a)
            return 0.08 if d > -9 else (0.9 if inner < 0 else 0.45)
    # Pale brows above the mask and pale cheeks below it.
    for bx in (200, 312):
        t = min(t, 0.27 - 0.32 * smoothstep(10, -14, ellipse_sdf(x, y, bx, 206, 48, 20)))
    for bx, a in ((168, 0.25), (344, -0.25)):
        t = min(t, 0.27 - 0.26 * smoothstep(8, -16, ellipse_sdf(x, y, bx, 318, 48, 26, a)))
    # Pale muzzle and chin.
    t = min(t, 0.27 - 0.33 * smoothstep(6, -22, ellipse_sdf(x, y, CX, 330, 60, 50)))
    t = min(t, 0.27 - 0.2 * smoothstep(6, -12, ellipse_sdf(x, y, CX, 384, 42, 18)))
    # Form shading: darker towards the cheek edges and the shoulders.
    dx = abs(x - CX) / 180.0
    t += 0.22 * smoothstep(0.55, 1.0, dx)
    if y > 395:
        t = max(t, 0.4 + 0.2 * smoothstep(400, 500, y))
    # Darker crown between the ears and a darker band along the outer cheeks.
    t += 0.18 * smoothstep(210, 130, y) * smoothstep(110, 20, abs(x - CX))
    t += 0.16 * smoothstep(110, 170, abs(x - CX)) * smoothstep(250, 300, y) * smoothstep(400, 330, y)
    t = max(t, 0.8 * stripe_tone(x, y))
    # The mask ramps to solid ink through hatching.
    t = max(t, smoothstep(12, -3, mask_sdf(x, y)))
    return max(0.0, min(1.0, t))


def fur_direction(x, y):
    """Unit vector the fur grows in: out from the nose, down along the cheeks."""
    ox, oy = CX, 318.0
    dx, dy = x - ox, y - oy
    n = math.hypot(dx, dy) or 1.0
    ux, uy = dx / n, dy / n
    side = (x - CX) / 180.0
    # Cheeks sweep outwards and down; the crown fans upwards from the stripe.
    uy += 0.55 * abs(side) * smoothstep(200, 320, y)
    ux += 0.25 * side
    if y < 230:
        uy -= 0.35
    if y > 395:  # chest fur falls downwards
        ux, uy = ux * 0.35, 1.0
    n = math.hypot(ux, uy) or 1.0
    return ux / n, uy / n


# ---- strokes ----------------------------------------------------------------------------------

def lens(x0, y0, x1, y1, bend, width):
    """A tapered stroke: two quadratic curves between the ends, apart by `width` at the middle."""
    mx, my = (x0 + x1) / 2, (y0 + y1) / 2
    dx, dy = x1 - x0, y1 - y0
    length = math.hypot(dx, dy) or 1.0
    nx, ny = -dy / length, dx / length
    cx, cy = mx + nx * bend, my + ny * bend
    w = width  # a quadratic peaks at half its control offset
    return (
        f"M{x0:.1f} {y0:.1f}Q{cx + nx * w:.1f} {cy + ny * w:.1f} {x1:.1f} {y1:.1f}"
        f"Q{cx - nx * w:.1f} {cy - ny * w:.1f} {x0:.1f} {y0:.1f}Z"
    )


def strokes(head, body):
    ink, paper = [], []
    step = 5.6
    y = 40.0
    row = 0
    while y < 520:
        x = 40.0 + (step / 2 if row % 2 else 0)
        while x < 472:
            px = x + rng.uniform(-2.2, 2.2)
            py = y + rng.uniform(-2.2, 2.2)
            if inside_figure(px, py, head, body):
                t = tone(px, py)
                ux, uy = fur_direction(px, py)
                length = 9 + 11 * rng.random() + 6 * t
                bend = rng.uniform(-2.5, 2.5)
                if t > 0.93:
                    # Inside the solid mask: sparse pale hairs give it texture.
                    if rng.random() < 0.22:
                        w = 0.35 + 0.35 * rng.random()
                        paper.append(lens(px, py, px + ux * length * 0.8, py + uy * length * 0.8, bend, w))
                elif t > 0.06:
                    w = 0.2 + 3.1 * t ** 1.3
                    ink.append(lens(px, py, px + ux * length, py + uy * length, bend, w))
                    if t > 0.55 and rng.random() < 0.7:
                        # Cross-hatching in the darkest fur.
                        vx, vy = -uy, ux
                        cl = length * 0.7
                        ink.append(lens(px - vx * cl / 2, py - vy * cl / 2, px + vx * cl / 2, py + vy * cl / 2, bend * 0.5, w * 0.6))
            x += step
        y += step * 0.866
        row += 1
    return ink, paper


# ---- solid features ---------------------------------------------------------------------------

def mask_shapes():
    """The solid part of the mask: the lobes and the bridge, 2 units inside their edges."""
    shapes = [(cx, cy, rx - 2, ry - 2, a) for cx, cy, rx, ry, a in MASK] + [(CX, 254, 28, 20, 0.0)]
    return "".join(
        f'<ellipse cx="{cx}" cy="{cy}" rx="{rx}" ry="{ry}" transform="rotate({math.degrees(a):.1f} {cx} {cy})"/>'
        for cx, cy, rx, ry, a in shapes
    )


def eyes():
    out = []
    for ex, ey in EYES:
        out.append(f'<circle cx="{ex}" cy="{ey}" r="15.5" fill="none" stroke="{PAPER}" stroke-width="2.2"/>')
        out.append(f'<circle cx="{ex}" cy="{ey}" r="12" fill="{INK}"/>')
        out.append(f'<circle cx="{ex - 4.5}" cy="{ey - 5}" r="4" fill="{PAPER}"/>')
        out.append(f'<circle cx="{ex + 5}" cy="{ey + 5}" r="1.6" fill="{PAPER}"/>')
    return out


def nose_and_mouth():
    nose = (
        f'<path fill="{INK}" d="M{CX - 22} 330 Q{CX} 318 {CX + 22} 330 Q{CX + 20} 346 {CX} 352 '
        f'Q{CX - 20} 346 {CX - 22} 330Z"/>'
        f'<ellipse cx="{CX - 6}" cy="329" rx="5" ry="2.6" fill="{PAPER}"/>'
    )
    mouth = (
        f'<path fill="none" stroke="{INK}" stroke-width="2.6" stroke-linecap="round" '
        f'd="M{CX} 352 L{CX} 366 M{CX - 26} 370 Q{CX - 12} 378 {CX} 366 Q{CX + 12} 378 {CX + 26} 370"/>'
    )
    return nose + mouth


def main():
    head = head_outline()
    body = body_outline()
    head_d = path_from_points(head)
    body_d = path_from_points(body)
    ear_shapes = "".join(
        f'<ellipse cx="{ex}" cy="{ey}" rx="{rx}" ry="{ry}" transform="rotate({math.degrees(a):.1f} {ex} {ey})"/>'
        for ex, ey, rx, ry, a in EARS
    )
    figure = f'<path d="{body_d}"/><path d="{head_d}"/>{ear_shapes}'
    ink, paper = strokes(head, body)
    parts = [
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512">',
        '<defs><clipPath id="t"><rect width="512" height="512" rx="112"/></clipPath>',
        f'<clipPath id="f">{figure}</clipPath></defs>',
        '<g clip-path="url(#t)">',
        f'<rect width="512" height="512" fill="{FIELD}"/>',
        # Contour: the figure drawn thick in ink, then paper on top leaves only the outer edge.
        f'<g fill="{INK}" stroke="{INK}" stroke-width="9" stroke-linejoin="round">{figure}</g>',
        f'<g fill="{PAPER}">{figure}</g>',
        '<g clip-path="url(#f)">',
        f'<g fill="{INK}">{mask_shapes()}</g>',
        f'<path fill="{INK}" d="{"".join(ink)}"/>',
        f'<path fill="{PAPER}" d="{"".join(paper)}"/>',
        *eyes(),
        nose_and_mouth(),
        "</g></g></svg>",
    ]
    print("".join(parts))


if __name__ == "__main__":
    main()
