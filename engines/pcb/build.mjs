// Bundles the engine into one file, dist/pcb-engine.mjs, that runs without node_modules.
import { readFile } from "node:fs/promises";
import { build } from "esbuild";

// In Node, manifold-2d (copper pours) looks for manifold.wasm beside the bundle, which is not there.
// Make it use the copy embedded in the package, as it does in a browser.
const embeddedManifold = {
  name: "embedded-manifold",
  setup(b) {
    b.onLoad({ filter: /@tscircuit[\\/]manifold-2d[\\/]index\.js$/ }, async (args) => {
      const source = await readFile(args.path, "utf8");
      const patched = source.replace(/: isNode\s*\?\s*undefined\s*:/, ": false ? undefined :");
      if (patched === source) throw new Error("manifold-2d loader changed; update build.mjs");
      return { contents: patched, loader: "js" };
    });
  },
};

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
  plugins: [embeddedManifold],
  banner: { js: "import{createRequire as __cr}from'module';const require=__cr(import.meta.url);" },
});
