#!/usr/bin/env python3
"""Minimal newline-delimited JSON-RPC 2.0 client for the MCP stdio transport.

Usage: jsonrpc_harness.py <server-command> '<tool>' '<json-args>' ['<tool>' '<json-args>' ...]
Sends initialize -> notifications/initialized -> tools/list -> tools/call (each pair),
<server-command> is split with shlex, e.g. "cargo run --release --quiet".
Printing every raw request (">>") and response (">>"/"<<") line. Standard library only.
"""
import json
import shlex
import subprocess
import sys


def main() -> int:
    binary, pairs = sys.argv[1], sys.argv[2:]
    if len(pairs) % 2:
        sys.exit("tool/args must come in pairs")
    proc = subprocess.Popen(shlex.split(binary), stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    next_id = 0

    def send(method, params=None, notify=False):
        nonlocal next_id
        msg = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            msg["params"] = params
        if not notify:
            next_id += 1
            msg["id"] = next_id
        line = json.dumps(msg)
        print(">> " + line, flush=True)
        proc.stdin.write(line + "\n")
        proc.stdin.flush()
        if notify:
            return None
        while True:
            raw = proc.stdout.readline()
            if not raw:
                sys.exit("server closed stdout")
            print("<< " + raw.rstrip("\n"), flush=True)
            resp = json.loads(raw)
            if resp.get("id") == msg["id"]:
                return resp

    init = send("initialize", {
        "protocolVersion": "2025-06-18",
        "capabilities": {},
        "clientInfo": {"name": "jsonrpc_harness.py", "version": "0.1"},
    })
    assert "result" in init, init
    send("notifications/initialized", notify=True)
    tools = send("tools/list", {})
    assert len(tools["result"]["tools"]) == 4, tools
    failures = 0
    for name, args in zip(pairs[::2], pairs[1::2]):
        r = send("tools/call", {"name": name, "arguments": json.loads(args)})
        failures += "error" in r
    proc.stdin.close()
    proc.wait(timeout=10)
    print(f"## harness: tools={len(tools['result']['tools'])} calls={len(pairs)//2} jsonrpc_errors={failures} exit={proc.returncode}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
