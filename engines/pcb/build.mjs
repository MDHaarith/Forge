// Bundles the engine into one file, dist/pcb-engine.mjs, that runs without node_modules.
import { build } from "esbuild";

await build({
  entryPoints: ["runner.mjs"],
  outfile: "dist/pcb-engine.mjs",
  bundle: true,
  platform: "node",
  format: "esm",
  target: "node22",
  minify: true,
  legalComments: "none",
  logLevel: "warning",
  loader: { ".wasm": "binary" },
  alias: { "@resvg/resvg-js": "./resvg-wasm-shim.mjs" },
  // Optional image and STEP helpers the board pipeline never calls.
  external: ["sharp", "occt-import-js", "occt-import-js/*"],
  banner: { js: "import{createRequire as __cr}from'module';const require=__cr(import.meta.url);" },
});
