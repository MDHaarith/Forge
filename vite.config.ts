import { defineConfig } from "vite";

export default defineConfig({
  root: "web",
  build: { outDir: "../dist", emptyOutDir: true, target: "es2022", chunkSizeWarningLimit: 700 },
  server: { host: "127.0.0.1", proxy: { "/api": "http://127.0.0.1:4317" } },
});
