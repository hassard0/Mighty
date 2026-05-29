import { defineConfig } from "vite";

// v0.33 T3 / v0.35 T1 — static-hostable Mighty playground.
//
// `base: "./"` makes the build relocatable: GH Pages can host it at
// `/Mighty/playground/` and a local `file://` open still works for
// previewing the dist artifact.
//
// The wasm artifact lives in `public/wasm/` (emitted by
// `wasm-pack build --target web --no-default-features --features
// playground-wasm --out-dir ../../tools/playground/public/wasm
// crates/mty-cli`) and is copied verbatim by Vite. `runner.ts` reads
// `import.meta.env.USE_WASM_BACKEND` to decide whether to load it;
// when truthy the wasm path is tried with an automatic fallback to
// the mock runner if it fails to initialise.
//
// v0.35 T1 flips the default to `true` — the GH-Pages workflow runs
// `wasm-pack build` before `npm run build`, so the artifact ships
// alongside the rest of `dist/`. Override with `MTY_PLAYGROUND_MOCK=1`
// at build time to force the offline / no-wasm path.
const useWasmBackend =
  process.env.MTY_PLAYGROUND_MOCK === "1" ? false : true;

export default defineConfig({
  base: "./",
  root: ".",
  build: {
    outDir: "build",
    emptyOutDir: true,
    target: "es2022",
    sourcemap: true,
    // The wasm artifact is large enough (~1.1 MB) that Vite's default
    // 500 kB chunk warning would spam every build. Bump it so the log
    // stays useful.
    chunkSizeWarningLimit: 2048,
    rollupOptions: {
      output: {
        // wasm-pack ships `mty_cli_bg.wasm` next to `mty_cli.js`;
        // ensure Vite emits them un-hashed so the runtime fetch in
        // `runner.ts` finds them at the URL it expects.
        assetFileNames: (assetInfo) => {
          if (assetInfo.name?.endsWith(".wasm")) {
            return "wasm/[name][extname]";
          }
          return "assets/[name]-[hash][extname]";
        },
      },
    },
  },
  server: {
    port: 5173,
    open: true,
  },
  define: {
    "import.meta.env.USE_WASM_BACKEND": JSON.stringify(useWasmBackend),
  },
  worker: {
    format: "es",
  },
});
