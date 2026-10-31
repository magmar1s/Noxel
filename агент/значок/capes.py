#!/usr/bin/env python3
"""Генератор готовых плащей Noxel (PNG 64x32). Запуск: python3 capes.py -> исходники/ui/assets/capes/"""
import math, os, random, struct, zlib

OUT = os.path.join(os.path.dirname(__file__), "..", "..", "исходники", "ui", "assets", "capes")
W, H = 10, 16

def lerp(a, b, t): return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))
def grad(stops, t):
    for i in range(len(stops) - 1):
        (t0, c0), (t1, c1) = stops[i], stops[i + 1]
        if t <= t1: return lerp(c0, c1, (t - t0) / (t1 - t0) if t1 > t0 else 0)
    return stops[-1][1]
def mul(c, k): return tuple(max(0, min(255, int(v * k))) for v in c)

def rng(name): return random.Random(name)

def noxel():
    px = [[grad([(0, (58, 58, 64)), (1, (6, 6, 8))], y / (H - 1)) for x in range(W)] for y in range(H)]
    cube = {(3, 4): 255, (4, 3): 255, (5, 3): 255, (6, 4): 255, (4, 4): 240, (5, 4): 240, (3, 5): 150, (4, 5): 150, (4, 6): 150, (4, 7): 140,
            (5, 5): 70, (5, 6): 70, (5, 7): 60, (6, 5): 70, (6, 6): 60}
    cube.update({(4, 3): 255, (5, 3): 255, (3, 4): 235, (6, 4): 235, (3, 5): 160, (3, 6): 150, (4, 7): 150, (5, 7): 70, (6, 5): 70, (6, 6): 70})
    for (x, y), v in cube.items(): px[y + 2][x] = (v, v, v)
    return px

def aurora():
    px = []
    for y in range(H):
        row = []
        for x in range(W):
            base = grad([(0, (6, 36, 46)), (.5, (16, 150, 130)), (1, (80, 36, 130))], y / (H - 1))
            band = math.sin(x * .8 + y * .5) * .5 + .5
            row.append(lerp(base, (150, 255, 200), band * .35 * (1 - abs(y - 7) / 9 if abs(y - 7) < 9 else 0)))
        px.append(row)
    return px

def ember():
    r = rng("ember"); px = []
    for y in range(H):
        row = []
        for x in range(W):
            t = y / (H - 1)
            flame = max(0, (t - .35) * 1.4 + (r.random() - .5) * .35 + math.sin(x * 1.3) * .08)
            row.append(grad([(0, (16, 4, 2)), (.5, (150, 30, 6)), (.8, (255, 120, 10)), (1, (255, 210, 80))], min(1, flame + t * .15)))
        px.append(row)
    return px

def frost():
    r = rng("frost"); px = [[grad([(0, (236, 247, 255)), (1, (88, 160, 222))], y / (H - 1)) for x in range(W)] for y in range(H)]
    for _ in range(14):
        x, y = r.randrange(W), r.randrange(H); px[y][x] = (255, 255, 255)
        for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            if 0 <= x + dx < W and 0 <= y + dy < H and r.random() < .5: px[y + dy][x + dx] = lerp(px[y + dy][x + dx], (255, 255, 255), .6)
    return px

def neon():
    px = [[(10, 8, 20) for x in range(W)] for y in range(H)]
    for y in range(H):
        for x in range(W):
            if y % 4 == 3: px[y][x] = (0, 230, 255)
            elif x in (2, 7): px[y][x] = lerp((10, 8, 20), (255, 40, 200), .85)
    return px

def obsidian():
    r = rng("obs"); px = []
    for y in range(H):
        px.append([lerp((14, 4, 26), (52, 16, 92), r.random() * .8) for x in range(W)])
    for i in range(5):
        for dx in range(-i, i + 1):
            for yy in (5 + i, 5 - i) if i else (5,):
                x, y = 4 + dx + (1 if False else 0), yy + 2
                if 0 <= x < W and 0 <= y < H and abs(dx) in (i,): px[y][x] = (255, 120, 220)
    return px

def gold():
    px = [[grad([(0, (255, 226, 120)), (1, (160, 108, 22))], y / (H - 1)) for x in range(W)] for y in range(H)]
    for y in range(H):
        for x in range(W):
            if x in (0, W - 1) or y in (0, H - 1): px[y][x] = (90, 58, 10)
    for x, y in ((4, 5), (5, 5), (3, 6), (4, 6), (5, 6), (6, 6), (4, 7), (5, 7), (4, 4), (5, 4), (3, 7), (6, 7)): px[y + 1][x] = (255, 250, 200)
    return px

def creeper():
    r = rng("creeper"); px = [[lerp((38, 140, 38), (78, 190, 70), r.random()) for x in range(W)] for y in range(H)]
    dark = (12, 40, 12)
    for x, y in ((2, 4), (3, 4), (2, 5), (3, 5), (6, 4), (7, 4), (6, 5), (7, 5), (4, 6), (5, 6), (4, 7), (5, 7), (3, 8), (4, 8), (5, 8), (6, 8), (3, 9), (6, 9), (3, 10), (6, 10)):
        px[y][x] = dark
    return px

def sunset():
    px = [[grad([(0, (30, 20, 90)), (.45, (200, 60, 130)), (.8, (255, 140, 60)), (1, (255, 215, 120))], y / (H - 1)) for x in range(W)] for y in range(H)]
    for x in range(W): px[11][x] = (255, 232, 160) if x in (3, 4, 5, 6) else px[11][x]
    return px

def waves():
    px = []
    for y in range(H):
        row = []
        for x in range(W):
            w = math.sin((x + y * .7) * .9) * .5 + .5
            row.append(lerp((8, 40, 90), (40, 130, 220), w * (.4 + y / (H * 1.2))))
        px.append(row)
    return px

CAPES = [("noxel", "Noxel", noxel), ("aurora", "Северное сияние", aurora), ("ember", "Пламя", ember), ("frost", "Мороз", frost),
         ("neon", "Неон", neon), ("obsidian", "Обсидиан", obsidian), ("gold", "Золото", gold), ("creeper", "Крипер", creeper),
         ("sunset", "Закат", sunset), ("waves", "Волны", waves)]

def png(w, h, px):
    raw = b"".join(b"\x00" + b"".join(bytes(p) for p in row) for row in px)
    def chunk(t, d): return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xffffffff)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")

def cape_image(art):
    img = [[(0, 0, 0, 0)] * 64 for _ in range(32)]
    def put(x0, y0, f, w, h):
        for y in range(h):
            for x in range(w): img[y0 + y][x0 + x] = f(x, y) + (255,)
    put(12, 1, lambda x, y: art[y][x], W, H)                          # внешняя сторона (её видят сзади)
    put(1, 1, lambda x, y: mul(art[y][x], .5), W, H)                  # внутренняя
    put(1, 0, lambda x, y: mul(art[0][x], .8), W, 1)                  # верх
    put(11, 0, lambda x, y: mul(art[H - 1][x], .8), W, 1)             # низ
    put(0, 1, lambda x, y: mul(art[y][0], .7), 1, H)                  # левый край
    put(11, 1, lambda x, y: mul(art[y][W - 1], .7), 1, H)             # правый край
    return img

if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    names = []
    for cid, name, f in CAPES:
        open(os.path.join(OUT, cid + ".png"), "wb").write(png(64, 32, cape_image(f())))
        names.append({"id": cid, "name": name})
    import json
    json.dump(names, open(os.path.join(OUT, "index.json"), "w"), ensure_ascii=False)
    print(len(names), "плащей ->", os.path.abspath(OUT))
