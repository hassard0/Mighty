import { defineConfig } from "vite";

// v0.33 T3 — static-hostable Mighty playground.
//
// `base: "./"` makes the build relocatable: GH Pages can host it at
// `/Mighty/playground/` and a local `file://` open still works for
// previewing the dist artifact.
//
// The wasm artifact (when present) lives in `public/` and is copied
// verbatim by Vite. `runner.ts` reads `import.meta.env.USE_WASM_BACKEND`
// to decide whether to load it; falsey → mock backend.
export default defineConfig({
  base: "./",
  root: ".",
  build: {
    outDir: "build",
    emptyOutDir: true,
    target: "es2022",
    sourcemap: true,
  },
  server: {
    port: 5173,
    open: true,
  },
  define: {
    // Flip to `true` at build time once mty-playground.wasm ships.
    "import.meta.env.USE_WASM_BACKEND": JSON.stringify(false),
  },
  worker: {
    format: "es",
  },
});
