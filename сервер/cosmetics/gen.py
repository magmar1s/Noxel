#!/usr/bin/env python3
"""Генератор косметики Noxel: пиксель-арт 16×16 для мечей и тотемов, нарисованный кодом (свои работы, лицензия MIT).
Результат: cosmetics.json (PNG в base64 + метаданные). Запуск: python3 gen.py > ../../исходники/src-tauri/assets/cosmetics.json"""
import zlib, struct, base64, json, math, colorsys

def png(w, h, px):
    raw = b"".join(b"\x00" + b"".join(bytes(px[y][x]) for x in range(w)) for y in range(h))
    def ch(t, d): c = struct.pack(">I", len(d)) + t + d; return c + struct.pack(">I", zlib.crc32(t + d) & 0xffffffff)
    return b"\x89PNG\r\n\x1a\n" + ch(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)) + ch(b"IDAT", zlib.compress(raw, 9)) + ch(b"IEND", b"")

def hx(s):
    s = s.lstrip("#")
    if len(s) == 3: s = "".join(c * 2 for c in s)
    return (int(s[0:2], 16), int(s[2:4], 16), int(s[4:6], 16), 255)
def shade(c, k): return (max(0, min(255, int(c[0] * k))), max(0, min(255, int(c[1] * k))), max(0, min(255, int(c[2] * k))), 255)
def blank(): return [[(0, 0, 0, 0)] * 16 for _ in range(16)]
def put(g, x, y, c):
    if 0 <= x < 16 and 0 <= y < 16: g[y][x] = c
def outline(g, col):
    src = [row[:] for row in g]
    for y in range(16):
        for x in range(16):
            if src[y][x][3] == 0:
                for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    nx, ny = x + dx, y + dy
                    if 0 <= nx < 16 and 0 <= ny < 16 and src[ny][nx][3] and src[ny][nx] != col:
                        g[y][x] = col; break

def sword(blade, guard, handle, kind="classic", glow=None, phase=0):
    """blade = (светлый, средний, тёмный); диагональ снизу-слева вверх-вправо."""
    g = blank(); lo, mid, dk = blade
    P = lambda t: (1 + t, 14 - t)
    for t in range(0, 4):                      # рукоять
        x, y = P(t); put(g, x, y, handle if t % 2 else shade(handle, .8))
    x, y = P(0); put(g, x, y, guard)           # навершие
    gt = {"classic": 4, "broad": 4, "katana": 5, "dagger": 4, "long": 4, "crystal": 4, "flame": 4}[kind]
    gl = {"dagger": 3, "broad": 3}.get(kind, 2)
    for o in range(-gl, gl + 1):               # гарда поперёк
        put(g, P(gt)[0] + o, P(gt)[1] - o, guard if o else shade(guard, 1.15))
    top = {"dagger": 9, "katana": 14, "long": 14}.get(kind, 13)
    for t in range(gt + 1, top + 1):
        x, y = P(t)
        wide = kind in ("broad", "crystal", "flame") and t < top - 1
        put(g, x, y, mid)
        put(g, x - 1, y, lo)
        put(g, x, y + 1, dk)
        if wide: put(g, x - 1, y + 1, mid)
        if kind == "katana" and t > gt + 1: g[y][x - 1] = (0, 0, 0, 0); put(g, x, y + 1, (0, 0, 0, 0)) if t % 2 else None
        if kind == "flame" and (t + phase) % 2 == 0: put(g, x - 1, y - 1, glow or lo)
        if kind == "crystal" and (t + phase) % 3 == 0 and glow: put(g, x, y, glow)
    if glow and kind not in ("flame", "crystal"):
        for t in range(gt + 2, top, 3): put(g, P(t + phase % 2)[0], P(t + phase % 2)[1], glow)
    outline(g, shade(dk, .35))
    return g

