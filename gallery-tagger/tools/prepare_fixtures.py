#!/usr/bin/env python3
"""Rebuild the small parity fixtures in tests/fixtures/images.

Photos are US federal government works (public domain) from Wikimedia Commons;
see tests/fixtures/ATTRIBUTION.md. They are downscaled to at most 640 px and
re-encoded without metadata so fixtures stay small and orientation-neutral.
Synthetic fixtures cover alpha, grayscale and upscaling paths.

Usage: prepare_fixtures.py <download_dir> <repo_root>
"""
import pathlib
import sys

from PIL import Image, ImageDraw, ImageOps

PHOTOS = ["stork_pair", "stork_fledglings", "cat", "bicycles", "sunset"]


def main() -> None:
    downloads, repo = (pathlib.Path(arg) for arg in sys.argv[1:3])
    out = repo / "gallery-tagger" / "tests" / "fixtures" / "images"
    out.mkdir(parents=True, exist_ok=True)
    for name in PHOTOS:
        with Image.open(downloads / f"{name}.jpg") as image:
            image = ImageOps.exif_transpose(image).convert("RGB")
            image.thumbnail((640, 640), Image.Resampling.LANCZOS)
            image.save(out / f"{name}.jpg", quality=88, optimize=True)
    with Image.open(repo / "gallery-site" / "assets" / "demo_light.jpg") as image:
        image = image.convert("RGB")
        image.thumbnail((640, 640), Image.Resampling.LANCZOS)
        image.save(out / "screenshot.jpg", quality=88, optimize=True)

    rgba = Image.new("RGBA", (300, 200))
    draw = ImageDraw.Draw(rgba)
    for x in range(300):
        draw.line([(x, 0), (x, 199)], fill=(x * 255 // 299, 80, 255 - x * 255 // 299, 64 + x * 191 // 299))
    draw.ellipse([100, 50, 200, 150], fill=(250, 220, 40, 255))
    rgba.save(out / "synthetic_rgba.png")

    gray = Image.linear_gradient("L").resize((320, 240))
    gray.save(out / "synthetic_gray.jpg", quality=90)

    tiny = Image.new("RGB", (16, 16), (30, 120, 200))
    ImageDraw.Draw(tiny).rectangle([4, 4, 11, 11], fill=(240, 240, 240))
    tiny.save(out / "synthetic_tiny.png")


if __name__ == "__main__":
    main()
