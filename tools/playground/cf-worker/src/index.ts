// v0.35 T1 — Cloudflare Worker LLM proxy for the Mighty playground.
//
// Routes:
//
//   POST /v1/anthropic/{path}  → https://api.anthropic.com/{path}
//   POST /v1/openai/{path}     → https://api.openai.com/{path}
//   POST /v1/gemini/{path}     → https://generativelanguage.googleapis.com/{path}
//   GET  /v1/health            → { ok: true }
//
// Each upstream gets its provider-shaped auth header swapped in from a
// Cloudflare secret (ANTHROPIC_API_KEY / OPENAI_API_KEY /
// GEMINI_API_KEY). The request body is forwarded verbatim.
//
// The per-IP rate limiter uses a tiny KV-backed counter keyed by
// "<provider>:<ip>:<utc-hour>". If KV isn't bound we degrade to
// "always allow" (useful for local `wrangler dev` runs).
//
// CORS: we echo `Access-Control-Allow-Origin` only when the request
// `Origin` is in `ALLOWED_ORIGINS`. Preflight `OPTIONS` requests are
// handled directly without proxying.

export interface Env {
  // Provider secrets — set with `wrangler secret put <NAME>`.
  ANTHROPIC_API_KEY?: string;
  OPENAI_API_KEY?: string;
  GEMINI_API_KEY?: string;
  // Optional KV namespace for rate-limiting. If missing, the limiter
  // is bypassed.
  RATE_LIMIT_KV?: KVNamespace;
  // Vars (see wrangler.toml).
  RATE_LIMIT_PER_HOUR: string;
  ALLOWED_ORIGINS: string;
}

interface KVNamespace {
  get(key: string): Promise<string | null>;
  put(key: string, value: string, opts?: { expirationTtl?: number }): Promise<void>;
}

interface ProviderRoute {
  provider: "anthropic" | "openai" | "gemini";
  upstream: string;
  authHeader: (env: Env) => Record<string, string>;
}

const ANTHROPIC_ROUTE: ProviderRoute = {
  provider: "anthropic",
  upstream: "https://api.anthropic.com",
  authHeader: (env) =>
    env.ANTHROPIC_API_KEY
      ? {
          "x-api-key": env.ANTHROPIC_API_KEY,
          "anthropic-version": "2023-06-01",
        }
      : {},
};

const OPENAI_ROUTE: ProviderRoute = {
  provider: "openai",
  upstream: "https://api.openai.com",
  authHeader: (env) =>
    env.OPENAI_API_KEY
      ? { Authorization: `Bearer ${env.OPENAI_API_KEY}` }
      : {},
};

const GEMINI_ROUTE: ProviderRoute = {
  provider: "gemini",
  upstream: "https://generativelanguage.googleapis.com",
  authHeader: (env) =>
    env.GEMINI_API_KEY ? { "x-goog-api-key": env.GEMINI_API_KEY } : {},
};

function routeFor(path: string): { route: ProviderRoute; rest: string } | null {
  if (path.startsWith("/v1/anthropic/")) {
    return { route: ANTHROPIC_ROUTE, rest: path.substring("/v1/anthropic".length) };
  }
  if (path.startsWith("/v1/openai/")) {
    return { route: OPENAI_ROUTE, rest: path.substring("/v1/openai".length) };
  }
  if (path.startsWith("/v1/gemini/")) {
    return { route: GEMINI_ROUTE, rest: path.substring("/v1/gemini".length) };
  }
  return null;
}

function corsHeaders(origin: string | null, env: Env): Record<string, string> {
  const allowList = env.ALLOWED_ORIGINS
    ? env.ALLOWED_ORIGINS.split(",").map((s) => s.trim()).filter(Boolean)
    : [];
  // Empty list = open. Non-empty = strict allowlist.
  const ok = allowList.length === 0 || (origin !== null && allowList.includes(origin));
  if (!ok) return {};
  return {
    "Access-Control-Allow-Origin": origin ?? "*",
    "Access-Control-Allow-Methods": "GET, POST, OPTIONS",
    "Access-Control-Allow-Headers": "Content-Type, X-Mighty-Trace-Id",
    "Access-Control-Max-Age": "86400",
  };
}