def totem(body, trim, face, gem, kind="idol", phase=0):
    g = blank()
    rows = [(5, 10), (4, 11), (4, 11), (4, 11), (4, 11), (5, 10)]       # голова
    for i, (a, b) in enumerate(rows):
        for x in range(a, b + 1): put(g, x, 1 + i, trim if i == 0 or x in (a, b) else body)
    for i in range(6):                                                    # туловище
        a, b = 3 + (1 if i > 3 else 0), 12 - (1 if i > 3 else 0)
        for x in range(a, b + 1): put(g, x, 7 + i, trim if x in (a, b) else shade(body, .85 if i % 2 else 1))
    put(g, 6, 3, face); put(g, 9, 3, face); put(g, 6, 4, shade(face, .6)); put(g, 9, 4, shade(face, .6))
    put(g, 7, 5, shade(face, .7)); put(g, 8, 5, shade(face, .7))
    for x in range(6, 10): put(g, x, 9, gem if (x + phase) % 2 == 0 else shade(gem, .6))
    put(g, 7, 10, gem); put(g, 8, 10, gem)
    if kind == "wings":
        for i in range(3): put(g, 2 - i, 8 + i, trim); put(g, 13 + i, 8 + i, trim)
    if kind == "horns":
        put(g, 4, 0, trim); put(g, 11, 0, trim); put(g, 3, 1, shade(trim, .8)); put(g, 12, 1, shade(trim, .8))
    if kind == "crown":
        for x in (5, 7, 8, 10): put(g, x, 0, gem)
    for x in range(5, 11): put(g, x, 13, shade(trim, .8))
    outline(g, shade(trim, .3))
    return g

def sheet(frames):
    h = 16 * len(frames); px = [[(0, 0, 0, 0)] * 16 for _ in range(h)]
    for i, g in enumerate(frames):
        for y in range(16): px[i * 16 + y] = g[y][:]
    return png(16, h, px)

FIRST = []
def entry(id, name, frames, desc):
    FIRST.append(frames[0])
    d = {"id": id, "name": name, "desc": desc, "frames": len(frames), "png": base64.b64encode(sheet(frames)).decode()}
    if len(frames) > 1: d["mcmeta"] = {"animation": {"frametime": 6, "interpolate": True}}
    return d

swords, totems = [], []
S = [
 ("rubin", "Рубиновый клинок", ("#ff9aa8", "#e0334a", "#7a1426"), "#d9b44a", "#5a3a22", "broad", None, "Тяжёлый широкий клинок"),
 ("frost", "Ледяной шип", ("#e8fbff", "#8fd8ff", "#3b7fb0"), "#9fb4c8", "#34506a", "classic", "#ffffff", "Холодный блеск льда"),
 ("toxic", "Токсин", ("#d8ff8a", "#6fd13a", "#2f6b1a"), "#3a3a44", "#2a2a30", "classic", "#eaff9f", "Едкий зелёный клинок"),
 ("void", "Клинок пустоты", ("#c7a2ff", "#6a3fd6", "#25104f"), "#2b2540", "#1d1730", "crystal", "#f0dcff", "Кристалл из края мира"),
 ("sun", "Солнечный меч", ("#fff3b0", "#ffc931", "#b4730b"), "#fff0a8", "#8a5a1d", "broad", "#ffffff", "Золото и свет"),
 ("katana", "Катана-сакура", ("#fff4f8", "#ffb3cb", "#c46a8c"), "#2a2a30", "#4a1f2c", "katana", None, "Тонкая и быстрая"),
 ("dagger", "Теневой кинжал", ("#b8b8c8", "#6d6d82", "#2d2d3b"), "#1a1a22", "#22222c", "dagger", "#ff3355", "Короткий и злой"),
 ("flame", "Огненный", ("#fff08a", "#ff8a1f", "#a92b0a"), "#4a2a1a", "#3a2216", "flame", "#ffd25a", "Пламя по лезвию"),
 ("aqua", "Морской", ("#bff8ff", "#2fc4d8", "#0d6577"), "#e8d28a", "#30506a", "classic", "#e8ffff", "Глубокая вода"),
 ("neon", "Неон", ("#bfffee", "#2fffc0", "#0a8a68"), "#161a22", "#10141a", "crystal", "#ffffff", "Неоновая линия"),
 ("blood", "Кровавый", ("#ff7a7a", "#a1121c", "#4a0508"), "#3a0f12", "#240a0c", "long", "#ffb0b0", "Тёмная сталь"),
 ("gold", "Королевский", ("#fff7c2", "#f2c53a", "#9a6b0a"), "#c0392b", "#6a3a1e", "long", "#ffffff", "Для настоящих королей"),
 ("obsidian", "Обсидиан", ("#7d6aa8", "#3a2a5a", "#150d28"), "#6a4fa0", "#1d1233", "broad", "#c9a8ff", "Закалён в лаве"),
 ("candy", "Леденец", ("#ffffff", "#ff7bb0", "#c43a78"), "#7bd8ff", "#7bd8ff", "classic", "#fff", "Сладкий меч"),
 ("storm", "Громовой", ("#f4f8ff", "#9fb4ff", "#4a5fc4"), "#ffe45a", "#3a3a52", "classic", "#ffe45a", "Молния в стали"),
 ("emerald", "Изумрудный", ("#c8ffe0", "#2fd37a", "#0e6a3a"), "#e8d28a", "#3a2a1a", "crystal", "#ffffff", "Чистый изумруд"),
]
for id, name, blade, guard, handle, kind, glow, desc in S:
    b = tuple(hx(c) for c in blade)
    fr = [sword(b, hx(guard), hx(handle), kind, hx(glow) if glow else None, p) for p in ((0, 1, 2, 3) if kind in ("flame", "crystal") or glow else (0,))]
    swords.append(entry(id, name, fr, desc))
