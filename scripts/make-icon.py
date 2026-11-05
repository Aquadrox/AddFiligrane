#!/usr/bin/env python3
"""Génère assets/icon/icon.png (256 px) et icon.ico : page blanche, coin plié,
bandes rouges inclinées évoquant un filigrane. Sans dépendance externe."""
import math, struct, zlib, pathlib

SIZE = 256
SS = 4  # suréchantillonnage pour l'anticrénelage

def inside(x, y):
    """Couleur RGBA du point (x, y) en coordonnées 0..1, ou None (transparent)."""
    left, right, top, bottom, fold = 0.18, 0.82, 0.06, 0.94, 0.20
    if not (left <= x <= right and top <= y <= bottom):
        return None
    # Coin supérieur droit plié.
    if x - (right - fold) > y - top:
        return None
    if x > right - fold and y < top + fold and (x - (right - fold)) <= (y - top) and (x - (right - fold)) + ((top + fold) - y) < fold * 1.0:
        return (205, 211, 222, 255)
    border = 0.022
    if x < left + border or x > right - border or y > bottom - border or y < top + border:
        return (120, 130, 148, 255)
    # Bandes inclinées (30°).
    a = math.radians(30)
    u = (x - 0.5) * math.sin(a) + (y - 0.5) * math.cos(a)
    band = (u * 4.5) % 1.0
    if band < 0.24:
        return (208, 31, 31, 255)
    return (255, 255, 255, 255)

pixels = bytearray()
for py in range(SIZE):
    pixels.append(0)
    for px in range(SIZE):
        acc = [0, 0, 0, 0]
        for sy in range(SS):
            for sx in range(SS):
                c = inside((px + (sx + 0.5) / SS) / SIZE, (py + (sy + 0.5) / SS) / SIZE)
                if c:
                    for i in range(3):
                        acc[i] += c[i] * c[3]
                    acc[3] += c[3]
        n = SS * SS
        alpha = acc[3] / n
        if acc[3]:
            pixels.extend(int(acc[i] / acc[3]) for i in range(3))
        else:
            pixels.extend((0, 0, 0))
        pixels.append(int(round(alpha)))

def chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)) \
    + chunk(b"IDAT", zlib.compress(bytes(pixels), 9)) + chunk(b"IEND", b"")
out = pathlib.Path(__file__).resolve().parent.parent / "assets" / "icon"
(out / "icon.png").write_bytes(png)
# ICO contenant directement le PNG (accepté depuis Windows Vista).
ico = struct.pack("<HHH", 0, 1, 1) + struct.pack("<BBBBHHII", 0, 0, 0, 0, 1, 32, len(png), 22) + png
(out / "icon.ico").write_bytes(ico)
print("icône écrite dans", out)
