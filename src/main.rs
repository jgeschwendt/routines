mod cron;
mod doc;
#[cfg_attr(not(test), expect(dead_code, reason = "stage 4 (run --due) wires it"))]
mod due;
#[cfg_attr(not(test), expect(dead_code, reason = "stage 3 (run <name>) wires it"))]
mod lock;
#[cfg_attr(not(test), expect(dead_code, reason = "stage 3 (run <name>) wires it"))]
mod record;
#[cfg_attr(not(test), expect(dead_code, reason = "stage 3 (run <name>) wires it"))]
mod run;
mod state;
mod status;

use std::process::ExitCode;

// Byte-identical to `bin/routine help` — tests/routine.rs diffs the two.
const USAGE: &str = "routine — executable markdown on rails

  routine run <name>     run one routine document
  routine run --due      run every due routine (silent when none are)
  routine status         schedule × due? × last run, one row per routine
  routine install        install the launchd tick agent (dev.gigantic.routines.due)
  routine uninstall      remove it
  routine sync-ci        (re)write ./.github/workflows/routines.yaml here
  routine help           this text

exit: 64 usage/unknown routine, recorded for a bad doc or no home · 73 mktemp failed or state unwritable ·
      75 lock held · 78 requires missing · 124 timeout · 129/130/143 HUP/INT/TERM ·
      else the failing block's code · run --due 1 when any routine failed ·
      install 1 when the plist cannot land or bootstrap fails

env: ROUTINE_HOME ROUTINE_DIR ROUTINE_STATE ROUTINE_AGENTS_DIR
     ROUTINE_LAUNCHCTL ROUTINE_CLAUDE ROUTINE_NOW ROUTINE_PMSET
";

fn main() -> ExitCode {
    match std::env::args_os()
        .nth(1)
        .as_deref()
        .map(std::ffi::OsStr::to_string_lossy)
        .as_deref()
    {
        None | Some("" | "help" | "-h" | "--help") => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("status") => match state::now() {
            Ok(now) => {
                let (dir, states) = state::roots();
                status::status(&dir, &states, now);
                ExitCode::SUCCESS
            }
            // bash evaluates it as arithmetic; an error there names the script's own line
            Err(v) => {
                eprintln!("routine: ROUTINE_NOW={v} is not an integer");
                ExitCode::from(64)
            }
        },
        Some(verb) => {
            eprintln!("routine: {verb} is not ported yet — use bin/routine");
            ExitCode::from(64)
        }
    }
}
