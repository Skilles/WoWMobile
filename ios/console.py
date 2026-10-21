#!/usr/bin/env python3
"""Talk to benilla's remote console (crates/benilla-app/src/mobile/console.rs).

    ios/console.py [--host H] [--token T] 'cmd' ['cmd' ...]   # run commands, print replies
    ios/console.py --host H                                   # read commands from stdin

The token is --token, $BENILLA_CONSOLE_TOKEN, or the console-token file in the app's settings
folder (copy it off the iPhone with `ios/run-device.sh`'s devicectl, or read it in the Simulator's
BENILLA_HOME). Replies are JSON lines: {"ok": bool, "out": str}.
"""
import argparse
import json
import os
import socket
import sys


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--host", default=os.environ.get("BENILLA_CONSOLE_HOST", "127.0.0.1"))
    ap.add_argument("--port", type=int, default=7979)
    ap.add_argument("--token", default=os.environ.get("BENILLA_CONSOLE_TOKEN"))
    ap.add_argument("--token-file")
    ap.add_argument("commands", nargs="*")
    args = ap.parse_args()
    token = args.token
    if not token and args.token_file:
        token = open(args.token_file).read().strip()
    if not token:
        sys.exit("no token: --token, $BENILLA_CONSOLE_TOKEN or --token-file")
    commands = args.commands or [l.rstrip("\n") for l in sys.stdin if l.strip()]
    with socket.create_connection((args.host, args.port), timeout=40) as s:
        f = s.makefile("rw", encoding="utf-8", newline="\n")
        ok = True
        for line in [f"auth {token}"] + commands:
            f.write(line + "\n")
            f.flush()
            answer = json.loads(f.readline())
            if line.startswith("auth "):
                if not answer["ok"]:
                    sys.exit(answer["out"])
                continue
            ok &= answer["ok"]
            print(("" if answer["ok"] else "ERROR: ") + answer["out"])
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
