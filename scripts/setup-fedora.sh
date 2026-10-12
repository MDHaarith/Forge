#!/usr/bin/env bash
# Sets Forge up on Fedora: system packages with dnf, FreeCAD from Flathub (Fedora's
# repositories have no FreeCAD), the app's own dependencies and build, and a `forge`
# command with a desktop launcher that runs Forge as a systemd user service.
#
#   scripts/setup-fedora.sh                 everything (3D, PCB, chip tools)
#   scripts/setup-fedora.sh --only "3d"     just some domains: any of 3d, pcb, chip, rf
#   scripts/setup-fedora.sh --with-rf       also build openEMS for antenna simulation (about 20 min)
#   scripts/setup-fedora.sh --desktop       also build and install the Forge desktop app (RPM)
#   scripts/setup-fedora.sh --dry-run       print what would happen, change nothing
set -euo pipefail

DOMAINS="3d pcb chip"
DRY_RUN=0
LAUNCHER=1
DESKTOP=0
FORCE=0
WITH_RF=0
OS_RELEASE="${OS_RELEASE:-/etc/os-release}"
REPO="$(cd "$(dirname "$0")/.." && pwd)"

usage() {
  sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'
  echo
  echo "Options: --only \"3d pcb chip rf\"  --with-rf  --desktop  --no-launcher  --dry-run  --force (run on a non-Fedora system)"
}

while [ $# -gt 0 ]; do
  case "$1" in
    --only) DOMAINS="${2:?--only needs a list, e.g. \"3d chip\"}"; shift ;;
    --with-rf) WITH_RF=1 ;;
    --no-launcher) LAUNCHER=0 ;;
    --desktop) DESKTOP=1 ;;
    --dry-run) DRY_RUN=1 ;;
    --force) FORCE=1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
  esac
  shift
done

[ "$WITH_RF" = 1 ] && DOMAINS="$DOMAINS rf"
for d in $DOMAINS; do
  case "$d" in 3d|pcb|chip|rf) ;; *) echo "unknown domain \"$d\" (use 3d, pcb, chip, rf)" >&2; exit 2 ;; esac
done
want() { case " $DOMAINS " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }

say() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }
run() {
  printf '+ %s\n' "$*"
  [ "$DRY_RUN" = 1 ] || "$@"
}
# Writes stdin to a file (or shows it, in a dry run).
write() {
  if [ "$DRY_RUN" = 1 ]; then
    printf '+ write %s\n' "$1"
    cat >/dev/null
  else
    mkdir -p "$(dirname "$1")"
    cat >"$1"
  fi
}

# --- 1. Check the system -------------------------------------------------------------

field() { sed -n "s/^$1=\"\{0,1\}\([^\"]*\)\"\{0,1\}$/\1/p" "$OS_RELEASE" | head -n 1; }
ID="$(field ID)"; ID_LIKE="$(field ID_LIKE)"; VERSION_ID="$(field VERSION_ID)"
case " $ID $ID_LIKE " in
  *" fedora "*) ;;
  *) if [ "$FORCE" = 1 ]; then echo "warning: not Fedora ($ID); continuing because of --force" >&2
     else echo "This script is for Fedora (found \"$ID\"). Use --force to try anyway, or see the README for other systems." >&2; exit 1; fi ;;
esac
say "Setting up Forge on ${ID^} ${VERSION_ID} for: $DOMAINS"
[ "$(id -u)" = 0 ] && SUDO="" || SUDO="sudo"

# --- 2. System packages --------------------------------------------------------------

PACKAGES=(git nodejs nodejs-npm curl xdg-utils)
want 3d && PACKAGES+=(blender python3-numpy openscad librsvg2-tools flatpak)
want pcb && PACKAGES+=(kicad)
want chip && PACKAGES+=(iverilog yosys)
# openEMS is not packaged for Fedora; these build it from source (step 3b).
want rf && PACKAGES+=(gcc gcc-c++ cmake git boost-devel tinyxml-devel hdf5-devel CGAL-devel vtk-devel
  python3-pip python3-setuptools python3-wheel python3-setuptools_scm python3-Cython python3-numpy python3-h5py python3-matplotlib)
