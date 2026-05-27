#!/usr/bin/env python3
# demos/07_research_agent/tools/mock_llm/server.py
#
# Tiny stand-in for the Anthropic Messages API. Bound to localhost
# on a configurable port (default 8775); answers POST /v1/messages
# with a canned non-streaming reply that mentions every Track A/B/C
# surface name the smoke test asserts on. ~60 LOC, stdlib-only — no
# pip install required, runs on every Python 3.8+ developer box.
#
# Wire shape matches https://docs.claude.com/en/api/messages — the
# `content` array of `{"type":"text", "text":"..."}` blocks is what
# `AnthropicClient::complete` deserialises into `Message::text()`.
#
# Usage:
#   PORT=8775 python3 server.py &
#   curl -sS -XPOST http://localhost:8775/v1/messages \
#       -H 'Content-Type: application/json' \
#       -d '{"model":"claude-opus-4-7","messages":[{"role":"user","content":"hi"}],"max_tokens":256}'

from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
import sys

CANNED_REPLY = (
    "MOCK_LLM: std.memory provides three handle types — VectorStore "
    "(semantic search), Episodic (conversation history), and Working "
    "(per-turn scratchpad). All integrate with v0.19 deterministic "
    "replay."
)


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):  # noqa: N802 — BaseHTTPRequestHandler API
        n = int(self.headers.get("content-length", "0"))
        body = self.rfile.read(n) if n else b""
        try:
            req = json.loads(body or b"{}")
        except json.JSONDecodeError:
            self.send_error(400, "bad json")
            return

        if not self.path.startswith("/v1/messages"):
            self.send_error(404, f"unknown path: {self.path}")
            return

        model = req.get("model", "claude-opus-4-7")
        reply = {
            "id": "msg_mock_001",
            "type": "message",
            "role": "assistant",
            "model": model,
            "stop_reason": "end_turn",
            "stop_sequence": None,
            "content": [{"type": "text", "text": CANNED_REPLY}],
            "usage": {"input_tokens": 32, "output_tokens": 48},
        }
        payload = json.dumps(reply).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, fmt, *args):
        sys.stderr.write("mock_llm: " + (fmt % args) + "\n")


def main():
    port = int(os.environ.get("PORT", "8775"))
    server = HTTPServer(("127.0.0.1", port), Handler)
    sys.stderr.write(f"mock_llm: listening on http://127.0.0.1:{port}\n")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        sys.stderr.write("mock_llm: shutting down\n")
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
