import assert from "node:assert/strict";
import { test } from "node:test";
import { rfTools } from "../server/tools/rf.ts";
import { coupledMicrostrip, cpwg, ellipK, fsplDb, linkBudget, lMatch, microstrip, nearestStandard, networkInput, reflection, solveFor, stripline, wavelengthMm } from "../server/tools/rfmath.ts";
import { ToolError } from "../server/tools/types.ts";

const calc = rfTools.find((t) => t.name === "rf_calc")!;
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
