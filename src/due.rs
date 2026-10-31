//! `routine run --due` — due.log's rotation, the sweep and its tick line, the dark-wake gate, then every due routine at once,
//! each a `routine run <name>` child, waited for in turn.

use crate::{cron, record, state, status, sys};
use std::{
    env,
    ffi::OsStr,
    fs,
    os::unix::process::ExitStatusExt,
    path::Path,
    process::{Command, Stdio},
    sync::{Mutex, PoisonError},
};

/// The dispatched pids, and the stop's exit code once a signal lands.
static KIDS: Mutex<(Vec<i32>, Option<i32>)> = Mutex::new((Vec::new(), None));

fn kids() -> std::sync::MutexGuard<'static, (Vec<i32>, Option<i32>)> {
    KIDS.lock().unwrap_or_else(PoisonError::into_inner)
}

// ※ due-tick
/// 1 when any due routine failed. launchd stops the tick with a TERM to it alone,
/// then KILLs its group, so a HUP, INT or TERM forwards TERM to every routine — a background
/// child ignores INT — dispatches no more, and exits 128 + the signal once all are reaped.
pub fn due(now: i64) -> i32 {
    let (dir, states, _) = state::roots();
    let log = |m: &str| record::tick_log(&states, state::now().unwrap_or(now), m);
    rotate(&states);
    let due = sweep(&dir, &states, now);
    sys::catch(|sig| {
        let mut k = kids();
        k.1 = Some(128 + sig);
        k.0.iter().for_each(|&p| _ = sys::signal(p, sys::TERM));
    });
    let pmset = state::var("ROUTINE_PMSET").unwrap_or_else(|| "pmset".into());
    // a signal during the probe falls through to the dispatch, which then starts none
    if !due.is_empty() && dark_wake(&pmset) && kids().1.is_none() {
        log(&format!("defer dark-wake due={}", due.join(",")));
        return 0;
    }
    let exe = env::current_exe().unwrap_or_default();
    // ※ parallel-due
    // spawned under the lock: a signal either sees the pid or stops the spawn
    let runs: Vec<_> = (due.iter())
        .filter_map(|name| {
            let mut k = kids();
            k.1.is_none().then(|| {
                log(&format!("dispatch {name}"));
                let mut c = Command::new(&exe);
                let child = sys::background(c.args(["run", name]).stdin(Stdio::null())).spawn();
                k.0.extend(child.as_ref().map(|c| c.id().cast_signed()));
                (name, child)
            })
        })
        .collect();
    let mut rc = 0;
    for (name, child) in runs {
        let s = child.and_then(|mut c| c.wait());
        let code = s.map_or(127, |s| s.code().unwrap_or(128 + s.signal().unwrap_or(0)));
        if kids().1.is_none() {
            rc |= i32::from(code != 0);
            log(&format!("dispatched {name} exit={code}"));
        }
    }
    kids().1.unwrap_or(rc)
}

/// launchd holds due.log's fd open (`StandardOutPath`), so a rename orphans every later write
/// into an unlinked inode: past 5 MB it is copied to `due.log.1` and truncated.
fn rotate(states: &Path) {
    let f = states.join("due.log");
    if fs::metadata(&f).is_ok_and(|m| m.len() > 5_242_880)
        && fs::copy(&f, states.join("due.log.1")).is_ok()
    {
        let _ = fs::write(&f, "");
    }
}

/// The due routines in byte order, journalled as one tick line — a skipped document ahead of
/// it; a rejected schedule reads apart from an on-demand doc.
fn sweep(dir: &Path, states: &Path, now: i64) -> Vec<String> {
    let names = state::routines(dir, |m| record::tick_log(states, now, m));
    let (mut due, mut next) = (vec![], vec![]);
    for name in &names {
        let sched = status::schedule(dir, name);
        match status::next_fire_of(states, name, &sched, now) {
            Some(n) if n <= now => due.push(name.clone()),
            Some(n) => next.push(format!("{name}@{}", cron::iso(n))),
            None if sched.is_empty() => next.push(format!("{name}@unscheduled")),
            None => next.push(format!("{name}@bad")),
        }
    }
    let or_dash = |v: &[String]| {
        if v.is_empty() {
            "-".into()
        } else {
            v.join(",")
        }
    };
    let line = format!(
        "tick docs={} due={} next={}",
        names.len(),
        or_dash(&due),
        or_dash(&next)
    );
    record::tick_log(states, now, &line);
    due
}

// ※ dark-wake-gate
/// A Power Nap dark wake: `pmset -g systemstate` lists the capabilities and Graphics is not
/// among them. No such line — the probe missing, failing or unparseable — fails open.
fn dark_wake(pmset: &OsStr) -> bool {
    let out = Command::new(pmset)
        .args(["-g", "systemstate"])
        .stderr(Stdio::null())
        .output()
        .map(|o| o.stdout)
        .unwrap_or_default();
    let out = String::from_utf8_lossy(&out);
    let mut caps = out
        .lines()
        .filter(|l| l.starts_with("Current System Capabilities are:"))
        .peekable();
    caps.peek().is_some() && !caps.any(|l| l.contains("Graphics"))
}
