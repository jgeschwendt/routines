# Hazards

| claim | node | anchor |
| --- | --- | --- |
| macOS /bin/bash is frozen at 3.2.57 — no mapfile, declare -A, ${var,,}, negative substrings, or wait -n anywhere in this script | bin | ※ bash-3-2 |
| no process substitution — Seatbelt (the shift sandbox) denies the /dev/fd paths >(…) opens, so the run log tees through fifos in RUN_TMP | bin | ※ no-process-substitution |
