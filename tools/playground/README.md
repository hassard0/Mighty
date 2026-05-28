# Mighty Playground

A static-hostable browser playground for Mighty. The 30-second-to-first-run
sidekick to the full toolchain.

```
visit page  ->  Monaco editor with a pre-loaded example
            ->  hit Run
            ->  wasm-built mty-cli parses + typechecks + runs
            ->  output panel renders stdout + structured diagnostics
            ->  hit Save & Share for a Base64 permalink
```

Today (v0.33 T3) the UI is **complete** and ships with a **mock backend**
that produces plausible diagnostics + stdout for the bundled examples.
The **wasm backend** ships a Cargo target entry point in
`crates/mty-cli` (`src/playground_main.rs`); building the `.wasm`
artifact takes one `wasm-pack` invocation (see below).

## Quick start

```bash
cd tools/playground
npm install
npm run dev
# -> http://localhost:5173
```

## Build

```bash
npm run build
# -> tools/playground/build/   (static, deploy anywhere)
```

The output is a vanilla static site. `base: "./"` in `vite.config.ts`
makes it relocatable — GH Pages at `/Mighty/playground/`, a CDN at
`/play/`, or just `file://` previewing the dist directory all work.

## WASM build of `mty-cli`

The playground binary lives in `crates/mty-cli/src/playground_main.rs`
behind `#[cfg(target_arch = "wasm32")]`. It exposes:

```rust
#[wasm_bindgen] pub fn check(src: &str) -> JsValue;
#[wasm_bindgen] pub fn run(src: &str) -> JsValue;
```

Both return JSON envelopes whose shape is documented in
`src/runner.ts` (look for `RawDiagPayload` / `RawRunPayload`).

To build the artifact:

```bash
# One-time:
cargo install wasm-pack

# Each build:
wasm-pack build \
  --target web \
  --out-dir ../../tools/playground/public \
  --out-name mty_playground \
  --no-default-features \
  --features playground-wasm \
  crates/mty-cli
```

That drops `mty_playground.js` + `mty_playground_bg.wasm` into
`tools/playground/public/`. Then flip the build flag:

```ts
// vite.config.ts
"import.meta.env.USE_WASM_BACKEND": JSON.stringify(true),
```

…and rebuild the playground (`npm run build`). The mock backend stays
in the bundle for offline fallback.

### Why the artifact isn't committed

The `.wasm` blob is ~2–4 MB depending on feature set. It's a build
artifact, not source — committing it pins us to a workspace version
and bloats the repo. CI builds it from `crates/mty-cli` on every
release.

### Excluded crates

The WASM build excludes everything that can't compile to `wasm32-unknown-unknown`:

| Crate                    | Why excluded                                     |
| ------------------------ | ------------------------------------------------ |
| `mty-codegen-cranelift`  | Native JIT — host-only.                          |
| `mty-codegen-llvm`       | Inkwell can't target wasm32 from a wasm32 build. |
| `mty-runtime` (full)     | Tokio multi-thread runtime — uses host APIs.     |
| `mty-stdlib` (`runner`)  | reqwest/rusqlite/git2 — host-only.               |

The playground only needs **parser + HIR-lower + types + borrow + IR + tree-walker interpreter**.
That's the same set the v0.16 `mty inspect` thin client uses, so the
feature factoring is well-trodden.

## Deploy to GitHub Pages

```bash
# 1. Build:
cd tools/playground
npm install && npm run build

# 2. Publish the build/ dir on the gh-pages branch.
#    (CI-driven; manual recipe below for local testing.)
git worktree add /tmp/gh-pages gh-pages
rsync -av --delete build/ /tmp/gh-pages/playground/
cd /tmp/gh-pages && git add playground && git commit -m "publish playground" && git push origin gh-pages
```

v0.34 follow-up: a `.github/workflows/playground.yml` that builds on
every tagged release.

## Contribute an example to the gallery

The gallery is the source of truth for "things you can do in Mighty in
one paste". Add an entry by:

1. Drop a `main.mty` in `tools/gallery/examples/<NN>_<slug>/`.
2. Append a record to `tools/gallery/index.json`:
   ```json
   {
     "id": "08_my_example",
     "title": "08 — Your example",
     "summary": "One line.",
     "capabilities": ["tag", "tag"],
     "permalinkPayload": "<base64-url of the source>"
   }
   ```
   Generate `permalinkPayload` by saving the example in the live playground
   and copying the `#code=` portion of the URL.
3. (Optional, for in-page picker) Mirror the source into
   `tools/playground/src/examples.ts`.

## File layout

```
tools/playground/
├── README.md                         this file
├── package.json                      vite + monaco
├── tsconfig.json
├── vite.config.ts                    base "./"; USE_WASM_BACKEND flag
├── index.html                        topbar + editor + output panes
├── src/
│   ├── main.ts                       bootstrap
│   ├── editor.ts                     Monaco wrapper + Mighty Monarch grammar
│   ├── runner.ts                     mock + wasm runner behind one interface
│   ├── diagnostics.ts                fix-envelope renderer (T4 contract)
│   ├── share.ts                      base64url encode/decode of the source
│   └── examples.ts                   bundled examples
├── public/
│   ├── favicon.svg
│   └── mty_playground.{js,wasm}      generated; gitignored
├── styles/
│   └── playground.css                dark, low-chroma; Mighty accent
└── build/                            generated; gitignored
```

## Roadmap

- v0.34: real wasm backend live; mty-fmt wasm export wired to the
  Format button; gh-pages CI deploy; `tree-sitter-mighty` WASM driving
  the editor highlight instead of the Monarch fallback.
- v0.35: live LLM proxy (so the swarm/eval/computer-use examples
  actually run); session sharing via Cloudflare KV; embed mode for
  blog/docs use.
