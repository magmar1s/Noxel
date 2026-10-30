#!/usr/bin/env bash
# Публикация установщика на сервер Noxel: файл + запись в downloads.json (его читает сайт).
#   ./publish-setup.sh linux-x86_64 путь/к/noxel-setup
#   ./publish-setup.sh windows-x86_64 путь/к/noxel-setup.exe
set -euo pipefail
PLAT="${1:?платформа: linux-x86_64 или windows-x86_64}"; FILE="${2:?путь к файлу установщика}"
HOST="${NOXEL_SSH:-root@mr1s.site}"; read -r -a SO <<< "${SSH_OPTS:-}"
EXT=""; [[ "$PLAT" == windows* ]] && EXT=".exe"
NAME="noxel-setup-$PLAT$EXT"; SHA=$(sha256sum "$FILE" | cut -d' ' -f1); SIZE=$(stat -c %s "$FILE")
scp -q "${SO[@]}" "$FILE" "$HOST:/tmp/$NAME"
ssh "${SO[@]}" "$HOST" "install -m 644 -o noxel -g noxel /tmp/$NAME /var/lib/noxel/dl/$NAME && rm -f /tmp/$NAME && python3 - '$PLAT' '$NAME' '$SHA' '$SIZE' <<'PY'
import json, os, sys, time
plat, name, sha, size = sys.argv[1:5]
p = '/var/lib/noxel/dl/downloads.json'
d = json.load(open(p)) if os.path.exists(p) else {}
d.setdefault('installers', {})[plat] = {'file': name, 'sha256': sha, 'size': int(size), 'updated': int(time.time())}
json.dump(d, open(p + '.tmp', 'w'), indent=1)
os.replace(p + '.tmp', p); os.chmod(p, 0o644)
PY"
echo "Установщик опубликован: $NAME ($SIZE байт, sha256 $SHA)"
