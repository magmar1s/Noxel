#!/usr/bin/env bash
# Source and history backup. Private player data and signing keys are excluded.
set -euo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="$ROOT/готовый продукт/Noxel-backup-$STAMP.tar.gz"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/Noxel/source" 'готовый продукт'
git archive HEAD | tar -xf - -C "$STAGE/Noxel/source"
git bundle create "$STAGE/Noxel/Noxel.git.bundle" --all
git rev-parse HEAD > "$STAGE/Noxel/commit.txt"
printf '%s\n' 'Source: source/. History: git clone Noxel.git.bundle Noxel.' 'Player data, signing keys and compiled output are excluded.' > "$STAGE/Noxel/RESTORE.txt"
tar -czf "$OUT" -C "$STAGE" Noxel
sha256sum "$OUT" > "$OUT.sha256"
printf '%s\n' "$OUT"
