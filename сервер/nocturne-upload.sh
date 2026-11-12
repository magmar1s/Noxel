#!/usr/bin/env bash
# Выкладывает Nocturne на сервер Noxel и добавляет его в «Рекомендуемое» (раздел «Визуалы»).
#   NOXEL_SSH=root@166.1.144.187 SSH_OPTS="..." ./nocturne-upload.sh
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"
HOST="${NOXEL_SSH:-root@mr1s.site}"
read -r -a SO <<< "${SSH_OPTS:-}"
scp -q "${SO[@]}" dl-nocturne/nocturne-1.0.0.jar dl-nocturne/nocturne.png noxel-server.py recommended.seed.json "$HOST:/tmp/"
ssh "${SO[@]}" "$HOST" bash -s <<'SH'
set -e
install -m 644 -o noxel -g noxel /tmp/nocturne-1.0.0.jar /var/lib/noxel/dl/nocturne-1.0.0.jar
install -m 644 -o noxel -g noxel /tmp/nocturne.png /var/lib/noxel/dl/nocturne.png
install -m 755 /tmp/noxel-server.py /opt/noxel/noxel-server.py
# раздел «Визуалы» из seed ставится первым в живой список, остальное не трогаем
python3 - <<'PY'
import json, time
live = "/var/lib/noxel/recommended.json"
try: cur = json.load(open(live))
except Exception: cur = {"categories": []}
seed = json.load(open("/tmp/recommended.seed.json"))
vis = next(c for c in seed["categories"] if c["id"] == "visuals")
cur["categories"] = [vis] + [c for c in cur.get("categories", []) if c.get("id") != "visuals"]
cur["version"] = int(time.time())
json.dump(cur, open(live, "w"), ensure_ascii=False, indent=1)
PY
chown noxel:noxel /var/lib/noxel/recommended.json
rm -f /tmp/nocturne-1.0.0.jar /tmp/nocturne.png /tmp/noxel-server.py /tmp/recommended.seed.json
systemctl restart noxel
SH
curl -sfI https://mr1s.site/noxel/dl/nocturne-1.0.0.jar | head -1
curl -s https://mr1s.site/noxel/v1/recommended | grep -c '"nocturne"'
