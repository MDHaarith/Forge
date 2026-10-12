// Forge's built-in PCB engine. Builds a tscircuit board (.tsx) into everything Forge shows and a fab needs.
//
// Forge runs the bundle of this file (npm run bundle -> dist/pcb-engine.mjs) under Node's permission model, in the project folder,
// with FORGE_ARGS (JSON):
//   source        project-relative .tsx file whose default export is the <board>
//   name          base name for the outputs
//   lookup_parts  allow network access to look up JLCPCB part numbers (default false: fully offline)
// Writes pcb/<name>.circuit.json, pcb/<name>-pcb.svg, pcb/<name>-schematic.svg, pcb/<name>.glb,
// pcb/<name>.board.json (board and component boxes for the 3D fit check, millimetres, Z up)
// and fab/<name>/ (Gerbers, drill files, pick-and-place and BOM CSVs).
// Prints one line: FORGE_RESULT <json> or FORGE_ERROR <json string>.

import fs from "node:fs";
import path from "node:path";

const args = JSON.parse(process.env.FORGE_ARGS ?? "{}");

function finish(tag, value) {
  process.stdout.write(`${tag} ${JSON.stringify(value)}\n`, () => process.exit(tag === "FORGE_RESULT" ? 0 : 1));
}

if (!args.lookup_parts) {
  // Board code is the agent's: keep it, and the part lookup, off the network unless asked.
  globalThis.fetch = async (url) => {
    throw new Error(`network access is off (${String(url).slice(0, 80)})`);
  };
}

// The texture renderer tries a font file in /tmp first, which the sandbox refuses; it then embeds the font instead.
const warn = console.warn;
console.warn = (...a) => String(a[0]).startsWith("Failed to create temporary font file") || warn(...a);

const { runTscircuitCode } = await import("tscircuit");
const { convertCircuitJsonToPcbSvg, convertCircuitJsonToSchematicSvg } = await import("circuit-to-svg");
const gerber = await import("circuit-json-to-gerber");
const { convertCircuitJsonTo3D, convertSceneToGLTF } = await import("circuit-json-to-gltf");
const { convertCircuitJsonToPickAndPlaceCsv } = await import("circuit-json-to-pnp-csv");

const r = (x, n = 3) => Math.round(x * 10 ** n) / 10 ** n;

/** Every TypeScript and JSON file under the source's folder, so a board can import its own modules. */
function sourceFiles(source) {
  const root = path.dirname(source);
  const map = {};
  const walk = (dir) => {
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      const rel = path.join(dir, e.name);
      if (e.isDirectory()) {
        if (e.name !== "node_modules" && !e.name.startsWith(".")) walk(rel);
      } else if (/\.(tsx?|jsx?)$/.test(e.name) || (e.name.endsWith(".json") && !e.name.endsWith(".circuit.json") && !e.name.endsWith(".board.json"))) {
        map[rel.split(path.sep).join("/")] = fs.readFileSync(rel, "utf8");
      }
    }
  };
  walk(root);
  return map;
}

