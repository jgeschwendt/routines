#!/bin/bash
# routine.test.sh — contract suite for bin/routine (docs/PLAN.md).
# Hermetic: one mktemp -d sandbox, every runner path redirected by env override,
# claude/launchctl replaced by stubs. Never touches $HOME, the network, or the
# repo tree. bash 3.2-safe (rules/bash.md) — runs under macOS /bin/bash and CI.
set -u

# The runner formats every epoch with `date -u` and floors days at UTC midnight,
# so cron evaluation is UTC regardless of the ambient zone; TZ is pinned anyway
# so a failure is never a zone artifact.
TZ=UTC; export TZ

TEST_DIR=$(cd "$(dirname "$0")" && pwd)
SRC_REPO=$(cd "$TEST_DIR/.." && pwd)
RUNNER="$SRC_REPO/bin/routine"

# ─── harness ──────────────────────────────────────────────────────────────────

N=0; PASS=0; FAIL=0

ok() { # $1=desc
  N=$((N + 1)); PASS=$((PASS + 1)); printf 'ok %d — %s\n' "$N" "$1"
}

nok() { # $1=desc $2=detail
  N=$((N + 1)); FAIL=$((FAIL + 1)); printf 'FAIL %d — %s (%s)\n' "$N" "$1" "$2"
}

assert_eq() { # $1=desc $2=expected $3=actual
  if [ "$2" = "$3" ]; then ok "$1"; else nok "$1" "want [$2] got [$3]"; fi
}

assert_contains() { # $1=desc $2=haystack $3=needle
  case "$2" in
    *"$3"*) ok "$1" ;;
    *) nok "$1" "no [$3] in [$(printf '%s' "$2" | tr '\n' '/' | cut -c1-160)]" ;;
  esac
}

assert_missing() { # $1=desc $2=haystack $3=needle
  case "$2" in
    *"$3"*) nok "$1" "unexpected [$3]" ;;
    *) ok "$1" ;;
  esac
}

assert_file() { # $1=desc $2=path
  if [ -f "$2" ]; then ok "$1"; else nok "$1" "no such file: $2"; fi
}

assert_dir() { # $1=desc $2=path
  if [ -d "$2" ]; then ok "$1"; else nok "$1" "no such directory: $2"; fi
}

assert_no_file() { # $1=desc $2=path
  if [ -e "$2" ]; then nok "$1" "exists: $2"; else ok "$1"; fi
}

# stdout of a run reaches us through the runner's `tee` co-process, so a tail can
# land microseconds after the runner exits — poll instead of racing it.
wait_grep() { # $1=file $2=needle
  local i=0
  while [ "$i" -lt 30 ]; do
    if [ -f "$1" ] && grep -qiF -- "$2" "$1" 2>/dev/null; then return 0; fi
    sleep 0.1; i=$((i + 1))
  done
  return 1
}

assert_grep() { # $1=desc $2=file $3=needle
  if wait_grep "$2" "$3"; then ok "$1"; else nok "$1" "no [$3] in $2"; fi
}

# ─── sandbox ──────────────────────────────────────────────────────────────────

SANDBOX=$(cd "$(mktemp -d "${TMPDIR:-/tmp}/routine-test.XXXXXX")" && pwd)
LOCK_HOLDER=""

cleanup() {
  [ -n "$LOCK_HOLDER" ] && kill "$LOCK_HOLDER" 2>/dev/null
  rm -rf "$SANDBOX"
}
trap cleanup EXIT

RHOME="$SANDBOX/home"                  # ROUTINE_HOME — a routines home, never the real one
RCWD="$SANDBOX/cwd"                    # every invocation's cwd — sync-ci writes here
RDIR="$SANDBOX/routines"               # ROUTINE_DIR — reassigned per due-fixture group
RSTATE="$SANDBOX/state"
RAGENTS="$SANDBOX/agents"
LCTL="$SANDBOX/bin/launchctl"
CLAUDE="$SANDBOX/bin/claude"
LCTL_LOG="$SANDBOX/launchctl.log"
CLAUDE_ARGV="$SANDBOX/claude.argv"     # one arg per line
CLAUDE_FLAGS="$SANDBOX/claude.flags"   # argv minus the -p value, space-joined
CLAUDE_PROMPT="$SANDBOX/claude.prompt"
CLAUDE_CALLED="$SANDBOX/claude.called"
REPAIR_MARKER="$SANDBOX/repaired.marker"

