#!/usr/bin/env bash
# Сборка агента. Нужны JDK и jar-файлы LWJGL 3 (из папки libraries лаунчера: lwjgl и lwjgl-glfw).
#   LWJGL=путь/к/libraries/org/lwjgl ./build.sh
# Результат: ../исходники/src-tauri/assets/noxel-agent.jar (хранится в репозитории, чтобы сборке на CI не нужен был LWJGL)
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"
LW="${LWJGL:-../данные/minecraft/libraries/org/lwjgl}"
CP=$(find "$LW" -name 'lwjgl-3*.jar' -o -name 'lwjgl-glfw-3*.jar' | grep -v -e natives -e sources | tr '\n' ':')
rm -rf build && mkdir -p build/classes
javac --release 8 -Xlint:-options -cp "$CP" -d build/classes src/noxel/*.java
printf 'Premain-Class: noxel.Agent\nCan-Retransform-Classes: false\n' > build/MANIFEST.MF
jar cfm build/noxel-agent.jar build/MANIFEST.MF -C build/classes .
mkdir -p ../исходники/src-tauri/assets
cp build/noxel-agent.jar ../исходники/src-tauri/assets/noxel-agent.jar
ls -l ../исходники/src-tauri/assets/noxel-agent.jar
