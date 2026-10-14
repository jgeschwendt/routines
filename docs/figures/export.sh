#!/bin/sh
# Render every Claude Design artboard under docs/figures/project/ to its PNG at 2×,
# the same pixels the canvas exports — agent-browser, headless. Run from anywhere: mise run figures.
# support.js is the canvas's artifact-type/dc-runtime.js; the .dc.html files load it by relative path.
# The board is drawn once, in the gigantic design system's LIGHT token values; the -dark.png is
# rendered from a temp copy rewritten light→dark by darken.py against project/ds/gigantic/tokens.json
# (read back from the canvas). darken.py prints any hex that is not a light token value.
set -e
P=$(cd "$(dirname "$0")" && pwd)
shoot() { # <source .dc.html> <height> <out.png> — retried: a browser relaunching
  # right after the last run's close can fail to connect or drop the viewport,
  # so every step may fail and the size is checked
  want="$((1600 * 2)) x $(($2 * 2))"
  for _ in 1 2 3; do
    { agent-browser open "file://$1" && agent-browser set viewport 1600 "$2" 2 \
      && agent-browser wait 1500 && agent-browser screenshot "$3"; } >/dev/null 2>&1 || { sleep 1; continue; }
    got=$(file "$3" | sed 's/.*PNG image data, //; s/,.*//')
    [ "$got" = "$want" ] && { printf '%s %s\n' "$(basename "$3")" "$got"; return; }
  done
  printf '%s: got %s, want %s\n' "$(basename "$3")" "$got" "$want" >&2; exit 1
}
h() { python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["boards"][sys.argv[2]]["h"])' "$1" "$2"; }
S=$P/project
H=$(h "$S/canvas.json" Main.dc.html)
shoot "$S/Main.dc.html" "$H" "$P/execution-path.png"
D=$(mktemp -d); trap 'rm -rf "$D"' EXIT
cp "$S/Main.dc.html" "$S/support.js" "$D/"
python3 -I "$P/darken.py" "$S/ds/gigantic/tokens.json" "$D/Main.dc.html"
shoot "$D/Main.dc.html" "$H" "$P/execution-path-dark.png"
agent-browser close >/dev/null
