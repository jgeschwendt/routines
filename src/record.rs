//! What a run leaves behind and how it stays bounded: the tail kept, the tick journal, the
//! degraded marker, the record, the log's name and its pruning.

use crate::{cron::iso, state};
use std::{
    fs, io,
    io::Write,
    path::{Path, PathBuf},
    process,
};

pub fn append(file: &Path, text: &str) -> io::Result<()> {
    (fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(file)?)
    .write_all(text.as_bytes())
}

/// The last `n` (≥ 1) lines of `b` as `tail -n` counts them: an unterminated last line is one.
pub fn tail(b: &[u8], n: usize) -> &[u8] {
    let body = b.strip_suffix(b"\n").unwrap_or(b);
    let cut = (0..body.len())
        .rev()
        .filter(|&i| body[i] == b'\n')
        .nth(n - 1);
    &b[cut.map_or(0, |i| i + 1)..]
}

/// Once `file` holds more than `max` lines (bytes unless `lines`), its last `keep` lines,
/// renamed into place — never rewritten in the reader's fd.
pub fn keep_tail(file: &Path, lines: bool, max: usize, keep: usize) {
    let Ok(b) = fs::read(file) else { return };
    if (if lines {
        b.split(|&c| c == b'\n').count() - 1
    } else {
        b.len()
    }) > max
    {
        let mut tmp = file.as_os_str().to_owned();
        tmp.push(format!(".trim.{}", process::id()));
        let _ = fs::write(&tmp, tail(&b, keep)).and_then(|()| fs::rename(&tmp, file));
        let _ = fs::remove_file(tmp);
    }
}

/// A line in the sweep's own journal, `<states>/tick.log`, bounded at ~1 MB.
pub fn tick_log(states: &Path, now: i64, msg: &str) {
    let f = states.join("tick.log");
    let _ = fs::create_dir_all(states);
    let _ = append(&f, &format!("{} {msg}\n", iso(now)));
    keep_tail(&f, false, 1_048_576, 2000);
}

/// The `degraded` marker: the log is per-run and the exit code the block's, so without it a
/// degraded catch is invisible to status.
pub fn degrade(dir: &Path, now: i64) {
    let _ = fs::create_dir_all(dir);
    let _ = fs::write(dir.join("degraded"), iso(now) + "\n");
}

/// The run's record: `last-run.json` renamed into place (a crash mid-write never leaves an
/// empty record, read as never run), the same line appended to `runs.jsonl`. A clean exit
/// clears `degraded` before stamping, so the record and the marker never disagree.
pub fn record(dir: &Path, started: i64, finished: i64, rc: i32) {
    let _ = fs::create_dir_all(dir);
    if rc == 0 {
        let _ = fs::remove_file(dir.join("degraded"));
    }
    let line = format!(
        "{{\"started\":\"{}\",\"started_epoch\":{started},\"finished\":\"{}\",\"exit\":{rc},\"duration\":{},\"degraded\":{}}}\n",
        iso(started),
        iso(finished),
        finished - started,
        dir.join("degraded").is_file()
    );
    let tmp = dir.join(format!("last-run.json.{}", process::id()));
    let _ = fs::write(&tmp, &line).and_then(|()| fs::rename(&tmp, dir.join("last-run.json")));
    let runs = dir.join("runs.jsonl");
    let _ = append(&runs, &line);
    keep_tail(&runs, true, 5000, 2000);
}

/// `logs/` made and rid of `*.log` files at least 15 days old (find's `-mtime +14`), then this
/// run's log: the stamp is second-resolution and a routine can run twice inside one, so the pid
/// keeps two runs apart.
pub fn log_file(dir: &Path, started: i64) -> PathBuf {
    let logs = dir.join("logs");
    let _ = fs::create_dir_all(&logs);
    for e in fs::read_dir(&logs).into_iter().flatten().flatten() {
        if e.file_type().is_ok_and(|t| t.is_file())
            && e.file_name().as_encoded_bytes().ends_with(b".log")
            && state::age(&e.path()).is_some_and(|a| a >= 15 * 86400)
        {
            let _ = fs::remove_file(e.path());
        }
    }
    let stamp = iso(started).replace(['-', ':'], "");
    logs.join(format!("{stamp}-{}.log", process::id()))
}

#[cfg(test)]
mod tests {
    use super::{degrade, keep_tail, log_file, record, tick_log};
    use crate::doc::tests::scratch;
    use std::{
        fs,
        os::unix::fs::symlink,
        process,
        time::{Duration, SystemTime},
    };

