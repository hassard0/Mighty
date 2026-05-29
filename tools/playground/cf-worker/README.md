# Mighty playground LLM proxy (Cloudflare Worker)

A tiny Cloudflare Worker that fronts Anthropic / OpenAI / Gemini for the browser-hosted Mighty playground so the demo can call real models without exposing API keys in client JavaScript.

## What it does

| Route                     | Upstream                                                                   |
| ------------------------- | -------------------------------------------------------------------------- |
| `POST /v1/anthropic/{p}`  | `https://api.anthropic.com/{p}`                                            |
| `POST /v1/openai/{p}`     | `https://api.openai.com/{p}`                                               |
| `POST /v1/gemini/{p}`     | `https://generativelanguage.googleapis.com/{p}`                            |
| `GET  /v1/health`         | (returns `{ ok: true }`)                                                   |

Per-IP rate-limited via Cloudflare KV (default 10 req/hour, configurable). CORS is locked to the configured `ALLOWED_ORIGINS` list (default: the GH Pages URL + local-dev origins). Each provider's secret is injected into the upstream request server-side; the client never sees it.

## Deploy (one-time)

1. Install the Wrangler CLI and authenticate:
   ```sh
   npm install
   npx wrangler login
   ```

2. Create the KV namespace and paste the returned ID into `wrangler.toml` (uncomment the `[[kv_namespaces]]` block):
   ```sh
   npx wrangler kv namespace create RATE_LIMIT_KV
   ```

3. Add your provider secrets (any subset; missing secrets surface as `503 provider_not_configured`):
   ```sh
   npx wrangler secret put ANTHROPIC_API_KEY
   npx wrangler secret put OPENAI_API_KEY
   npx wrangler secret put GEMINI_API_KEY
   ```

4. (Optional) tighten the per-IP rate cap via a secret instead of the public var:
   ```sh
   npx wrangler secret put RATE_LIMIT_PER_HOUR
   ```

5. Deploy:
   ```sh
   npm run deploy
   ```

The Worker's URL is printed on success — typically `https://mighty-proxy.<account>.workers.dev`.

## Local dev

```sh
npm run dev
```

Wrangler boots a local Worker on `http://127.0.0.1:8787`. The KV binding is auto-mocked; the rate limiter degrades to "always allow" so you can iterate.

## Pointing the playground at the deployed URL

Set the proxy URL in `tools/playground/src/config.ts` (or whatever the playground UI exposes). The default points at `https://mighty-proxy.workers.dev`; override it for your own account before publishing the playground build.

## Cost

Cloudflare's Workers free tier covers 100 000 requests / day with KV reads/writes well under the per-day limit at 10 req / IP / hour. The playground demo should comfortably fit inside the free tier for normal traffic.
