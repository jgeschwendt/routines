# src

```stele
kind: container
purpose: the Rust port of bin/routine — doc, cron, state, status (byte-identical) and sync-ci ported; run's lock, record, gates, catch prompt and run --due's sweep unwired; bin/routine runs until cutover
```

Accepted divergences from bin/routine:

- a leading-zero number is decimal, where bash reads `08`/`09` as bad octal
- a debug build panics on i64 overflow near `i64::MAX`, where bash wraps (a release build wraps alike)
- bytes that aren't UTF-8 in docs and state read lossily — the runner writes ASCII
- a non-integer `ROUTINE_NOW` exits 64, where bash's arithmetic error names its own line; a blank-padded or `+`-signed one is written normalised, where bash writes it raw
- an unreadable doc reads as empty, without awk's stderr line
- a lock pid that isn't bare digits is no holder, where bash's builtin kill also takes a signed or blank-padded live pid
- a `requires` name holding `=` (`A=B`) reads as getenv does, the part before the `=` (`A`), where printenv finds an `A=B=…` entry
- `requires: --` reads getenv, where macOS `printenv --` prints the whole environment and passes
- pmset output holding a NUL is read as text, where BSD grep calls it binary and the tick defers — real pmset writes none
- sync-ci's failed write warns `routine: <path>: <io error>`, where bash's redirection names its own line
- the lock's minute and the logs' 14 days are elapsed whole seconds from std's clock, as macOS find floors them; GNU find agrees on `-mtime`, and sub-second boundaries may differ

<!-- @stele -->
<!-- @end -->
