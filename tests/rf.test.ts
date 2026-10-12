import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { findProgram } from "../server/tools/exec.ts";
import { describeSim, OPENEMS_PYTHON, rfTools } from "../server/tools/rf.ts";
import { coupledMicrostrip, cpwg, ellipK, fsplDb, linkBudget, lMatch, microstrip, nearestStandard, networkInput, reflection, solveFor, stripline, wavelengthMm } from "../server/tools/rfmath.ts";
import { ToolError } from "../server/tools/types.ts";

const calc = rfTools.find((t) => t.name === "rf_calc")!;
const sim = rfTools.find((t) => t.name === "rf_antenna_sim")!;
const hasOpenEMS = findProgram(OPENEMS_PYTHON) !== undefined;
const run = async (input: Record<string, unknown>) => (await calc.run(input, { projectDir: "/nonexistent" })).text;
const near = (actual: number, expected: number, tol: number, what = "") => assert.ok(Math.abs(actual - expected) <= tol, `${what} ${actual} not within ${tol} of ${expected}`);

test("elliptic integral matches known values", () => {
  near(ellipK(0), Math.PI / 2, 1e-12);
  near(ellipK(Math.SQRT1_2), 1.8540746773, 1e-9);
});

test("microstrip matches published references", () => {
  // 3 mm on 1.6 mm FR-4 is the textbook 50 ohm line.
  const ms = microstrip(3.0, 1.6, 4.5, 0.035);
  near(ms.z0, 49.66, 0.05, "Z0");
  near(ms.eeff, 3.37, 0.01, "eeff");
  // Wide and narrow lines bracket 50 ohm; impedance falls as the line widens.
  assert.ok(microstrip(0.2, 1.6, 4.5).z0 > microstrip(3, 1.6, 4.5).z0);
  // Dispersion raises the effective permittivity with frequency.
  assert.ok(microstrip(3, 1.6, 4.5, 0, 6e9).eeff > microstrip(3, 1.6, 4.5, 0, 0).eeff);
  // JLCPCB 4-layer 7628: their calculator gives about 0.35 mm for 50 ohm.
  near(microstrip(0.36, 0.2104, 4.4, 0.035, 2.4e9).z0, 50.9, 0.3, "JLC 7628");
});

test("coupled, coplanar and stripline models agree with their limits", () => {
  // Far apart, each line of a pair is a plain microstrip.
  const pair = coupledMicrostrip(0.5, 20, 0.5, 4.4);
  const single = microstrip(0.5, 0.5, 4.4);
  near(pair.zOdd!, single.z0, 0.1);
  near(pair.zEven!, single.z0, 0.1);
  // Close together, coupling splits the modes: odd below, even above.
  const close = coupledMicrostrip(0.2, 0.15, 0.2104, 4.4);
  assert.ok(close.zOdd! < single.z0 && close.zEven! > close.zOdd!);
  near(close.z0, 2 * close.zOdd!, 1e-9);
  near(close.z0, 116.5, 0.5, "Zdiff");

  near(cpwg(1.0, 0.2, 1.6, 4.5).z0, 53.87, 0.05, "CPWG");
  // A wider gap moves the side grounds away, so impedance rises.
  assert.ok(cpwg(1.0, 0.5, 1.6, 4.5).z0 > cpwg(1.0, 0.2, 1.6, 4.5).z0);

  // Cohn's exact zero-thickness stripline and Wheeler's formula agree.
  near(stripline(0.5, 1.0, 4.4).z0, 47.91, 0.05, "Cohn");
  near(stripline(0.5, 1.0, 4.4, 1e-4).z0, 47.91, 0.15, "Wheeler");
  assert.equal(stripline(0.5, 1.0, 4.4).eeff, 4.4);
});

test("solveFor finds the width and refuses targets it cannot bracket", () => {
  const w = solveFor((x) => microstrip(x, 1.6, 4.5, 0.035).z0, 50, 0.01, 50)!;
  near(microstrip(w, 1.6, 4.5, 0.035).z0, 50, 0.01);
  assert.equal(solveFor((x) => microstrip(x, 1.6, 4.5).z0, 1, 0.01, 50), undefined);
});

test("reflection, wavelength and free-space loss", () => {
  const matched = reflection({ re: 50, im: 0 });
  assert.equal(matched.vswr, 1);
  assert.equal(matched.returnLossDb, Infinity);
  const twice = reflection({ re: 100, im: 0 });
  near(twice.vswr, 2, 1e-9);
  near(twice.returnLossDb, 9.54, 0.01);
  near(wavelengthMm(2.45e9), 122.36, 0.01);
  near(fsplDb(1, 2.4e9), 40.05, 0.01);
});

