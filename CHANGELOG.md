# Changelog

## 0.3.0 — 2026-10-12

- Fedora first: scripts/setup-fedora.sh installs dnf packages, FreeCAD from Flathub, builds Forge, and adds a forge command, systemd user service and desktop launcher.
- Blender and FreeCAD also run from Flatpak installs, sandboxed to the project folder.
- The capabilities panel shows the install command for this system: dnf or Flathub on Fedora, apt on Debian/Ubuntu, Homebrew on macOS.
- Settings and the API key load from ~/.config/forge/env.

## 0.2.0 — 2026-10-12

- FreeCAD tool for engineering parts: runs a script headless, exports STEP, STL and FCStd, reports validity, volume and size.
- Blender tool for visual 3D: runs a bpy script headless, exports GLB/STL/OBJ/.blend, renders a PNG with an automatic camera and lights.
- The agent is told when to use FreeCAD, Blender or the quick primitive builder.
- Viewer shows GLB models with their materials and rendered images; tools may accept several program names or a path from FORGE_BLENDER / FORGE_FREECAD.

## 0.1.0 — 2026-10-12

- Agent loop with Claude Opus 5.5: streamed replies, tool input validation, append-only history, refusal fallback.
- Offline demo model, so the full pipeline runs without an API key.
- Projects as git repositories under ~/Forge with saved conversations.
- Tools: workspace files, git, opt-in commands, 3D parts (STL/OpenSCAD), OpenSCAD render, KiCad DRC and fab export, Verilog simulation and Yosys synthesis.
- Web UI: projects, chat with tool timeline, file preview, 3D STL viewer, capabilities panel.
