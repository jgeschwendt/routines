# src

```stele
kind: container
purpose: the Rust port of bin/routine, stage 2 of the strangler — doc, cron, state reading and status ported, status byte-identical under the bash suite; bin/routine stays the runner until the cutover
```

Accepted divergences from bin/routine:

- a leading-zero number is decimal, where bash reads `08`/`09` as bad octal
- a debug build panics on i64 overflow near `i64::MAX`, where bash wraps (a release build wraps alike)
- bytes that aren't UTF-8 in docs and state read lossily — the runner writes ASCII
- a non-integer `ROUTINE_NOW` exits 64, where bash's arithmetic error names its own line
- an unreadable doc reads as empty, without awk's stderr line

<!-- @stele -->
<!-- @end -->
