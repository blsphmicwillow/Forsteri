#!/usr/bin/env python3
"""Render Forsteri OS brand assets from code.

Everything the boot chain and the desktop need as a bitmap is generated here,
so the repository carries no opaque binary art: the emblem is a small set of
cubic Bezier curves (the same geometry as branding/logo/forsteri-logo.svg).

    python3 branding/render_assets.py

Outputs (branding/dist):
    logo-512.png            transparent emblem, 512x512
    logo-1024.png           transparent emblem, 1024x1024
    plymouth-logo.png       splash emblem, 320x320
    plymouth-bullet.png     password bullet
    plymouth-dot.png        progress dot
    grub-background.png     1920x1080 boot menu background
    wallpaper-aurora.png    2560x1440 default wallpaper

Requires Pillow (a build-time dependency only).
"""

from __future__ import annotations

import math
import os
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

INK = (14, 14, 18)
INK_SOFT = (22, 22, 28)
IVORY = (247, 241, 225)
GOLD = (242, 193, 78)
CORAL = (231, 154, 134)
FROST = (159, 182, 200)

OUT = Path(__file__).resolve().parent / "dist"
SS = 3  # supersampling factor


def bezier(points: list[tuple[float, float]], steps: int = 40) -> list[tuple[float, float]]:
    """Sample a chain of cubic Bezier segments: [p0, c1, c2, p1, c3, c4, p2, ...]."""
    out: list[tuple[float, float]] = [points[0]]
    i = 1
    while i + 2 < len(points) + 1 and i + 2 <= len(points) - 1:
        p0 = out[-1]
        c1, c2, p3 = points[i], points[i + 1], points[i + 2]
        for step in range(1, steps + 1):
            t = step / steps
            mt = 1 - t
            x = (
                mt**3 * p0[0]
                + 3 * mt**2 * t * c1[0]
                + 3 * mt * t**2 * c2[0]
                + t**3 * p3[0]
            )
            y = (
                mt**3 * p0[1]
                + 3 * mt**2 * t * c1[1]
                + 3 * mt * t**2 * c2[1]
                + t**3 * p3[1]
            )
            out.append((x, y))
        i += 3
    return out


# The mark: an emperor penguin reduced to a body sweep, a head, a golden ear
# patch and a coral beak. Coordinates are fractions of the emblem box.
BODY = [
    (0.440, 0.185),
    (0.630, 0.150), (0.790, 0.275), (0.795, 0.450),
    (0.800, 0.605), (0.665, 0.685), (0.585, 0.775),
    (0.505, 0.865), (0.470, 0.935), (0.480, 0.975),
    (0.395, 0.905), (0.305, 0.780), (0.298, 0.640),
    (0.291, 0.495), (0.335, 0.315), (0.440, 0.185),
]

HEAD = [
    (0.455, 0.170),
    (0.365, 0.195), (0.298, 0.245), (0.232, 0.292),
    (0.318, 0.338), (0.422, 0.338), (0.492, 0.302),
    (0.520, 0.252), (0.500, 0.188), (0.455, 0.170),
]

BEAK = [
    (0.245, 0.286),
    (0.168, 0.296), (0.094, 0.316), (0.044, 0.352),
    (0.128, 0.374), (0.216, 0.357), (0.282, 0.330),
    (0.262, 0.306), (0.252, 0.294), (0.245, 0.286),
]


def scaled(points: list[tuple[float, float]], size: int, pad: float = 0.0) -> list[tuple[float, float]]:
    inner = size * (1 - 2 * pad)
    off = size * pad
    return [(off + x * inner, off + y * inner) for x, y in points]


def draw_emblem(size: int, background: bool = True, pad: float = 0.04) -> Image.Image:
    canvas = Image.new("RGBA", (size * SS, size * SS), (0, 0, 0, 0))
    draw = ImageDraw.Draw(canvas)
    big = size * SS

    if background:
        margin = int(big * 0.005)
        draw.ellipse(
            [margin, margin, big - margin, big - margin],
            fill=INK,
            outline=(42, 42, 52, 255),
            width=max(1, int(big * 0.004)),
        )

    # body
    draw.polygon(scaled(bezier(BODY), big, pad), fill=IVORY)

    # golden ear patch, tucked under the head
    cx, cy = 0.415 * big, 0.250 * big
    rx, ry = 0.083 * big, 0.066 * big
    draw.ellipse([cx - rx, cy - ry, cx + rx, cy + ry], fill=GOLD)

    # head over the patch
    draw.polygon(scaled(bezier(HEAD), big, pad), fill=(10, 10, 12, 255))

    # beak
    draw.polygon(scaled(bezier(BEAK), big, pad), fill=CORAL)

    return canvas.resize((size, size), Image.LANCZOS)


