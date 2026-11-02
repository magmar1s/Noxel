"""Предпросмотр моделей Minecraft (elements + текстуры) без внешних библиотек."""
import json, math, zlib, struct, sys

def read_png(data):
    assert data[:8] == b"\x89PNG\r\n\x1a\n"
    pos = 8; idat = b""; w = h = ct = bd = 0; plte = None; trns = None
    while pos < len(data):
        ln, = struct.unpack(">I", data[pos:pos + 4]); t = data[pos + 4:pos + 8]; d = data[pos + 8:pos + 8 + ln]; pos += 12 + ln
        if t == b"IHDR": w, h, bd, ct = struct.unpack(">IIBB", d[:10])
        elif t == b"PLTE": plte = d
        elif t == b"tRNS": trns = d
        elif t == b"IDAT": idat += d
    raw = zlib.decompress(idat)
    ch = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[ct]
    bpp = max(1, ch * bd // 8); stride = (w * ch * bd + 7) // 8
    rows = []; prev = bytearray(stride); p = 0
    for y in range(h):
        f = raw[p]; line = bytearray(raw[p + 1:p + 1 + stride]); p += 1 + stride
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0; b = prev[i]; c = prev[i - bpp] if i >= bpp else 0
            if f == 1: line[i] = (line[i] + a) & 255
            elif f == 2: line[i] = (line[i] + b) & 255
            elif f == 3: line[i] = (line[i] + (a + b) // 2) & 255
            elif f == 4:
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                pr = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[i] = (line[i] + pr) & 255
        rows.append(line); prev = line
    px = []
    for y in range(h):
        row = []; line = rows[y]
        for x in range(w):
            if bd == 8:
                if ct == 6: row.append(tuple(line[x * 4:x * 4 + 4]))
                elif ct == 2: row.append(tuple(line[x * 3:x * 3 + 3]) + (255,))
                elif ct == 3:
                    i = line[x]; c = tuple(plte[i * 3:i * 3 + 3]); a = trns[i] if trns and i < len(trns) else 255; row.append(c + (a,))
                elif ct == 0: row.append((line[x],) * 3 + (255,))
                elif ct == 4: row.append((line[x * 2],) * 3 + (line[x * 2 + 1],))
            else:
                row.append((128, 128, 128, 255))
        px.append(row)
    return w, h, px

def rot_pt(p, rot):
    if not rot: return p
    ax = rot["axis"]; a = math.radians(rot["angle"]); o = rot["origin"]; c, s = math.cos(a), math.sin(a)
    x, y, z = p[0] - o[0], p[1] - o[1], p[2] - o[2]
    if ax == "z": x, y = x * c - y * s, x * s + y * c
    elif ax == "y": x, z = x * c + z * s, -x * s + z * c
    else: y, z = y * c - z * s, y * s + z * c
    return (x + o[0], y + o[1], z + o[2])

def view(p, yaw, pitch, roll, cx=8, cy=8, cz=8):
    x, y, z = p[0] - cx, p[1] - cy, p[2] - cz
    a = math.radians(yaw); x, z = x * math.cos(a) + z * math.sin(a), -x * math.sin(a) + z * math.cos(a)
    a = math.radians(pitch); y, z = y * math.cos(a) - z * math.sin(a), y * math.sin(a) + z * math.cos(a)
    a = math.radians(roll); x, y = x * math.cos(a) - y * math.sin(a), x * math.sin(a) + y * math.cos(a)
    return (x, y, z)

FACES = {"north": ((1, 0, 0), (0, 0, 0), (0, 1, 0), (1, 1, 0)), "south": ((0, 0, 1), (1, 0, 1), (1, 1, 1), (0, 1, 1)),
         "west": ((0, 0, 0), (0, 0, 1), (0, 1, 1), (0, 1, 0)), "east": ((1, 0, 1), (1, 0, 0), (1, 1, 0), (1, 1, 1)),
         "up": ((0, 1, 0), (0, 1, 1), (1, 1, 1), (1, 1, 0)), "down": ((0, 0, 1), (0, 0, 0), (1, 0, 0), (1, 0, 1))}
SHADE = {"up": 1.0, "down": .55, "north": .8, "south": .8, "east": .65, "west": .65}

def render(model, tex, size=256, yaw=-30, pitch=20, roll=0, bg=(32, 32, 40, 255)):
    els = model.get("elements", [])
    texmap = model.get("textures", {})
    ts = [16, 16]
    quads = []
    allp = []
    for e in els:
        x0, y0, z0 = e["from"]; x1, y1, z1 = e["to"]
        for fn, f in e.get("faces", {}).items():
            cs = FACES[fn]
            pts = []
            for cx_, cy_, cz_ in cs:
                p = (x0 + (x1 - x0) * cx_, y0 + (y1 - y0) * cy_, z0 + (z1 - z0) * cz_)
                p = rot_pt(p, e.get("rotation"))
                pts.append(p)
            allp += pts
            quads.append((e, fn, f, pts))
    if not allp: return None
    xs = [p[0] for p in allp]; ys = [p[1] for p in allp]; zs = [p[2] for p in allp]
    cx, cy, cz = (min(xs) + max(xs)) / 2, (min(ys) + max(ys)) / 2, (min(zs) + max(zs)) / 2
    ext = max(max(xs) - min(xs), max(ys) - min(ys), max(zs) - min(zs), 1)
    k = size * 0.86 / (ext * 1.2)
    polys = []
    for e, fn, f, pts in quads:
        v = [view(p, yaw, pitch, roll, cx, cy, cz) for p in pts]
        # нормаль в экране: отбрасываем обратные грани
        ax, ay = v[1][0] - v[0][0], v[1][1] - v[0][1]; bx, by = v[3][0] - v[0][0], v[3][1] - v[0][1]
        cross = ax * by - ay * bx
        zc = sum(p[2] for p in v) / 4
        polys.append((zc, v, fn, f, cross))
    polys.sort(key=lambda t: t[0])
    px = [[bg] * size for _ in range(size)]
    zb = [[-1e9] * size for _ in range(size)]
    for zc, v, fn, f, cross in polys:
        tname = f.get("texture", "#0")
        ref = texmap.get(tname.lstrip("#"))
        tw = th = 16; img = None
        if ref and ref in tex: tw, th, img = tex[ref]
        uv = f.get("uv", [0, 0, ts[0], ts[1]])
        sx = tw / ts[0]; sy = th / ts[1]
        u0, v0, u1, v1 = uv
        # углы граней: (0,0)->(u0,v1)...: используем стандартное соответствие верх-лево -> (u0,v0)
        corner_uv = [(u0, v1), (u1, v1), (u1, v0), (u0, v0)]
        P = [(size / 2 + p[0] * k, size / 2 - p[1] * k) for p in v]
        for p, cu in zip(P, corner_uv): pass
        # аффинное отображение из параллелограмма: P0 + s*(P1-P0) + t*(P3-P0)
        (x0_, y0_), (x1_, y1_), (x3_, y3_) = P[0], P[1], P[3]
        a, b, c, d = x1_ - x0_, y1_ - y0_, x3_ - x0_, y3_ - y0_
        det = a * d - b * c
        if abs(det) < 1e-6: continue
        minx = int(max(0, min(p[0] for p in P))); maxx = int(min(size - 1, max(p[0] for p in P) + 1))
        miny = int(max(0, min(p[1] for p in P))); maxy = int(min(size - 1, max(p[1] for p in P) + 1))
        for y in range(miny, maxy + 1):
            for x in range(minx, maxx + 1):
                dx, dy = x + .5 - x0_, y + .5 - y0_
                s = (dx * d - dy * c) / det; t = (a * dy - b * dx) / det
                if s < 0 or s > 1 or t < 0 or t > 1: continue
                col = (160, 160, 160, 255)
                if img:
                    u = u0 + (u1 - u0) * s; vv = v1 + (v0 - v1) * t
                    ix = min(tw - 1, max(0, int(u * sx))); iy = min(th - 1, max(0, int(vv * sy)))
                    col = img[iy][ix]
                if col[3] < 40: continue
                sh = SHADE.get(fn, .8)
                px[y][x] = (min(255, int(col[0] * sh)), min(255, int(col[1] * sh)), min(255, int(col[2] * sh)), 255)
    return size, px

def write_png(path, size, px):
    raw = b"".join(b"\x00" + b"".join(bytes(px[y][x][:4]) for x in range(size)) for y in range(size))
    def ch(t, d): c = struct.pack(">I", len(d)) + t + d; return c + struct.pack(">I", zlib.crc32(t + d) & 0xffffffff)
    open(path, "wb").write(b"\x89PNG\r\n\x1a\n" + ch(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)) + ch(b"IDAT", zlib.compress(raw, 9)) + ch(b"IEND", b""))
