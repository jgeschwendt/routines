---
schedule: 0 * * * *
timeout: 60
on_error: fail
---

# Heartbeat

The smallest complete routine: it proves the tick reaches the runner and the run
log lands in `.state/heartbeat/`. Prose is the context an error handler inherits
— when the block below fails, the machine's clock or its `date` is the suspect,
never the routine, so a handler should report rather than repair.

```sh
date -u +%FT%TZ
```
