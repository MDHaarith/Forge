import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { detectDistro, installText } from "../server/platform.ts";
import { findProgram } from "../server/tools/exec.ts";
import { launchArgs } from "../server/tools/launch.ts";
import { FREECAD } from "../server/tools/script3d.ts";

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");

test("detects Fedora and its relatives, Debian family and macOS", () => {
  assert.equal(detectDistro('NAME="Fedora Linux"\nID=fedora\nVERSION_ID=43\n', "linux"), "fedora");
  assert.equal(detectDistro('ID=nobara\nID_LIKE="rhel centos fedora"\n', "linux"), "fedora");
  assert.equal(detectDistro("ID=ubuntu\nID_LIKE=debian\n", "linux"), "debian");
  assert.equal(detectDistro("ID=arch\n", "linux"), "other");
  assert.equal(detectDistro("", "darwin"), "macos");
});

test("install hints prefer dnf on Fedora, then Flathub", () => {
  const blender = { dnf: "blender python3-numpy", apt: "blender", flatpak: "org.blender.Blender", url: "https://blender.org" };
  const freecad = { flatpak: "org.freecad.FreeCAD", apt: "freecad", url: "https://freecad.org" };
  assert.equal(installText(blender, "fedora"), "sudo dnf install blender python3-numpy");
  assert.equal(installText(freecad, "fedora"), "flatpak install flathub org.freecad.FreeCAD");
  assert.equal(installText(freecad, "debian"), "sudo apt install freecad");
  assert.equal(installText({ url: "https://x" }, "macos"), "https://x");
  assert.equal(installText(undefined, "fedora"), undefined);
});

test("Flatpak launches get only the folders they need and their environment via --env", () => {
  const launch = launchArgs({ kind: "flatpak", id: "org.freecad.FreeCAD", command: "freecadcmd" }, ["runner.py"], {
    rw: ["/home/me/Forge/rover"],
    ro: ["/opt/forge/runners"],
    env: { FORGE_NAME: "plate" },
  });
  assert.equal(launch.cmd, "flatpak");
  assert.deepEqual(launch.args, [
    "run",
    "--command=freecadcmd",
    "--filesystem=/home/me/Forge/rover",
    "--filesystem=/opt/forge/runners:ro",
    "--env=FORGE_NAME=plate",
    "org.freecad.FreeCAD",
    "runner.py",
  ]);
  const native = launchArgs({ kind: "native", program: "/usr/bin/blender" }, ["-b"], { rw: [], ro: [], env: {} });
  assert.deepEqual([native.cmd, native.args], ["/usr/bin/blender", ["-b"]]);
});

const realFreecad = findProgram(FREECAD.native);

test("FreeCAD installed only as a Flatpak is found and runs", { skip: !realFreecad && "freecadcmd not installed" }, async () => {
  // A stand-in `flatpak` that behaves like the real one for an installed org.freecad.FreeCAD,
  // running the command on this machine instead of in a sandbox.
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "forge-flatpak-"));
  const bin = path.join(dir, "bin");
  await fs.mkdir(bin);
  await fs.writeFile(
    path.join(bin, "flatpak"),
    `#!/bin/bash
[ "$1" = info ] && { [ "$2" = org.freecad.FreeCAD ]; exit; }
[ "$1" = run ] || exit 1
shift
cmd=
while [ $# -gt 0 ]; do
  case "$1" in
    --command=*) cmd="\${1#--command=}" ;;
    --filesystem=*) echo "$1" >> "${dir}/access.log" ;;
    --env=*) export "\${1#--env=}" ;;
    org.freecad.FreeCAD) shift; break ;;
  esac
  shift
done
[ "$cmd" = freecadcmd ] && cmd="${realFreecad}"
PATH="$(dirname "${realFreecad}"):$PATH" exec "$cmd" "$@"
`,
    { mode: 0o755 },
  );
  const project = path.join(dir, "project");
  await fs.mkdir(path.join(project, "cad"), { recursive: true });
  await fs.writeFile(path.join(project, "cad", "cube.py"), "result = Part.makeBox(10, 20, 30)\n");

  // A child process, so no freecadcmd is on PATH and the Flatpak is the only way to reach FreeCAD.
  const child = spawnSync(
    process.execPath,
    [
      "--input-type=module",
      "-e",
      `const { script3dTools } = await import(${JSON.stringify(path.join(ROOT, "server/tools/script3d.ts"))});
       const tool = script3dTools.find((t) => t.name === "freecad_build");
       const out = await tool.run({ script: "cad/cube.py", name: "cube", formats: ["step"] }, { projectDir: ${JSON.stringify(project)} });
       console.log(out.text);`,
    ],
    { encoding: "utf8", env: { ...process.env, PATH: `${bin}:/usr/bin:/bin`, FORGE_FREECAD: "" } },
  );
  assert.equal(child.status, 0, child.stderr);
  assert.match(child.stdout, /10 × 20 × 30 mm, volume 6000 mm³, valid: true/);
  const access = (await fs.readFile(path.join(dir, "access.log"), "utf8")).trim().split("\n");
  assert.deepEqual(access, [`--filesystem=${project}`, `--filesystem=${path.join(ROOT, "server/tools/runners")}:ro`]);
});
