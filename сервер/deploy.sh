#!/usr/bin/env bash
# Установка сервера Noxel на чистый Debian/Ubuntu. Запускать от root в этой папке:
#   bash deploy.sh mr1s.site
# Домен должен указывать на этот сервер, порты 80 и 443 должны быть открыты.
set -euo pipefail
DOMAIN="${1:?укажи домен, например: bash deploy.sh mr1s.site}"
cd "$(dirname "$0")"

export DEBIAN_FRONTEND=noninteractive
apt-get install -y -qq nginx certbot python3-certbot-nginx

id noxel >/dev/null 2>&1 || useradd --system --home /var/lib/noxel --shell /usr/sbin/nologin noxel
install -d -o noxel -g noxel -m 711 /var/lib/noxel
install -d -o noxel -g noxel -m 755 /var/lib/noxel/files
install -d -o noxel -g noxel -m 755 /var/lib/noxel/dl
install -d -o root -g noxel -m 750 /etc/noxel
# токен администратора (нужен, чтобы менять список серверов из лаунчера); создаётся один раз
[ -s /etc/noxel/admin.token ] || { umask 077; openssl rand -hex 32 > /etc/noxel/admin.token; }
chown root:noxel /etc/noxel/admin.token; chmod 640 /etc/noxel/admin.token
# ключ CurseForge кладётся вручную: install -m 640 -o root -g noxel /dev/stdin /etc/noxel/cf.key
[ -s /var/lib/noxel/servers.json ] || install -m 644 -o noxel -g noxel servers.seed.json /var/lib/noxel/servers.json
install -d -m 755 /var/www/noxel-site
[ -d ../сайт ] && cp -r ../сайт/. /var/www/noxel-site/ && chmod -R a+rX /var/www/noxel-site
install -d -m 755 /opt/noxel
install -m 755 noxel-server.py /opt/noxel/noxel-server.py
sed "s#https://mr1s.site/noxel#https://${DOMAIN}/noxel#" noxel.service > /etc/systemd/system/noxel.service
sed "s#mr1s.site#${DOMAIN}#g" noxel-nginx.conf > /etc/nginx/conf.d/noxel.conf
rm -f /etc/nginx/sites-enabled/default

systemctl daemon-reload
systemctl enable --now noxel
systemctl restart noxel
nginx -t
systemctl enable --now nginx
systemctl reload nginx

# HTTPS (Let's Encrypt, проверка по HTTP). Почту не передаём.
certbot --nginx -d "$DOMAIN" -d "www.$DOMAIN" --non-interactive --agree-tos \
  --register-unsafely-without-email --redirect
systemctl reload nginx
echo "Готово: https://${DOMAIN}/noxel/v1/ping"
