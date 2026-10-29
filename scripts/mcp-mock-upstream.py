#!/usr/bin/env python3
"""Stage 143 acceptance aid: a JSON-RPC MCP upstream on localhost.

Real-client acceptance (`claude mcp add` / Codex TOML) must not depend on a
reachable public upstream, or it stops being reproducible. Run this via
`task mcp-mock`, register `http://127.0.0.1:8899/mcp` as an MCP server in aigw,
then point the client at `/mcp/{alias}`.

Every request is logged as one JSON line on stderr, including the inbound
`Authorization` and `Mcp-Session-Id`. That log IS the acceptance evidence:

  - Claude Code sends `server/discover` and never sends `initialize`
  - Codex sends only `initialize`
  - aigw's outbound `Authorization` is the sealed token from
    `mcp_server_tokens`, never the client's aigw virtual key

`--legacy` makes the same process behave like a `2025-06-18` upstream (405 on
`server/discover`, `initialize` -> session id) so both outbound branches can be
exercised against one script.
"""

import argparse
import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

MODERN = "2026-07-28"
LEGACY = "2025-06-18"

TOOLS = [
    {
        "name": "search",
        "description": "Search a corpus",
        "inputSchema": {"type": "object", "properties": {"query": {"type": "string"}}},
    },
    {
        "name": "read_document",
        "description": "Read one document by id",
        "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}}},
    },
]

LEGACY_MODE = False


def log(payload):
    print(json.dumps(payload, ensure_ascii=False), file=sys.stderr, flush=True)


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):
        pass

    def _read_body(self):
        length = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(length) if length else b""
        try:
            return json.loads(raw) if raw else {}
        except json.JSONDecodeError:
            return {}

    def _respond(self, status, body, extra_headers=None):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        for key, value in (extra_headers or {}).items():
            self.send_header(key, value)
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        request = self._read_body()
        method = request.get("method") or ""
        request_id = request.get("id")

        log(
            {
                "method": method,
                "authorization": self.headers.get("Authorization"),
                "session_id": self.headers.get("Mcp-Session-Id"),
                "protocol_version": self.headers.get("MCP-Protocol-Version"),
                "params": request.get("params"),
            }
        )

        if method == "server/discover":
            if LEGACY_MODE:
                self._respond(405, {"error": "method not allowed"})
                return
            self._respond(
                200,
                {
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "result": {
                        "supportedVersions": [MODERN],
                        "ttlMs": 60000,
                        "serverInfo": {"name": "aigw-mock-upstream", "version": "1.0"},
                    },
                },
            )
            return

        if method == "initialize":
            self._respond(
                200,
                {
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "result": {
                        "protocolVersion": LEGACY,
                        "capabilities": {"tools": {}},
                        "serverInfo": {"name": "aigw-mock-upstream", "version": "1.0"},
                    },
                },
                {"Mcp-Session-Id": "mock-session-1"},
            )
            return

        if method == "tools/list":
            self._respond(
                200,
                {"jsonrpc": "2.0", "id": request_id, "result": {"tools": TOOLS}},
            )
            return

        if method == "tools/call":
            params = request.get("params") or {}
            self._respond(
                200,
                {
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "result": {
                        "content": [
                            {
                                "type": "text",
                                "text": f"mock upstream ran {params.get('name')} "
                                f"with {json.dumps(params.get('arguments') or {}, ensure_ascii=False)}",
                            }
                        ]
                    },
                },
            )
            return

        self._respond(
            200,
            {
                "jsonrpc": "2.0",
                "id": request_id,
                "error": {"code": -32601, "message": f"method not found: {method}"},
            },
        )

    def do_GET(self):
        log({"method": "GET", "path": self.path})
        self._respond(405, {"error": "notification stream is not served"})


def main():
    global LEGACY_MODE
    parser = argparse.ArgumentParser(description="mock MCP upstream for acceptance")
    parser.add_argument("--port", type=int, default=8899)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument(
        "--legacy",
        action="store_true",
        help="behave as a 2025-06-18 upstream (405 on server/discover)",
    )
    args = parser.parse_args()
    LEGACY_MODE = args.legacy

    server = ThreadingHTTPServer((args.host, args.port), Handler)
    flavour = "legacy " + LEGACY if LEGACY_MODE else "modern " + MODERN
    print(
        f"mock MCP upstream ({flavour}) listening on http://{args.host}:{args.port}/mcp",
        file=sys.stderr,
        flush=True,
    )
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
