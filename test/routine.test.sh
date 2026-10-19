#!/bin/bash
# routine.test.sh — contract suite for bin/routine (docs/index.html).
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
PMSET="$SANDBOX/bin/pmset"             # ROUTINE_PMSET — a full wake, so no test ever defers
PMSET_DARK="$SANDBOX/bin/pmset-dark"   # the same probe reporting a Power Nap dark wake
LCTL_LOG="$SANDBOX/launchctl.log"
CLAUDE_ARGV="$SANDBOX/claude.argv"     # one arg per line
CLAUDE_FLAGS="$SANDBOX/claude.flags"   # argv minus the -p value, space-joined
CLAUDE_PROMPT="$SANDBOX/claude.prompt"
CLAUDE_CALLED="$SANDBOX/claude.called"
CLAUDE_ENV="$SANDBOX/claude.env"       # the ROUTINE_* vars the catch agent sees
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

# `pmset -g systemstate` — a full wake lists Graphics, a dark wake never does.
# The real probe is never called: every invocation path exports ROUTINE_PMSET.
cat > "$PMSET" <<'EOF'
#!/bin/bash
printf 'Current System Capabilities are: CPU Graphics Audio Network \n'
printf 'Current Power State: 4\n'
EOF
chmod +x "$PMSET"

cat > "$PMSET_DARK" <<'EOF'
#!/bin/bash
printf 'Current System Capabilities are: CPU Network \n'
printf 'Current Power State: 4\n'
EOF
chmod +x "$PMSET_DARK"

