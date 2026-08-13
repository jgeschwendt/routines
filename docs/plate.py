#!/usr/bin/env python3
"""Render docs/PLAN.png — the routines design plate, as a transit map.

    uv run --with opencv-python-headless --with numpy python3 docs/plate.py

Deterministic: no timestamps, no randomness. Rendered at 2x and box-filtered
down, so Hershey strokes and 45-degree joins come out clean.
"""

from pathlib import Path

import cv2
import numpy as np

# ── canvas ────────────────────────────────────────────────────────────────────

W, H = 1900, 820
SS = 2  # supersample factor

BG = (242, 247, 250)  # #FAF7F2
INK = (48, 40, 32)  # #202830
MUTE = (128, 118, 110)  # captions
WHITE = (255, 255, 255)

BLUE = (235, 111, 31)  # #1F6FEB  launchd
GREEN = (92, 145, 23)  # #17915C  CI cron
GRAY = (150, 143, 138)  # #8A8F96  shell
TRUNK = (66, 50, 43)  # #2B3242  the shared wrapper
AMBER = (30, 138, 224)  # #E08A1E  catch / retry
RED = (44, 56, 210)  # #D2382C  bypass

W_TRUNK, W_BRANCH, W_LOOP, W_DASH = 15, 11, 11, 6

FONT = cv2.FONT_HERSHEY_DUPLEX
FONT2 = cv2.FONT_HERSHEY_SIMPLEX

img = np.zeros((H * SS, W * SS, 3), np.uint8)
img[:] = BG

boxes = []  # (x0, y0, x1, y1, label) — collision audit


def _i(v):
    return int(round(v * SS))


def _t(scale):
    return max(1, int(round(scale * SS * 1.9)))


# ── geometry ──────────────────────────────────────────────────────────────────


