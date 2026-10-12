# test

```stele
kind: container
purpose: Hermetic contract suite — sandboxed state dirs, stubbed claude and launchctl, runs under /bin/bash 3.2 in about thirty-five seconds, nearly all of it timeout and watchdog-grace sleeps.
commands:
  test: /bin/bash test/routine.test.sh
```

<!-- @stele -->
<!-- @end -->
