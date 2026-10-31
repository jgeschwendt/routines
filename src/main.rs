mod cron;
mod doc;
mod due;
mod lock;
mod record;
mod run;
mod setup;
mod state;
mod status;
mod sync_ci;
mod sys;

use std::process::ExitCode;

// Byte-identical to `bin/routine help` — tests/routine.rs diffs the two.
const USAGE: &str = "routine — executable markdown on rails

  routine run <name>     run one routine document
  routine run --due      run every due routine (silent when none are)
  routine status         schedule × due? × last run, one row per routine
  routine install        install the launchd tick agent (dev.gigantic.routines.due)
  routine uninstall      remove it
  routine setup          install plus the hotswap upgrade agent; --remove undoes both (Rust port)
  routine upgrade        swap to mise's latest release; --check, --back, --retry (Rust port)
  routine sync-ci        (re)write ./.github/workflows/routines.yaml here
  routine help           this text

exit: 64 usage/unknown routine, recorded for a bad doc or no home · 73 mktemp failed or state unwritable ·
      75 lock held · 78 requires missing · 124 timeout · 129/130/143 HUP/INT/TERM ·
      else the failing block's code · run --due 1 when any routine failed ·
      install/setup 1 when a plist cannot land or bootstrap fails · upgrade 1 red, 2 unable to try

env: ROUTINE_HOME ROUTINE_DIR ROUTINE_STATE ROUTINE_AGENTS_DIR
     ROUTINE_LAUNCHCTL ROUTINE_CLAUDE ROUTINE_NOW ROUTINE_PMSET
";

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let code = match &args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        [] | ["" | "help" | "-h" | "--help", ..] => {
            print!("{USAGE}");
            0
        }
        ["run"] | ["run", ""] => {
            eprint!("{USAGE}");
            64
        }
        [v @ ("status" | "run"), rest @ ..] => {
            match state::now() {
                Ok(now) if *v == "run" && rest[0] == "--due" => due::due(now),
                Ok(now) if *v == "run" => run::run(rest[0], now),
                Ok(now) => {
                    let (dir, states, _) = state::roots();
                    status::status(&dir, &states, now);
                    0
                }
                // bash evaluates it as arithmetic; an error there names the script's own line
                Err(v) => {
                    eprintln!("routine: ROUTINE_NOW={v} is not an integer");
                    64
                }
            }
        }
        ["sync-ci", ..] => {
            sync_ci::sync_ci();
            0
        }
        [v @ ("install" | "uninstall"), ..] | [v @ "setup"] | [v @ "setup", "--remove"] => {
            setup::setup(*v == "uninstall" || args.len() > 1 && *v == "setup").map_or_else(
                |e| {
                    eprintln!("routine: {e}");
                    1
                },
                |()| 0,
            )
        }
        ["setup", ..] => {
            eprint!("{USAGE}");
            64
        }
        ["upgrade", ..] => setup::upgrade(&args[1..]),
        [verb, ..] => {
            eprint!("routine: unknown command {verb}\n{USAGE}");
            64
        }
    };
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
