"""
Pixel-art icon for ai-usage-widget.
Bar chart motif with Claude purple + Codex gold colors.
"""
from PIL import Image, ImageDraw

def make_icon(size):
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    s = size / 32  # scale factor (base grid: 32x32)

    def px(x, y, w, h, color):
        x0, y0 = round(x*s), round(y*s)
        x1, y1 = round((x+w)*s)-1, round((y+h)*s)-1
        if x1 < x0: x1 = x0
        if y1 < y0: y1 = y0
        d.rectangle([x0, y0, x1, y1], fill=color)

    # Background rounded square
    bg = (18, 18, 24, 255)
    d.rounded_rectangle([0, 0, size-1, size-1], radius=round(4*s), fill=bg)

    # Three bars (pixel style) — heights vary to look like a usage chart
    # Bar colors: Claude purple, Codex gold, Gemini blue
    bars = [
        (4,  20, 7,  "#a78bfa"),  # purple - tall
        (13, 14, 7,  "#f0c47f"),  # gold   - medium
        (22, 9,  7,  "#60a5fa"),  # blue   - short (Gemini placeholder)
    ]
    for bx, bh, bw, color in bars:
        # bar body
        px(bx, 28-bh, bw, bh, color)
        # highlight top row (only if bar tall enough)
        if bh >= 2:
            hc = tuple(min(255, int(c*1.3)) for c in bytes.fromhex(color.lstrip('#'))) + (255,)
            px(bx, 28-bh, bw, 1, hc)

    # Small dots / pixels on top for decoration
    px(4, 27, 7, 1, (255,255,255,40))
    px(13, 27, 7, 1, (255,255,255,40))
    px(22, 27, 7, 1, (255,255,255,40))

    return img


import struct, io

sizes = [16, 32, 48, 64, 128, 256]
frames = [make_icon(s) for s in sizes]

# Save each frame as PNG bytes
pngs = []
for img in frames:
    buf = io.BytesIO()
    img.save(buf, format="PNG")
    pngs.append(buf.getvalue())

# Build ICO manually
n = len(sizes)
header = struct.pack("<HHH", 0, 1, n)  # reserved, type=1(ico), count
offset = 6 + n * 16
entries = b""
for i, (s, png) in enumerate(zip(sizes, pngs)):
    w = s if s < 256 else 0
    h = s if s < 256 else 0
    entries += struct.pack("<BBBBHHII", w, h, 0, 0, 1, 32, len(png), offset)
    offset += len(png)

out_path = "C:/Users/1/Projects/ai-usage-widget/assets/icon.ico"
with open(out_path, "wb") as f:
    f.write(header + entries + b"".join(pngs))

print(f"icon.ico created ({sum(len(p) for p in pngs)//1024} KB)")
