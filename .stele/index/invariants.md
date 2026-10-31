# Invariants

| claim | node | anchor |
| --- | --- | --- |
| blocks are idempotent — a catch-retry, a cache eviction, or an overlapping tick can each double-fire; the lock serializes, idempotency absorbs | routines | routines/heartbeat.md#heartbeat |
| a failed block gets exactly one claude repair and one deterministic retry — the retry is the verdict, never the agent's exit code | src | ※ catch-retry |
| run --due never dispatches during a macOS dark wake — the tick probes once before it dispatches, logs defer dark-wake with what stayed due and stops, and derived due-ness carries the routines to the next real wake | src | ※ dark-wake-gate |
| due-ness is derived — next-fire-after(last started run, schedule) ≤ now, evaluated in UTC — never matched against the current minute | src | ※ due-derivation |
| run --due prints nothing when nothing is due — it fires every 60 seconds from launchd, so silence is the contract | src | ※ due-tick |
| last-run.json is written on every exit path a run that holds the lock takes, a signal's included, and a 75 lock refusal never clobbers the owner's state | src | ※ last-run-contract |
| run --due dispatches every due routine at once and waits for all — parallel dispatch unblocks routines within one sweep, but the tick lives until its slowest routine exits and launchd fires no new tick meanwhile, so one long run still holds every later sweep; a signal to the tick reaches every routine as TERM | src | ※ parallel-due |
