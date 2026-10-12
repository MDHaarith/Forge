// Stands in for @resvg/resvg-js (a native addon) with its WebAssembly build, so the engine runs
// under Node's permission model without --allow-addons and without writing font files to /tmp.
import { initWasm, Resvg as WasmResvg } from "@resvg/resvg-wasm";
import wasm from "@resvg/resvg-wasm/index_bg.wasm";
import font from "@tscircuit/alphabet/base64font";

await initWasm(wasm);
const fontBuffers = [Buffer.from(font, "base64")];

export class Resvg extends WasmResvg {
  constructor(svg, options = {}) {
    const { fontFiles, ...rest } = options.font ?? {};
    super(svg, { ...options, font: { ...rest, fontBuffers, loadSystemFonts: false } });
  }
}