write_claude_stub() { # $1=mode: record | repair
  cat > "$CLAUDE" <<EOF
#!/bin/bash
: > "$CLAUDE_CALLED"
: > "$CLAUDE_ARGV"
printf 'name=%s\nstate_dir=%s\nhome=%s\n' "\${ROUTINE_NAME:-}" "\${ROUTINE_STATE_DIR:-}" "\${ROUTINE_HOME:-}" > "$CLAUDE_ENV"
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
    export ROUTINE_PMSET="$PMSET"
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
    export ROUTINE_PMSET="$PMSET"
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
echo SLOW-OUTPUT
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

mkfix "$SANDBOX/r-star/star.md" <<EOF
---
schedule: 0 0 */2 * 1
---

\`\`\`sh
printf 'x\n' >> "$SANDBOX/star.runs"
\`\`\`
EOF

mkfix "$SANDBOX/r-leap/leap.md" <<EOF
---
schedule: 30 2 29 2 *
---

\`\`\`sh
printf 'x\n' >> "$SANDBOX/leap.runs"
\`\`\`
EOF

# CRLF line ends and a YAML-quoted schedule — the spelling an editor or a YAML habit hands over
printf -- '---\r\nschedule: "0 * * * *"\r\n---\r\n\r\n```sh\r\nprintf x >> "%s/quoted.runs"\r\n```\r\n' \
  "$SANDBOX" | mkfix "$SANDBOX/r-quoted/quoted.md"

mkfix "$RDIR/fences.md" <<EOF
# Fences

~~~markdown
\`\`\`sh
printf 'example\n' >> "$SANDBOX/fences.txt"
\`\`\`
~~~

  \`\`\`\`\` \`\`\`\`sh \`\`\`\`\`; an inline span, never an opener

\`\`\`\`sh
cat >> "$SANDBOX/fences.txt" <<'X'
\`\`\`
X
\`\`\`\`

1. item

   \`\`\`bash
   printf 'indented\n' >> "$SANDBOX/fences.txt"
   \`\`\`
EOF

mkfix "$SANDBOX/r-dark/dark.md" <<EOF
---
schedule: 0 * * * *
---

# Dark

\`\`\`sh
printf 'x\n' >> "$SANDBOX/dark.runs"
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

runr run fences
assert_eq "~~~ and \`\`\`\` fences hold a \`\`\` line; an indented fence runs dedented" '```
indented' "$(cat "$SANDBOX/fences.txt" 2>/dev/null)"

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
  export ROUTINE_PMSET="$PMSET"
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
assert_missing "the log tee opens no /dev/fd path — Seatbelt denies it" "$(cat "$RUNNER")" '> >('

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
assert_contains "a timed-out block's output still prints" "$OUT" "SLOW-OUTPUT"
assert_contains "a timed-out block's exit divider still prints" "$OUT" "block 1 · exit 143"
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
assert_contains "the prompt forbids a needs-input/failed last-message line" "$PROMPT" \
  'Never start a line of your final message with `needs input:` or `failed:`'
assert_contains "the prompt points at the doc's stable thread keys" "$PROMPT" '`ci:<repo>:<sha7>`'
CENV=$(cat "$CLAUDE_ENV" 2>/dev/null)
assert_contains "the catch agent sees ROUTINE_NAME" "$CENV" "name=repair-yes"
assert_contains "the catch agent sees ROUTINE_STATE_DIR" "$CENV" "state_dir=$RSTATE/repair-yes"
assert_contains "the catch agent sees ROUTINE_HOME" "$CENV" "home=$RHOME"
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

# ─── run-path hazards ─────────────────────────────────────────────────────────

bgr() { # $1=name — runr in the background; the runner's rc lands in $SANDBOX/$1.rc
  ( O="$SANDBOX/$1.out"; E="$SANDBOX/$1.err"; runr run "$1"; printf '%s\n' "$RC" > "$SANDBOX/$1.rc" ) &
}

assert_gone() { # $1=desc $2=pid file — polls up to 3s for the pid to exit
  local i=0 p
  p=$(cat "$2")
  while kill -0 "$p" 2>/dev/null; do
    [ "$i" -lt 30 ] || { nok "$1" "pid $p survived"; return; }
    sleep 0.1; i=$((i + 1))
  done
  ok "$1"
}

# A TERM-ignoring group member outlives its leader; the watchdog must poll the
# group and escalate. Its 10s grace runs in the background under the cases below.
mkfix "$RDIR/stubborn.md" <<EOF
---
timeout: 1
on_error: fail
---
\`\`\`sh
sh -c 'trap "" TERM; echo \$\$ > "$SANDBOX/stubborn.pid"; exec sleep 30' &
wait
\`\`\`
EOF
bgr stubborn; STUBBORN=$!

mkfix "$RDIR/sig.md" <<EOF
---
on_error: fail
---
\`\`\`sh
sh -c 'trap "" TERM; echo \$\$ > "$SANDBOX/sig.pid"; exec sleep 30' &
wait
\`\`\`
EOF
for s in HUP:129 TERM:143; do
  rm -f "$SANDBOX/sig.pid"
  mkdir -p "$RSTATE/sig"; : > "$RSTATE/sig/degraded"
  bgr sig
  wait_grep "$SANDBOX/sig.pid" ""
  kill -"${s%:*}" "$(cat "$RSTATE/sig/lock/pid")"; wait $!
  assert_eq "SIG${s%:*} exits ${s#*:}" "${s#*:}" "$(cat "$SANDBOX/sig.rc")"
  assert_contains "SIG${s%:*} records exit ${s#*:}" "$(cat "$(lastrun sig)")" "\"exit\":${s#*:}"
  assert_file "SIG${s%:*} keeps the degraded marker" "$RSTATE/sig/degraded"
  assert_gone "SIG${s%:*} kills the block's TERM-ignoring member" "$SANDBOX/sig.pid"
done

printf '#!/bin/bash\nsleep 30\n' > "$SANDBOX/bin/claude-slow"; chmod +x "$SANDBOX/bin/claude-slow"
mkfix "$RDIR/catch-late.md" <<EOF
---
timeout: 1
---
\`\`\`sh
[ ! -f "$SANDBOX/catch-late.first" ] || { : > "$SANDBOX/catch-late.retried"; exit 0; }
: > "$SANDBOX/catch-late.first"; exit 3
\`\`\`
EOF
EXPORTS="ROUTINE_CLAUDE=$SANDBOX/bin/claude-slow"
runr run catch-late
EXPORTS=""
assert_eq "a deadline inside the catch exits 124" 124 "$RC"
assert_no_file "a deadline inside the catch skips the retry" "$SANDBOX/catch-late.retried"

mkfix "$RDIR/octal.md" <<'EOF'
---
timeout: 08
on_error: fail
---
```sh
true
```
EOF
runr run octal
assert_eq "a zero-padded timeout is decimal" 0 "$RC"

# an empty pid is a holder between mkdir and its pid write
mkdir -p "$RSTATE/locked/lock"
runr run locked
assert_eq "a fresh lock with no pid yet is held" 75 "$RC"
touch -t 202001010000 "$RSTATE/locked/lock"
runr run locked
assert_eq "an old lock with no pid is reaped" 0 "$RC"

sleep 5 & LOCK_HOLDER=$!
mkdir -p "$RSTATE/requires/lock"
printf '%s\n' "$LOCK_HOLDER" > "$RSTATE/requires/lock/pid"
REQ_BEFORE=$(cat "$(lastrun requires)")
runr run requires
assert_eq "the requires gate waits for the lock" 75 "$RC"
assert_eq "a 75 on requires leaves last-run.json to the holder" "$REQ_BEFORE" \
  "$(cat "$(lastrun requires)")"
kill "$LOCK_HOLDER" 2>/dev/null; wait "$LOCK_HOLDER" 2>/dev/null; LOCK_HOLDER=""
runr run requires
assert_contains "a 78 is recorded under the lock" "$(cat "$(lastrun requires)")" '"exit":78'

wait "$STUBBORN"
assert_eq "a TERM-ignoring block still times out 124" 124 "$(cat "$SANDBOX/stubborn.rc")"
assert_gone "the watchdog escalates to KILL on the block's group" "$SANDBOX/stubborn.pid"

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
T_DARK_NEXT=1786705260       # Fri 2026-08-14T11:01:00Z — an hour past T_HOURLY_DUE

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

T_STAR_RUN=1791763200        # Mon 2026-10-12T00:00:00Z
T_STAR_WED=1791936060        # Wed 2026-10-14T00:01:00Z — odd-free, not Monday
T_STAR_MON=1792368060        # Mon 2026-10-19T00:01:00Z — Monday on an odd day
RDIR="$SANDBOX/r-star"
RNOW="$T_STAR_RUN"
runr run --due
RNOW="$T_STAR_WED"
runr run --due
assert_eq "'0 0 */2 * 1' — a */N day field ANDs (cronie), so Wed 14th is not due" 1 \
  "$(nlines "$SANDBOX/star.runs")"
RNOW="$T_STAR_MON"
runr run --due
assert_eq "'0 0 */2 * 1' — Mon 19th satisfies both, due" 2 "$(nlines "$SANDBOX/star.runs")"

RDIR="$SANDBOX/r-leap"
RNOW=1835404260              # Tue 2028-02-29T02:31:00Z
runr run --due
RNOW=1961712000              # Mon 2032-03-01T00:00:00Z
runr run --due
assert_eq "'30 2 29 2 *' fires again four years on" 2 "$(nlines "$SANDBOX/leap.runs")"

RDIR="$SANDBOX/r-quoted"
for RNOW in "$T_HOURLY_RUN" "$T_HOURLY_PLUS600" "$T_HOURLY_DUE"; do runr run --due; done
assert_eq "a CRLF doc's quoted schedule is due on the hour, not before" 2 \
  "$(wc -c < "$SANDBOX/quoted.runs" | tr -d ' ')"

# ─── dark wake ────────────────────────────────────────────────────────────────

# A capabilities line without Graphics is a Power Nap dark wake: the tick defers
# rather than dispatching into a window that ends in seconds. Due-ness is derived,
# so the same routine is still due on the next probe.
RDIR="$SANDBOX/r-dark"
RNOW="$T_HOURLY_DUE"
EXPORTS="ROUTINE_PMSET=$PMSET_DARK"
runr run --due
EXPORTS=""
assert_eq "a dark wake defers ⇒ exit 0" 0 "$RC"
assert_eq "a deferred tick is silent on stdout" "" "$OUT"
assert_eq "a deferred tick is silent on stderr" "" "$ERR"
assert_eq "a dark wake dispatches nothing" 0 "$(nlines "$SANDBOX/dark.runs")"
assert_no_file "a deferred routine writes no last-run.json" "$(lastrun dark)"
assert_no_file "a deferred routine writes no logs" "$RSTATE/dark/logs"
assert_grep "the deferral names what stayed due" "$RSTATE/tick.log" \
  "defer dark-wake due=dark"

runr run --due
assert_eq "a full wake dispatches the still-due routine" 1 "$(nlines "$SANDBOX/dark.runs")"

# fail open: an unprobeable pmset is never read as a dark wake.
RNOW="$T_DARK_NEXT"
EXPORTS="ROUTINE_PMSET=/nonexistent/pmset"
runr run --due
EXPORTS=""
assert_eq "an absent pmset fails open and dispatches" 2 "$(nlines "$SANDBOX/dark.runs")"

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
RNOW_WAS="$RNOW"; RNOW=1  # before every recorded run — an hour crossing makes nothing due
runr run --due
RNOW="$RNOW_WAS"
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
assert_contains "plist stamps a PATH — the tick inherits none" "$PL" "<key>PATH</key>"
assert_contains "plist PATH carries ~/.local/bin (claude, mise, sandman, stele)" \
  "$PL" "$HOME/.local/bin:"
assert_missing "plist never cds into a repo — the runner cds itself" "$PL" "&amp;&amp;"
LC=$(cat "$LCTL_LOG" 2>/dev/null)
assert_contains "install boots the old agent out" "$LC" "bootout gui/$UID_NOW/com.routines.due"
assert_contains "install bootstraps the agent" "$LC" "bootstrap gui/$UID_NOW"
assert_eq "bootout precedes bootstrap" 1 "$(grep -n bootout "$LCTL_LOG" | head -1 | cut -d: -f1)"

FRESH_HOME="$SANDBOX/fresh <&> 'home"
runr_home "$FRESH_HOME" install
assert_eq "install into an absent home exits 0" 0 "$RC"
assert_dir "install creates the routines home" "$FRESH_HOME"
assert_dir "install creates the derived state dir" "$FRESH_HOME/.state"
assert_contains "plist stamps the custom home, XML-escaped — the tick runs against it" \
  "$(cat "$PLIST" 2>/dev/null)" "<key>ROUTINE_HOME</key><string>$SANDBOX/fresh &lt;&amp;&gt; 'home</string>"

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
WF="$RCWD/.github/workflows/routines.yaml"
mkdir -p "$RCWD/.github/workflows" && : > "$RCWD/.github/workflows/routines.yml"
runr sync-ci
assert_no_file "sync-ci removes the legacy routines.yml" "$RCWD/.github/workflows/routines.yml"
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
assert_contains "workflow runs the due tick" "$WFC" "bin/routine run --due"
assert_contains "workflow saves state even when a routine fails" "$WFC" "actions/cache/save@v4
        if: always()"
assert_contains "workflow ticks never overlap — runners share no lock" "$WFC" "concurrency:
  cancel-in-progress: false
  group: routines"
assert_no_file "sync-ci targets its cwd, never the routines home" "$RHOME/.github"
if [ "$SRC_STATE_PRE" = 0 ]; then
  assert_no_file "sync-ci never writes into the source tree under test" \
    "$SRC_REPO/.state"
else
  assert_eq "sync-ci never writes into the source tree under test (pre-existing .state)" \
    1 "$SRC_STATE_PRE"
fi

# ─── edge inputs ──────────────────────────────────────────────────────────────

mkfix "$RDIR/comment.md" <<EOF
<!--->
<!-- a one-line comment ends where it opens -->
<!--
\`\`\`sh
: > "$SANDBOX/commented.marker"
\`\`\`
-->
<!-->
\`\`\`sh
: > "$SANDBOX/uncommented.marker"
\`\`\`
EOF
runr run comment
assert_no_file "a fence inside an HTML comment never runs" "$SANDBOX/commented.marker"
assert_file "a fence after the comment closes runs" "$SANDBOX/uncommented.marker"

printf -- '---\ntimeout: 0\non_error: fail\n---\n```sh\ntrue\n```\n' | mkfix "$RDIR/zero.md"
runr run zero
assert_eq "timeout: 0 falls back to the default, never an instant 124" 0 "$RC"

printf -- '---\nrequires: ROUTINE_TEST_REQ_A 2FA\non_error: fail\n---\n```sh\ntrue\n```\n' | mkfix "$RDIR/digit.md"
EXPORTS="ROUTINE_TEST_REQ_A=1"
runr run digit
EXPORTS=""
assert_eq "a requires name opening with a digit is missing, never the last var's value" 78 "$RC"

head -c 2000000 /dev/zero | tr '\0' x > "$SANDBOX/big.txt"
printf -- '---\non_error: fail\n---\n```sh\ncat "%s"\n```\n' "$SANDBOX/big.txt" | mkfix "$RDIR/big.md"
runr run big
BIGLOG="$RSTATE/big/logs/$(ls "$RSTATE/big/logs" | tail -1)"
assert_grep "a runaway block's log copy is cut, and says so" "$BIGLOG" "951424 bytes cut"
assert_eq "the cut keeps the last MiB" 1 "$(( $(wc -c < "$BIGLOG") < 1100000 ))"

# a live pid whose start time is not the holder's is a reused pid, not the holder;
# where ps cannot run (a Seatbelt that denies setuid binaries) the lock stays held
sleep 5 & LOCK_HOLDER=$!
mkdir -p "$RSTATE/locked/lock"
printf '%s\n' "$LOCK_HOLDER" > "$RSTATE/locked/lock/pid"
TZ=UTC0 ps -o lstart= -p "$LOCK_HOLDER" > "$RSTATE/locked/lock/started" 2>/dev/null
EXPORTS="TZ=America/Chicago"
runr run locked
EXPORTS=""
assert_eq "the holder's start time reads the same from any caller's zone" 75 "$RC"
PS_OK=0; [ -s "$RSTATE/locked/lock/started" ] && PS_OK=1
printf 'Thu Jan  1 00:00:00 1970\n' > "$RSTATE/locked/lock/started"
runr run locked
if [ "$PS_OK" = 1 ]; then
  assert_eq "a lock whose pid was reused is reaped" 0 "$RC"
else
  assert_eq "a lock whose pid's start time cannot be read stays held" 75 "$RC"
fi
kill "$LOCK_HOLDER" 2>/dev/null; wait "$LOCK_HOLDER" 2>/dev/null; LOCK_HOLDER=""
rm -rf "$RSTATE/locked/lock"

mkfix "$RDIR/pipe.md" <<EOF
\`\`\`sh
printf 'a\nb\n'; sleep 1
\`\`\`
\`\`\`sh
: > "$SANDBOX/pipe.marker"
\`\`\`
EOF
( export ROUTINE_HOME="$RHOME" ROUTINE_DIR="$RDIR" ROUTINE_STATE="$RSTATE" ROUTINE_PMSET="$PMSET"
  exec "$RUNNER" run pipe ) 2>/dev/null | head -1 > /dev/null
wait_grep "$(lastrun pipe)" '"exit":0'
assert_file "a reader that leaves early stops no block" "$SANDBOX/pipe.marker"
assert_contains "and the record stays JSON" "$(cat "$(lastrun pipe)")" '"exit":0'

rm -f "$SANDBOX/sig.pid"
bgr sig
wait_grep "$SANDBOX/sig.pid" ""
kill -TERM "$(cat "$RSTATE/sig/lock/pid")"; sleep 0.5; kill -TERM "$(cat "$RSTATE/sig/lock/pid")" 2>/dev/null; wait $!
assert_gone "a second signal does not cut the KILL escalation short" "$SANDBOX/sig.pid"
assert_no_file "nor leave the lock behind" "$RSTATE/sig/lock"

# ─── host events ──────────────────────────────────────────────────────────────

RNOW="$T_HOURLY_DUE"
mkfix "$SANDBOX/r-bad/at.md" <<EOF
---
schedule: "@daily"
---
\`\`\`sh
: > "$SANDBOX/at.marker"
\`\`\`
EOF
printf -- '---\nschedule: "* * * * *"\n---\n```sh\ntrue\n```\n' | mkfix "$SANDBOX/r-bad/--due.md"
RDIR="$SANDBOX/r-bad"
runr run --due
assert_no_file "a schedule the parser rejects never fires, not even once" "$SANDBOX/at.marker"
assert_grep "a document named like a flag is skipped, never dispatched as one" "$RSTATE/tick.log" "skip --due.md"
printf -- '---\nschedule: "0 9 */31 2 3"\n---\n```sh\ntrue\n```\n' | mkfix "$SANDBOX/r-bad/feb.md"
runr status
assert_contains "status flags the rejected schedule" "$OUT" "@daily           bad"
assert_missing "status lists no flag-named document" "$OUT" "--due"
assert_missing "a schedule whose next fire is years out is valid (Wed Feb 1 2034)" "$OUT" "0 9 */31 2 3     bad"

# launchd stops the tick with a TERM to the tick alone: the routine must still reap and record
for n in terma termb; do
  printf -- '---\nschedule: "* * * * *"\non_error: fail\n---\n```sh\necho $$ > "%s/%s.pid"; sleep 30\n```\n' \
    "$SANDBOX" "$n" | mkfix "$SANDBOX/r-term/$n.md"
done
( export ROUTINE_HOME="$RHOME" ROUTINE_DIR="$SANDBOX/r-term" ROUTINE_STATE="$RSTATE" ROUTINE_PMSET="$PMSET"
  exec "$RUNNER" run --due ) >/dev/null 2>&1 & DUEP=$!
wait_grep "$SANDBOX/terma.pid" ""; wait_grep "$SANDBOX/termb.pid" ""
kill -TERM "$DUEP"; wait "$DUEP"
for n in terma termb; do
  assert_contains "a TERM to the tick reaches $n, which records 143" "$(cat "$(lastrun "$n")" 2>/dev/null)" '"exit":143'
  assert_gone "and kills $n's block" "$SANDBOX/$n.pid"
done

# every due routine runs at once: each waits for the other's marker, so a serial
# sweep times the first out and fails it
for n in ova:ovb ovb:ova; do
  printf -- '---\nschedule: "0 * * * *"\non_error: fail\n---\n```sh\n: > "%s/%s.on"; i=0\nwhile [ ! -f "%s/%s.on" ] && [ $i -lt 30 ]; do sleep 0.1; i=$((i + 1)); done\n[ -f "%s/%s.on" ]\n```\n' \
    "$SANDBOX" "${n%:*}" "$SANDBOX" "${n#*:}" "$SANDBOX" "${n#*:}" | mkfix "$SANDBOX/r-overlap/${n%:*}.md"
done
RDIR="$SANDBOX/r-overlap"
runr run --due
assert_eq "two due routines overlap ⇒ both succeed" 0 "$RC"
assert_grep "the tick records each dispatch's exit" "$RSTATE/tick.log" "dispatched ovb exit=0"
RNOW=""
RDIR="$SANDBOX/routines"

# ─── summary ──────────────────────────────────────────────────────────────────

printf '\n1..%d — %d passed, %d failed\n' "$N" "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ] || exit 1
exit 0
