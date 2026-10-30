#!/usr/bin/env bash
# Noxel: установка ярлыка и иконки. Использование:
#   ./install.sh            поставить ярлык для уже собранного «готовый продукт/Noxel»
#   ./install.sh --build    сначала собрать приложение (нужны rust и webkit2gtk-4.1)
#   ./install.sh --remove   убрать ярлыки и иконки
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"
ROOT="$PWD"
BIN="$ROOT/готовый продукт/Noxel"
APPS="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICONS="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor"
DESK="$(xdg-user-dir DESKTOP 2>/dev/null || echo "$HOME/Desktop")"

if [[ "${1:-}" == "--remove" ]]; then
  rm -f "$APPS/noxel.desktop" "$DESK/Noxel.desktop" "$ICONS/scalable/apps/noxel.svg" "$ICONS"/*/apps/noxel.png
  echo "Ярлыки и иконки удалены."; exit 0
fi

if [[ "${1:-}" == "--build" ]]; then
  ( cd исходники/src-tauri && cargo build --release --features custom-protocol )
  mkdir -p "готовый продукт"
  cp -f исходники/src-tauri/target/release/noxel "$BIN"
fi
[[ -x "$BIN" ]] || { echo "Нет файла $BIN. Запусти ./install.sh --build"; exit 1; }

# иконки
mkdir -p "$ICONS/scalable/apps"
cp -f исходники/ui/assets/logo.svg "$ICONS/scalable/apps/noxel.svg"
for s in 32 64 128 256 512; do
  mkdir -p "$ICONS/${s}x${s}/apps"
  rsvg-convert -w "$s" -h "$s" исходники/ui/assets/logo.svg -o "$ICONS/${s}x${s}/apps/noxel.png" 2>/dev/null || true
done
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$ICONS" 2>/dev/null || true

# ярлык
ENTRY="[Desktop Entry]
Type=Application
Name=Noxel
GenericName=Minecraft Launcher
Comment=Лаунчер Minecraft: свои моды для каждой версии
Exec=\"$BIN\"
Icon=noxel
Terminal=false
Categories=Game;
StartupWMClass=Noxel"
mkdir -p "$APPS"; printf '%s\n' "$ENTRY" > "$APPS/noxel.desktop"; chmod +x "$APPS/noxel.desktop"
if [[ -d "$DESK" ]]; then
  printf '%s\n' "$ENTRY" > "$DESK/Noxel.desktop"; chmod +x "$DESK/Noxel.desktop"
  command -v gio >/dev/null && gio set "$DESK/Noxel.desktop" metadata::trusted true 2>/dev/null || true
fi
command -v update-desktop-database >/dev/null && update-desktop-database "$APPS" 2>/dev/null || true
echo "Готово. Noxel есть в меню приложений${DESK:+ и на рабочем столе ($DESK)}."