async function checkRateLimit(
  provider: string,
  ip: string,
  env: Env,
): Promise<{ allowed: boolean; remaining: number; cap: number }> {
  const cap = Number.parseInt(env.RATE_LIMIT_PER_HOUR ?? "10", 10);
  if (!env.RATE_LIMIT_KV) {
    // No KV binding — degrade to "always allow" so the Worker is
    // usable in `wrangler dev` without provisioning KV.
    return { allowed: true, remaining: cap, cap };
  }
  const hour = Math.floor(Date.now() / 3_600_000);
  const key = `${provider}:${ip}:${hour}`;
  const raw = await env.RATE_LIMIT_KV.get(key);
  const used = raw ? Number.parseInt(raw, 10) || 0 : 0;
  if (used >= cap) {
    return { allowed: false, remaining: 0, cap };
  }
  await env.RATE_LIMIT_KV.put(key, String(used + 1), {
    // Auto-expire after 1h05m so old buckets get GC'd.
    expirationTtl: 3900,
  });
  return { allowed: true, remaining: cap - used - 1, cap };
}

function clientIp(req: Request): string {
  // CF-Connecting-IP is the canonical Cloudflare-injected header.
  return (
    req.headers.get("cf-connecting-ip") ??
    req.headers.get("x-forwarded-for")?.split(",")[0].trim() ??
    "unknown"
  );
}

async function handleProxy(
  req: Request,
  env: Env,
  ctx: { route: ProviderRoute; rest: string },
): Promise<Response> {
  const origin = req.headers.get("origin");
  const cors = corsHeaders(origin, env);

  if (req.method !== "POST") {
    return new Response(
      JSON.stringify({ error: "method_not_allowed" }),
      { status: 405, headers: { "Content-Type": "application/json", ...cors } },
    );
  }

  const ip = clientIp(req);
  const rate = await checkRateLimit(ctx.route.provider, ip, env);
  if (!rate.allowed) {
    return new Response(
      JSON.stringify({
        error: "rate_limited",
        message: `per-IP cap of ${rate.cap} req/hour reached for ${ctx.route.provider}`,
        retry_after_seconds: 3600,
      }),
      {
        status: 429,
        headers: {
          "Content-Type": "application/json",
          "Retry-After": "3600",
          "X-RateLimit-Limit": String(rate.cap),
          "X-RateLimit-Remaining": "0",
          ...cors,
        },
      },
    );
  }

  const authHeader = ctx.route.authHeader(env);
  if (Object.keys(authHeader).length === 0) {
    return new Response(
      JSON.stringify({
        error: "provider_not_configured",
        message: `${ctx.route.provider} secret missing on this Worker`,
      }),
      {
        status: 503,
        headers: { "Content-Type": "application/json", ...cors },
      },
    );
  }

  const upstreamUrl = `${ctx.route.upstream}${ctx.rest}`;
  const body = await req.arrayBuffer();
  // Forward the request 1:1 — pass through Content-Type and any
  // streaming-related Accept headers the playground sets. We
  // deliberately strip Origin (upstream APIs reject Origin headers
  // they don't expect).
  const passthroughHeaders = new Headers();
  for (const [k, v] of req.headers) {
    if (
      [
        "content-type",
        "accept",
        "anthropic-beta",
        "x-stainless-os",
        "x-mighty-trace-id",
      ].includes(k.toLowerCase())
    ) {
      passthroughHeaders.set(k, v);
    }
  }
  for (const [k, v] of Object.entries(authHeader)) {
    passthroughHeaders.set(k, v);
  }

  const upstream = await fetch(upstreamUrl, {
    method: "POST",
    headers: passthroughHeaders,
    body,
  });

  // Mirror the upstream response back, including streaming bodies.
  const headers = new Headers();
  for (const [k, v] of upstream.headers) {
    headers.set(k, v);
  }
  for (const [k, v] of Object.entries(cors)) {
    headers.set(k, v);
  }
  headers.set("X-RateLimit-Limit", String(rate.cap));
  headers.set("X-RateLimit-Remaining", String(rate.remaining));

  return new Response(upstream.body, {
    status: upstream.status,
    headers,
  });
}

export default {
  async fetch(req: Request, env: Env): Promise<Response> {
    const url = new URL(req.url);
    const origin = req.headers.get("origin");
    const cors = corsHeaders(origin, env);

    // CORS preflight.
    if (req.method === "OPTIONS") {
      return new Response(null, { status: 204, headers: cors });
    }

    if (url.pathname === "/v1/health") {
      return new Response(
        JSON.stringify({ ok: true, ts: Date.now() }),
        { headers: { "Content-Type": "application/json", ...cors } },
      );
    }

    const ctx = routeFor(url.pathname);
    if (!ctx) {
      return new Response(
        JSON.stringify({
          error: "not_found",
          message: "available routes: POST /v1/{anthropic,openai,gemini}/...",
        }),
        {
          status: 404,
          headers: { "Content-Type": "application/json", ...cors },
        },
      );
    }
    return handleProxy(req, env, ctx);
  },
};
