# Invariants

| claim | node | anchor |
| --- | --- | --- |
| a failed block gets exactly one claude repair and one deterministic retry — the retry is the verdict, never the agent's exit code | bin | lm:catch-retry |
| run --due never dispatches during a macOS dark wake — the tick logs defer dark-wake and exits 0, and derived due-ness carries the routines to the next real wake | bin | lm:dark-wake-gate |
| due-ness is derived — next-fire-after(last started run, schedule) ≤ now, evaluated in UTC — never matched against the current minute | bin | lm:due-derivation |
| run --due prints nothing when nothing is due — it fires every 60 seconds from launchd, so silence is the contract | bin | lm:due-tick |
| last-run.json is written on every exit path via the EXIT trap, and a 75 lock refusal never clobbers the owner's state | bin | lm:last-run-contract |
| blocks are idempotent — a catch-retry, a cache eviction, or an overlapping tick can each double-fire; the lock serializes, idempotency absorbs | routines | routines/heartbeat.md#heartbeat |
