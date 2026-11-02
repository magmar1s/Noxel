#!/usr/bin/env python3
"""Импорт готовых 3D-ножей с открытыми лицензиями в каталог косметики Noxel.
Источники (скачаны с Modrinth):
  • Karambit Mod, автор KJ20series (лицензия BSD-2-Clause по Modrinth, Ms-RL по файлу мода): геометрия керамбита и 16 цветов
  • Kamish Dagger, автор LIsPixel (MIT)
Запуск: python3 knives_import.py <папка со скачанными файлами> > knives.json"""
import zipfile, json, sys, os, base64, re
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rend

src = sys.argv[1]
out = {"models": []}

def minify(m):
    def r(o):
        if isinstance(o, float): return round(o, 4)
        if isinstance(o, list): return [r(x) for x in o]
        if isinstance(o, dict): return {k: r(v) for k, v in o.items()}
        return o
    return r(m)

# ── Керамбит ──
z = zipfile.ZipFile(os.path.join(src, "m_karambit-mod.jar"))
base = "assets/karambit_mod/"
geo = json.loads(z.read(base + "models/custom/karambit.json"))
geo = minify(geo)
for e in geo["elements"]:
    for f in e["faces"].values(): f["texture"] = "#t"
geo.pop("credit", None); geo.pop("textures", None)
geo["textures"] = {"t": "noxel:item/knife", "particle": "noxel:item/knife"}
# модель автора лежит на «земле» (y≈0): поднимаем в центр ячейки, чтобы вращение в руке шло вокруг самого ножа
for e in geo["elements"]:
    e["from"][1] = round(e["from"][1] + 7.75, 4); e["to"][1] = round(e["to"][1] + 7.75, 4)
    if "rotation" in e: e["rotation"]["origin"][1] = round(e["rotation"]["origin"][1] + 7.75, 4)
geo["display"] = {
    "thirdperson_righthand": {"rotation": [90, -90, 35], "translation": [0, 3, 1], "scale": [0.5, 0.5, 0.5]},
    "thirdperson_lefthand": {"rotation": [90, 90, -35], "translation": [0, 3, 1], "scale": [0.5, 0.5, 0.5]},
    "firstperson_righthand": {"rotation": [90, -90, 35], "translation": [1.2, 3.2, 1.5], "scale": [0.42, 0.42, 0.42]},
    "firstperson_lefthand": {"rotation": [90, 90, -35], "translation": [1.2, 3.2, 1.5], "scale": [0.42, 0.42, 0.42]},
    "gui": {"rotation": [90, 0, -35], "translation": [0, 0, 0], "scale": [1.05, 1.05, 1.05]},
    "fixed": {"rotation": [90, 0, -35], "translation": [0, 0, 0], "scale": [1.0, 1.0, 1.0]},
    "ground": {"rotation": [90, 0, 0], "translation": [0, 2, 0], "scale": [0.55, 0.55, 0.55]},
}
names = {"karambit": "Чёрный", "karambit_blue": "Синий", "karambit_brown": "Коричневый", "karambitcyannotlightblue": "Бирюзовый", "golden_karambit": "Золото", "karambitgray": "Серый",
         "karambit_green": "Зелёный", "karambit_light_blue": "Голубой", "karambit_lime": "Лайм", "karambitmagenta": "Пурпурный", "karambit_orange": "Оранжевый", "karambitpink": "Розовый",
         "karambit_purple": "Фиолетовый", "karambitred": "Красный", "karambit_white": "Белый", "karambityellow": "Жёлтый"}
texs = []
for item, ru in names.items():
    try: it = json.loads(z.read(base + f"models/item/{item}.json"))
    except KeyError: continue
    tref = [v for k, v in it["textures"].items() if k != "particle"][0]
    path = base + "textures/" + tref.split(":")[-1] + ".png"
    try: data = z.read(path)
    except KeyError: continue
    pxs = rend.read_png(data)
    cols = [p for row in pxs[2] for p in row if p[3] > 200 and max(p[:3]) - min(p[:3]) > 40]
    avg = [sum(c[i] for c in cols) // len(cols) for i in range(3)] if cols else [150, 150, 160]
    texs.append({"id": item, "name": ru, "png": base64.b64encode(data).decode(), "_px": pxs, "color": "#%02x%02x%02x" % tuple(avg)})
def preview(model, px, yaw=-35, pitch=18, roll=0):
    m = json.loads(json.dumps(model)); 
    tex = {"noxel:item/knife": px, "item/knife": px}
    r = rend.render(m, tex, 224, yaw=yaw, pitch=pitch, roll=roll, bg=(0, 0, 0, 0))
    import io, tempfile
    p = tempfile.mktemp(suffix=".png"); rend.write_png(p, *r); d = open(p, "rb").read(); os.remove(p)
    return base64.b64encode(d).decode()
for t in texs: t["preview"] = preview(geo, t.pop("_px"), -20, 62, 0)
out["models"].append({"id": "karambit", "name": "Керамбит", "desc": "Изогнутый коготь с кольцом для пальца. 16 цветов", "author": "KJ20series", "license": "BSD-2-Clause / Ms-RL", "source": "https://modrinth.com/mod/karambit-mod",
                      "model": json.dumps(geo, separators=(",", ":"), ensure_ascii=False), "textures": texs})

# ── Кинжал Камиш ──
z = zipfile.ZipFile(os.path.join(src, "kamish-dagger.zip"))
geo = json.loads(z.read("assets/minecraft/models/kamish_dagger/kamish_dagger.json"))
geo = minify(geo)
for e in geo["elements"]:
    for f in e["faces"].values(): f["texture"] = "#t"
geo.pop("credit", None); geo.pop("parent", None)
geo["textures"] = {"t": "noxel:item/knife", "particle": "noxel:item/knife"}
geo["display"]["firstperson_righthand"] = {"rotation": [-180, -88.5, 168.25], "translation": [1.75, 1.5, 0.5], "scale": [0.5, 0.5, 0.5]}   # у автора крупнее руки
geo["display"]["firstperson_lefthand"] = {"rotation": [-29.75, -88.5, 168.25], "translation": [3.75, -1.5, 0.5], "scale": [0.5, 0.5, 0.5]}
data = z.read("assets/minecraft/textures/item/kamish_dagger/kamish_dagger.png")
px = rend.read_png(data)
out["models"].append({"id": "kamish", "name": "Кинжал Камиш", "desc": "Тёмный обоюдоострый кинжал", "author": "LIsPixel", "license": "MIT", "source": "https://modrinth.com/resourcepack/kamish-dagger",
                      "model": json.dumps(geo, separators=(",", ":"), ensure_ascii=False),
                      "textures": [{"id": "kamish", "name": "Оригинал", "png": base64.b64encode(data).decode(), "preview": preview(geo, px, -25, 25, 0)}]})
print(json.dumps(out, ensure_ascii=False))
