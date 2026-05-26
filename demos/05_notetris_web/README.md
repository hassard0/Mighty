# 05 — Notetris (web)

Notetris as a localhost web app. The Mighty agent in `src/main.mty`
owns the game state through the `NotetrisInput` protocol; every input
emits a `log("evt:…")` line the JS host parses. The renderer is a
`<canvas>` driven by the host shim.

## Run it

```bash
# 1. build the compiler if you haven't already
cargo build -p mty-cli

# 2. build + smoke-check the wasm component
bash demos/05_notetris_web/smoke.sh

# 3. serve on localhost:8000
bash demos/05_notetris_web/web/serve.sh
# (or:  pwsh demos/05_notetris_web/web/serve.ps1 )
```

Open <http://localhost:8000> and play with:

| key | action |
|-----|--------|
| ← → | move |
| ↑   | rotate (CW) |
| ↓   | soft drop |
| Space | hard drop |
| R   | reset (also after Game Over) |

## What this demo exercises

- `mty build --target wasm32-web` produces a Component Model component
- `package` / `protocol` / `agent` / `export fn` on a wasm-targeted unit
- The host imports `mighty.log(ptr, len)` and parses `evt:…` lines
- Game loop in the JS host (`requestAnimationFrame`) calls the wasm
  exports on every input so the round-trip is real

## What's stub vs real today

- **Real**: `mty check`, `mty fmt --check`, `mty build --target wasm32-web`,
  the Component artifact, the `log()` round-trip, the canonical Notetris
  rendering on `<canvas>`, keyboard input, score / level / lines, hard
  drop, line clearing, gravity by level.
- **Stub**: the rich canvas binding lives in the JS host because the
  `mty:web/canvas` WIT interface is on the post-v0.22 polish list; the
  Mighty agent emits state-change events as log lines today.
  When the canvas binding ships, the JS shim shrinks to ~30 lines and
  the agent owns the draw calls directly.

See `docs/internals/codegen-wasm.md` for the wasm backend's current
binding surface.
