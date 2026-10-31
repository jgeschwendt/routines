//! `routine run --due`'s sweep short of dispatch: `cmd_due`'s due.log rotation, its tick line
//! and `dark_wake` of bin/routine. Dispatch is stage 4b.

use crate::{cron, record, state, status};
use std::{
    ffi::OsStr,
    fs,
    path::Path,
    process::{Command, Stdio},
};

/// launchd holds due.log's fd open (`StandardOutPath`), so a rename orphans every later write
/// into an unlinked inode: past 5 MB it is copied to `due.log.1` and truncated.
pub fn rotate(states: &Path) {
    let f = states.join("due.log");
    if fs::metadata(&f).is_ok_and(|m| m.len() > 5_242_880)
        && fs::copy(&f, states.join("due.log.1")).is_ok()
    {
        let _ = fs::write(&f, "");
    }
}

/// The due routines in byte order, journalled as one tick line — a skipped document ahead of
/// it; a rejected schedule reads apart from an on-demand doc.
pub fn sweep(dir: &Path, states: &Path, now: i64) -> Vec<String> {
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

/// A Power Nap dark wake: `pmset -g systemstate` lists the capabilities and Graphics is not
/// among them. No such line — the probe missing, failing or unparseable — fails open.
pub fn dark_wake(pmset: &OsStr) -> bool {
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

#[cfg(test)]
mod tests {
    use super::{dark_wake, rotate, sweep};
    use crate::doc::tests::scratch;
    use std::{fs, os::unix::fs::symlink, process::Command};

    // A dark wake makes bin/routine's `run --due` sweep, journal and defer without dispatching,
    // so one fixture home per side compares rotation, the tick line and the gate end to end.
    #[test]
    fn sweep_and_gate_match_bin_routine() {
        let dir = scratch("due");
        let pmset = |name: &str, body: &str| {
            let p = dir.join(name);
            fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
            Command::new("chmod").arg("+x").arg(&p).status().unwrap();
            p
        };
        let dark = pmset(
            "dark",
            "echo 'Current System Capabilities are: CPU Network'",
        );
        let lit = pmset(
            "lit",
            "printf 'x\\nCurrent System Capabilities are: CPU Graphics\\n'",
        );
        let none = pmset("none", "echo 'Current Power State: 4'; exit 1");
        let homes = ["sh", "rs"].map(|side| {
            let home = dir.join(side);
            let st = home.join(".state");
            let sched = |s: &str| format!("---\nschedule: \"{s}\"\n---\n");
            for (path, body) in [
                ("hourly.md", sched("0 * * * *")),
                ("yearly.md", sched("0 0 1 1 *")),
                (
                    ".state/yearly/last-run.json",
                    "{\"started_epoch\":1786698000}\n".into(),
                ),
                ("ahead.md", sched("0 0 1 1 *")),
                (
                    ".state/ahead/last-run.json",
                    "{\"started_epoch\":1786699999}\n".into(),
                ),
                ("bad.md", sched("@daily")),
                ("nul.md", "---\nschedule: 0 * * * *\0 5\n---\n".into()), // awk cuts at the NUL
                ("nulq.md", "---\nschedule: \"0 * * * *\"\0x\n---\n".into()),
                ("blank.md", sched("")),
                ("Not_Slug.md", sched("0 * * * *")),
                ("notes.txt", String::new()),
                (".state/due.log", "x".repeat(5_242_881)),
            ] {
                fs::create_dir_all(home.join(path).parent().unwrap()).unwrap();
                fs::write(home.join(path), body).unwrap();
            }
            symlink("gone.md", home.join("dangling.md")).unwrap();
            if side == "sh" {
                let o = Command::new("/bin/bash")
                    .args([
                        concat!(env!("CARGO_MANIFEST_DIR"), "/bin/routine"),
                        "run",
                        "--due",
                    ])
                    .env("ROUTINE_HOME", &home)
                    .env("ROUTINE_NOW", "1786698065")
                    .env("ROUTINE_PMSET", &dark)
                    .output()
                    .unwrap();
                assert_eq!((o.status.code(), o.stdout), (Some(0), vec![]));
            } else {
                rotate(&st);
                let due = sweep(&home, &st, 1_786_698_065);
                assert_eq!(due, ["ahead", "hourly", "nul", "nulq"]);
                assert!(dark_wake(dark.as_os_str()));
                let defer = format!("defer dark-wake due={}", due.join(","));
                crate::record::tick_log(&st, 1_786_698_065, &defer);
            }
            let mut names: Vec<_> = (fs::read_dir(&st).unwrap())
                .map(|e| e.unwrap().file_name().into_string().unwrap())
                .collect();
            names.sort();
            let size = |f: &str| fs::metadata(st.join(f)).unwrap().len();
            (
                fs::read_to_string(st.join("tick.log")).unwrap(),
                names,
                size("due.log"),
                size("due.log.1"),
            )
        });
        assert_eq!(homes[0], homes[1]);
        assert_eq!(homes[0].2, 0);
        assert!(homes[0].0.contains(
            " tick docs=7 due=ahead,hourly,nul,nulq next=bad@bad,blank@unscheduled,yearly@2027-01-01T00:00:00Z\n"
        ));
        for p in [lit, none, dir.join("missing")] {
            assert!(!dark_wake(p.as_os_str()));
        }
        // under 5 MB, and no due.log at all, rotation leaves the dir as it was
        rotate(&dir.join("rs/.state"));
        rotate(&dir.join("nowhere"));
        assert!(!dir.join("nowhere").exists());
        // no home at all: an empty sweep, still journalled
        assert!(sweep(&dir.join("nowhere"), &dir.join("nowhere"), 0).is_empty());
        assert_eq!(
            fs::read_to_string(dir.join("nowhere/tick.log")).unwrap(),
            "1970-01-01T00:00:00Z tick docs=0 due=- next=-\n"
        );
    }
}
