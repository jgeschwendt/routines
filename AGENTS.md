# routines

```stele
kind: system
purpose: Executable markdown routines in $ROUTINE_HOME (default ~/.routines) — bin/routine runs a doc's sh blocks under schedule, lock, timeout; claude repairs failures; launchd and CI tick into run --due.
commands:
  test: /bin/bash test/routine.test.sh
```

<!-- stele:begin router -->

## Hazards (1 active)

- ⚠ `bin`: macOS /bin/bash is frozen at 3.2.57 — no mapfile, declare -A, ${var,,}, negative substrings, or wait -n anywhere in this script (→ lm:bash-3-2)

## Map

| node     | kind      | purpose                                                                                                                                                                                                 | unfold                                                 |
| -------- | --------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| bin      | container | The only runner — one bash-3.2-safe, zero-dependency script; every scheduler (launchd tick, CI cron, a shell) ends at routine run.                                                                      | `stele unfold bin` · or read `bin/AGENTS.md`           |
| routines | container | This repo's own routines — the CI convention, where a repo's routines/ dir is its ROUTINE_HOME. A document is frontmatter, prose as the error-handler's context, and fenced sh blocks as the mechanics. | `stele unfold routines` · or read `routines/AGENTS.md` |
| test     | container | Hermetic contract suite — sandboxed state dirs, stubbed claude and launchctl, runs under /bin/bash 3.2 in about six seconds.                                                                            | `stele unfold test` · or read `test/AGENTS.md`         |

## Indexes

All invariants: `.stele/index/invariants.md` · all hazards: `.stele/index/hazards.md`

## Engine

`stele` CLI available → `stele root | unfold <id> | invariants --touching <path> | hazards | nodes --kind <k>`. MCP: `stele serve`.
No engine → everything above is complete; nested AGENTS.md files carry the detail (nearest file wins).
<!-- stele:end -->
