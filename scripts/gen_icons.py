#!/usr/bin/env python3
"""Generate app icons (ico + png) with PIL."""
import os
from PIL import Image, ImageDraw, ImageFont

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "app", "icons")
os.makedirs(OUT, exist_ok=True)

SIZE = 512
img = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
d = ImageDraw.Draw(img)

# Rounded-square background (deep blue gradient-ish)
d.rounded_rectangle([24, 24, SIZE - 24, SIZE - 24], radius=110, fill=(13, 62, 105, 255))
# Lighter accent panel
d.rounded_rectangle([110, 96, SIZE - 110, SIZE - 96], radius=40, fill=(46, 138, 214, 255))
# White "document" lines
for i, y in enumerate([210, 268, 326, 384]):
    x0 = 168
    x1 = 344 if i < 3 else 280
    d.rounded_rectangle([x0, y, x1, y + 26], radius=13, fill=(255, 255, 255, 235))
# Green check mark dot (task done)
d.ellipse([380, 360, 452, 432], fill=(64, 196, 120, 255))
d.line([396, 396, 414, 414, 440, 380], fill=(255, 255, 255, 255), width=18, joint="curve")

# sizes
ico_sizes = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]
img.save(os.path.join(OUT, "icon.png"))
for s in ico_sizes:
    img.resize(s, Image.LANCZOS).save(os.path.join(OUT, f"icon-{s[0]}x{s[1]}.png"))
img.save(os.path.join(OUT, "icon.ico"), sizes=ico_sizes)
img.save(os.path.join(OUT, "icon.icns"))
print("icons written to", OUT)