mkdir -p "$RHOME" "$RCWD" "$RDIR" "$RSTATE" "$SANDBOX/bin"

mkfix() { # $1=path — body on stdin
  mkdir -p "$(dirname "$1")"
  cat > "$1"
}

# ─── stubs ────────────────────────────────────────────────────────────────────

cat > "$LCTL" <<EOF
#!/bin/bash
printf '%s\n' "\$*" >> "$LCTL_LOG"
exit 0
EOF
chmod +x "$LCTL"

write_claude_stub() { # $1=mode: record | repair
  cat > "$CLAUDE" <<EOF
#!/bin/bash
: > "$CLAUDE_CALLED"
: > "$CLAUDE_ARGV"
for a in "\$@"; do printf '%s\n' "\$a" >> "$CLAUDE_ARGV"; done
flags=""; prompt=""
while [ \$# -gt 0 ]; do
  case "\$1" in
    -p) flags="\$flags -p"; prompt="\${2:-}"; shift 2 ;;
    *) flags="\$flags \$1"; shift ;;
  esac
done
printf '%s\n' "\$flags" > "$CLAUDE_FLAGS"
printf '%s' "\$prompt" > "$CLAUDE_PROMPT"
EOF
  if [ "$1" = repair ]; then printf '%s\n' ": > \"$REPAIR_MARKER\"" >> "$CLAUDE"; fi
  printf 'exit 0\n' >> "$CLAUDE"
  chmod +x "$CLAUDE"
}
write_claude_stub record

# ─── invocation ───────────────────────────────────────────────────────────────

RNOW=""        # ROUTINE_NOW for the next runr; "" = wall clock
EXPORTS=""     # extra KEY=VALUE pairs (no spaces in values)
RC=0; OUT=""; ERR=""
O="$SANDBOX/stdout"; E="$SANDBOX/stderr"

runr() { # verb + args
  (
    unset ROUTINE_TEST_REQ_A ROUTINE_TEST_REQ_B
    export ROUTINE_HOME="$RHOME" ROUTINE_DIR="$RDIR" ROUTINE_STATE="$RSTATE"
    export ROUTINE_AGENTS_DIR="$RAGENTS" ROUTINE_LAUNCHCTL="$LCTL" ROUTINE_CLAUDE="$CLAUDE"
    if [ -n "$RNOW" ]; then export ROUTINE_NOW="$RNOW"; else unset ROUTINE_NOW; fi
    for kv in $EXPORTS; do export "$kv"; done
    cd "$RCWD" || exit 1
    exec "$RUNNER" "$@"
  ) > "$O" 2> "$E"
  RC=$?
  OUT=$(cat "$O"); ERR=$(cat "$E")
}

# The bare-home path: only ROUTINE_HOME is set, so ROUTINE_DIR and ROUTINE_STATE
# must derive from it rather than from an override.
runr_home() { # $1=home, then verb + args
  local home="$1"; shift
  (
    unset ROUTINE_DIR ROUTINE_STATE ROUTINE_NOW
    export ROUTINE_HOME="$home"
    export ROUTINE_AGENTS_DIR="$RAGENTS" ROUTINE_LAUNCHCTL="$LCTL" ROUTINE_CLAUDE="$CLAUDE"
    cd "$RCWD" || exit 1
    exec "$RUNNER" "$@"
  ) > "$O" 2> "$E"
  RC=$?
  OUT=$(cat "$O"); ERR=$(cat "$E")
}

lastrun() { # $1=name
  printf '%s\n' "$RSTATE/$1/last-run.json"
}

nlines() { # $1=file
  if [ -f "$1" ]; then awk 'END { print NR + 0 }' "$1"; else printf '0\n'; fi
}