test("L-match networks bring the load to 50 ohm", () => {
  const loads = [{ re: 20, im: -15 }, { re: 120, im: 40 }, { re: 10, im: 30 }];
  for (const zl of loads) {
    const nets = lMatch(zl, 50, 2.45e9);
    assert.ok(nets.length >= 1, `no network for ${zl.re}+j${zl.im}`);
    for (const parts of nets) {
      const zin = networkInput(parts, zl, 2.45e9);
      near(zin.re, 50, 1e-6);
      near(zin.im, 0, 1e-6);
      assert.ok(parts.every((p) => p.value > 0));
    }
  }
  assert.deepEqual(lMatch({ re: 50, im: 0 }, 50, 2.45e9), []);
  assert.equal(nearestStandard(4.6e-9), 4.7e-9);
  assert.equal(nearestStandard(9.6e-12), 1e-11);
});

test("link budget range and received power are consistent", () => {
  const b = linkBudget({ txDbm: 20, txGainDbi: 2, rxGainDbi: 2, sensitivityDbm: -90, f: 2.4e9, distanceM: 100, marginDb: 10 });
  near(b.budgetDb, 114, 1e-9);
  // At the computed range the margin left is exactly the fade margin.
  near(fsplDb(b.rangeM, 2.4e9), 104, 1e-6);
  near(b.rxDbm!, 24 - fsplDb(100, 2.4e9), 1e-9);
  near(b.marginAtDistanceDb!, b.rxDbm! + 90, 1e-9);
  const indoor = linkBudget({ txDbm: 20, txGainDbi: 2, rxGainDbi: 2, sensitivityDbm: -90, f: 2.4e9, exponent: 3.3 });
  assert.ok(indoor.rangeM < b.rangeM);
});

test("rf_calc reports widths, matches and links", async () => {
  const width = await run({ calc: "line", stackup: "jlc-4layer-1.6-7628", target_ohms: 50 });
  assert.match(width, /Width for 50 ohm: 0\.3[5-9]\d? mm/);
  assert.match(width, /quarter wave/);

  const diff = await run({ calc: "line", line: "diff_microstrip", stackup: "jlc-4layer-1.6-7628", target_ohms: 90, gap_mm: 0.15 });
  assert.match(diff, /Width for 90 ohm differential/);
  assert.match(diff, /odd mode/);

  const thin = await run({ calc: "line", height_mm: 0.05, er: 4.4, target_ohms: 50 });
  assert.match(thin, /below the 0\.1 mm/);

  const match = await run({ calc: "match", load_re: 20, load_im: -15, frequency_mhz: 2450 });
  assert.match(match, /VSWR 2\.\d+ before matching/);
  assert.match(match, /with E24 parts/);
  assert.match(match, /pi footprint/);
  assert.match(await run({ calc: "match", load_re: 50, frequency_mhz: 2450 }), /already matched/);

  const link = await run({ calc: "link", tx_dbm: 20, sensitivity_dbm: -97, frequency_mhz: 2450, distance_m: 30, environment: "indoor" });
  assert.match(link, /exponent 3\.3/);
  assert.match(link, /At 30 m: received/);
});

test("rf_calc rejects incomplete or impossible requests", async () => {
  const bad: Record<string, unknown>[] = [
    { calc: "nope" },
    { calc: "line", width_mm: 1 },
    { calc: "line", stackup: "mystery", width_mm: 1 },
    { calc: "line", stackup: "jlc-2layer-1.6" },
    { calc: "line", line: "cpwg", stackup: "jlc-2layer-1.6", width_mm: 1 },
    { calc: "line", stackup: "jlc-2layer-1.6", target_ohms: 250 },
    { calc: "match", load_re: -5, frequency_mhz: 2450 },
    { calc: "link", tx_dbm: 20, sensitivity_dbm: -90, frequency_mhz: 2450, environment: "space" },
  ];
  for (const input of bad) await assert.rejects(run(input), ToolError, JSON.stringify(input));
});

/** A board holding a strip dipole in air: two 1 mm pads 0.5 mm apart, arms to ±29 mm. */
function dipoleBoard(arm = 29) {
  const wire = (x: number) => ({ route_type: "wire", x, y: 0, width: 1, layer: "top" });
  return [
    { type: "pcb_board", pcb_board_id: "b", center: { x: 0, y: 0 }, width: 70, height: 10, thickness: 0.5, num_layers: 2 },
    { type: "source_component", source_component_id: "s1", name: "ANT1" },
    { type: "pcb_component", pcb_component_id: "c1", source_component_id: "s1", center: { x: 0, y: 0 }, width: 2 * arm + 1, height: 1 },
    { type: "pcb_smtpad", pcb_component_id: "c1", shape: "rect", x: -0.75, y: 0, width: 1, height: 1, layer: "top", port_hints: ["pin1"] },
    { type: "pcb_smtpad", pcb_component_id: "c1", shape: "rect", x: 0.75, y: 0, width: 1, height: 1, layer: "top", port_hints: ["pin2"] },
    { type: "pcb_trace", pcb_component_id: "c1", route: [wire(-0.75), wire(-arm)] },
    { type: "pcb_trace", pcb_component_id: "c1", route: [wire(0.75), wire(arm)] },
  ];
}

async function boardProject(circuit: unknown[]) {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "forge-rf-"));
  await fs.mkdir(path.join(dir, "pcb"));
  await fs.writeFile(path.join(dir, "pcb", "ant.circuit.json"), JSON.stringify(circuit));
  return dir;
}