function csv(rows) {
  return rows.map((row) => row.map((v) => (/[",\n]/.test(String(v ?? "")) ? `"${String(v).replaceAll('"', '""')}"` : String(v ?? ""))).join(",")).join("\n") + "\n";
}

/** One BOM line per distinct part, with designators grouped, in the column order JLCPCB accepts. */
function bom(json) {
  const footprint = new Map();
  for (const e of json) if (e.type === "cad_component" && e.source_component_id) footprint.set(e.source_component_id, footprintName(e.footprinter_string));
  const lines = new Map();
  for (const c of json.filter((e) => e.type === "source_component")) {
    const value = c.display_value ?? c.display_resistance ?? c.display_capacitance ?? c.display_inductance ?? c.manufacturer_part_number ?? c.color ?? "";
    const lcsc = c.supplier_part_numbers?.jlcpcb?.[0] ?? "";
    const fp = footprint.get(c.source_component_id) ?? "";
    const key = [c.ftype, value, fp, lcsc].join("|");
    const line = lines.get(key) ?? { comment: String(value || c.ftype?.replace(/^simple_/, "") || ""), designators: [], footprint: fp, lcsc };
    line.designators.push(c.name);
    lines.set(key, line);
  }
  const rows = [["Comment", "Designator", "Footprint", "LCSC Part #"]];
  for (const l of lines.values()) rows.push([l.comment, l.designators.sort().join(","), l.footprint, l.lcsc]);
  return csv(rows);
}

function readGlb(buf) {
  const jsonLength = buf.readUInt32LE(12);
  const json = JSON.parse(buf.subarray(20, 20 + jsonLength).toString("utf8"));
  const bin = buf.length > 20 + jsonLength ? buf.subarray(28 + jsonLength, 28 + jsonLength + buf.readUInt32LE(20 + jsonLength)) : null;
  return { json, bin };
}

function writeGlb({ json, bin }) {
  const pad = (b, fill) => Buffer.concat([b, Buffer.alloc((4 - (b.length % 4)) % 4, fill)]);
  const jsonChunk = pad(Buffer.from(JSON.stringify(json)), 0x20);
  const chunks = [Buffer.from(new Uint32Array([jsonChunk.length, 0x4e4f534a]).buffer), jsonChunk];
  if (bin) {
    const binChunk = pad(bin, 0);
    chunks.push(Buffer.from(new Uint32Array([binChunk.length, 0x004e4942]).buffer), binChunk);
  }
  const body = Buffer.concat(chunks);
  return Buffer.concat([Buffer.from(new Uint32Array([0x46546c67, 2, 12 + body.length]).buffer), body]);
}

/** Rotates v by the unit quaternion q = [x, y, z, w]. */
function rotate(v, [x, y, z, w]) {
  const [vx, vy, vz] = v;
  const tx = 2 * (y * vz - z * vy), ty = 2 * (z * vx - x * vz), tz = 2 * (x * vy - y * vx);
  return [vx + w * tx + (y * tz - z * ty), vy + w * ty + (z * tx - x * tz), vz + w * tz + (x * ty - y * tx)];
}

/**
 * The world-space box of each named node in a glTF, in Forge's frame. tscircuit writes glTF with
 * x = -board X, y = up, z = board Y; Forge is board X, board Y, Z up, in millimetres.
 */
function nodeBoxes(gltf) {
  const boxes = new Map();
  const visit = (index, parent) => {
    const node = gltf.nodes[index];
    const s = node.scale ?? [1, 1, 1], q = node.rotation ?? [0, 0, 0, 1], t = node.translation ?? [0, 0, 0];
    const place = (p) => parent(rotate([p[0] * s[0], p[1] * s[1], p[2] * s[2]], q).map((c, i) => c + t[i]));
    if (node.mesh !== undefined) {
      let lo = [Infinity, Infinity, Infinity], hi = [-Infinity, -Infinity, -Infinity];
      for (const prim of gltf.meshes[node.mesh].primitives) {
        const a = gltf.accessors[prim.attributes.POSITION];
        if (!a?.min || !a?.max) continue;
        for (let corner = 0; corner < 8; corner++) {
          const g = place([0, 1, 2].map((i) => ((corner >> i) & 1 ? a.max[i] : a.min[i])));
          const f = [-g[0], g[2], g[1]];
          lo = lo.map((v, i) => Math.min(v, f[i]));
          hi = hi.map((v, i) => Math.max(v, f[i]));
        }
      }
      if (node.name && lo[0] <= hi[0]) {
        const old = boxes.get(node.name);
        boxes.set(node.name, old ? { lo: lo.map((v, i) => Math.min(v, old.lo[i])), hi: hi.map((v, i) => Math.max(v, old.hi[i])) } : { lo, hi });
      }
    }
    for (const child of node.children ?? []) visit(child, place);
  };
  for (const root of gltf.scenes[gltf.scene ?? 0].nodes) visit(root, (p) => p);
  return boxes;
}

/** Wraps the scene in a node turned half a turn about Y, so the model reads like any glTF: X right, -Z away, Y up. */
function standardOrientation(gltf) {
  const scene = gltf.scenes[gltf.scene ?? 0];
  gltf.nodes.push({ name: "board", rotation: [0, 1, 0, 0], children: scene.nodes });
  scene.nodes = [gltf.nodes.length - 1];
}

/** "res0805" -> "0805", "0805_color(red)" -> "0805", "pinrow2_p2.54_nopinlabels" -> "pinrow2_p2.54". */
function footprintName(fp) {
  if (!fp) return "";
  const size = fp.match(/^(?:res|cap|led|diode)?(\d{4})(?:_|$)/);
  return size ? size[1] : fp.replace(/_nopinlabels$/, "");
}

async function main() {
  const source = args.source;
  const name = args.name;
  const fsMap = sourceFiles(source);
  const json = await runTscircuitCode(fsMap, { mainComponentPath: source.split(path.sep).join("/") });

  const errors = json.filter((e) => /_error$/.test(e.type)).map((e) => `${e.type}: ${e.message ?? e.error_type ?? ""}`.trim());
  const warnings = json.filter((e) => /_warning$/.test(e.type)).map((e) => `${e.type}: ${e.message ?? ""}`.trim());
  const board = json.find((e) => e.type === "pcb_board");
  if (!board) {
    finish("FORGE_ERROR", errors.length ? `the circuit did not build:\n${errors.slice(0, 20).join("\n")}` : "no <board> in the circuit: the default export must render a <board>");
    return;
  }

  fs.mkdirSync("pcb", { recursive: true });
  const fab = path.join("fab", name);
  fs.rmSync(fab, { recursive: true, force: true });
  fs.mkdirSync(fab, { recursive: true });
  const files = [];
  const write = (rel, data) => {
    fs.writeFileSync(rel, data);
    files.push(rel.split(path.sep).join("/"));
  };

  write(`pcb/${name}.circuit.json`, JSON.stringify(json, null, 1));
  write(`pcb/${name}-pcb.svg`, convertCircuitJsonToPcbSvg(json, { width: 1200, height: 900 }));
  let schematicNote;
  try {
    write(`pcb/${name}-schematic.svg`, convertCircuitJsonToSchematicSvg(json, { width: 1200, height: 900 }));
  } catch (e) {
    schematicNote = `no schematic: ${e.message}`;
  }

  const scene = await convertCircuitJsonTo3D(json, {});
  const glb = readGlb(Buffer.from(await convertSceneToGLTF(scene, { binary: true })));
  const boxes = nodeBoxes(glb.json);
  standardOrientation(glb.json);
  write(`pcb/${name}.glb`, writeGlb(glb));

  const thickness = board.thickness ?? 1.6;
  const sourceById = new Map(json.filter((e) => e.type === "source_component").map((c) => [c.source_component_id, c]));
  const components = [];
  for (const pc of json.filter((e) => e.type === "pcb_component")) {
    const label = sourceById.get(pc.source_component_id)?.name ?? pc.pcb_component_id;
    const box = boxes.get(label);
    // A part with no 3D model is still a box: its footprint area, 1 mm tall, on its side of the board.
    const top = pc.layer !== "bottom";
    const lo = box?.lo ?? [pc.center.x - pc.width / 2, pc.center.y - pc.height / 2, top ? thickness / 2 : -thickness / 2 - 1];
    const hi = box?.hi ?? [pc.center.x + pc.width / 2, pc.center.y + pc.height / 2, top ? thickness / 2 + 1 : -thickness / 2];
    components.push({ name: label, layer: top ? "top" : "bottom", min: lo.map((v) => r(v)), max: hi.map((v) => r(v)) });
  }
  const outline = board.outline?.map((p) => [r(p.x), r(p.y)]);
  write(
    `pcb/${name}.board.json`,
    JSON.stringify({ size: [r(board.width), r(board.height), r(thickness)], center: [r(board.center.x), r(board.center.y)], outline, components }),
  );

  const layers = gerber.stringifyGerberCommandLayers(gerber.convertCircuitJsonToGerberCommands(json));
  for (const [layer, text] of Object.entries(layers)) write(path.join(fab, `${name}-${layer}.gbr`), text);
  for (const plated of [true, false]) {
    const drill = gerber.stringifyExcellonDrill(gerber.convertCircuitJsonToExcellonDrillCommands({ circuitJson: json, is_plated: plated }));
    write(path.join(fab, `${name}-${plated ? "PTH" : "NPTH"}.drl`), drill);
  }
  write(path.join(fab, `${name}-pnp.csv`), await convertCircuitJsonToPickAndPlaceCsv(json));
  write(path.join(fab, `${name}-bom.csv`), bom(json));

  const count = (type) => json.filter((e) => e.type === type).length;
  const copper = new Set(json.filter((e) => e.type === "pcb_trace").flatMap((t) => t.route.map((p) => p.layer).filter(Boolean)));
  finish("FORGE_RESULT", {
    files,
    board: { width: r(board.width), height: r(board.height), thickness, layers: board.num_layers ?? Math.max(2, copper.size) },
    components: components.map((c) => c.name),
    nets: count("source_net"),
    connections: count("source_trace"),
    traces: count("pcb_trace"),
    vias: count("pcb_via"),
    holes: count("pcb_plated_hole") + count("pcb_hole"),
    errors,
    warnings,
    schematicNote,
  });
}

try {
  await main();
} catch (e) {
  // The bundle is minified, so a stack trace says nothing; the message names the board's problem.
  finish("FORGE_ERROR", `the board failed to build: ${e?.message ?? e}`.slice(0, 4000));
}