# What building the desktop app (Tauri) needs.
[ "$DESKTOP" = 1 ] && PACKAGES+=(gcc rust cargo webkit2gtk4.1-devel openssl-devel librsvg2-devel libxdo-devel)

say "Installing packages with dnf"
want pcb && echo "(KiCad with its libraries is a large download, about 1-2 GB.)"
run $SUDO dnf install -y "${PACKAGES[@]}"

# Forge needs Node.js 22.18 or newer, for built-in TypeScript support.
node_ok() { "$1" -e 'const [a,b]=process.versions.node.split(".").map(Number); process.exit(a>22||(a===22&&b>=18)?0:1)' 2>/dev/null; }
find_node() { for n in node node-24 node-22; do command -v "$n" >/dev/null && node_ok "$(command -v "$n")" && { command -v "$n"; return; }; done; }
NODE="$(find_node || true)"
if [ -z "$NODE" ] && [ "$DRY_RUN" = 0 ]; then
  say "The default Node.js is older than 22.18; installing nodejs24"
  run $SUDO dnf install -y nodejs24 nodejs24-npm
  NODE="$(find_node || true)"
  [ -n "$NODE" ] || { echo "Could not get Node.js 22.18 or newer. Install it, then run this script again." >&2; exit 1; }
fi
NODE="${NODE:-/usr/bin/node}"
NPM="${NPM:-$(dirname "$NODE")/npm}"; [ -x "$NPM" ] || command -v "$NPM" >/dev/null || NPM="npm"

# --- 3. FreeCAD from Flathub ---------------------------------------------------------

if want 3d; then
  say "Installing FreeCAD from Flathub (per user, no root needed)"
  run flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
  run flatpak install --user -y --noninteractive flathub org.freecad.FreeCAD
fi

# --- 3b. openEMS (antenna simulation) -----------------------------------------------

OPENEMS_VERSION=v0.37.0
OPENEMS="${FORGE_OPENEMS_HOME:-$HOME/opt/openEMS}"
if want rf; then
  if [ -x "$OPENEMS/venv/bin/python3" ] && "$OPENEMS/venv/bin/python3" -c "import openEMS" 2>/dev/null; then
    say "openEMS is already installed in $OPENEMS"
  else
    say "Building openEMS $OPENEMS_VERSION into $OPENEMS (per user, about 20 minutes)"
    SRC="$HOME/.cache/forge/openEMS-Project"
    if [ ! -d "$SRC/.git" ]; then
      run git clone --recursive --shallow-submodules --depth 1 --branch "$OPENEMS_VERSION" https://github.com/thliebig/openEMS-Project.git "$SRC"
    fi
    run env -C "$SRC" ./update_openEMS.sh "$OPENEMS" --disable-GUI --python --python-venv-mode site
  fi
fi

# --- 4. Forge itself -----------------------------------------------------------------

say "Installing Forge's dependencies, its built-in PCB engine, and building the interface"
run "$NPM" --prefix "$REPO" ci --no-audit --no-fund
run "$NPM" --prefix "$REPO" run pcb-engine
run "$NPM" --prefix "$REPO" run build

if [ "$DESKTOP" = 1 ]; then
  say "Building the Forge desktop app (the first build compiles for several minutes)"
  run "$NPM" --prefix "$REPO" run desktop:build
  VERSION="$(sed -n 's/^  "version": "\(.*\)",$/\1/p' "$REPO/package.json")"
  run $SUDO dnf install -y "$REPO/desktop/target/release/bundle/rpm/Forge-$VERSION-1.$(uname -m).rpm"
fi

# --- 5. Settings, service, command and launcher --------------------------------------

CONFIG="$HOME/.config/forge"
ENV_FILE="$CONFIG/env"
if [ ! -e "$ENV_FILE" ]; then
  say "Creating $ENV_FILE for your API key and settings"
  write "$ENV_FILE" <<'EOF'
