# bin

```stele
kind: container
purpose: The only runner — one bash-3.2-safe, zero-dependency script; every scheduler (launchd tick, CI cron, a shell) ends at routine run.
invariants:
  - claim: due-ness is derived — next-fire-after(last started run, schedule) ≤ now, evaluated in UTC — never matched against the current minute
    anchor: ※ due-derivation
  - claim: last-run.json is written on every exit path via the EXIT trap, and a 75 lock refusal never clobbers the owner's state
    anchor: ※ last-run-contract
  - claim: a failed block gets exactly one claude repair and one deterministic retry — the retry is the verdict, never the agent's exit code
    anchor: ※ catch-retry
  - claim: run --due prints nothing when nothing is due — it fires every 60 seconds from launchd, so silence is the contract
    anchor: ※ due-tick
  - claim: run --due never dispatches during a macOS dark wake — the tick logs defer dark-wake and exits 0, and derived due-ness carries the routines to the next real wake
    anchor: ※ dark-wake-gate
hazards:
  - claim: macOS /bin/bash is frozen at 3.2.57 — no mapfile, declare -A, ${var,,}, negative substrings, or wait -n anywhere in this script
    anchor: ※ bash-3-2
```

<!-- @stele -->
<!-- @end -->
