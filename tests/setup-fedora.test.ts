import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import type { AddressInfo } from "node:net";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { createServer } from "../server/index.ts";
import { OfflineModel, SwitchableModel } from "../server/model.ts";
import { Projects } from "../server/projects.ts";

const SCRIPT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "scripts", "setup-fedora.sh");
const hasBash = spawnSync("bash", ["--version"]).status === 0;

const FEDORA = 'NAME="Fedora Linux"\nID=fedora\nVERSION_ID=43\n';
const UBUNTU = "NAME=Ubuntu\nID=ubuntu\nID_LIKE=debian\nVERSION_ID=24.04\n";

/** A fake machine: an os-release file, a home folder, and stub programs that log their arguments. */
async function sandbox(osRelease: string) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "forge-setup-"));
  const bin = path.join(root, "bin");
  const home = path.join(root, "home");
  await fs.mkdir(bin);
  await fs.mkdir(home);
  await fs.writeFile(path.join(root, "os-release"), osRelease);
  const log = path.join(root, "calls.log");
  for (const name of ["dnf", "sudo", "flatpak", "systemctl", "npm", "update-desktop-database", "xdg-open"]) {
    // sudo runs its command so `sudo dnf` is logged as dnf too.
    const body = name === "sudo" ? `echo "sudo $*" >> "${log}"; exec "$@"` : `echo "${name} $*" >> "${log}"`;
    await fs.writeFile(path.join(bin, name), `#!/bin/sh\n${body}\n`, { mode: 0o755 });
  }
  const env = {
    ...process.env,
    HOME: home,
    PATH: `${bin}:${process.env.PATH}`,
    OS_RELEASE: path.join(root, "os-release"),
    NPM: path.join(bin, "npm"),
  };
  const setup = (...args: string[]) => spawnSync("bash", [SCRIPT, ...args], { env, encoding: "utf8" });
  const calls = async () => (await fs.readFile(log, "utf8").catch(() => "")).trim().split("\n");
  return { root, home, bin, env, setup, calls };
}

test("refuses to run on a system that is not Fedora", { skip: !hasBash }, async () => {
  const { setup, calls } = await sandbox(UBUNTU);
  const r = setup();
  assert.equal(r.status, 1);
  assert.match(r.stderr, /This script is for Fedora/);
  assert.deepEqual(await calls(), [""]);
});

test("a dry run lists the dnf packages per domain and changes nothing", { skip: !hasBash }, async () => {
  const { setup, home, calls } = await sandbox(FEDORA);
  const all = setup("--dry-run");
  assert.equal(all.status, 0, all.stderr);
  const dnf = all.stdout.split("\n").find((l) => l.includes("dnf install"))!;
  for (const pkg of ["git", "nodejs", "nodejs-npm", "blender", "python3-numpy", "openscad", "librsvg2-tools", "flatpak", "kicad", "iverilog", "yosys"]) {
    assert.ok(dnf.split(" ").includes(pkg), `${pkg} missing from: ${dnf}`);
  }
  assert.match(all.stdout, /flatpak install --user -y --noninteractive flathub org\.freecad\.FreeCAD/);

  const chip = setup("--dry-run", "--only", "chip");
  const chipDnf = chip.stdout.split("\n").find((l) => l.includes("dnf install"))!;
  assert.ok(chipDnf.includes("yosys") && !chipDnf.includes("blender") && !chipDnf.includes("kicad"));
  assert.doesNotMatch(chip.stdout, /FreeCAD/);

  assert.deepEqual(await fs.readdir(home), []);
  assert.deepEqual(await calls(), [""]);
  assert.equal(setup("--only", "cnc").status, 2);
});

test("--with-rf adds openEMS's build packages and builds it into the home folder", { skip: !hasBash }, async () => {
  const { setup, home, calls } = await sandbox(FEDORA);
  const r = setup("--dry-run", "--with-rf", "--only", "pcb");
  assert.equal(r.status, 0, r.stderr);
  const dnf = r.stdout.split("\n").find((l) => l.includes("dnf install"))!;
  for (const pkg of ["kicad", "gcc-c++", "cmake", "boost-devel", "hdf5-devel", "CGAL-devel", "vtk-devel", "python3-Cython", "python3-h5py"]) assert.ok(dnf.split(" ").includes(pkg), pkg);
  assert.match(r.stdout, /git clone --recursive --shallow-submodules --depth 1 --branch v\d+\.\d+\.\d+ https:\/\/github\.com\/thliebig\/openEMS-Project\.git \S+\/\.cache\/forge\/openEMS-Project/);
  assert.match(r.stdout, new RegExp(`update_openEMS\\.sh ${home}/opt/openEMS --disable-GUI --python --python-venv-mode site`));
  assert.doesNotMatch(setup("--dry-run").stdout, /openEMS|vtk-devel/);
  assert.equal(setup("--dry-run", "--only", "rf").status, 0);
  assert.deepEqual(await calls(), [""]);
});

