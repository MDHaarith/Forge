#!/bin/sh
# Stages the built interface, Python runners and optional bundled PCB engine
# in build/app (the desktop app's "app" resource folder).
set -eu
cd "$(dirname "$0")/.."
npm run build
rm -rf build/app
mkdir -p build/app/server
cp -R dist build/app/
cp -R server/runners build/app/server/
if [ -f engines/pcb/dist/pcb-engine.mjs ]; then
  mkdir -p build/app/engines/pcb/dist
  cp engines/pcb/dist/pcb-engine.mjs build/app/engines/pcb/dist/
fi
find build/app -name __pycache__ -type d -prune -exec rm -rf {} +