# Forge settings, read when Forge starts. Keep this file private (chmod 600).
# Get a key at https://console.anthropic.com/ and paste it after the = sign:
ANTHROPIC_API_KEY=
# FORGE_HOME=~/Forge          # where projects are stored
# FORGE_MODEL=claude-opus-5-5
# FORGE_ALLOW_COMMANDS=1      # let the agent run programs in a project folder
# FORGE_OPENEMS_PYTHON=~/opt/openEMS/venv/bin/python3   # openEMS for rf_antenna_sim
# PORT=4317
EOF
  run chmod 600 "$ENV_FILE"
fi

if [ "$LAUNCHER" = 1 ]; then
  say "Installing the forge command, a systemd user service and a desktop launcher"

  write "$HOME/.config/systemd/user/forge.service" <<EOF
[Unit]
Description=Forge AI workshop
Documentation=file://$REPO/README.md

[Service]
WorkingDirectory=$REPO
ExecStart=$NODE $REPO/server/index.ts
Restart=on-failure

[Install]
WantedBy=default.target
EOF

  write "$HOME/.local/bin/forge" <<'EOF'
#!/bin/sh
# forge          start Forge in the background and open it in the browser
# forge stop     stop it          forge logs     follow its log
# forge status   show its state   forge autostart on|off   start it at login
set -e
port=$(sed -n 's/^PORT=\([0-9]*\).*/\1/p' "$HOME/.config/forge/env" 2>/dev/null | tail -n 1)
url="http://127.0.0.1:${port:-4317}/"
case "${1:-open}" in
  open)
    systemctl --user start forge.service
    i=0
    until curl -fs --max-time 2 "${url}api/status" >/dev/null 2>&1; do
      i=$((i + 1))
      [ "$i" -gt 50 ] && { echo "Forge did not start; see: forge logs" >&2; exit 1; }
      sleep 0.2
    done
    xdg-open "$url" >/dev/null 2>&1 || echo "Forge is running at $url" ;;
  stop) systemctl --user stop forge.service ;;
  restart) systemctl --user restart forge.service ;;
  status) exec systemctl --user status forge.service ;;
  logs) exec journalctl --user -u forge.service -f ;;
  autostart)
    case "$2" in
      on) systemctl --user enable forge.service ;;
      off) systemctl --user disable forge.service ;;
      *) echo "usage: forge autostart on|off" >&2; exit 2 ;;
    esac ;;
  *) sed -n '2,4p' "$0" | sed 's/^# //'; exit 2 ;;
esac
EOF
  run chmod +x "$HOME/.local/bin/forge"

  write "$HOME/.local/share/icons/hicolor/scalable/apps/forge.svg" <<'EOF'
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" rx="14" fill="#8b7cf6"/><path d="M18 46V18h28v8H27v6h15v8H27v6z" fill="#fff"/></svg>
EOF

  # The desktop app brings its own menu entry; otherwise add one that opens the browser.
  if [ "$DESKTOP" = 1 ]; then
    if [ -e "$HOME/.local/share/applications/forge.desktop" ]; then run rm "$HOME/.local/share/applications/forge.desktop"; fi
  else
  write "$HOME/.local/share/applications/forge.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Forge
Comment=AI workshop for code, 3D, PCB and chip design
Exec=$HOME/.local/bin/forge
Icon=forge
Terminal=false
Categories=Development;Engineering;
Keywords=CAD;3D;PCB;FPGA;AI;
EOF
  fi

  run systemctl --user daemon-reload
  if command -v update-desktop-database >/dev/null; then
    run update-desktop-database "$HOME/.local/share/applications"
  fi
fi

say "Done"
cat <<EOF
1. Add your Anthropic API key to $ENV_FILE
   (without one, Forge runs an offline demo).
2. Start Forge from the app menu, or run: forge (opens it in your browser)
   (if 'forge' is not found, log out and back in so ~/.local/bin is on your PATH).
3. Optional: start Forge at login with: forge autostart on
EOF
