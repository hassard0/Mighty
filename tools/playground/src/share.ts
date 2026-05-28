// v0.33 T3 — permalink encode/decode.
//
// We Base64-url-encode the UTF-8 source bytes and stick it in the URL
// hash (`#code=...`). Hash > query because:
//
//   - Hash isn't sent to the server, which matters once we host on a
//     CDN that might log query strings.
//   - GH Pages routes are HTML files, and a hash doesn't trip the 404
//     fallback some setups need.
//
// We also support `#example=<id>` for canonical permalinks to bundled
// examples. If both are present, `code` wins.
//
// The encoded source can get big. We don't pre-zip — at the example
// sizes shipped, the saving doesn't justify pulling in pako. v0.34
// follow-up: gzip + base64url if any example crosses ~6 KB.

export function encodeShareLink(source: string): string {
  const bytes = new TextEncoder().encode(source);
  const b64 = base64UrlEncode(bytes);
  return `${locationBase()}#code=${b64}`;
}

export interface ShareState {
  code?: string;
  exampleId?: string;
}

export function decodeShareState(hash: string): ShareState {
  const trimmed = hash.startsWith("#") ? hash.slice(1) : hash;
  if (!trimmed) return {};
  const out: ShareState = {};
  for (const part of trimmed.split("&")) {
    const eq = part.indexOf("=");
    if (eq < 0) continue;
    const key = part.slice(0, eq);
    const val = part.slice(eq + 1);
    if (key === "code") {
      try {
        out.code = new TextDecoder().decode(base64UrlDecode(val));
      } catch {
        // Malformed — ignore. Falls back to default example.
      }
    } else if (key === "example") {
      out.exampleId = decodeURIComponent(val);
    }
  }
  return out;
}

function locationBase(): string {
  const { origin, pathname } = window.location;
  return `${origin}${pathname}`;
}

// ---- base64url (no padding) ------------------------------------------------

function base64UrlEncode(bytes: Uint8Array): string {
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin)
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}

function base64UrlDecode(s: string): Uint8Array {
  const norm = s.replace(/-/g, "+").replace(/_/g, "/");
  const pad = norm.length % 4 === 0 ? "" : "=".repeat(4 - (norm.length % 4));
  const bin = atob(norm + pad);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

/** Best-effort copy-to-clipboard helper. Returns true on success. */
export async function copyToClipboard(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    // fall through to legacy path
  }
  try {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    const ok = document.execCommand("copy");
    document.body.removeChild(ta);
    return ok;
  } catch {
    return false;
  }
}
