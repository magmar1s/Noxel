#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
if [[ "${1:-}" != --no-build ]]; then
  (cd исходники/src-tauri && cargo build --locked --release --features custom-protocol)
fi
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/Noxel-Linux" 'готовый продукт'
install -m755 исходники/src-tauri/target/release/noxel "$STAGE/Noxel-Linux/Noxel"
install -m755 исходники/linux/noxel исходники/linux/install.sh "$STAGE/Noxel-Linux/"
install -m644 исходники/ui/assets/logo.svg "$STAGE/Noxel-Linux/logo.svg"
install -m644 исходники/linux/README.txt "$STAGE/Noxel-Linux/README.txt"
install -m644 LICENSE "$STAGE/Noxel-Linux/LICENSE"
install -m644 исходники/src-tauri/THIRD_PARTY_NOTICES.md "$STAGE/Noxel-Linux/THIRD_PARTY_NOTICES.md"
tar -czf 'готовый продукт/Noxel-linux-x86_64.tar.gz' -C "$STAGE" Noxel-Linux
sha256sum 'готовый продукт/Noxel-linux-x86_64.tar.gz' > 'готовый продукт/Noxel-linux-x86_64.tar.gz.sha256'
printf '%s\n' 'Собран готовый продукт/Noxel-linux-x86_64.tar.gz'
