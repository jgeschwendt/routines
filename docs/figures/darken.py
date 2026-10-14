"""Rewrite a light board to its dark variant: darken.py <tokens.json> <board.dc.html>.
Every 6-digit hex (#rrggbb, or %23rrggbb in data URIs) that is a gigantic colour
token's light value becomes that token's dark value; the first token in tokens.json
order wins a shared light value (#ffffff → ui-bg, #476349 → ui-accent, not ui-ok).
Hex that is no light token value is printed to stderr — nothing silently stays light."""
import json, re, sys

tokens_path, board = sys.argv[1:3]
HEX = re.compile(r"^#[0-9a-f]{6}$")
m = {}
for t in json.load(open(tokens_path))["color"]["tokens"]:
    v = t["value"]
    if not isinstance(v, dict):
        continue
    light, dark = v["light"].lower(), v["dark"].lower()
    if HEX.match(light) and HEX.match(dark):
        m.setdefault(light[1:], dark[1:])
# Non-token hex → nearest dark token: none needed yet; add explicit entries here.
EXTRA = {}
m.update(EXTRA)

unmapped = set()
def sub(x):
    h = x.group(2).lower()
    if h in m:
        return x.group(1) + m[h]
    unmapped.add(x.group(1) + x.group(2))
    return x.group(0)

src = open(board).read()
out = re.sub(r"(#|%23)([0-9a-fA-F]{6})(?![0-9a-fA-F])", sub, src)
open(board, "w").write(out)
for h in sorted(unmapped):
    print("unmapped (stays light): " + h, file=sys.stderr)
