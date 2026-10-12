import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import fs from "node:fs/promises";
import http from "node:http";
import type { AddressInfo } from "node:net";
import os from "node:os";
import path from "node:path";
import { after, before, test } from "node:test";
import { fileURLToPath } from "node:url";
import { createServer } from "../server/index.ts";
import { OfflineModel } from "../server/model.ts";
import { Projects } from "../server/projects.ts";

let base = "";
let home = "";
let close = () => {};

before(async () => {
  home = await fs.mkdtemp(path.join(os.tmpdir(), "forge-home-"));
  const model = new OfflineModel();
  const server = createServer(new Projects(home, model), model);
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  base = `http://127.0.0.1:${(server.address() as AddressInfo).port}/api`;
  close = () => server.close();
});
after(() => close());

const post = (url: string, body: unknown) =>
  fetch(base + url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });

test("status lists every capability with availability", async () => {
  const status = await (await fetch(`${base}/status`)).json();
  assert.equal(status.offline, true);
  const names = status.tools.map((t: { name: string }) => t.name);
  for (const n of ["write_file", "git_commit", "cad_model", "pcb_export_fab", "chip_synthesize"]) assert.ok(names.includes(n), n);
  assert.equal(status.tools.find((t: { name: string }) => t.name === "cad_model").available, true);
});

test("offline demo builds and commits an enclosure end to end", async () => {
  assert.equal((await post("/projects", { name: "rover" })).status, 201);
  assert.equal((await post("/projects", { name: "rover" })).status, 400);
  assert.equal((await post("/projects", { name: "../evil" })).status, 400);

  const res = await post("/projects/rover/chat", { message: "Design an enclosure" });
  const events = (await res.text()).trim().split("\n").map((l) => JSON.parse(l));
  const calls = events.filter((e) => e.type === "tool_call").map((e) => e.name);
  assert.deepEqual(calls, ["cad_model", "git_commit"]);
  assert.ok(events.filter((e) => e.type === "tool_result").every((e) => e.ok));
  assert.equal(events.at(-1).type, "done");

  const files: string[] = await (await fetch(`${base}/projects/rover/files`)).json();
  assert.ok(files.includes("models/demo-enclosure.stl"));
  const stl = await (await fetch(`${base}/projects/rover/file?path=models/demo-enclosure.stl`)).text();
  assert.match(stl, /^solid demo-enclosure/);

  const history = await (await fetch(`${base}/projects/rover/history`)).json();
  assert.equal(history.events.length, events.length);
  const saved = JSON.parse(await fs.readFile(path.join(home, ".state", "rover.json"), "utf8"));
  assert.deepEqual(saved.messages.map((m: { role: string }) => m.role), ["user", "assistant", "user", "assistant", "user", "assistant"]);
});

test("rejects foreign hosts, non-JSON posts and path escapes", async () => {
  const foreign = await new Promise<number>((resolve, reject) =>
    http.get(`${base}/status`, { headers: { host: "evil.example" } }, (r) => resolve(r.statusCode!)).on("error", reject),
  );
  assert.equal(foreign, 403);
  const form = await fetch(`${base}/projects`, { method: "POST", body: "name=x" });
  assert.equal(form.status, 415);
  const escape = await fetch(`${base}/projects/rover/file?path=../../etc/passwd`);
  assert.equal(escape.status, 400);
  assert.equal((await fetch(`${base}/projects/nope/files`)).status, 404);
});

test("under the desktop app, the server stops when its stdin closes", { timeout: 20_000 }, async () => {
  const entry = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "server", "index.ts");
  const env = { ...process.env, PORT: "0", FORGE_HOME: home, FORGE_OFFLINE: "1", FORGE_ENV_FILE: path.join(home, "no-env"), FORGE_EXIT_WITH_STDIN: "1" };
  const child = spawn(process.execPath, [entry], { env, stdio: ["pipe", "pipe", "inherit"] });
  const exited = new Promise<number | null>((resolve) => child.on("exit", resolve));
  let out = "";
  await new Promise<void>((resolve) =>
    child.stdout.on("data", (d) => {
      out += d;
      if (out.includes("Forge is running")) resolve();
    }),
  );
  child.stdin.end();
  assert.equal(await exited, 0);
});