    #[test]
    fn keep_tail_matches_the_table() {
        let cases = [
            ("a\nb\nc\nd\n", "l 3 2", "c\nd\n"),
            ("a\nb\nc\n", "l 3 2", "a\nb\nc\n"), // at max, untouched
            ("a\nb\nc\nd", "l 2 2", "c\nd"),     // an unterminated last line is one
            ("x\n\n\n", "l 2 1", "\n"),
            ("abcdefghijk\nxy\n", "c 10 1", "xy\n"),
            ("abcdefghij", "c 10 1", "abcdefghij"),
            ("a", "c 0 5", "a"),
            ("", "l 0 1", ""),
        ];
        let dir = scratch("keep_tail");
        let ours: Vec<_> = (cases.iter().enumerate())
            .map(|(i, c)| {
                let f = dir.join(format!("{i}.rs"));
                fs::write(&f, c.0).unwrap();
                let a: Vec<_> = c.1.split(' ').collect();
                keep_tail(
                    &f,
                    a[0] == "l",
                    a[1].parse().unwrap(),
                    a[2].parse().unwrap(),
                );
                fs::read_to_string(f).unwrap()
            })
            .collect();
        let want: Vec<_> = cases.iter().map(|c| c.2.to_owned()).collect();
        assert_eq!(ours, want);
        keep_tail(&dir.join("missing"), true, 0, 1);
        assert!(!dir.join("missing").exists());
    }

    // (exit code, degraded before) → the record, twice (last-run.json, runs.jsonl), and the dir
    #[test]
    fn record_clears_degraded_on_success() {
        let dir = scratch("record");
        for (i, (rc, before, after)) in [
            (0, false, false),
            (0, true, false),
            (3, true, true),
            (3, false, false),
        ]
        .into_iter()
        .enumerate()
        {
            let d = dir.join(i.to_string());
            fs::create_dir(&d).unwrap();
            if before {
                fs::write(d.join("degraded"), "").unwrap();
            }
            record(&d, 1_786_698_000, 1_786_698_065, rc);
            let mut names: Vec<_> = (fs::read_dir(&d).unwrap())
                .map(|e| e.unwrap().file_name().into_string().unwrap() + "\n")
                .collect();
            names.sort();
            let line = format!(
                "{{\"started\":\"2026-08-14T09:00:00Z\",\"started_epoch\":1786698000,\"finished\":\"2026-08-14T09:01:05Z\",\"exit\":{rc},\"duration\":65,\"degraded\":{after}}}\n"
            );
            let held = if after { "degraded\n" } else { "" };
            assert_eq!(
                fs::read_to_string(d.join("last-run.json")).unwrap()
                    + &fs::read_to_string(d.join("runs.jsonl")).unwrap()
                    + &names.concat(),
                format!("{line}{line}{held}last-run.json\nruns.jsonl\n")
            );
        }
    }

    #[test]
    fn journals_and_logs() {
        let dir = scratch("record-journals");
        let st = dir.join("st");
        tick_log(&st, 1_786_698_000, "run refused: x");
        degrade(&st.join("r"), 1_786_698_065);
        assert_eq!(
            (
                fs::read_to_string(st.join("tick.log")).unwrap(),
                fs::read_to_string(st.join("r/degraded")).unwrap()
            ),
            (
                "2026-08-14T09:00:00Z run refused: x\n".into(),
                "2026-08-14T09:01:05Z\n".into()
            )
        );
        // find -mtime +14 counts whole days: 15 days old goes, a breath under stays
        let logs = st.join("r/logs");
        let day = 86400;
        for (name, age) in [
            ("old.log", 15 * day + 10),
            ("young.log", 15 * day - 10),
            ("old.txt", 30 * day),
            ("dir.log", 30 * day),
        ] {
            let p = logs.join(name);
            fs::create_dir_all(&logs).unwrap();
            if name == "dir.log" {
                fs::create_dir(&p).unwrap();
            } else {
                fs::write(&p, "").unwrap();
            }
            let t = SystemTime::now() - Duration::from_secs(age);
            fs::File::open(&p).unwrap().set_modified(t).unwrap();
        }
        symlink("old.log", logs.join("link.log")).unwrap();
        let log = log_file(&st.join("r"), 1_786_698_000);
        let mut left: Vec<_> = (fs::read_dir(&logs).unwrap())
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(
            (log, left),
            (
                logs.join(format!("20260814T090000Z-{}.log", process::id())),
                vec![
                    "dir.log".into(),
                    "link.log".into(),
                    "old.txt".into(),
                    "young.log".into()
                ]
            )
        );
    }
}
