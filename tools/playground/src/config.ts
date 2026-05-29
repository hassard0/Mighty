// v0.35 T1 — playground runtime configuration.
//
// Single source of truth for things the UI needs at runtime that
// aren't sensible to hardcode at the call sites:
//
//   - PROXY_URL:   the Cloudflare Worker that fronts Anthropic /
//                  OpenAI / Gemini (see `cf-worker/`). The playground
//                  hits `{PROXY_URL}/v1/{provider}/...` so the
//                  browser never holds a real API key.
//   - WASM_PATH:   relative path to the wasm-pack artifacts under
//                  the deployed site. `vite build` resolves this
//                  against `BASE_URL` at runtime.
//
// `process.env.*` and `import.meta.env.*` are both available at Vite
// build time, so secrets-style overrides can flow in via env vars
// without recompiling Rust.

const PROXY_URL: string =
  ((import.meta as unknown as { env: Record<string, string | undefined> }).env
    ?.VITE_MIGHTY_PROXY_URL as string | undefined) ??
  "https://mighty-proxy.workers.dev";

const WASM_PATH = "wasm/mty_cli.js";

export const config = {
  PROXY_URL,
  WASM_PATH,
};