def poly(pts, color, w, cap=True):
    """Thick polyline with rounded joins/caps (horizontal / vertical / 45 only)."""
    r = max(1, _i(w) // 2)
    for (x0, y0), (x1, y1) in zip(pts, pts[1:]):
        cv2.line(img, (_i(x0), _i(y0)), (_i(x1), _i(y1)), color, _i(w), cv2.LINE_AA)
    joints = pts if cap else pts[1:-1]
    for x, y in joints:
        cv2.circle(img, (_i(x), _i(y)), r, color, -1, cv2.LINE_AA)


def dashed(pts, color, w, dash=17, gap=13):
    """Dashed polyline, phase carried across vertices so dashes stay even."""
    r = max(1, _i(w) // 2)
    phase, on = 0.0, True
    for (x0, y0), (x1, y1) in zip(pts, pts[1:]):
        dx, dy = x1 - x0, y1 - y0
        length = (dx * dx + dy * dy) ** 0.5
        if length == 0:
            continue
        ux, uy = dx / length, dy / length
        t = 0.0
        while t < length:
            span = (dash if on else gap) - phase
            seg = min(span, length - t)
            if on:
                a = (_i(x0 + ux * t), _i(y0 + uy * t))
                b = (_i(x0 + ux * (t + seg)), _i(y0 + uy * (t + seg)))
                cv2.line(img, a, b, color, _i(w), cv2.LINE_AA)
                cv2.circle(img, a, r, color, -1, cv2.LINE_AA)
                cv2.circle(img, b, r, color, -1, cv2.LINE_AA)
            t += seg
            if seg == span:
                on, phase = not on, 0.0
            else:
                phase += seg


def station(x, y, color, r=13, ring=5):
    cv2.circle(img, (_i(x), _i(y)), _i(r), color, -1, cv2.LINE_AA)
    cv2.circle(img, (_i(x), _i(y)), _i(r - ring), WHITE, -1, cv2.LINE_AA)


# ── text (Hershey is ASCII-only; · and × are drawn as glyphs) ─────────────────

SPECIAL = {"·", "×"}


def _tokens(s):
    out, buf = [], ""
    for ch in s:
        if ch in SPECIAL:
            if buf:
                out.append(buf)
                buf = ""
            out.append(ch)
        else:
            # Hershey fonts are ASCII-only; anything else must join SPECIAL first.
            assert ch.isascii(), f"no glyph for {ch!r}"
            buf += ch
    if buf:
        out.append(buf)
    return out


def _em(font, scale):
    return cv2.getTextSize("H", font, scale * SS, _t(scale))[0][1]


def measure(s, font, scale):
    em = _em(font, scale)
    w = 0
    for tok in _tokens(s):
        if tok == "·":
            w += int(em * 0.52)
        elif tok == "×":
            w += int(em * 0.78)
        else:
            w += cv2.getTextSize(tok, font, scale * SS, _t(scale))[0][0]
    h = cv2.getTextSize("Hg", font, scale * SS, _t(scale))[0][1]
    return w, h


def text(s, x, y, scale=0.55, color=INK, font=FONT, anchor="c", tag=None):
    """anchor: c | l | r  (x is the centre / left / right edge). y is the baseline."""
    tw, th = measure(s, font, scale)
    px = _i(x) - {"c": tw // 2, "l": 0, "r": tw}[anchor]
    py = _i(y)
    em = _em(font, scale)
    cur = px
    for tok in _tokens(s):
        if tok == "·":
            adv = int(em * 0.52)
            cv2.circle(
                img,
                (cur + adv // 2, py - int(em * 0.34)),
                max(1, int(em * 0.115)),
                color,
                -1,
                cv2.LINE_AA,
            )
            cur += adv
        elif tok == "×":
            adv = int(em * 0.78)
            cx, cy, k = cur + adv // 2, py - int(em * 0.36), int(em * 0.30)
            lw = max(1, int(em * 0.10))
            cv2.line(img, (cx - k, cy - k), (cx + k, cy + k), color, lw, cv2.LINE_AA)
            cv2.line(img, (cx - k, cy + k), (cx + k, cy - k), color, lw, cv2.LINE_AA)
            cur += adv
        else:
            cv2.putText(img, tok, (cur, py), font, scale * SS, color, _t(scale), cv2.LINE_AA)
            cur += cv2.getTextSize(tok, font, scale * SS, _t(scale))[0][0]
    box = (px / SS, (py - th) / SS, (px + tw) / SS, (py + th * 0.22) / SS, tag or s)
    boxes.append(box)
    return box


# ── layout ────────────────────────────────────────────────────────────────────

TRUNK_Y = 420.0
LOOP_Y = 575.0

X_RUN = 680.0
STATIONS = [
    (X_RUN, "run <name>", "below"),
    (812.0, "frontmatter", "above"),
    (946.0, "requires gate ·78", "below"),
    (1080.0, "lock ·75", "above"),
    (1214.0, "timeout ·124", "below"),
    (1360.0, "blocks ```sh", "above"),
    (1515.0, "last-run.json", "above"),
    (1670.0, "exit code", "above"),
    (1800.0, "status", "above"),
]
X_BLOCKS, X_LASTRUN, X_EXIT, X_STATUS = 1360.0, 1515.0, 1670.0, 1800.0

X_DUE, Y_DUE = 560.0, 300.0
Y_BLUE, Y_GREEN, Y_GRAY = 250.0, 350.0, 520.0
X_TERM = 110.0

X_RETRY = (X_BLOCKS + X_EXIT) / 2  # 45-degree legs both ways: 1515
DEPTH = X_RETRY - X_BLOCKS  # 155  → LOOP_Y = TRUNK_Y + DEPTH
X_LFOOT = X_BLOCKS - DEPTH
X_CLAUDE = 1300.0

# ── lines ─────────────────────────────────────────────────────────────────────

# feeders — blue and green run in parallel into the interchange, whose disc
# covers the last stub, so both colours stay readable right up to the merge.
OFF, X_FORK = 11.0, 340.0
poly(
    [
        (X_TERM, Y_BLUE),
        (X_FORK, Y_BLUE),
        (X_FORK + (Y_DUE - OFF - Y_BLUE), Y_DUE - OFF),
        (X_DUE, Y_DUE - OFF),
    ],
    BLUE,
    W_BRANCH,
)
poly(
    [
        (X_TERM, Y_GREEN),
        (X_FORK, Y_GREEN),
        (X_FORK + (Y_GREEN - Y_DUE - OFF), Y_DUE + OFF),
        (X_DUE, Y_DUE + OFF),
    ],
    GREEN,
    W_BRANCH,
)
poly([(X_TERM, Y_GRAY), (520, Y_GRAY), (620, TRUNK_Y), (X_RUN, TRUNK_Y)], GRAY, W_BRANCH)

# interchange → trunk
poly([(X_DUE, Y_DUE), (X_RUN, TRUNK_Y)], TRUNK, W_TRUNK)

# catch loop (chevron hanging off `blocks`) and the bypass spur
poly(
    [(X_BLOCKS, TRUNK_Y), (X_LFOOT, LOOP_Y), (X_RETRY, LOOP_Y), (X_BLOCKS, TRUNK_Y)],
    AMBER,
    W_LOOP,
)
dashed([(X_RETRY, LOOP_Y), (X_EXIT, TRUNK_Y)], RED, W_DASH)

# trunk
poly([(X_RUN, TRUNK_Y), (X_STATUS, TRUNK_Y)], TRUNK, W_TRUNK)

# ── stations ──────────────────────────────────────────────────────────────────

station(X_TERM, Y_BLUE, BLUE, r=15, ring=6)
station(X_TERM, Y_GREEN, GREEN, r=15, ring=6)
station(X_TERM, Y_GRAY, GRAY, r=15, ring=6)
station(X_DUE, Y_DUE, TRUNK, r=19, ring=6)
for x, _, _ in STATIONS:
    station(x, TRUNK_Y, TRUNK)
station(X_STATUS, TRUNK_Y, TRUNK, r=19, ring=6)
station(X_CLAUDE, LOOP_Y, AMBER)
station(X_RETRY, LOOP_Y, AMBER)

# ── labels ────────────────────────────────────────────────────────────────────

text("routines · one execution path", 92, 76, 1.05, INK, FONT, "l")
text(
    "one markdown document per routine · frontmatter, prose, fenced sh blocks · "
    "every scheduler is a dumb tick that ends here",
    94,
    112,
    0.5,
    MUTE,
    FONT2,
    "l",
)

# the contract, top-right
text("try   { blocks, in order }", 1836, 72, 0.52, INK, FONT2, "r")
text("catch { claude · handle $error } · retry ×1", 1836, 104, 0.52, INK, FONT2, "r")

# feeders
text("launchd tick · 60s", 94, Y_BLUE - 26, 0.55, INK, FONT, "l")
text("CI cron · 15m + cache(.state)", 94, Y_GREEN + 34, 0.55, INK, FONT, "l")
text("shell · manual", 94, Y_GRAY - 26, 0.55, INK, FONT, "l")

# interchange
text("run --due", X_DUE + 26, Y_DUE - 66, 0.58, INK, FONT, "c")
text("due = next-fire-after(last-run, cron)", X_DUE + 26, Y_DUE - 38, 0.42, MUTE, FONT2, "c")

# trunk stations
for x, label, side in STATIONS:
    y = TRUNK_Y - 30 if side == "above" else TRUNK_Y + 44
    text(label, x, y, 0.55, INK, FONT, "c")

# catch loop
text("claude · handle $error", X_CLAUDE, LOOP_Y + 44, 0.52, INK, FONT, "c")
text("retry ×1", X_RETRY, LOOP_Y + 44, 0.52, INK, FONT, "c")
text("still failing", X_EXIT - 62, TRUNK_Y + 108, 0.44, RED, FONT2, "l")
text(
    "routine = markdown · prose is the handler's context",
    X_BLOCKS,
    LOOP_Y + 100,
    0.46,
    MUTE,
    FONT2,
    "c",
)

# ── legend ────────────────────────────────────────────────────────────────────

LEG_Y = 730.0
LEG = [
    (BLUE, False, "launchd tick"),
    (GREEN, False, "CI cron"),
    (GRAY, False, "shell"),
    (TRUNK, False, "run · the shared wrapper"),
    (AMBER, False, "catch · claude repairs, retry once"),
    (RED, True, "bypass · exit with the block's code"),
]
lx = 94.0
for color, is_dash, label in LEG:
    seg = [(lx, LEG_Y - 5), (lx + 46, LEG_Y - 5)]
    if is_dash:
        dashed(seg, color, 6, dash=13, gap=9)
    else:
        poly(seg, color, 9)
    b = text(label, lx + 60, LEG_Y, 0.46, INK, FONT2, "l", tag=f"legend:{label}")
    lx = b[2] + 44

text(
    "exit · 64 usage · 75 lock held · 78 requires missing · 124 timeout · "
    "else the failing block's code",
    94,
    778,
    0.46,
    MUTE,
    FONT2,
    "l",
)

# ── audit + write ─────────────────────────────────────────────────────────────


def audit():
    hits = []
    for i in range(len(boxes)):
        for j in range(i + 1, len(boxes)):
            a, b = boxes[i], boxes[j]
            if a[0] < b[2] and b[0] < a[2] and a[1] < b[3] and b[1] < a[3]:
                hits.append((a[4], b[4]))
    return hits


out = Path(__file__).resolve().parent / "PLAN.png"
cv2.imwrite(str(out), cv2.resize(img, (W, H), interpolation=cv2.INTER_AREA))

collisions = audit()
print(f"{out}  {W}x{H}")
print("label collisions:", collisions if collisions else "none")
