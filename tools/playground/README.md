# Mighty Playground

A static-hostable browser playground for Mighty. The 30-second-to-first-run
sidekick to the full toolchain. **Live at
[hassard0.github.io/Mighty/playground/](https://hassard0.github.io/Mighty/playground/).**

```
visit page  ->  Monaco editor with a pre-loaded example
            ->  hit Run
            ->  wasm-built mty-cli parses + typechecks + runs
            ->  output panel renders stdout + structured diagnostics
            ->  hit Save & Share for a Base64 permalink
```

v0.35 T1 ships the **real WASM backend** — the playground's `Run` button
goes through the same parser, HIR-lowerer, typechecker, borrow-checker
and tree-walk interpreter as the native `mty` binary, compiled to
`wasm32-unknown-unknown` via `wasm-pack`. v0.33 T3's mock backend
sticks around as an offline fallback (used automatically if the WASM
artifact failed to load — see `src/runner.ts`).

## Quick start

```bash
cd tools/playground
npm install

# Build the wasm artifact (one-time; cached in target/).
cd ../../crates/mty-cli && wasm-pack build --target web \
  -d ../../tools/playground/public/wasm \
  -- --no-default-features --features playground-wasm

# Run the playground in dev mode.
cd ../../tools/playground && npm run dev
# -> http://localhost:5173
```

If you skip the `wasm-pack` step, the playground starts with the mock
backend and surfaces a warning in the devtools console — handy for UI
work that doesn't touch the compiler.

## Build (production)

```bash
npm run build
# -> tools/playground/build/   (static, deploy anywhere)
```

The output is a vanilla static site. `base: "./"` in `vite.config.ts`
makes it relocatable — GH Pages at `/Mighty/playground/`, a CDN at
`/play/`, or `file://` previewing the dist directory all work.

## Smoke tests

```bash
npm run test:install        # one-time; downloads chromium
npm test
```

This boots `vite preview` and drives the playground through Playwright,
asserting that the WASM backend loads, `01_hello_agent` produces
`hello, Mighty`, and the v0.33 taint example surfaces MT4099. The
GH workflow (`.github/workflows/playground.yml`) runs the same suite
on every PR that touches the playground or its Rust front-end deps.

## WASM build of `mty-cli`

The browser-side wasm-bindgen exports live in
`crates/mty-cli/src/playground.rs` and are visible on the cdylib face
of the `mty-cli` crate (see `[lib] crate-type = ["rlib", "cdylib"]`).
The exports are gated behind `#[cfg(all(target_arch = "wasm32",
feature = "playground-wasm"))]` so the native CLI build never sees
them.

The wasm exports are:

```rust
#[wasm_bindgen] pub fn init();
#[wasm_bindgen] pub fn check(src: &str) -> JsValue;
#[wasm_bindgen] pub fn run(src: &str)   -> JsValue;
```

Both `check` and `run` return JSON envelopes whose shape is documented
in `src/runner.ts` (look for `RawDiagPayload` / `RawRunPayload`).

To build:

```bash
# One-time:
cargo install wasm-pack

# Each build:
cd crates/mty-cli
wasm-pack build --target web \
  -d ../../tools/playground/public/wasm \
  -- --no-default-features --features playground-wasm
```

`--no-default-features --features playground-wasm` is the key
incantation: it switches off the default-on `host-toolchain` feature
which gates everything that can't compile to `wasm32-unknown-unknown`.
See `crates/mty-cli/Cargo.toml` + `crates/mty-driver/Cargo.toml`.

### Excluded crates (host-toolchain feature)

The WASM build excludes everything that can't compile to `wasm32-unknown-unknown`:

| Crate                       | Why excluded                                    |
| --------------------------- | ----------------------------------------------- |
| `mty-codegen-cranelift`     | Native JIT — host-only.                         |
| `mty-codegen-llvm`          | inkwell — host-only.                            |
| `mty-codegen-wasm` (driver) | Pulls `wasmtime` for the codegen tests.         |
| `mty-runtime`               | Tokio multi-thread runtime + libloading + TLS.  |
| `mty-stdlib`                | reqwest / rusqlite / hyper / tokio.             |
| `mty-lsp`, `mty-doc`, `mty-pkg` | git2 / file-watchers / native paths.       |
| `tokio`, `hyper`, `notify`  | Host-only IO surfaces.                          |

The playground only needs **parser + HIR-lower + types + borrow + IR +
tree-walk interpreter**. Same set the v0.16 `mty inspect` thin client
uses; the feature factoring is well-trodden.

## Deploy to GitHub Pages

Automatic — `.github/workflows/pages.yml` runs `wasm-pack build`, then
`npm run build`, then merges `build/` under the mkdocs site at
`site/playground/` and deploys the whole thing on every push to `main`.

For local testing of the production build:

```bash
cd tools/playground
npm run build
npm run preview   # -> http://localhost:4173
```

## Cloudflare Worker LLM proxy (optional)

The swarm / eval / computer-use examples need real LLM access to
actually run. `cf-worker/` ships a Cloudflare Worker that fronts
Anthropic / OpenAI / Gemini with per-IP rate-limiting; see its
[README](./cf-worker/README.md) for deploy + setup.

The playground points at `https://mighty-proxy.workers.dev` by default;
override in `src/config.ts` if you want to publish your own copy.

## Contribute an example to the gallery

The gallery is the source of truth for "things you can do in Mighty in
one paste". Add an entry by:

1. Drop a `main.mty` in `tools/gallery/examples/<NN>_<slug>/`.
2. Append a record to `tools/gallery/index.json`.
3. (Optional, for the in-page picker) Mirror the source into
   `tools/playground/src/examples.ts`.

## File layout

```
tools/playground/
├── README.md                         this file
├── package.json                      vite + monaco + playwright
├── playwright.config.ts              smoke harness
├── tsconfig.json
├── vite.config.ts                    base "./"; USE_WASM_BACKEND default-on
├── index.html                        topbar + editor + output panes
├── src/
│   ├── main.ts                       bootstrap
│   ├── editor.ts                     Monaco wrapper + Mighty Monarch grammar
│   ├── runner.ts                     wasm + mock runner behind one interface
│   ├── diagnostics.ts                fix-envelope renderer (T4 contract)
│   ├── share.ts                      base64url encode/decode of the source
│   ├── config.ts                     runtime config (proxy URL etc)
│   └── examples.ts                   bundled examples
├── public/
│   ├── favicon.svg
│   └── wasm/                         wasm-pack output; mty_cli.{js,d.ts}, mty_cli_bg.wasm
├── styles/
│   └── playground.css                dark, low-chroma; Mighty accent
├── tests/
│   └── wasm_smoke.spec.ts            Playwright; exercises every example
├── cf-worker/                        Cloudflare Worker LLM proxy (optional)
│   ├── wrangler.toml
│   ├── package.json
│   ├── tsconfig.json
│   ├── README.md
│   └── src/index.ts
└── build/                            generated; gitignored
```

## Roadmap

- v0.36: mty-fmt wasm export wired to the Format button; tree-sitter-mighty
  WASM driving the editor highlight instead of the Monarch fallback;
  session sharing via Cloudflare KV; embed mode for blog/docs use.
- v0.37: live LLM proxy *enabled* (currently shipped as source; deploy
  it to your own Cloudflare account); swarm/eval examples calling
  through the proxy by default.
