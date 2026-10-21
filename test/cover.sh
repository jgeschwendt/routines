#!/bin/bash
# cover.sh — line coverage of bin/routine: the suite runs against a copy whose
# line 4 also sets a DEBUG trap (no line shifts; set -T carries it into functions,
# $(…), subshells and the "$SELF" run re-exec), then every executable line the
# trap never reported is listed. --min fails on a failed test or a miss.
# bash 3.2-safe: no BASH_XTRACEFD, so the trap appends $LINENO itself.
set -u

SRC=$(cd "$(dirname "$0")/.." && pwd)
T=$(cd "$(mktemp -d "${TMPDIR:-/tmp}/routine-cover.XXXXXX")" && pwd)
trap 'rm -rf "$T"' EXIT
mkdir -p "$T/bin" "$T/test"
cp "$SRC/test/routine.test.sh" "$T/test/"
PROBE='set -uT; trap '\''printf "%s\n" "$LINENO" >> "$ROUTINE_COVER_LOG"'\'' DEBUG' \
  awk 'NR == 4 { if ($0 != "set -u") exit 1; $0 = ENVIRON["PROBE"] } 1' "$SRC/bin/routine" > "$T/bin/routine" ||
  { printf 'cover: bin/routine line 4 is no longer `set -u`\n' >&2; exit 2; }
chmod +x "$T/bin/routine"
ROUTINE_COVER_LOG="$T/hits" /bin/bash "$T/test/routine.test.sh"; RC=$?

# bash reports a multi-line command at its first, last or next-to-last line, so a
# statement is a span — quotes, $(…) and backslashes left open, heredoc bodies —
# credited at its first line by a hit anywhere inside. A span opening on a blank,
# comment, closer (} fi done esac ;; else then do) or bare case pattern is inert.
# Line 4 carries the probe. A hit on an inert span is an orphan: classifier drift.
# It reads the copy, never bin/routine: an edit mid-run would shift every line.
awk '
  FILENAME == ARGV[1] { hit[$1] = 1; next }
  { sp[FNR] = s = st == "" && !hd && !c ? FNR : s; src[FNR] = $0 }
  hd { if ($0 == hd) hd = ""; next }
  {
    n = length($0); code = $0; c = 0
    for (i = 1; i <= n; i++) {
      ch = substr($0, i, 1); top = substr(st, length(st))
      if (top == "S") { if (ch == "\047") st = substr(st, 1, length(st) - 1); continue }
      if (ch == "\\") { c = i == n; i++; continue }
      if (top == "D") {
        if (ch == "\"") st = substr(st, 1, length(st) - 1)
        else if (substr($0, i, 2) == "$(") { st = st "P"; i++ }
        continue
      }
      if (ch == "#" && (i == 1 || substr($0, i - 1, 1) ~ /[ \t]/)) { code = substr($0, 1, i - 1); break }
      if (ch == "\047") st = st "S"
      else if (ch == "\"") st = st "D"
      else if (ch == "(") st = st "P"
      else if (ch == ")" && top == "P") st = substr(st, 1, length(st) - 1)
      else if (substr($0, i, 2) == "<<") { w = substr($0, i + 2); gsub(/[-\047" ]/, "", w); match(w, /^[A-Za-z_]+/); hd = substr(w, 1, RLENGTH); i++ }
    }
    if (FNR == s) { gsub(/^[ \t]+|[ \t]+$/, "", code); ex[s] = FNR != 4 && code != "" && code !~ /^((}|fi|done|esac|;;|else|then|do)[ ;]*)+$/ && code !~ /^[^ (]+\)$/ }
  }
  END {
    for (l in hit) if (l in sp) { if (ex[sp[l]]) cov[sp[l]] = 1; else orph[sp[l]] = 1 }
    for (l = 1; l <= FNR; l++) if (ex[l]) { tot++; if (l in cov) got++; else out = out sprintf("miss   %4d  %s\n", l, src[l]) }
    for (l = 1; l <= FNR; l++) if (l in orph) out = out sprintf("orphan %4d  %s\n", l, src[l])
    printf "\n%scoverage: %d/%d executable lines (%.1f %%)\n", out, got, tot, 100 * got / tot
    exit (got < tot)
  }
' "$T/hits" "$T/bin/routine"; CRC=$?

[ "${1:-}" != --min ] || [ "$RC$CRC" = 00 ]
