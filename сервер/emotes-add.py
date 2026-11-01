#!/usr/bin/env python3
"""Каталог эмоций Noxel: добавляет эмоции Emotecraft (.json) из папки или .zip в каталог и пересобирает index.json.

  ./emotes-add.py ПАПКА_ИЛИ_ZIP --license CC0-1.0 [--author "Автор"] [--source "https://..."]
  ./emotes-add.py --list              показать, сколько эмоций и какие лицензии
  ./emotes-add.py --remove id1 id2    убрать эмоции из каталога

Каталог лежит в «emotes-catalog/» рядом со скриптом. Залить на сервер: ./emotes-deploy.sh
Добавляй только то, что можно раздавать: свои эмоции, CC0/CC-BY/MIT, либо с разрешения автора.
Эмоции с пометкой nsfw пропускаются.
"""
import argparse, json, os, re, sys, zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
CAT = os.path.join(HERE, "emotes-catalog")
FILES = os.path.join(CAT, "files")
IDX = os.path.join(CAT, "index.json")

TAGS = [
    ("dance", "Танцы", r"danc|floss|dab|twerk|penguin|justice|bps|headspin|orange|jumping|party|disco|moonwalk|shuffle|macarena|breakdance|spin|groove|bloop|naruto|train|fortnite|gangnam"),
    ("gesture", "Жесты", r"wave|bow|cross|heart|hug|clap|salute|point|thumb|hand|shake|nod|yes|no\b|greet|hello|bye|kiss|handshake|facepalm|shrug|fist|peace|gesture"),
    ("sit", "Сидя и лёжа", r"sit|lay|sleep|lie|meditat|campfire|chair|rest|relax|squat|kneel"),
    ("emotion", "Эмоции", r"cry|laugh|angry|sad|happy|scare|nervous|shy|think|confus|love|cheer|celebrat|victory|win|lose|panic|dizzy|tired"),
    ("action", "Действия", r"jump|run|walk|fight|punch|kick|sword|attack|swim|fly|climb|dive|throw|catch|selfie|inspect|look|lean|stretch|yoga|pose|lunge|tpose|t-pose|hunchback"),
]

def slug(s):
    s = re.sub(r"[^a-z0-9]+", "-", s.lower()).strip("-")
    return s or "emote"

def truthy(v):
    return v is True or (isinstance(v, str) and v.lower() == "true")

def tags_for(name, em):
    out = []
    low = name.lower()
    for tid, _title, rx in TAGS:
        if re.search(rx, low):
            out.append(tid)
    moves = em.get("moves") or []
    parts = set()
    for m in moves:
        for p in ("head", "torso", "rightArm", "leftArm", "rightLeg", "leftLeg"):
            if isinstance(m.get(p), dict) and m[p]:
                parts.add(p)
    if not out:
        out.append("other")
    if truthy(em.get("isLoop")):
        out.append("loop")
    if {"rightLeg", "leftLeg"} & parts:
        out.append("legs")
    return out

def load():
    if os.path.exists(IDX):
        return json.load(open(IDX, encoding="utf8"))
    return {"version": 1, "tags": [{"id": t[0], "title": t[1]} for t in TAGS] + [{"id": "other", "title": "Разное"}], "emotes": []}

def save(idx):
    idx["emotes"].sort(key=lambda e: e["name"].lower())
    idx["count"] = len(idx["emotes"])
    os.makedirs(CAT, exist_ok=True)
    with open(IDX, "w", encoding="utf8") as f:
        json.dump(idx, f, ensure_ascii=False, separators=(",", ":"))

def sources(path):
    if os.path.isdir(path):
        for root, _d, files in os.walk(path):
            if "nsfw" in root.lower():
                continue
            for fn in files:
                if fn.lower().endswith(".json"):
                    yield fn, open(os.path.join(root, fn), "rb").read()
    else:
        z = zipfile.ZipFile(path)
        for n in z.namelist():
            if n.lower().endswith(".json") and "nsfw" not in n.lower() and not n.endswith("/"):
                yield os.path.basename(n), z.read(n)

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("src", nargs="?")
    ap.add_argument("--license", default="")
    ap.add_argument("--author", default="")
    ap.add_argument("--source", default="")
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--remove", nargs="+")
    a = ap.parse_args()
    idx = load()
    if a.list:
        from collections import Counter
        print(len(idx["emotes"]), "эмоций;", dict(Counter(e["license"] for e in idx["emotes"])))
        return
    if a.remove:
        idx["emotes"] = [e for e in idx["emotes"] if e["id"] not in a.remove]
        for i in a.remove:
            try: os.remove(os.path.join(FILES, i + ".json"))
            except OSError: pass
        save(idx); print("Готово, осталось", len(idx["emotes"])); return
    if not a.src or not a.license:
        ap.error("нужны ПАПКА_ИЛИ_ZIP и --license (например CC0-1.0, CC-BY-4.0, MIT, own)")
    os.makedirs(FILES, exist_ok=True)
    have = {e["id"] for e in idx["emotes"]}
    added = skipped = 0
    for fn, raw in sources(a.src):
        try:
            j = json.loads(raw.decode("utf-8-sig"))
            em = j["emote"]
            if not isinstance(em.get("moves"), list):
                raise ValueError("нет moves")
        except Exception:
            skipped += 1; continue
        if truthy(em.get("nsfw")) or truthy(j.get("nsfw")):
            skipped += 1; continue
        name = (j.get("name") or os.path.splitext(fn)[0]).strip()
        name = re.sub(r"§.", "", name).strip() or os.path.splitext(fn)[0]
        base = slug(os.path.splitext(fn)[0]); eid = base; n = 2
        while eid in have:
            eid = f"{base}-{n}"; n += 1
        have.add(eid)
        open(os.path.join(FILES, eid + ".json"), "wb").write(raw)
        idx["emotes"].append({"id": eid, "name": name, "author": re.sub(r"§.", "", str(j.get("author") or a.author or "")).strip() or a.author,
                              "desc": re.sub(r"§.", "", str(j.get("description") or "")).strip()[:160], "license": a.license, "source": a.source,
                              "tags": tags_for(name, em), "ticks": int(em.get("endTick") or 0), "loop": truthy(em.get("isLoop")),
                              "file": f"files/{eid}.json", "size": len(raw)})
        added += 1
    save(idx)
    print(f"Добавлено {added}, пропущено {skipped}, всего {len(idx['emotes'])}")

main()
