# routines

```stele
kind: container
purpose: This repo's own routines — the CI convention, where a repo's routines/ dir is its ROUTINE_HOME. A document is frontmatter, prose as the error-handler's context, and fenced sh blocks as the mechanics.
invariants:
  - claim: blocks are idempotent — a catch-retry, a cache eviction, or an overlapping tick can each double-fire; the lock serializes, idempotency absorbs
    anchor: routines/heartbeat.md#heartbeat
```

<!-- stele:begin router -->
<!-- stele:end -->
