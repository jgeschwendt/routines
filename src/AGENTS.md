# src

```stele
kind: container
purpose: the runner — doc, cron, state, status, run <name>, run --due, sync-ci, setup and upgrade (hotswap); one binary crate, std plus hotswap, unsafe only in src/sys.rs
invariants:
  - claim: due-ness is derived — next-fire-after(last started run, schedule) ≤ now, evaluated in UTC — never matched against the current minute
    anchor: ※ due-derivation
  - claim: last-run.json is written on every exit path a run that holds the lock takes, a signal's included, and a 75 lock refusal never clobbers the owner's state
    anchor: ※ last-run-contract
  - claim: a failed block gets exactly one claude repair and one deterministic retry — the retry is the verdict, never the agent's exit code
    anchor: ※ catch-retry
  - claim: run --due prints nothing when nothing is due — it fires every 60 seconds from launchd, so silence is the contract
    anchor: ※ due-tick
  - claim: run --due dispatches every due routine at once and waits for all — parallel dispatch unblocks routines within one sweep, but the tick lives until its slowest routine exits and launchd fires no new tick meanwhile, so one long run still holds every later sweep; a signal to the tick reaches every routine as TERM
    anchor: ※ parallel-due
  - claim: run --due never dispatches during a macOS dark wake — the tick probes once before it dispatches, logs defer dark-wake with what stayed due and stops, and derived due-ness carries the routines to the next real wake
    anchor: ※ dark-wake-gate
```

- the watchdog is a thread: a runner killed -9 takes it along and its block runs on to its own end; the dead pid's lock is reaped by the next run
- the runner writes the run log itself, stdout and stderr in the order printed

<!-- @stele -->
<!-- @end -->
