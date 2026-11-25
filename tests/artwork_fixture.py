#!/usr/bin/env python3
import os
from pathlib import Path
from PIL import Image

root = Path(os.environ["XDG_DATA_HOME"]) / "glacier"
root.mkdir(parents=True, exist_ok=True)
path = root / "artwork-fixture.png"
image = Image.new("RGB", (32, 32), (188, 37, 45))
for y in range(8, 24):
    for x in range(8, 24):
        image.putpixel((x, y), (249, 213, 72))
image.save(path)
next_image = Image.new("RGB", (32, 32), (45, 94, 214))
next_image.paste((85, 205, 215), (8, 8, 24, 24))
next_image.save(root / "artwork-fixture-next.png")
