#!/usr/bin/env python3
# demos/08_swarm_review/tools/mock_llm/server.py
#
# Three-provider stand-in for the swarm demo. One Python script binds
# a single port (default 8776) and serves the three provider shapes the
# v0.27 Track C clients speak:
#
#   * POST /v1/messages                        -> Anthropic
#   * POST /v1/responses                       -> OpenAI
#   * POST /v1beta/models/<model>:generateContent
#                                              -> Gemini
#
# The wire shapes match
#   docs.claude.com/en/api/messages,
#   platform.openai.com/docs/api-reference/responses,
#   ai.google.dev/api/rest/v1beta/models/generateContent.
#
# Each handler picks its canned reply based on a `MOCK_REPLY_<PROVIDER>`
# env var; defaults are tuned so the swarm consensus on the same prompt
# lands on `SAFE` (Anthropic+OpenAI agree; Gemini dissents with UNCLEAR)
# — which is what `smoke.sh` asserts.
#
# Stdlib-only (`http.server` + `json` + `os` + `sys`) — runs on any
# Python 3.8+ developer box without pip.
#
# Usage:
#   PORT=8776 python3 server.py &
#   curl -sS -XPOST http://localhost:8776/v1/messages \
#       -H 'Content-Type: application/json' \
#       -d '{"model":"claude-opus-4-7","messages":[{"role":"user","content":"hi"}]}'

from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
import re
import sys

# Per-provider canned replies. Override via env so the smoke test can
# drive the same script through every consensus path (unanimous, split,
# dissent). The defaults are the "majority SAFE, Gemini dissents" shape
# the demo's README documents.
REPLY_ANTHROPIC = os.environ.get(
    "MOCK_REPLY_ANTHROPIC",
    "MOCK_LLM[anthropic]: SAFE. No I/O, no user-controlled input, "
    "no external effects; a pure arithmetic helper.",
)
REPLY_OPENAI = os.environ.get(
    "MOCK_REPLY_OPENAI",
    "MOCK_LLM[openai]: SAFE. Pure function over an integer; the call "
    "site has no privilege boundary to cross.",
)
REPLY_GEMINI = os.environ.get(
    "MOCK_REPLY_GEMINI",
    "MOCK_LLM[gemini]: UNCLEAR. The snippet is small; in a larger "
    "context the caller's input source would matter.",
)

GEMINI_PATH_RE = re.compile(r"^/v1beta/models/([^:/]+):generateContent")


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):  # noqa: N802 — BaseHTTPRequestHandler API
        n = int(self.headers.get("content-length", "0"))
        raw = self.rfile.read(n) if n else b""
        try:
            req = json.loads(raw or b"{}")
        except json.JSONDecodeError:
            self.send_error(400, "bad json")
            return

        # ---- Anthropic Messages API -----------------------------------
        if self.path.startswith("/v1/messages"):
            model = req.get("model", "claude-opus-4-7")
            payload = {
                "id": "msg_mock_001",
                "type": "message",
                "role": "assistant",
                "model": model,
                "stop_reason": "end_turn",
                "stop_sequence": None,
                "content": [{"type": "text", "text": REPLY_ANTHROPIC}],
                "usage": {"input_tokens": 32, "output_tokens": 48},
            }
            self._send_json(payload)
            return

        # ---- OpenAI Responses API -------------------------------------
        if self.path.startswith("/v1/responses"):
            model = req.get("model", "gpt-5")
            payload = {
                "id": "resp_mock_001",
                "object": "response",
                "created_at": 1_700_000_000,
                "model": model,
                "status": "completed",
                "output": [
                    {
                        "type": "message",
                        "role": "assistant",
                        "content": [
                            {"type": "output_text", "text": REPLY_OPENAI}
                        ],
                    }
                ],
                "usage": {
                    "input_tokens": 28,
                    "output_tokens": 44,
                    "total_tokens": 72,
                },
            }
            self._send_json(payload)
            return

        # ---- Google Gemini ------------------------------------------
        m = GEMINI_PATH_RE.match(self.path)
        if m is not None:
            model = m.group(1)
            payload = {
                "candidates": [
                    {
                        "content": {
                            "role": "model",
                            "parts": [{"text": REPLY_GEMINI}],
                        },
                        "finishReason": "STOP",
                        "index": 0,
                    }
                ],
                "usageMetadata": {
                    "promptTokenCount": 30,
                    "candidatesTokenCount": 40,
                    "totalTokenCount": 70,
                },
                "modelVersion": model,
            }
            self._send_json(payload)
            return

        self.send_error(404, f"unknown path: {self.path}")

    def _send_json(self, obj):
        payload = json.dumps(obj).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, fmt, *args):
        sys.stderr.write("mock_llm: " + (fmt % args) + "\n")


def main():
    port = int(os.environ.get("PORT", "8776"))
    server = HTTPServer(("127.0.0.1", port), Handler)
    sys.stderr.write(
        f"mock_llm: listening on http://127.0.0.1:{port} "
        "(routes: /v1/messages, /v1/responses, /v1beta/models/.../:generateContent)\n"
    )
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        sys.stderr.write("mock_llm: shutting down\n")
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
