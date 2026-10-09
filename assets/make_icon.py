"""Erzeugt assets/icon-256.png, assets/icon-1024.png und assets/icon.ico (Pillow erforderlich)."""
from PIL import Image, ImageDraw
import os

S = 1024  # Zeichnen in hoher Auflösung, dann herunterskalieren
BG, BG2 = (20, 21, 26), (11, 12, 14)
ACCENT, AMBER, WHITE = (76, 154, 255), (255, 180, 46), (235, 238, 245)

img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
d.rounded_rectangle((24, 24, S - 24, S - 24), radius=210, fill=BG)
d.rounded_rectangle((24, 24, S - 24, S - 24), radius=210, outline=(60, 64, 76), width=10)

# Videofläche
d.rounded_rectangle((150, 170, S - 150, 640), radius=60, fill=BG2, outline=(70, 75, 90), width=8)
# Play-Dreieck
d.polygon([(425, 280), (425, 530), (650, 405)], fill=ACCENT)

# Timeline mit Keyframe-Markern und Playhead
y = 800
d.rounded_rectangle((170, y - 10, S - 170, y + 10), radius=10, fill=(58, 62, 74))
d.rounded_rectangle((170, y - 10, 520, y + 10), radius=10, fill=ACCENT)
for x in (170, 345, 520, 695, 870):
    d.rectangle((x - 7, y - 50, x + 7, y - 18), fill=AMBER)
d.ellipse((520 - 34, y - 34, 520 + 34, y + 34), fill=WHITE)

here = os.path.dirname(os.path.abspath(__file__))
img.resize((256, 256), Image.LANCZOS).save(os.path.join(here, "icon-256.png"))
img.save(os.path.join(here, "icon-1024.png"))  # Quelle für das macOS-.icns
sizes = [16, 24, 32, 48, 64, 128, 256]
img.save(os.path.join(here, "icon.ico"), sizes=[(s, s) for s in sizes])
print("ok")
