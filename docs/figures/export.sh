#!/bin/sh
# Render every Claude Design artboard under docs/figures/project/ to its PNG at 2×,
# the same pixels the canvas exports — agent-browser, headless. Run from anywhere: mise run figures.
# support.js is the canvas's artifact-type/dc-runtime.js; the .dc.html files load it by relative path.
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
shoot "$S/Main.dc.html" "$(h "$S/canvas.json" Main.dc.html)" "$P/execution-path.png"
agent-browser close >/dev/null
