# Invariants

| claim | node | anchor |
| --- | --- | --- |
| a failed block gets exactly one claude repair and one deterministic retry — the retry is the verdict, never the agent's exit code | bin | ※ catch-retry |
| run --due never dispatches during a macOS dark wake — the tick probes once before it dispatches, logs defer dark-wake with what stayed due and stops, and derived due-ness carries the routines to the next real wake | bin | ※ dark-wake-gate |
| due-ness is derived — next-fire-after(last started run, schedule) ≤ now, evaluated in UTC — never matched against the current minute | bin | ※ due-derivation |
| run --due prints nothing when nothing is due — it fires every 60 seconds from launchd, so silence is the contract | bin | ※ due-tick |
| last-run.json is written on every exit path via the EXIT trap, and a 75 lock refusal never clobbers the owner's state | bin | ※ last-run-contract |
| run --due dispatches every due routine at once and waits for all — parallel dispatch unblocks routines within one sweep, but the tick lives until its slowest routine exits and launchd fires no new tick meanwhile, so one long run still holds every later sweep; a signal to the tick reaches every routine as TERM | bin | ※ parallel-due |
| blocks are idempotent — a catch-retry, a cache eviction, or an overlapping tick can each double-fire; the lock serializes, idempotency absorbs | routines | routines/heartbeat.md#heartbeat |
