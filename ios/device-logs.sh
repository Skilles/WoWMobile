#!/usr/bin/env bash
# List benilla's crash and memory (jetsam) reports on the paired iPhone, or fetch one and print
# what killed it.
#
#   ios/device-logs.sh                  # list
#   ios/device-logs.sh <name>.ips       # fetch and summarize
set -euo pipefail

DEVICE="${DEVICE:-$(xcrun devicectl list devices 2>/dev/null \
    | awk '/physical/ && /connected/ { for (i = 1; i <= NF; i++) { if ($i ~ /^[0-9A-F]{8}-[0-9A-F]{16}$/) { print $i; exit } } }')}"
if [[ -z "$DEVICE" ]]; then
    echo "No paired, connected iPhone found." >&2
    exit 1
fi

if [[ $# == 0 ]]; then
    xcrun devicectl device info files --device "$DEVICE" --domain-type systemCrashLogs 2>/dev/null \
        | grep -E 'benilla|JetsamEvent' | grep -v Retired
    exit 0
fi

out="$(mktemp -d)/$(basename "$1")"
xcrun devicectl device copy from --device "$DEVICE" --domain-type systemCrashLogs \
    --source "$1" --destination "$out" >/dev/null
echo "saved: $out"
python3 -I - "$out" <<'PY'
import json, sys
head, body = open(sys.argv[1]).read().split("\n", 1)
d = json.loads(body)
if "processes" in d:
    page = d.get("memoryStatus", {}).get("pageSize", 16384)
    for p in d["processes"]:
        if "benilla" in p.get("name", ""):
            mb = p.get("rpages", 0) * page / 2**20
            print(f"jetsam: benilla at {mb:.0f} MB, reason {p.get('reason')}")
    sys.exit(0)
print(d.get("exception"), d.get("termination", {}).get("indicator"))
th = d["threads"][d.get("faultingThread", 0)]
imgs = d["usedImages"]
for fr in th["frames"][:25]:
    print(f"{imgs[fr['imageIndex']].get('name', '?'):22} {fr.get('symbol', '?')[:140]}")
PY
