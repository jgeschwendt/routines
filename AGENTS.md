# routines

```stele
kind: system
purpose: Executable markdown routines — bin/routine runs a document's fenced sh blocks under schedule, lock, and timeout; claude repairs failures; launchd and CI are dumb ticks ending at run --due.
commands:
  test: /bin/bash test/routine.test.sh
```

<!-- stele:begin router -->

## Hazards (1 active)

- ⚠ `bin`: macOS /bin/bash is frozen at 3.2.57 — no mapfile, declare -A, ${var,,}, negative substrings, or wait -n anywhere in this script (→ lm:bash-3-2)

## Map

| node     | kind      | purpose                                                                                                                                                       | unfold                                                 |
| -------- | --------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| bin      | container | The only runner — one bash-3.2-safe, zero-dependency script; every scheduler (launchd tick, CI cron, a shell) ends at routine run.                            | `stele unfold bin` · or read `bin/AGENTS.md`           |
| routines | container | The routine documents — frontmatter (schedule, timeout, requires, on_error), prose written as the error-handler's context, fenced sh blocks as the mechanics. | `stele unfold routines` · or read `routines/AGENTS.md` |
| test     | container | Hermetic contract suite — sandboxed state dirs, stubbed claude and launchctl, runs under /bin/bash 3.2 in about six seconds.                                  | `stele unfold test` · or read `test/AGENTS.md`         |

## Indexes

All invariants: `.stele/index/invariants.md` · all hazards: `.stele/index/hazards.md`

## Engine

`stele` CLI available → `stele root | unfold <id> | invariants --touching <path> | hazards | nodes --kind <k>`. MCP: `stele serve`.
No engine → everything above is complete; nested AGENTS.md files carry the detail (nearest file wins).
<!-- stele:end -->