test("--desktop adds the Tauri build packages, builds the RPM, installs it, and skips the browser launcher", { skip: !hasBash }, async () => {
  const { setup } = await sandbox(FEDORA);
  const r = setup("--dry-run", "--desktop", "--only", "chip");
  assert.equal(r.status, 0, r.stderr);
  const dnf = r.stdout.split("\n").find((l) => l.includes("dnf install"))!.split(" ");
  for (const pkg of ["gcc", "rust", "cargo", "webkit2gtk4.1-devel", "openssl-devel", "librsvg2-devel", "libxdo-devel"]) assert.ok(dnf.includes(pkg), pkg);
  assert.match(r.stdout, /npm --prefix \S+ run desktop:build/);
  const { version } = JSON.parse(await fs.readFile(path.join(path.dirname(SCRIPT), "..", "package.json"), "utf8"));
  assert.match(r.stdout, new RegExp(`dnf install -y \\S+/desktop/target/release/bundle/rpm/Forge-${version.replaceAll(".", "\\.")}-1\\.\\w+\\.rpm`));
  assert.doesNotMatch(r.stdout, /applications\/forge\.desktop/);
  assert.doesNotMatch(setup("--dry-run").stdout, /webkit2gtk|desktop:build/);
});

test("a real run installs packages, FreeCAD, the service and a working forge command", { skip: !hasBash, timeout: 60_000 }, async () => {
  const { setup, home, env, calls, root } = await sandbox(FEDORA);
  const r = setup();
  assert.equal(r.status, 0, r.stdout + r.stderr);

  const log = await calls();
  assert.ok(log.some((l) => /^dnf install -y git nodejs nodejs-npm .*blender .*kicad .*yosys/.test(l)), log.join("\n"));
  assert.ok(log.includes("flatpak install --user -y --noninteractive flathub org.freecad.FreeCAD"));
  assert.ok(log.some((l) => l.startsWith("npm --prefix") && l.endsWith("ci --no-audit --no-fund")));
  assert.ok(log.some((l) => l.startsWith("npm --prefix") && l.endsWith("run pcb-engine")));
  assert.ok(log.some((l) => l.startsWith("npm --prefix") && l.endsWith("run build")));
  assert.ok(log.includes("systemctl --user daemon-reload"));

  const envFile = path.join(home, ".config/forge/env");
  assert.equal((await fs.stat(envFile)).mode & 0o777, 0o600);
  assert.match(await fs.readFile(envFile, "utf8"), /^ANTHROPIC_API_KEY=$/m);
  const service = await fs.readFile(path.join(home, ".config/systemd/user/forge.service"), "utf8");
  assert.match(service, /^ExecStart=\S*node \S+\/server\/index\.ts$/m);
  const desktop = await fs.readFile(path.join(home, ".local/share/applications/forge.desktop"), "utf8");
  assert.match(desktop, new RegExp(`^Exec=${home}/.local/bin/forge$`, "m"));

  // Running setup again keeps the user's settings.
  await fs.writeFile(envFile, "ANTHROPIC_API_KEY=kept\n");
  assert.equal(setup("--only", "chip").status, 0);
  assert.equal(await fs.readFile(envFile, "utf8"), "ANTHROPIC_API_KEY=kept\n");

  // The generated forge command starts the service, waits for Forge to answer, and opens the browser.
  const model = new SwitchableModel("offline", new OfflineModel());
  const server = createServer(new Projects(path.join(root, "projects"), model), model);
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const port = (server.address() as AddressInfo).port;
  await fs.writeFile(envFile, `PORT=${port}\n`);
  // Async spawn: the server runs in this process, so a blocking spawnSync would starve it.
  const forge = await new Promise<{ status: number | null }>((resolve) => {
    spawn("sh", [path.join(home, ".local/bin/forge")], { env, stdio: "ignore" }).on("exit", (status) => resolve({ status }));
  });
  server.close();
  assert.equal(forge.status, 0);
  const after = await calls();
  assert.ok(after.includes("systemctl --user start forge.service"));
  assert.ok(after.includes(`xdg-open http://127.0.0.1:${port}/`));
});
