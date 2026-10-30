#!/usr/bin/env bash
# Обновить только сайт на сервере (без переустановки остального):  ./deploy-site.sh
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"
HOST="${NOXEL_SSH:-root@mr1s.site}"; read -r -a SO <<< "${SSH_OPTS:-}"
ssh "${SO[@]}" "$HOST" "mkdir -p /var/www/noxel-site"
tar --exclude='deploy-site.sh' -cf - . | ssh "${SO[@]}" "$HOST" "tar -xf - -C /var/www/noxel-site && chmod -R a+rX /var/www/noxel-site"
echo "Сайт обновлён: https://mr1s.site/"
