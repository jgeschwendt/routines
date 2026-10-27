//! The run's lock, `<dir>/lock`: `started_at`, `take_lock`, `acquire_lock`, `stale`, `on_exit`'s
//! release and `cmd_run`'s lock-blocked ledger of bin/routine. A lock outlives a SIGKILL or a
//! power cut, and after a reboot its pid may belong to any process: alive is the pid AND its
//! start time, wherever both can be read.

use crate::{cron::iso, record, state};
use std::{
    ffi::OsStr,
    fs,
    os::unix::process::ExitStatusExt,
    path::Path,
    process::{self, Command, ExitStatus, Output},
};

/// `cmd` under the C locale (kill's message is matched) and UTC (ps's zone must not differ by
/// caller); one that cannot run (the agent sandbox denies `ps`) exits 1 printing nothing.
fn run(cmd: &str, args: &[&OsStr]) -> Output {
    let mut c = Command::new(cmd);
    let failed = Output {
        status: ExitStatus::from_raw(256),
        stdout: Vec::new(),
        stderr: Vec::new(),
    };
    c.args(args)
        .env("LC_ALL", "C")
        .env("TZ", "UTC0")
        .output()
        .unwrap_or(failed)
}

/// The holder's pid as `$(cat lock/pid)` reads it.
fn holder(dir: &Path) -> String {
    let p = state::read(&dir.join("lock/pid"));
    p.trim_end_matches('\n').to_owned()
}

/// `ps -o lstart=` of `pid` — its start time, the pid's identity; empty when ps cannot say.
fn started_at(pid: &str) -> String {
    let o = run("ps", &["-o", "lstart=", "-p", pid].map(OsStr::new));
    String::from_utf8_lossy(&o.stdout).into()
}

fn take(dir: &Path) -> bool {
    let (lock, pid) = (dir.join("lock"), process::id().to_string());
    fs::create_dir(&lock).is_ok()
        && fs::write(lock.join("started"), started_at(&pid)).is_ok()
        && fs::write(lock.join("pid"), pid + "\n").is_ok()
}

/// The lock's holder is dead. Only ESRCH is dead — EPERM (Seatbelt, another uid) is a live pid
/// this run cannot signal, and a pid that is no number is no holder. No age ever frees a live
/// pid; an empty one is a holder between its mkdir and its pid write, reaped after a minute.
/// Inside the guard only its holder removes a lock and a dead holder never releases, so a
/// verdict stands while the lock it judged does: the pid re-read after the probe (its holder may
/// have released and a fresh run taken the lock meanwhile); no lock at all is no verdict.
pub fn stale(dir: &Path) -> bool {
    let p = holder(dir);
    if p.is_empty() {
        return state::age(&dir.join("lock")).is_some_and(|a| a >= 60);
    }
    let kill = || run("/bin/kill", &["-0", &p].map(OsStr::new)).stderr;
    let dead = !p.bytes().all(|b| b.is_ascii_digit())
        || String::from_utf8_lossy(&kill()).contains("No such process")
        || {
            let s = state::read(&dir.join("lock/started"));
            let (s, c) = (s.trim_end_matches('\n'), started_at(&p));
            let c = c.trim_end_matches('\n');
            !s.is_empty() && !c.is_empty() && c != s
        };
    dead && holder(dir) == p
}

/// `Some(true)` when this run now holds the lock, `Some(false)` when another does, `None` when
/// `dir` is unwritable (exit 73: a sudo run's root-owned leftover is no contention). Two runs
/// that judged one lock stale must not both reap it: the reap holds `lock.reap` and re-judges
/// inside it; a reaper killed in that instant leaves a guard honoured a minute. A lock gone by
/// then was released, and is simply taken.
pub fn acquire(dir: &Path) -> Option<bool> {
    let w = || run("test", &[OsStr::new("-w"), dir.as_os_str()]).status;
    if fs::create_dir_all(dir).is_err() || !w().success() {
        return None;
    }
    let g = dir.join("lock.reap");
    Some(
        take(dir)
            || stale(dir)
                && (fs::create_dir(&g).is_ok() || state::age(&g).is_some_and(|a| a >= 60))
                && {
                    let won = take(dir)
                        || stale(dir) && {
                            let _ = fs::remove_dir_all(dir.join("lock"));
                            take(dir)
                        };
                    let _ = fs::remove_dir_all(&g);
                    won
                },
    )
}

/// `on_exit`'s release: a lock reaped from under this run is its successor's now.
pub fn release(dir: &Path) {
    if holder(dir) == process::id().to_string() {
        let _ = fs::remove_dir_all(dir.join("lock"));
    }
}

