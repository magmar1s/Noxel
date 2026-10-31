#!/usr/bin/env bash
# Публикация релиза на сервер Noxel: подпись, загрузка, манифест для самообновления лаунчера.
#   ./publish.sh 0.2.0 "Первая строка заметок" "Вторая строка"
# Нужны: закрытый ключ .keys/release.pem, собранный «готовый продукт/Noxel» и доступ по ssh к серверу.
#   NOXEL_SSH=root@mr1s.site            куда заходить по ssh
#   SSH_OPTS="-o ControlPath=..."       дополнительные параметры ssh/scp (необязательно)
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"
VER="${1:?укажи версию, например: ./publish.sh 0.2.0 \"что нового\"}"; shift || true
BIN="${LIN_BIN:-готовый продукт/Noxel}"; KEY=".keys/release.pem"; HOST="${NOXEL_SSH:-root@mr1s.site}"; BASE="https://mr1s.site/noxel"
[[ -f "$BIN" && -f "$KEY" ]] || { echo "Нет файла $BIN или ключа $KEY"; exit 1; }
read -r -a SO <<< "${SSH_OPTS:-}"
NAME="noxel-linux-x86_64-$VER"
SHA=$(sha256sum "$BIN" | cut -d' ' -f1)
SIG=$(openssl pkeyutl -sign -rawin -inkey "$KEY" -in "$BIN" | od -An -tx1 | tr -d ' \n')
SIZE=$(stat -c %s "$BIN")
NEWS="[]"; [[ -f сервер/news.json ]] && NEWS=$(cat сервер/news.json)
NOTES=$(printf '%s\n' "$@" | python3 -c 'import json,sys; print(json.dumps([l.strip() for l in sys.stdin if l.strip()], ensure_ascii=False))')
# Windows-сборка (необязательно): WIN_BIN=путь/к/Noxel.exe ./publish.sh 0.4.0 "заметки"
WIN="{}"
if [[ -n "${WIN_BIN:-}" ]]; then
  WNAME="noxel-windows-x86_64-$VER.exe"
  WSHA=$(sha256sum "$WIN_BIN" | cut -d' ' -f1)
  WSIG=$(openssl pkeyutl -sign -rawin -inkey "$KEY" -in "$WIN_BIN" | od -An -tx1 | tr -d ' \n')
  WIN=$(python3 -c 'import json,sys; print(json.dumps({"version": sys.argv[5], "url": sys.argv[1], "sha256": sys.argv[2], "sig": sys.argv[3], "size": int(sys.argv[4])}))' "$BASE/dl/$WNAME" "$WSHA" "$WSIG" "$(stat -c %s "$WIN_BIN")" "$VER")
  scp -q "${SO[@]}" "$WIN_BIN" "$HOST:/tmp/$WNAME"
  ssh "${SO[@]}" "$HOST" "install -m 644 -o noxel -g noxel /tmp/$WNAME /var/lib/noxel/dl/$WNAME && rm -f /tmp/$WNAME"
fi
# без WIN_BIN запись Windows остаётся прежней (с её собственной версией), чтобы выпуск для Linux не ломал обновление на Windows
OLD=$(ssh "${SO[@]}" "$HOST" "cat /var/lib/noxel/launcher.json 2>/dev/null || echo {}")
python3 - "$VER" "$SHA" "$SIG" "$SIZE" "$BASE/dl/$NAME" "$NOTES" "$NEWS" "$WIN" "$OLD" > /tmp/noxel-launcher.json <<'PY'
import json, sys
ver, sha, sig, size, url, notes, news, win, old = sys.argv[1:10]
files = {"linux-x86_64": {"version": ver, "url": url, "sha256": sha, "sig": sig, "size": int(size)}}
if json.loads(win):
    files["windows-x86_64"] = json.loads(win)
else:
    o = json.loads(old or "{}")
    keep = (o.get("files") or {}).get("windows-x86_64")
    if keep:
        keep.setdefault("version", o.get("version", ""))
        files["windows-x86_64"] = keep
print(json.dumps({"version": ver, "notes": json.loads(notes), "news": json.loads(news), "files": files}, ensure_ascii=False, indent=1))
PY
scp -q "${SO[@]}" "$BIN" "$HOST:/tmp/$NAME"
scp -q "${SO[@]}" /tmp/noxel-launcher.json "$HOST:/tmp/launcher.json"
ssh "${SO[@]}" "$HOST" "install -m 644 -o noxel -g noxel /tmp/$NAME /var/lib/noxel/dl/$NAME && install -m 644 -o noxel -g noxel /tmp/launcher.json /var/lib/noxel/launcher.json && rm -f /tmp/$NAME /tmp/launcher.json && ls /var/lib/noxel/dl | tail -n 5"
rm -f /tmp/noxel-launcher.json
echo "Опубликовано: версия $VER, sha256 $SHA"
