#!/usr/bin/env bash
# Сборка мода Noxel для Minecraft 26.x (Fabric) обычным javac, без Gradle: в 26.x имена классов игры не обфусцированы.
# Нужны: данные/minecraft/versions/26.2/26.2.jar, библиотеки игры и любой fabric-api-*.jar из папки модов профиля 26.x.
set -euo pipefail
cd "$(dirname "$0")"
ROOT="$(cd .. && pwd)"; D="$ROOT/данные"
MC="$D/minecraft/versions/26.2/26.2.jar"; [[ -f "$MC" ]] || { echo "Нет $MC: запусти один раз игру 26.2"; exit 1; }
API=$(ls "$D"/instances/*/mods/fabric-api-*.jar 2>/dev/null | head -1); [[ -n "$API" ]] || { echo "Нет fabric-api-*.jar в папках модов"; exit 1; }
W=build; rm -rf $W; mkdir -p $W/api $W/classes
unzip -o -q "$API" 'META-INF/jars/fabric-screen-api-v1*' 'META-INF/jars/fabric-api-base*' 'META-INF/jars/fabric-rendering-v1*' 'META-INF/jars/fabric-key-mapping-api-v1*' 'META-INF/jars/fabric-lifecycle-events-v1*' 'META-INF/jars/fabric-message-api-v1*' -d $W/api
CP="$MC:$(find "$D/minecraft/libraries" -name '*.jar' ! -name '*natives*' | tr '\n' ':')$(ls $W/api/META-INF/jars/*.jar | tr '\n' ':')"
javac --release 25 -encoding UTF-8 -cp "$CP" -d $W/classes $(find src -name '*.java')
cp fabric.mod.json $W/classes/
[[ -d resources ]] && cp -r resources/. $W/classes/
java -cp "$W/classes:$CP" noxel.mod.vis.Dump > "$ROOT/исходники/src-tauri/assets/visuals-catalog.json"
( cd $W/classes && jar --create --file ../noxel-client.jar . )
cp $W/noxel-client.jar "$ROOT/исходники/src-tauri/assets/noxel-client.jar"
ls -l "$ROOT/исходники/src-tauri/assets/noxel-client.jar"
