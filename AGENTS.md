# routines

```stele
kind: system
purpose: Executable markdown routines in $ROUTINE_HOME (default ~/.routines) — `routine` runs a doc's sh blocks under schedule, lock, timeout; claude repairs failures; launchd and CI tick into run --due.
commands:
  check: mise run check
  test: cargo test
```

## Working here

The design is `docs/index.html`; `routine help` lists the verbs, exit codes and env.

- A routine is `$ROUTINE_HOME/<name>.md` (default `~/.routines`, the separate jgeschwendt/.routines repo), `<name>` matching `[a-z0-9][a-z0-9-]*`. Frontmatter: `schedule` (5-field cron, UTC; omit ⇒ on-demand only), `timeout` (seconds for the whole routine, default 600), `requires` (space-separated env vars that must be non-empty), `on_error` (`claude` default | `fail`; anything else exits 64 before a block runs). Only `sh`/`bash` fences run, each its own `bash -e` child, cwd the routines home, `ROUTINE_NAME` and `ROUTINE_STATE_DIR` exported.
- Install and wiring: `mise use -g packslip:jgeschwendt/routines` installs the release (`.github/workflows/release.yaml`: one per merge to main, `YYYY.M.N`, aarch64-apple-darwin and x86_64-unknown-linux-gnu, signed by packslip); `routine setup [--remove]` links `~/.local/bin/routine` to mise's shim and loads the launchd tick `dev.gigantic.routines.due` (`<shims>/routine run --due` every 60 s; labels follow `dev.gigantic.<project>[.<job>]`) and hotswap's `dev.gigantic.routines.upgrade` (`routine upgrade` every 900 s, gigantic.dev/hotswap/; record `~/.local/state/hotswap/routines.json`); `install`/`uninstall` alias it. `routine sync-ci` writes `.github/workflows/routines.yaml`, this repo's own tick compiled in: `ROUTINE_HOME` the repo's `routines/`, `routines/.state` cached, the runner fetched by `mise x packslip:jgeschwendt/routines@latest`. Both end at `routine run --due`, which exits 1 when any due routine failed.
- Dark wakes: `run --due` probes `pmset -g systemstate` and, with no `Graphics` capability, logs `defer dark-wake` to `tick.log` and exits. `ROUTINE_PMSET` overrides the probe; unreachable or unparseable fails open (tests, non-macOS hosts).
- State, under `$ROUTINE_STATE/<name>/` and never committed, bounds itself: `lock/` (and `lock.reap/`, held only while a stale lock is reaped), `last-run.json`, `runs.jsonl`, `logs/` (a log goes at 15 days, find's `-mtime +14`), `lock-blocked.log` and a `degraded` marker when either fires; `$ROUTINE_STATE/tick.log` journals each sweep; `due.log`, the tick's stdout, rotates by copy-truncate into `due.log.1`.
- Env defaults: `ROUTINE_HOME` `$HOME/.routines` · `ROUTINE_DIR` and `ROUTINE_STATE` derive from it (`.state/`) · seams `ROUTINE_AGENTS_DIR` (`~/Library/LaunchAgents`), `ROUTINE_LAUNCHCTL`, `ROUTINE_CLAUDE`, `ROUTINE_NOW` (freezes the clock), `ROUTINE_PMSET`.

<!-- @stele -->

## Hazards (0 active)


## Map

| node     | kind      | purpose                                                                                                                                                                                                 | unfold                                                 |
| -------- | --------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| routines | container | This repo's own routines — the CI convention, where a repo's routines/ dir is its ROUTINE_HOME. A document is frontmatter, prose as the error-handler's context, and fenced sh blocks as the mechanics. | `stele unfold routines` · or read `routines/AGENTS.md` |
| src      | container | the runner — doc, cron, state, status, run <name>, run --due, sync-ci, setup and upgrade (hotswap); one binary crate, std plus hotswap, unsafe only in src/sys.rs                                       | `stele unfold src` · or read `src/AGENTS.md`           |
| tests    | container | end-to-end suite — `routine.rs` runs the built binary on every verb against sandboxed homes and stubbed claude, launchctl, mise and pmset, asserting exit codes, output and state literally             | `stele unfold tests` · or read `tests/AGENTS.md`       |

## Indexes

All invariants: `.stele/index/invariants.md` · all hazards: `.stele/index/hazards.md`

## Engine

`stele` CLI available → `stele root | unfold <id> | invariants --touching <path> | hazards | nodes --kind <k>`. MCP: `stele serve`.
No engine → everything above is complete; nested AGENTS.md files carry the detail (nearest file wins).
<!-- @end -->