# радужный: цикл по цвету
rb = []
for p in range(6):
    hs = p / 6.0
    c = lambda h, s, v: tuple(int(x * 255) for x in colorsys.hsv_to_rgb(h % 1, s, v)) + (255,)
    rb.append(sword((c(hs, .35, 1), c(hs, .8, 1), c(hs + .08, .9, .55)), hx("#e8e8f0"), hx("#2a2a34"), "crystal", c(hs + .5, .2, 1), p))
swords.append(entry("rainbow", "Радужный", rb, "Переливается всеми цветами"))

T = [
 ("gold", "Золотой идол", "#f2c53a", "#9a6b0a", "#2a1a0a", "#ff5a5a", "idol", "Классика, но красивее"),
 ("frost", "Ледяной", "#bfe9ff", "#4a8ab5", "#0c2a44", "#ffffff", "crown", "Холодное воскрешение"),
 ("void", "Страж пустоты", "#3a2a5a", "#8a5cff", "#f0dcff", "#c9a8ff", "horns", "Из края мира"),
 ("nature", "Лесной дух", "#4fae4a", "#2a6a2a", "#f4ffd0", "#ffe45a", "idol", "Листва и свет"),
 ("flame", "Огненный феникс", "#ff8a1f", "#a92b0a", "#fff3b0", "#ffe45a", "wings", "Возрождение в пламени"),
 ("angel", "Ангел", "#fffbe8", "#e0c878", "#6a8aff", "#7bd8ff", "wings", "Белые крылья"),
 ("demon", "Демон", "#7a1426", "#3a0810", "#ffd25a", "#ff3355", "horns", "Рога и огонь"),
 ("cyber", "Кибер", "#2a3a52", "#2fffc0", "#2fffc0", "#ff3dbf", "crown", "Неоновая схема"),
 ("candy", "Леденцовый", "#ff9ac8", "#ffffff", "#7a1f4a", "#7bd8ff", "crown", "Сладкое воскрешение"),
 ("shadow", "Тень", "#22222e", "#5a5a6e", "#ff3355", "#ff3355", "horns", "Красные глаза в темноте"),
 ("ocean", "Морской", "#2fc4d8", "#0d6577", "#e8ffff", "#fff0a8", "idol", "С жемчужиной"),
 ("rainbow", "Радужный", "#ffffff", "#bbbbcc", "#333344", "#ff0000", "crown", "Переливается"),
]
for id, name, body, trim, face, gem, kind, desc in T:
    if id == "rainbow":
        fr = []
        for p in range(6):
            col = tuple(int(x * 255) for x in colorsys.hsv_to_rgb(p / 6.0, .8, 1)) + (255,)
            fr.append(totem(hx(body), hx(trim), hx(face), col, kind, p))
    else:
        fr = [totem(hx(body), hx(trim), hx(face), hx(gem), kind, p) for p in range(4)]
    totems.append(entry(id, name, fr, desc))

import sys
if len(sys.argv) > 1 and sys.argv[1] == "preview":
    K, cols = 6, 10
    rows = (len(FIRST) + cols - 1) // cols
    W, H = cols * 16 * K + (cols + 1) * 6, rows * 16 * K + (rows + 1) * 6
    px = [[(32, 32, 40, 255)] * W for _ in range(H)]
    for i, g in enumerate(FIRST):
        ox, oy = 6 + (i % cols) * (16 * K + 6), 6 + (i // cols) * (16 * K + 6)
        for y in range(16):
            for x in range(16):
                c = g[y][x]
                if c[3]:
                    for dy in range(K):
                        for dx in range(K): px[oy + y * K + dy][ox + x * K + dx] = c
    open(sys.argv[2], "wb").write(png(W, H, px))
else:
    import os as _o
    kn = json.load(open(_o.path.join(_o.path.dirname(_o.path.abspath(__file__)), "knives.json")))
    print(json.dumps({"version": 3, "swords": swords, "totems": totems, "knives": kn}, ensure_ascii=False))