test("rf_antenna_sim checks its inputs before simulating", async () => {
  const dir = await boardProject(dipoleBoard());
  const go = (input: Record<string, unknown>) => sim.run(input, { projectDir: dir });
  await assert.rejects(go({ board: "../ant" }), /letters, digits/);
  await assert.rejects(go({ board: "missing" }), /build the board with pcb_build first/);
  await assert.rejects(go({ board: "ant", antenna: "ANT 1" }), /component name/);
  await assert.rejects(go({ board: "ant", frequency_mhz: 2450, span_mhz: 5000 }), /less than twice/);
  await assert.rejects(go({ board: "ant", quality: "best" }), /fast/);
});

test("describeSim points to the matching network and the length fix", () => {
  const base = {
    antenna: "ANT1", port: "from the ground plane up to the pin1 pad (1.6 mm probe)", cells: 200_000, seconds: 61, f0: 2.45e9, span: [1.47e9, 3.43e9] as [number, number],
    zin_res: [48, 2] as [number, number], directivity_dbi: 2.5, peak_gain_dbi: 1.8, peak_dir: [90, 0] as [number, number], radiation_efficiency: 0.81, total_efficiency: 0.5, ground_under: 0, files: [],
  };
  const low = describeSim("rf", { ...base, resonance: 2.2e9, s11_min_db: -18, band: [2.15e9, 2.26e9, false], s11_f0_db: -3, zin_f0: [20, 35] }, "fast");
  assert.match(low, /resonates below the target: shorten its radiating arm by about 10 %/);
  assert.match(low, /load_re 20, load_im 35, frequency_mhz 2450/);
  assert.match(low, /from 2150 MHz to 2260 MHz \(110 MHz\)/);
  const good = describeSim("rf", { ...base, resonance: 2.45e9, s11_min_db: -22, band: [2.38e9, 2.52e9, false], s11_f0_db: -22, zin_f0: [52, -3] }, "accurate");
  assert.doesNotMatch(good, /rf_calc|shorten|lengthen/);
  assert.match(good, /Radiation efficiency 81 %/);
  assert.doesNotMatch(good, /Warning/);
  const crowded = describeSim("rf", { ...base, resonance: 1.43e9, s11_min_db: -1.6, band: null, s11_f0_db: -0.6, zin_f0: [169, 477], ground_under: 1 }, "fast");
  assert.match(crowded, /Warning: 100 % of the antenna's trace has ground copper under it or within 2 mm beside it/);
  assert.match(crowded, /never reaches -10 dB/);
  assert.doesNotMatch(crowded, /shorten|lengthen/);
});

test("rf_antenna_sim reproduces a half-wave dipole", { skip: !hasOpenEMS && "openEMS is not installed", timeout: 600_000 }, async () => {
  const dir = await boardProject(dipoleBoard());
  const out = await sim.run({ board: "ant", frequency_mhz: 2450, span_mhz: 1600, er: 1, loss_tangent: 0 }, { projectDir: dir });
  assert.deepEqual(out.files, ["rf/ant/ANT1-s11.svg", "rf/ant/ANT1-pattern.svg", "rf/ant/ANT1.s1p", "rf/ant/ANT1.json"]);
  const res = JSON.parse(await fs.readFile(path.join(dir, "rf/ant/ANT1.json"), "utf8"));
  // A 59 mm strip dipole resonates near 0.47 wavelengths with about 70 ohm and 2.15 dBi.
  assert.ok(res.resonance > 2.2e9 && res.resonance < 2.5e9, `resonance ${res.resonance}`);
  assert.ok(res.zin_res[0] > 55 && res.zin_res[0] < 85, `R ${res.zin_res[0]}`);
  assert.ok(res.directivity_dbi > 1.6 && res.directivity_dbi < 2.6, `D ${res.directivity_dbi}`);
  assert.ok(res.radiation_efficiency > 0.9 && res.radiation_efficiency < 1.1);
  assert.match(out.text, /across the gap between pin1 and pin2/);
  assert.match(await fs.readFile(path.join(dir, "rf/ant/ANT1-s11.svg"), "utf8"), /^<svg[^]*polyline[^]*<\/svg>$/);
  assert.match(await fs.readFile(path.join(dir, "rf/ant/ANT1.s1p"), "utf8"), /^! Forge[^\n]*\n# Hz S RI R 50\n\d+ -?\d/);
  assert.equal((await fs.readdir(dir)).sort().join(","), "pcb,rf");
});

test("rf_antenna_sim needs ground under a single-ended feed", { skip: !hasOpenEMS && "openEMS is not installed" }, async () => {
  const board = dipoleBoard().filter((e) => !(e.type === "pcb_smtpad" && (e as { port_hints?: string[] }).port_hints?.[0] === "pin2"));
  const dir = await boardProject(board);
  await assert.rejects(sim.run({ board: "ant" }, { projectDir: dir }), /no ground copper under the antenna feed/);
});
