"""Regenerates the CraftPulse icons: assets/icon.ico, icon.png and tray.rgba.

Run from the repo root:  python assets/make_icons.py   (needs Pillow)
"""
import math
from pathlib import Path

from PIL import Image, ImageDraw

OUT = Path(__file__).parent
S = 1024


def make_icon() -> Image.Image:
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    grad = Image.new("RGBA", (S, S))
    px = grad.load()
    c1, c2 = (255, 173, 105), (237, 99, 41)
    for y in range(S):
        for x in range(S):
            t = (x + y) / (2 * (S - 1))
            px[x, y] = tuple(int(c1[i] + (c2[i] - c1[i]) * t) for i in range(3)) + (255,)
    mask = Image.new("L", (S, S), 0)
    ImageDraw.Draw(mask).rounded_rectangle([24, 24, S - 24, S - 24], radius=230, fill=255)
    img.paste(grad, (0, 0), mask)

    # A thick "C" drawn as an arc with round ends.
    d = ImageDraw.Draw(img)
    cx = cy = S / 2
    r, w = 262, 120
    dark = (29, 23, 20, 255)
    start, end = 42, 318
    d.arc([cx - r, cy - r, cx + r, cy + r], start=start, end=end, fill=dark, width=w)
    for angle in (start, end):
        a = math.radians(angle)
        ex, ey = cx + (r - w / 2) * math.cos(a), cy + (r - w / 2) * math.sin(a)
        d.ellipse([ex - w / 2, ey - w / 2, ex + w / 2, ey + w / 2], fill=dark)
    return img


big = make_icon()
sizes = [16, 24, 32, 48, 64, 128, 256]
icons = [big.resize((s, s), Image.LANCZOS) for s in sizes]
icons[-1].save(
    OUT / "icon.ico",
    format="ICO",
    sizes=[(s, s) for s in sizes],
    append_images=icons[:-1],
)
icons[-1].save(OUT / "icon.png")
# Raw 64x64 RGBA used by the tray icon (src/tray.rs).
(OUT / "tray.rgba").write_bytes(big.resize((64, 64), Image.LANCZOS).tobytes())
print("icons written to", OUT)
