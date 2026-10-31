# routines

```stele
kind: system
purpose: Executable markdown routines in $ROUTINE_HOME (default ~/.routines) — bin/routine runs a doc's sh blocks under schedule, lock, timeout; claude repairs failures; launchd and CI tick into run --due.
commands:
  check: mise run check
  test: /bin/bash test/routine.test.sh
```

## Working here

The design is `docs/index.html`; `bin/routine help` lists the verbs, exit codes and env.

- A routine is `$ROUTINE_HOME/<name>.md` (default `~/.routines`, the separate jgeschwendt/.routines repo), `<name>` matching `[a-z0-9][a-z0-9-]*`. Frontmatter: `schedule` (5-field cron, UTC; omit ⇒ on-demand only), `timeout` (seconds for the whole routine, default 600), `requires` (space-separated env vars that must be non-empty), `on_error` (`claude` default | `fail`; anything else exits 64 before a block runs). Only `sh`/`bash` fences run, each its own `bash -e` child, cwd the routines home, `ROUTINE_NAME` and `ROUTINE_STATE_DIR` exported.
- Wiring: `bin/routine install` loads the launchd tick `dev.gigantic.routines.due` (labels follow `dev.gigantic.<project>[.<job>]`); the Rust port's `routine setup [--remove]` adds hotswap's `dev.gigantic.routines.upgrade` (install and uninstall alias it there) and `routine upgrade` swaps releases through hotswap (gigantic.dev/hotswap/), live from the stage-6 packslip release; `bin/routine sync-ci` writes `.github/workflows/routines.yaml`, setting `ROUTINE_HOME` to the repo's `routines/`, caching `routines/.state`, fetching `bin/routine` when the repo carries none. Both end at `routine run --due`, which exits 1 when any due routine failed.
- Dark wakes: `run --due` probes `pmset -g systemstate` and, with no `Graphics` capability, logs `defer dark-wake` to `tick.log` and exits. `ROUTINE_PMSET` overrides the probe; unreachable or unparseable fails open (tests, non-macOS hosts).
- State, under `$ROUTINE_STATE/<name>/` and never committed, bounds itself: `lock/` (and `lock.reap/`, held only while a stale lock is reaped), `last-run.json`, `runs.jsonl`, `logs/` (a log goes at 15 days, find's `-mtime +14`), `lock-blocked.log` and a `degraded` marker when either fires; `$ROUTINE_STATE/tick.log` journals each sweep; `due.log`, the tick's stdout, rotates by copy-truncate into `due.log.1`.
- Env defaults: `ROUTINE_HOME` `$HOME/.routines` · `ROUTINE_DIR` and `ROUTINE_STATE` derive from it (`.state/`) · seams `ROUTINE_AGENTS_DIR` (`~/Library/LaunchAgents`), `ROUTINE_LAUNCHCTL`, `ROUTINE_CLAUDE`, `ROUTINE_NOW` (freezes the clock), `ROUTINE_PMSET`.

<!-- @stele -->

## Hazards (2 active)

- ⚠ `bin`: macOS /bin/bash is frozen at 3.2.57 — no mapfile, declare -A, ${var,,}, negative substrings, or wait -n anywhere in this script (→ ※ bash-3-2)
- ⚠ `bin`: no process substitution — Seatbelt (the shift sandbox) denies the /dev/fd paths >(…) opens, so the run log tees through fifos in RUN_TMP (→ ※ no-process-substitution)

## Map

| node     | kind      | purpose                                                                                                                                                                                                 | unfold                                                 |
| -------- | --------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| bin      | container | The only runner — one bash-3.2-safe, zero-dependency script; every scheduler (launchd tick, CI cron, a shell) ends at routine run.                                                                      | `stele unfold bin` · or read `bin/AGENTS.md`           |
| routines | container | This repo's own routines — the CI convention, where a repo's routines/ dir is its ROUTINE_HOME. A document is frontmatter, prose as the error-handler's context, and fenced sh blocks as the mechanics. | `stele unfold routines` · or read `routines/AGENTS.md` |
| src      | container | the Rust port of bin/routine — doc, cron, state, status, sync-ci, setup, upgrade (hotswap), run <name> ported; the due sweep unwired; unsafe only in src/sys.rs; bin/routine runs until cutover         | `stele unfold src` · or read `src/AGENTS.md`           |
| test     | container | Hermetic contract suite — sandboxed state dirs, stubbed claude and launchctl, runs under /bin/bash 3.2 in about thirty seconds, nearly all of it timeout and watchdog-grace sleeps.                     | `stele unfold test` · or read `test/AGENTS.md`         |
| tests    | container | end-to-end suite — `routine.rs` diffs the built binary's help, status, run <name> (gates, catch, signals, timeouts), sync-ci and tick plist against `bin/routine`; setup and upgrade run on stubs       | `stele unfold tests` · or read `tests/AGENTS.md`       |

## Indexes

All invariants: `.stele/index/invariants.md` · all hazards: `.stele/index/hazards.md`

## Engine

`stele` CLI available → `stele root | unfold <id> | invariants --touching <path> | hazards | nodes --kind <k>`. MCP: `stele serve`.
No engine → everything above is complete; nested AGENTS.md files carry the detail (nearest file wins).
<!-- @end -->