# ─── fixtures ─────────────────────────────────────────────────────────────────

mkfix "$RDIR/blocks.md" <<EOF
---
timeout: 30
---

# Blocks

Prose is inert.

\`\`\`sh
printf 'one\n' >> "$SANDBOX/order.txt"
pwd > "$SANDBOX/pwd.txt"
printf '%s\n' "\$ROUTINE_NAME" > "$SANDBOX/name.txt"
printf '%s\n' "\$ROUTINE_STATE_DIR" > "$SANDBOX/statedir.txt"
FROM_BLOCK_ONE=leaked
\`\`\`

\`\`\`text
: > "$SANDBOX/inert-text.marker"
\`\`\`

\`\`\`
: > "$SANDBOX/inert-bare.marker"
\`\`\`

\`\`\`python
open("$SANDBOX/inert-python.marker", "w")
\`\`\`

\`\`\`bash
printf 'two\n' >> "$SANDBOX/order.txt"
[ -z "\${FROM_BLOCK_ONE:-}" ] || exit 3
\`\`\`
EOF

mkfix "$RDIR/errexit.md" <<EOF
---
on_error: fail
---

# Errexit

\`\`\`sh
false
: > "$SANDBOX/errexit-continued.marker"
\`\`\`
EOF

mkfix "$RDIR/noblocks.md" <<'EOF'
---
schedule: 0 * * * *
---

# No blocks

Prose only; the fenced material below is not a shell.

```json
{"not": "a block"}
```
EOF

mkfix "$RDIR/requires.md" <<EOF
---
requires: ROUTINE_TEST_REQ_A ROUTINE_TEST_REQ_B
on_error: fail
---

# Requires

\`\`\`sh
: > "$SANDBOX/requires-ran.marker"
\`\`\`
EOF

mkfix "$RDIR/failing.md" <<'EOF'
---
on_error: fail
---

# Failing

```sh
printf 'FAILING-OUTPUT\n'
exit 42
```
EOF

mkfix "$RDIR/slow.md" <<'EOF'
---
timeout: 2
---

# Slow

```sh
sleep 30
```
EOF

mkfix "$RDIR/deftimeout.md" <<'EOF'
---
schedule: 0 * * * *
---

# Default timeout

No `timeout` key — the default must not trip a two-second block.

```sh
sleep 2
```
EOF

mkfix "$RDIR/locked.md" <<'EOF'
---
schedule: 0 * * * *
---

# Locked

```sh
printf 'locked ran\n'
```
EOF

# repair-yes / repair-no share a shape but not a marker: on_error is absent, so
# the default (claude) must fire.
mkfix "$RDIR/repair-yes.md" <<EOF
---
timeout: 30
---

# Repair yes

PROSE-CANARY-4417 — this line must reach the catch prompt intact.

\`\`\`sh
printf 'x\n' >> "$SANDBOX/repair-yes.runs"
printf 'SENTINEL-OUTPUT-9271\n'
[ -f "$REPAIR_MARKER" ] || exit 7
\`\`\`
EOF

mkfix "$RDIR/repair-no.md" <<EOF
---
timeout: 30
---

# Repair no

\`\`\`sh
printf 'x\n' >> "$SANDBOX/repair-no.runs"
printf 'SENTINEL-OUTPUT-9271\n'
[ -f "$SANDBOX/never-created.marker" ] || exit 7
\`\`\`
EOF

# derived-defaults fixture — a routines home with no ROUTINE_DIR/ROUTINE_STATE
# override, so the document must be found at the home's root.
DHOME="$SANDBOX/home-default"
mkfix "$DHOME/derived.md" <<EOF
---
timeout: 30
---

# Derived

\`\`\`sh
pwd > "$SANDBOX/derived-pwd.txt"
\`\`\`
EOF

# due-ness fixtures — one routine per directory so `run --due` has a single
# candidate and its silence is unambiguous.
mkfix "$SANDBOX/r-hourly/hourly.md" <<EOF
---
schedule: 0 * * * *
---

# Hourly

\`\`\`sh
printf 'x\n' >> "$SANDBOX/hourly.runs"
\`\`\`
EOF

mkfix "$SANDBOX/r-quarter/quarter.md" <<EOF
---
schedule: */15 * * * *
---

# Quarter

\`\`\`sh
printf 'x\n' >> "$SANDBOX/quarter.runs"
\`\`\`
EOF

mkfix "$SANDBOX/r-weekday/weekday.md" <<EOF
---
schedule: 30 7 * * 1-5
---

# Weekday

\`\`\`sh
printf 'x\n' >> "$SANDBOX/weekday.runs"
\`\`\`
EOF

# never-due: no schedule key, and a frontmatter fence that does not lead the file.
mkfix "$SANDBOX/r-never/noschedule.md" <<EOF
---
timeout: 30
---

# No schedule

\`\`\`sh
: > "$SANDBOX/noschedule.marker"
\`\`\`
EOF

mkfix "$SANDBOX/r-never/latefm.md" <<EOF

---
schedule: * * * * *
---

# Late frontmatter

\`\`\`sh
: > "$SANDBOX/latefm.marker"
\`\`\`
EOF

# ─── usage & dispatch ─────────────────────────────────────────────────────────

runr
assert_eq "bare invocation exits 0" 0 "$RC"
assert_contains "bare invocation prints usage" "$OUT" "routine run"

runr help
assert_eq "help exits 0" 0 "$RC"
assert_contains "help prints usage" "$OUT" "routine sync-ci"

runr bogus-verb
assert_eq "unknown verb exits 64" 64 "$RC"

runr run no-such-routine
assert_eq "unknown routine exits 64" 64 "$RC"

runr run
assert_eq "run without a name exits 64" 64 "$RC"

runr run noblocks
assert_eq "document with zero sh/bash blocks exits 64" 64 "$RC"

# ─── blocks ───────────────────────────────────────────────────────────────────

runr run blocks
assert_eq "all blocks pass ⇒ exit 0" 0 "$RC"
assert_eq "sh and bash blocks run in document order" "one
two" "$(cat "$SANDBOX/order.txt" 2>/dev/null)"
assert_eq "blocks run with cwd = the routines home" "$RHOME" \
  "$(cat "$SANDBOX/pwd.txt" 2>/dev/null)"
assert_eq "ROUTINE_NAME is exported to blocks" "blocks" "$(cat "$SANDBOX/name.txt" 2>/dev/null)"
assert_eq "ROUTINE_STATE_DIR is exported to blocks" "$RSTATE/blocks" \
  "$(cat "$SANDBOX/statedir.txt" 2>/dev/null)"
assert_no_file "text fence is inert" "$SANDBOX/inert-text.marker"
assert_no_file "untagged fence is inert" "$SANDBOX/inert-bare.marker"
assert_no_file "python fence is inert" "$SANDBOX/inert-python.marker"

runr run errexit
assert_eq "a failing command aborts its block (bash -e)" 1 "$RC"
assert_no_file "no statement after the failure runs" "$SANDBOX/errexit-continued.marker"

# ─── frontmatter ──────────────────────────────────────────────────────────────

runr run requires
assert_eq "missing requires exits 78" 78 "$RC"
assert_contains "stderr names the first missing var" "$ERR" "ROUTINE_TEST_REQ_A"
assert_contains "stderr names every missing var" "$ERR" "ROUTINE_TEST_REQ_B"
assert_no_file "requires gate runs no block" "$SANDBOX/requires-ran.marker"

EXPORTS="ROUTINE_TEST_REQ_A=1 ROUTINE_TEST_REQ_B=2"
runr run requires
EXPORTS=""
assert_eq "satisfied requires runs the routine" 0 "$RC"
assert_file "satisfied requires reaches the block" "$SANDBOX/requires-ran.marker"

runr run deftimeout
assert_eq "absent timeout key defaults large (2s block survives)" 0 "$RC"

# ─── routines home ────────────────────────────────────────────────────────────

runr_home "$DHOME" run derived
assert_eq "ROUTINE_HOME alone locates \$home/<name>.md" 0 "$RC"
assert_eq "ROUTINE_HOME alone sets the block's cwd" "$DHOME" \
  "$(cat "$SANDBOX/derived-pwd.txt" 2>/dev/null)"
assert_file "ROUTINE_STATE derives to \$ROUTINE_HOME/.state" \
  "$DHOME/.state/derived/last-run.json"

runr_home "$DHOME" status
assert_contains "status reads \$ROUTINE_HOME/*.md" "$OUT" "derived"

# the cd guard: the document resolves (ROUTINE_DIR is overridden) but the home
# it would run in does not exist.
(
  export ROUTINE_HOME="$SANDBOX/no-such-home" ROUTINE_DIR="$RDIR" ROUTINE_STATE="$RSTATE"
  export ROUTINE_AGENTS_DIR="$RAGENTS" ROUTINE_LAUNCHCTL="$LCTL" ROUTINE_CLAUDE="$CLAUDE"
  cd "$RCWD" || exit 1
  exec "$RUNNER" run deftimeout
) > "$O" 2> "$E"
RC=$?
assert_eq "a missing routines home exits 64" 64 "$RC"

# ─── state ────────────────────────────────────────────────────────────────────

LR=$(lastrun blocks)
assert_file "last-run.json is written" "$LR"
assert_eq "last-run.json is a single line" 1 "$(nlines "$LR")"
LRC=$(cat "$LR" 2>/dev/null)
assert_contains "last-run.json has started" "$LRC" '"started"'
assert_contains "last-run.json has started_epoch" "$LRC" '"started_epoch"'
assert_contains "last-run.json has finished" "$LRC" '"finished"'
assert_contains "last-run.json has exit" "$LRC" '"exit"'
assert_contains "last-run.json has duration" "$LRC" '"duration"'
assert_contains "last-run.json records the success code" "$LRC" '"exit":0'

LOGS=$(ls "$RSTATE/blocks/logs" 2>/dev/null | wc -l | tr -d ' ')
assert_eq "one log file per run" 1 "$LOGS"
LOG="$RSTATE/blocks/logs/$(ls "$RSTATE/blocks/logs" 2>/dev/null | head -1)"
assert_grep "log delimits block 1" "$LOG" "block 1"
assert_grep "log delimits block 2" "$LOG" "block 2"

rm -f "$CLAUDE_CALLED"
runr run failing
assert_eq "on_error: fail passes the block's code through" 42 "$RC"
assert_no_file "on_error: fail never invokes claude" "$CLAUDE_CALLED"
assert_grep "the block's output lands in the log" \
  "$RSTATE/failing/logs/$(ls "$RSTATE/failing/logs" 2>/dev/null | tail -1)" "FAILING-OUTPUT"
assert_contains "last-run.json is written on the failure path" \
  "$(cat "$(lastrun failing)" 2>/dev/null)" '"exit":42'

# lock — a live pid holds it, so nothing races the assertion.
runr run locked
assert_eq "seed run for the lock case" 0 "$RC"
LOCKED_BEFORE=$(cat "$(lastrun locked)" 2>/dev/null)
sleep 5 & LOCK_HOLDER=$!
mkdir -p "$RSTATE/locked/lock"
printf '%s\n' "$LOCK_HOLDER" > "$RSTATE/locked/lock/pid"
runr run locked
assert_eq "live lock exits 75" 75 "$RC"
assert_contains "lock refusal names the routine" "$ERR" "locked"
assert_eq "a 75 does not clobber last-run.json" "$LOCKED_BEFORE" \
  "$(cat "$(lastrun locked)" 2>/dev/null)"
kill "$LOCK_HOLDER" 2>/dev/null; wait "$LOCK_HOLDER" 2>/dev/null; LOCK_HOLDER=""

# a dead pid in the lock is reaped rather than honoured
sleep 0 & DEAD=$!
wait "$DEAD" 2>/dev/null
mkdir -p "$RSTATE/locked/lock"
printf '%s\n' "$DEAD" > "$RSTATE/locked/lock/pid"
runr run locked
assert_eq "stale lock is reaped" 0 "$RC"
assert_no_file "the reaped lock is released" "$RSTATE/locked/lock"

# ─── timeout ──────────────────────────────────────────────────────────────────

runr run slow
assert_eq "a block past its timeout exits 124" 124 "$RC"
assert_contains "last-run.json is written on the timeout path" \
  "$(cat "$(lastrun slow)" 2>/dev/null)" '"exit":124'

# ─── catch ────────────────────────────────────────────────────────────────────

rm -f "$CLAUDE_CALLED" "$REPAIR_MARKER" "$CLAUDE_PROMPT" "$CLAUDE_FLAGS"
write_claude_stub repair
runr run repair-yes
assert_eq "catch repairs, the retried block passes ⇒ exit 0" 0 "$RC"
assert_file "on_error defaults to claude" "$CLAUDE_CALLED"
FLAGS=$(cat "$CLAUDE_FLAGS" 2>/dev/null)
assert_contains "claude is invoked with --model opus" "$FLAGS" "--model opus"
assert_contains "claude is invoked with --permission-mode=auto" "$FLAGS" "--permission-mode=auto"
assert_contains "claude is invoked with --no-session-persistence" "$FLAGS" "--no-session-persistence"
assert_contains "claude is invoked with -p" "$FLAGS" " -p"
PROMPT=$(cat "$CLAUDE_PROMPT" 2>/dev/null)
assert_contains "the prompt carries the document's prose" "$PROMPT" "PROSE-CANARY-4417"
assert_contains "the prompt carries the frontmatter" "$PROMPT" "timeout: 30"
assert_contains "the prompt carries the failing block's output" "$PROMPT" "SENTINEL-OUTPUT-9271"
assert_contains "claude records its full argv" "$(cat "$CLAUDE_ARGV" 2>/dev/null)" "--model"
assert_eq "the repaired block is executed twice — one retry, no loop" 2 \
  "$(nlines "$SANDBOX/repair-yes.runs")"

rm -f "$CLAUDE_CALLED"
write_claude_stub record
runr run repair-no
assert_eq "catch without repair passes the block's code through" 7 "$RC"
assert_file "claude is still invoked when it cannot repair" "$CLAUDE_CALLED"
assert_eq "an unrepaired block is retried exactly once" 2 \
  "$(nlines "$SANDBOX/repair-no.runs")"

rm -f "$CLAUDE_CALLED"
CLAUDE_REAL="$CLAUDE"
CLAUDE="$SANDBOX/bin/nonexistent-claude"
runr run repair-no
assert_eq "a missing claude binary degrades to fail" 7 "$RC"
assert_eq "a degraded catch does not retry" 3 "$(nlines "$SANDBOX/repair-no.runs")"
DEGRADED_LOG="$RSTATE/repair-no/logs/$(ls "$RSTATE/repair-no/logs" 2>/dev/null | tail -1)"
assert_grep "the degradation reason names claude in the log" "$DEGRADED_LOG" "claude"
CLAUDE="$CLAUDE_REAL"

# ─── due-ness ─────────────────────────────────────────────────────────────────

# Epochs are hand-computed UTC constants; the runner floors and formats in UTC.
T_HOURLY_RUN=1786698300      # Fri 2026-08-14T09:05:00Z
T_HOURLY_PLUS600=1786698900  # Fri 2026-08-14T09:15:00Z — before the 10:00 fire
T_HOURLY_DUE=1786701660      # Fri 2026-08-14T10:01:00Z — past it
T_QUARTER_EARLY=1786698840   # Fri 2026-08-14T09:14:00Z — before the 09:15 fire
T_QUARTER_DUE=1786698960     # Fri 2026-08-14T09:16:00Z — past it
T_THU=1786606260             # Thu 2026-08-13T07:31:00Z
T_FRI_EARLY=1786692540       # Fri 2026-08-14T07:29:00Z
T_FRI_DUE=1786692660         # Fri 2026-08-14T07:31:00Z
T_SAT=1786795200             # Sat 2026-08-15T12:00:00Z
T_SUN=1786881600             # Sun 2026-08-16T12:00:00Z
T_MON=1786951860             # Mon 2026-08-17T07:31:00Z

RDIR="$SANDBOX/r-never"
RNOW="$T_HOURLY_DUE"
runr run --due
assert_eq "nothing due ⇒ exit 0" 0 "$RC"
assert_eq "nothing due ⇒ silent stdout" "" "$OUT"
assert_eq "nothing due ⇒ silent stderr" "" "$ERR"
assert_no_file "no schedule ⇒ never due" "$SANDBOX/noschedule.marker"
assert_no_file "a frontmatter fence that does not lead the file is not parsed" \
  "$SANDBOX/latefm.marker"

RDIR="$SANDBOX/r-hourly"
RNOW="$T_HOURLY_RUN"
runr run --due
assert_eq "no last-run ⇒ due" 1 "$(nlines "$SANDBOX/hourly.runs")"
assert_eq "run --due exits 0 after a successful routine" 0 "$RC"
RNOW="$T_HOURLY_PLUS600"
runr run --due
assert_eq "'0 * * * *' ten minutes on is not due" 1 "$(nlines "$SANDBOX/hourly.runs")"
assert_eq "a skipped tick is silent" "" "$OUT"
RNOW="$T_HOURLY_DUE"
runr run --due
assert_eq "'0 * * * *' past the hour boundary is due" 2 "$(nlines "$SANDBOX/hourly.runs")"

RDIR="$SANDBOX/r-quarter"
RNOW="$T_HOURLY_RUN"
runr run --due
assert_eq "'*/15 * * * *' first run" 1 "$(nlines "$SANDBOX/quarter.runs")"
RNOW="$T_QUARTER_EARLY"
runr run --due
assert_eq "'*/15 * * * *' before the next quarter is not due" 1 \
  "$(nlines "$SANDBOX/quarter.runs")"
RNOW="$T_QUARTER_DUE"
runr run --due
assert_eq "'*/15 * * * *' past the next quarter is due" 2 \
  "$(nlines "$SANDBOX/quarter.runs")"

RDIR="$SANDBOX/r-weekday"
RNOW="$T_THU"
runr run --due
assert_eq "'30 7 * * 1-5' first run (Thu 07:31Z)" 1 "$(nlines "$SANDBOX/weekday.runs")"
RNOW="$T_FRI_EARLY"
runr run --due
assert_eq "Fri 07:29Z is a minute before the fire — not due" 1 \
  "$(nlines "$SANDBOX/weekday.runs")"
RNOW="$T_FRI_DUE"
runr run --due
assert_eq "Fri 07:31Z is past the fire — due" 2 "$(nlines "$SANDBOX/weekday.runs")"
RNOW="$T_SAT"
runr run --due
assert_eq "Saturday is gated by the 1-5 range" 2 "$(nlines "$SANDBOX/weekday.runs")"
RNOW="$T_SUN"
runr run --due
assert_eq "Sunday is gated by the 1-5 range" 2 "$(nlines "$SANDBOX/weekday.runs")"
RNOW="$T_MON"
runr run --due
assert_eq "Monday 07:31Z reopens the range — due" 3 "$(nlines "$SANDBOX/weekday.runs")"

RNOW=""
RDIR="$SANDBOX/routines"

# ─── status ───────────────────────────────────────────────────────────────────

runr status
assert_eq "status exits 0" 0 "$RC"
assert_contains "status lists each routine" "$OUT" "locked"
assert_contains "status joins the schedule" "$OUT" "0 * * * *"

printf 'not a routine\n' > "$RDIR/AGENTS.md"
runr status
assert_missing "status skips non-routine names (AGENTS.md)" "$OUT" "AGENTS"
runr run --due
assert_eq "run --due skips non-routine names" 0 "$RC"
rm "$RDIR/AGENTS.md"

# ─── install / uninstall ──────────────────────────────────────────────────────

PLIST="$RAGENTS/com.routines.due.plist"
UID_NOW=$(id -u)
runr install
assert_eq "install exits 0" 0 "$RC"
assert_file "install writes the launch agent" "$PLIST"
PL=$(cat "$PLIST" 2>/dev/null)
assert_contains "plist declares the label" "$PL" "com.routines.due"
assert_contains "plist sets StartInterval 60" "$PL" "<key>StartInterval</key><integer>60</integer>"
assert_contains "plist sets RunAtLoad" "$PL" "<key>RunAtLoad</key><true/>"
assert_contains "plist runs /bin/bash" "$PL" "/bin/bash"
assert_contains "plist uses a login shell" "$PL" "-lc"
assert_contains "plist carries the runner's absolute path" "$PL" "$RUNNER"
assert_contains "plist runs the due tick" "$PL" "run --due"
assert_missing "plist never cds into a repo — the runner cds itself" "$PL" "&amp;&amp;"
LC=$(cat "$LCTL_LOG" 2>/dev/null)
assert_contains "install boots the old agent out" "$LC" "bootout gui/$UID_NOW/com.routines.due"
assert_contains "install bootstraps the agent" "$LC" "bootstrap gui/$UID_NOW"
assert_eq "bootout precedes bootstrap" 1 "$(grep -n bootout "$LCTL_LOG" | head -1 | cut -d: -f1)"

FRESH_HOME="$SANDBOX/fresh-home"
runr_home "$FRESH_HOME" install
assert_eq "install into an absent home exits 0" 0 "$RC"
assert_dir "install creates the routines home" "$FRESH_HOME"
assert_dir "install creates the derived state dir" "$FRESH_HOME/.state"

: > "$LCTL_LOG"
runr uninstall
assert_eq "uninstall exits 0" 0 "$RC"
assert_no_file "uninstall removes the plist" "$PLIST"
assert_contains "uninstall boots the agent out" "$(cat "$LCTL_LOG" 2>/dev/null)" \
  "bootout gui/$UID_NOW/com.routines.due"

# ─── sync-ci ──────────────────────────────────────────────────────────────────

# sync-ci writes into the invoking repo — every run here has cwd = $RCWD, so the
# workflow lands in the sandbox and the real tree gains nothing. A live install
# may already own $SRC_REPO/.state, so assert against the snapshot, not bare
# existence.
SRC_STATE_PRE=0; [ -e "$SRC_REPO/.state" ] && SRC_STATE_PRE=1
WF="$RCWD/.github/workflows/routines.yml"
runr sync-ci
assert_eq "sync-ci exits 0" 0 "$RC"
assert_file "sync-ci writes the workflow into the invoking repo" "$WF"
WFC=$(cat "$WF" 2>/dev/null)
assert_contains "workflow ticks every 15 minutes" "$WFC" "cron: '*/15 * * * *'"
assert_contains "workflow is manually dispatchable" "$WFC" "workflow_dispatch"
assert_contains "workflow points ROUTINE_HOME at the repo's routines dir" "$WFC" \
  'ROUTINE_HOME: ${{ github.workspace }}/routines'
assert_contains "workflow caches state" "$WFC" "actions/cache"
assert_contains "workflow caches the derived state path" "$WFC" "path: routines/.state"
assert_contains "workflow restores the newest state key" "$WFC" "restore-keys"
assert_contains "workflow fetches the runner when the repo has none" "$WFC" \
  "[ -x bin/routine ] || (mkdir -p bin && curl -fsSL"
assert_contains "workflow ends at the due tick" "$WFC" "bin/routine run --due"
assert_no_file "sync-ci targets its cwd, never the routines home" "$RHOME/.github"
if [ "$SRC_STATE_PRE" = 0 ]; then
  assert_no_file "sync-ci never writes into the source tree under test" \
    "$SRC_REPO/.state"
else
  assert_eq "sync-ci never writes into the source tree under test (pre-existing .state)" \
    1 "$SRC_STATE_PRE"
fi

# ─── summary ──────────────────────────────────────────────────────────────────

printf '\n1..%d — %d passed, %d failed\n' "$N" "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ] || exit 1
exit 0
