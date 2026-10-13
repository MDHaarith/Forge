#!/bin/sh
# Stages the built interface and Python runners
# in build/app (the desktop app's "app" resource folder).
set -eu
cd "$(dirname "$0")/.."
npm run build
rm -rf build/app
mkdir -p build/app/server
cp -R dist build/app/
cp -R server/runners build/app/server/
cp -R LICENSE THIRD-PARTY-NOTICES.md licenses build/app/
find build/app -name __pycache__ -type d -prune -exec rm -rf {} +
