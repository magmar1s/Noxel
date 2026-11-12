#!/usr/bin/env bash
set -euo pipefail
SOURCE="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
DEST="${XDG_DATA_HOME:-$HOME/.local/share}/noxel-app"
APPS="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICONS="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/scalable/apps"
umask 077
if [[ "${1:-}" == --remove ]]; then
  rm -f "$APPS/noxel.desktop" "$ICONS/noxel.svg"
  rm -f "$DEST/Noxel" "$DEST/noxel" "$DEST/logo.svg"
  rmdir "$DEST" 2>/dev/null || true
  printf '%s\n' 'Noxel удалён. Профили, аккаунты и сохранения оставлены.'
  exit 0
fi
[[ -f "$SOURCE/Noxel" ]] || { printf '%s\n' 'Распакуй архив полностью.' >&2; exit 1; }
mkdir -p "$DEST" "$APPS" "$ICONS"
if [[ "$SOURCE" != "$DEST" ]]; then
  install -m755 "$SOURCE/Noxel" "$DEST/Noxel.new"
  mv -f "$DEST/Noxel.new" "$DEST/Noxel"
  install -m755 "$SOURCE/noxel" "$DEST/noxel"
  install -m644 "$SOURCE/logo.svg" "$DEST/logo.svg"
fi
install -m644 "$DEST/logo.svg" "$ICONS/noxel.svg"
# Desktop Entry quoting: escape reserved characters, including percent field codes.
EXEC_PATH="${DEST//\\/\\\\}/noxel"
EXEC_PATH="${EXEC_PATH//\$/\\\$}"
EXEC_PATH="${EXEC_PATH//\`/\\\`}"
EXEC_PATH="${EXEC_PATH//\"/\\\"}"
EXEC_PATH="${EXEC_PATH//%/%%}"
cat > "$APPS/noxel.desktop" <<ENTRY
[Desktop Entry]
Type=Application
Name=Noxel
Comment=Лаунчер Minecraft
Exec="$EXEC_PATH"
Icon=noxel
Terminal=false
Categories=Game;
StartupWMClass=Noxel
ENTRY
chmod 644 "$APPS/noxel.desktop"
if command -v update-desktop-database >/dev/null; then update-desktop-database "$APPS" 2>/dev/null || true; fi
printf '%s\n' 'Готово. Открой Noxel в меню приложений. Java скачивается самим лаунчером.'
