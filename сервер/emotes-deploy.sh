#!/usr/bin/env bash
# Залить каталог эмоций на сервер: https://mr1s.site/noxel/dl/emotes/index.json
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"
HOST="${NOXEL_SSH:-root@mr1s.site}"; read -r -a SO <<< "${SSH_OPTS:-}"
ssh "${SO[@]}" "$HOST" "mkdir -p /var/lib/noxel/dl/emotes"
tar -C emotes-catalog -cf - . | ssh "${SO[@]}" "$HOST" "tar -xf - -C /var/lib/noxel/dl/emotes && chown -R noxel:noxel /var/lib/noxel/dl/emotes && chmod -R a+rX /var/lib/noxel/dl/emotes"
echo "Каталог эмоций обновлён: https://mr1s.site/noxel/dl/emotes/index.json"
