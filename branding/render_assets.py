#!/usr/bin/env python3
"""Build Forsteri OS brand assets from the supplied artwork.

The canonical art lives in branding/source/:

    emblem.webp        transparent emperor-penguin mark
    lockup-light.webp  transparent mark + outlined wordmark for light grounds
    lockup-dark.webp   white wordmark lockup on black

This script only trims, scales and places those files. It does not redraw the
mark, recut the wordmark or invent a replacement. Replacing a source image and
re-running it updates every boot, login and desktop asset.

    python3 branding/render_assets.py

Outputs (branding/dist):
    logo-512.png            transparent mark, 512x512
    logo-1024.png           transparent mark, 1024x1024
    plymouth-logo.png       splash mark, 320x320
    plymouth-bullet.png     password bullet
    plymouth-dot.png        progress dot
    grub-background.png     1920x1080 boot menu background
    sddm-background.png     1920x1080 login background
    wallpaper-aurora.png    2560x1440 default wallpaper
    brand-lockup-light.png  1024x1024 transparent light lockup (reference)

Requires Pillow (a build-time dependency only).
"""

from __future__ import annotations

import math
import os
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw

IVORY = (247, 241, 225)
GOLD = (242, 193, 78)

HERE = Path(__file__).resolve().parent
SOURCE = HERE / "source"
OUT = HERE / "dist"
SS = 3  # supersampling factor for the small UI primitives


def load(name: str) -> Image.Image:
    path = SOURCE / name
    if not path.is_file():
        raise SystemExit(f"branding: missing supplied artwork: {path}")
    return Image.open(path).convert("RGBA")


def trim_alpha(image: Image.Image) -> Image.Image:
    """Crop a transparent image to its visible pixels."""
    box = image.getchannel("A").getbbox()
    return image.crop(box) if box else image


def trim_on_black(image: Image.Image, tolerance: int = 12) -> Image.Image:
    """Crop an opaque image to the area that differs from its black ground."""
    rgb = image.convert("RGB")
    diff = ImageChops.difference(rgb, Image.new("RGB", image.size, (0, 0, 0))).convert("L")
    box = diff.point(lambda value: 255 if value > tolerance else 0).getbbox()
    return image.crop(box) if box else image


def fit_square(image: Image.Image, size: int, pad: float = 0.04) -> Image.Image:
    """Fit the whole mark inside a transparent square without distorting it."""
    inner = max(1, round(size * (1 - 2 * pad)))
    scale = min(inner / image.width, inner / image.height)
    resized = image.resize(
        (max(1, round(image.width * scale)), max(1, round(image.height * scale))),
        Image.Resampling.LANCZOS,
    )
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    canvas.alpha_composite(
        resized,
        ((size - resized.width) // 2, (size - resized.height) // 2),
    )
    return canvas


def mark(size: int, pad: float = 0.04) -> Image.Image:
    return fit_square(trim_alpha(load("emblem.webp")), size, pad)


def lockup(height: int) -> Image.Image:
    """The supplied dark lockup, without its redundant black border."""
    image = trim_on_black(load("lockup-dark.webp"))
    width = max(1, round(image.width * height / image.height))
    return image.resize((width, height), Image.Resampling.LANCZOS)


def black_canvas(width: int, height: int) -> Image.Image:
    return Image.new("RGB", (width, height), (0, 0, 0))


def place_center(canvas: Image.Image, image: Image.Image, y: int | None = None) -> None:
    x = (canvas.width - image.width) // 2
    if y is None:
        y = (canvas.height - image.height) // 2
    canvas.paste(image, (x, y))


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
    return img.resize((width, height), Image.Resampling.LANCZOS)


def bullet(size: int = 16, color: tuple[int, int, int] = IVORY) -> Image.Image:
    img = Image.new("RGBA", (size * SS, size * SS), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.ellipse([0, 0, size * SS - 1, size * SS - 1], fill=color)
    return img.resize((size, size), Image.Resampling.LANCZOS)


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)

    mark(512).save(OUT / "logo-512.png")
    mark(1024).save(OUT / "logo-1024.png")
    mark(320, pad=0.02).save(OUT / "plymouth-logo.png")
    bullet(6).save(OUT / "plymouth-dot.png")
    bullet(14).save(OUT / "plymouth-bullet.png")
    bar(360, 4, IVORY).save(OUT / "plymouth-track.png")
    bar(140, 4, GOLD, fade_edges=True).save(OUT / "plymouth-comet.png")

    # Boot menu: the mark and wordmark sit above the menu rows.
    grub = black_canvas(1920, 1080)
    grub_lockup = lockup(400)
    place_center(grub, grub_lockup, 24)
    grub.save(OUT / "grub-background.png", optimize=True)

    # Login: artwork at the top, form at the bottom (see Main.qml).
    sddm = black_canvas(1920, 1080)
    sddm_lockup = lockup(500)
    place_center(sddm, sddm_lockup, 48)
    sddm.save(OUT / "sddm-background.png", optimize=True)

    # Desktop: quiet black ground with the lockup offset to the right, so
    # windows and icons on the left keep clear space.
    wallpaper = black_canvas(2560, 1440)
    wall_lockup = lockup(980)
    wallpaper.paste(
        wall_lockup,
        (
            int(wallpaper.width * 0.66) - wall_lockup.width // 2,
            (wallpaper.height - wall_lockup.height) // 2,
        ),
    )
    wallpaper.save(OUT / "wallpaper-aurora.png", optimize=True)

    # Keep the light lockup available as supplied art for light grounds.
    fit_square(trim_alpha(load("lockup-light.webp")), 1024, pad=0.02).save(
        OUT / "brand-lockup-light.png"
    )

    for path in sorted(OUT.glob("*.png")):
        print(f"{path.name:26} {os.path.getsize(path) // 1024:>5} KiB")


if __name__ == "__main__":
    main()
