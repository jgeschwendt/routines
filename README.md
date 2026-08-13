# routines

Executable markdown on rails. A routine is one markdown document — frontmatter
for the schedule, prose for judgment, fenced `sh` blocks for mechanics.
`bin/routine` runs the blocks in order with zero LLM involvement on the happy
path; when a block fails, the document itself becomes the prompt — claude
receives the full doc, the failing block, and its output, repairs state, and the
runner retries that block once. Every scheduler is a dumb tick that ends at the
same `routine run --due`: launchd locally, a cron'd workflow in CI, a human at a
shell. The runner is one bash script, 3.2-safe, with no dependencies beyond a
POSIX userland; claude is needed only when a catch fires.

```
try   { blocks, in order }          # cheap, fast, CI-safe
catch { claude("handle $error") }   # the doc's prose is the handler's context
```

## The document

`routines/<name>.md`, name `[a-z0-9-]+`:

````markdown
---
schedule: 30 7 * * 1-5    # 5-field cron, evaluated in UTC; omit ⇒ on-demand only
timeout: 600              # seconds for the whole routine, default 600
requires: GITHUB_TOKEN    # space-separated env vars that must be non-empty
on_error: claude          # claude (default) | fail
---

# Morning brief

Prose is not decoration — it is the context the error handler inherits: intent,
traps, judgment calls. Write it for the agent that arrives when the block below
has just failed.

```sh
gh api notifications --paginate > "$ROUTINE_STATE_DIR/inbox.json"
```
````

Only ` ```sh ` and ` ```bash ` fences execute, sequentially, each as its own
`bash -e` child with cwd at the repo root and `ROUTINE_NAME` /
`ROUTINE_STATE_DIR` exported. Other fences are material for the prose. Blocks
must be idempotent — a catch-retry, a cache eviction, or an overlapping tick can
each double-fire; the lock serializes, idempotency absorbs.

## Verbs

```
routine run <name>     run one document
routine run --due      run every due routine — silent when none are
routine status         schedule × due? × last run, one row per document
routine install        install the launchd tick agent (com.routines.due)
routine uninstall      remove it
routine sync-ci        (re)write .github/workflows/routines.yml
routine help           usage
```

Exit codes: `64` usage or unknown routine · `75` lock held · `78` requires
missing · `124` timeout · otherwise the failing block's own code. `run --due`
exits `1` when any due routine failed.

## Scheduling

A routine is due when `next-fire-after(last started run, schedule) ≤ now`; no
`last-run.json` means due. Due-ness is derived from the last run rather than
matched against the current minute, so a delayed CI tick or a sleeping laptop
catches up instead of skipping.

```sh
bin/routine install    # launchd: StartInterval 60 + RunAtLoad, running run --due
bin/routine sync-ci    # GitHub Actions: */15 cron + workflow_dispatch, .state cached
```

Run state lives in `.state/<name>/` — `lock/`, `last-run.json`, `logs/<ts>.log`
— and is never committed; CI persists it through `actions/cache` only.

## Environment

| variable | default |
| --- | --- |
| `ROUTINE_REPO` | resolved from the script path |
| `ROUTINE_DIR` | `$ROUTINE_REPO/routines` |
| `ROUTINE_STATE` | `$ROUTINE_REPO/.state` |
| `ROUTINE_AGENTS_DIR` | `$HOME/Library/LaunchAgents` |
| `ROUTINE_LAUNCHCTL` | `launchctl` |
| `ROUTINE_CLAUDE` | `claude` |
| `ROUTINE_NOW` | `date +%s` — freezes the runner's clock, making due-ness testable |

Design and rejected alternatives: [docs/PLAN.md](docs/PLAN.md).
