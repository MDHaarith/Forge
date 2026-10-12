#!/bin/sh
# Stages what the desktop app ships: the server, the built interface, the server's
# runtime packages and the bundled PCB engine, in build/app (the app's "app" resource folder).
set -eu
cd "$(dirname "$0")/.."
npm run build
npm run pcb-engine
rm -rf build/app
mkdir -p build/app/engines/pcb/dist
cp -R server dist package.json package-lock.json build/app/
cp engines/pcb/dist/pcb-engine.mjs build/app/engines/pcb/dist/
find build/app -name __pycache__ -type d -prune -exec rm -rf {} +
npm ci --prefix build/app --omit=dev --ignore-scripts --no-audit --no-fund
# The interface is prebuilt, so its three.js source is not needed at run time.
rm -rf build/app/node_modules/three build/app/node_modules/@types
find build/app/node_modules -type d -empty -delete