def vertical_gradient(size: tuple[int, int], top: tuple[int, int, int], bottom: tuple[int, int, int]) -> Image.Image:
    width, height = size
    strip = Image.new("RGB", (1, height))
    pixels = strip.load()
    for y in range(height):
        t = y / max(1, height - 1)
        pixels[0, y] = tuple(
            int(top[i] + (bottom[i] - top[i]) * t) for i in range(3)
        )
    return strip.resize((width, height), Image.BILINEAR)


def glow(size: tuple[int, int], center: tuple[float, float], radius: float, color: tuple[int, int, int]) -> Image.Image:
    width, height = size
    layer = Image.new("RGB", size, (0, 0, 0))
    draw = ImageDraw.Draw(layer)
    cx, cy = width * center[0], height * center[1]
    r = min(width, height) * radius
    steps = 26
    for i in range(steps, 0, -1):
        t = i / steps
        alpha = int(255 * (1 - t) ** 2.2)
        shade = tuple(int(c * alpha / 255) for c in color)
        rr = r * t
        draw.ellipse([cx - rr, cy - rr * 0.7, cx + rr, cy + rr * 0.7], fill=shade)
    return layer.filter(ImageFilter.GaussianBlur(min(width, height) * 0.06))


def wallpaper(width: int = 2560, height: int = 1440) -> Image.Image:
    base = vertical_gradient((width, height), (17, 18, 28), (42, 42, 56)).convert("RGB")
    aurora = glow((width, height), (0.72, 0.28), 0.62, (122, 96, 44))
    ice = glow((width, height), (0.16, 0.88), 0.55, (34, 56, 84))
    base = Image.blend(base, aurora, 0.5)
    base = Image.blend(base, ice, 0.35)

    mark = draw_emblem(int(height * 0.56), background=False, pad=0.02)
    faded = mark.copy()
    alpha = faded.split()[3].point(lambda a: int(a * 0.16))
    faded.putalpha(alpha)
    base.paste(faded, (int(width * 0.58), int(height * 0.20)), faded)

    # a thin horizon line, the "ice shelf"
    draw = ImageDraw.Draw(base, "RGBA")
    y = int(height * 0.82)
    draw.line([(0, y), (width, y)], fill=(120, 140, 160, 26), width=2)
    return base


def grub_background(width: int = 1920, height: int = 1080) -> Image.Image:
    base = vertical_gradient((width, height), (11, 11, 16), (26, 26, 34)).convert("RGB")
    base = Image.blend(base, glow((width, height), (0.5, 0.42), 0.45, (86, 70, 42)), 0.55)
    mark = draw_emblem(int(height * 0.30), background=False, pad=0.02)
    faded = mark.copy()
    faded.putalpha(faded.split()[3].point(lambda a: int(a * 0.30)))
    base.paste(faded, (width // 2 - mark.width // 2, int(height * 0.08)), faded)
    return base


def bar(width: int, height: int, color: tuple[int, int, int], fade_edges: bool = False) -> Image.Image:
    img = Image.new("RGBA", (width * SS, height * SS), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    radius = height * SS / 2
    draw.rounded_rectangle(
        [0, 0, width * SS - 1, height * SS - 1], radius=radius, fill=color + (255,)
    )
    if fade_edges:
        alpha = Image.new("L", (width * SS, height * SS), 0)
        ad = ImageDraw.Draw(alpha)
        for x in range(width * SS):
            t = x / max(1, width * SS - 1)
            value = int(255 * (math.sin(math.pi * t) ** 0.55))
            ad.line([(x, 0), (x, height * SS)], fill=value)
        img.putalpha(Image.composite(alpha, Image.new("L", img.size, 0), img.split()[3]))
    return img.resize((width, height), Image.LANCZOS)


def bullet(size: int = 16, color: tuple[int, int, int] = IVORY) -> Image.Image:
    img = Image.new("RGBA", (size * SS, size * SS), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.ellipse([0, 0, size * SS - 1, size * SS - 1], fill=color)
    return img.resize((size, size), Image.LANCZOS)


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)

    draw_emblem(512).save(OUT / "logo-512.png")
    draw_emblem(1024).save(OUT / "logo-1024.png")
    draw_emblem(320, pad=0.02).save(OUT / "plymouth-logo.png")
    bullet(6).save(OUT / "plymouth-dot.png")
    bullet(14).save(OUT / "plymouth-bullet.png")
    bar(360, 4, (247, 241, 225)).save(OUT / "plymouth-track.png")
    bar(140, 4, GOLD, fade_edges=True).save(OUT / "plymouth-comet.png")
    grub_background().save(OUT / "grub-background.png", optimize=True)
    wallpaper().save(OUT / "wallpaper-aurora.png", optimize=True)

    for path in sorted(OUT.glob("*.png")):
        print(f"{path.name:26} {os.path.getsize(path) // 1024:>5} KiB")


if __name__ == "__main__":
    main()