/// Contention's own ledger line (`last-run.json` is the holder's), bounded at 200 lines; the
/// holder's pid, read now, or `unknown`.
pub fn blocked(dir: &Path, started: i64) -> String {
    let p = Some(holder(dir)).filter(|p| !p.is_empty());
    let p = p.unwrap_or_else(|| "unknown".into());
    let log = dir.join("lock-blocked.log");
    let _ = record::append(
        &log,
        &format!("{} lock-blocked holder-pid={p}\n", iso(started)),
    );
    record::keep_tail(&log, true, 200, 200);
    p
}

#[cfg(test)]
mod tests {
    use super::{acquire, blocked, holder, release, stale, started_at};
    use crate::doc::tests::scratch;
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        process::{self, Command},
        time::{Duration, SystemTime},
    };

    #[test]
    fn acquire_judges_each_holder() {
        let me = process::id().to_string();
        let ps = !started_at(&me).is_empty(); // false under the agent sandbox, which denies ps
        let mut gone = Command::new("true").spawn().unwrap();
        let gone = (gone.id().to_string(), gone.wait()).0; // reaped: its pid is free
        let old = SystemTime::now() - Duration::from_secs(61);
        // (case, lock/pid, lock/started, lock a minute old, lock.reap a minute old, acquired)
        let cases = [
            ("free", None, None, false, None, Some(true)),
            (
                "mine",
                Some(&*me),
                Some(started_at(&me)),
                false,
                None,
                Some(false),
            ),
            (
                "reused",
                Some(&*me),
                Some("Thu Jan  1 00:00:00 1970\n".into()),
                false,
                None,
                Some(ps),
            ),
            ("unstamped", Some(&*me), None, false, None, Some(false)),
            ("eperm", Some("1"), None, false, None, Some(false)),
            ("dead", Some(&*gone), None, false, None, Some(true)),
            ("word", Some("12a\n"), None, false, None, Some(true)),
            ("unwritten", Some(""), None, false, None, Some(false)),
            ("crashed", Some(""), None, true, None, Some(true)),
            ("guarded", Some("a"), None, false, Some(false), Some(false)),
            ("guard-left", Some("a"), None, false, Some(true), Some(true)),
        ];
        for (name, pid, started, aged, guard, want) in cases {
            let dir = scratch(&format!("lock-{name}")).join("st"); // mkdir -p'd by acquire
            let (lock, reap) = (dir.join("lock"), dir.join("lock.reap"));
            let touch = |p, old_| {
                fs::File::open(p)
                    .unwrap()
                    .set_modified(if old_ { old } else { SystemTime::now() })
            };
            if let Some(p) = pid {
                fs::create_dir_all(&lock).unwrap();
                fs::write(lock.join("pid"), p).unwrap();
                if let Some(s) = &started {
                    fs::write(lock.join("started"), s).unwrap();
                }
                touch(&lock, aged).unwrap();
            }
            if let Some(g) = guard {
                fs::create_dir(&reap).unwrap();
                touch(&reap, g).unwrap();
            }
            let held = acquire(&dir);
            let mine = holder(&dir) == me;
            release(&dir);
            assert_eq!(
                (held, mine, reap.exists(), lock.exists()),
                (
                    want,
                    want == Some(true) || pid == Some(&*me),
                    guard == Some(false),
                    pid.is_some() && !mine
                ),
                "{name}"
            );
        }
        assert!(!stale(&scratch("lock-absent")), "no lock is no verdict");
        // a root-owned leftover: unwritable, and unmakeable beneath it
        let ro = scratch("lock-ro");
        fs::set_permissions(&ro, fs::Permissions::from_mode(0o500)).unwrap();
        assert_eq!((acquire(&ro), acquire(&ro.join("sub"))), (None, None));
        fs::set_permissions(&ro, fs::Permissions::from_mode(0o700)).unwrap();
    }

    #[test]
    fn blocked_keeps_a_ledger() {
        let dir = scratch("lock-blocked");
        assert_eq!(blocked(&dir, 1_786_698_000), "unknown");
        fs::create_dir(dir.join("lock")).unwrap();
        fs::write(dir.join("lock/pid"), "42\n\n").unwrap();
        fs::write(dir.join("lock-blocked.log"), "x\n".repeat(200)).unwrap();
        assert_eq!(blocked(&dir, 1_786_698_000), "42");
        let log = fs::read_to_string(dir.join("lock-blocked.log")).unwrap();
        assert_eq!(
            (log.lines().count(), log.lines().last()),
            (200, Some("2026-08-14T09:00:00Z lock-blocked holder-pid=42"))
        );
    }
}
