# routines

```stele
kind: container
purpose: The routine documents — frontmatter (schedule, timeout, requires, on_error), prose written as the error-handler's context, fenced sh blocks as the mechanics.
invariants:
  - claim: blocks are idempotent — a catch-retry, a cache eviction, or an overlapping tick can each double-fire; the lock serializes, idempotency absorbs
    anchor: routines/heartbeat.md#heartbeat
```

<!-- stele:begin router -->
<!-- stele:end -->
