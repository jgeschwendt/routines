//! `routine health`, `routine ack` and `routine pause` — one verdict over the home, derived from
//! the runner's own parsers and state, the ack that holds one failure until its exit changes, and
//! the pause that holds a routine off the tick until `routine resume`.

use crate::{
    cron::{self, iso},
    doc, lock, run, setup, state, status,
};
use std::{fmt::Write as _, fs, path::Path, process::Command, time::UNIX_EPOCH};

/// `s` as a JSON string: `"` and `\` escaped, every other control character `\u00XX`.
fn json(s: &str) -> String {
    let mut o = String::from('"');
    for c in s.chars() {
        match c {
            '"' | '\\' => o.extend(['\\', c]),
            c if c < ' ' => _ = write!(o, "\\u{:04x}", u32::from(c)),
            c => o.push(c),
        }
    }
    o + "\""
}

/// `json`'s escapes undone: `\uXXXX` its code point, `\X` the `X`.
fn unjson(s: &str) -> String {
    let (mut cs, mut o) = (s.chars(), String::new());
    while let Some(mut c) = cs.next() {
        if c == '\\' {
            c = cs.next().unwrap_or_default();
            if c == 'u' {
                let h: String = cs.by_ref().take(4).collect();
                let u = u32::from_str_radix(&h, 16).ok().and_then(char::from_u32);
                c = u.unwrap_or_default();
            }
        }
        o.push(c);
    }
    o
}

fn opt(v: Option<String>) -> String {
    v.unwrap_or_else(|| "null".into())
}

fn mtime(p: &Path) -> Option<i64> {
    let t = fs::metadata(p).ok()?.modified().ok()?;
    Some(t.duration_since(UNIX_EPOCH).ok()?.as_secs().cast_signed())
}

/// `<kind>: <name> — <finding>` and its fix, one finding as the plain report prints it.
fn report(kind: &str, name: &str, finding: &str, fix: &str) -> String {
    format!("{kind}: {name} — {finding}\n  fix: {fix}\n")
}

/// A marker's line — an ack's after its `"exit":N,`, a pause's whole — as its verb writes it and
/// health reports it.
fn stamp(head: &str, at: &str, reason: &str) -> String {
    format!("{{{head}\"at\":{},\"reason\":{}}}", json(at), json(reason))
}

/// `file` as `(head, at, reason)`, when it holds the line `stamp` writes.
fn stamped(file: &Path) -> Option<(String, String, String)> {
    let a = state::read(file);
    let (head, r) = a.strip_prefix('{')?.split_once("\"at\":\"")?;
    let (at, r) = r.split_once("\",\"reason\":\"")?;
    let r = r.trim_end().strip_suffix("\"}")?;
    Some((head.into(), unjson(at), unjson(r)))
}

/// `<dir>/ack` as `(exit, at, reason)`.
fn acked(dir: &Path) -> Option<(i64, String, String)> {
    let (head, at, r) = stamped(&dir.join("ack"))?;
    Some((
        cron::num(head.strip_prefix("\"exit\":")?.strip_suffix(',')?)?,
        at,
        r,
    ))
}

/// `<dir>/paused` as `(at, reason)`, both empty when a hand made it.
fn paused(dir: &Path) -> Option<(String, String)> {
    let f = dir.join("paused");
    (f.is_file()).then(|| stamped(&f).map_or_else(Default::default, |p| (p.1, p.2)))
}

fn ack_json(exit: i64, at: &str, reason: &str) -> String {
    stamp(&format!("\"exit\":{exit},"), at, reason)
}

/// `<dir>/last-run.json` as `(started_epoch, finished, exit, duration)`, when each is there.
fn last_run(dir: &Path) -> Option<(i64, String, i64, i64)> {
    let get = |k| state::json_get(&dir.join("last-run.json"), k);
    let n = |k| cron::num(&get(k));
    Some((
        n("started_epoch")?,
        get("finished"),
        n("exit")?,
        n("duration")?,
    ))
}

/// tick.log's tail as `(heard, swept)`: its last line's stamp, and the last sweep that could
/// dispatch — a `tick docs=` line no `defer dark-wake` follows.
fn sweeps(states: &Path) -> (Option<i64>, Option<i64>) {
    let (cut, log) = state::last_bytes(&states.join("tick.log"), 65536);
    let log = String::from_utf8_lossy(&log);
    let mut lines: Vec<_> = log.lines().skip(usize::from(cut > 0)).collect();
    let stamp = |l: &str| l.get(..20).and_then(cron::epoch);
    let is = |l: &str, m: &str| l.get(20..).is_some_and(|r| r.starts_with(m));
    let heard = lines.last().and_then(|l| stamp(l));
    lines.push("");
    let swept = (lines.windows(2).rev())
        .filter(|w| is(w[0], " tick docs=") && !is(w[1], " defer dark-wake"))
        .find_map(|w| stamp(w[0]));
    (heard, swept)
}

// ※ health-verdict
/// One routine's JSON object, whether it runs, and its report line with its bucket — 0 red, 1 a
/// finding, 2 acknowledged or paused. The first match wins: lint (what the runner refuses or
/// mis-reads), wedged (a live lock past its timeout and the watchdog's minute), paused (a `paused`
/// marker, whatever it holds: the tick passes it over, so it is never due), a non-zero last exit —
/// acknowledged while an ack holds it, else a finding for exit 1 (it ran and found something,
/// never red) and failed for 2 and above — stale (due, idle, and passed over by a sweep that could dispatch half an hour after it fell
/// due or was resumed — a slept machine's sweeps predate it), else ok.
fn routine(
    (dir, states): (&Path, &Path),
    name: &str,
    now: i64,
    swept: Option<i64>,
) -> (String, bool, Option<(usize, String)>) {
    let (doc, st) = (dir.join(format!("{name}.md")), states.join(name));
    let text = String::from_utf8_lossy(&fs::read(&doc).unwrap_or_default()).into_owned();
    let sched = status::schedule(dir, name);
    let next = status::next_fire_of(states, name, &sched, now);
    let (limit, bad) = run::timeout(&text);
    let lint: Vec<_> = [
        (doc::extract_blocks(&text).is_empty()).then(|| "no sh or bash block".into()),
        (run::on_error(&text).err()).map(|v| format!("on_error {v} is neither claude nor fail")),
        bad.map(|v| format!("timeout {v} is no positive number of seconds")),
        (!sched.is_empty() && next.is_none()).then(|| format!("schedule {sched} never fires")),
    ]
    .into_iter()
    .flatten()
    .collect();
    let (last, ack, paused) = (last_run(&st), acked(&st), paused(&st));
    let due = paused.is_none() && next.is_some_and(|n| n <= now);
    let running = st.join("lock").exists() && !lock::stale(&st);
    let held = mtime(&st.join("lock")).map_or(0, |t| now - t);
    let resumed = cron::num(state::read(&st.join("resumed")).trim_end()).unwrap_or(0);
    let since = (next.filter(|_| due)).and_then(|n| if n == 0 { mtime(&doc) } else { Some(n) });
    let since = since.map(|d| d.max(resumed));
    let (verdict, says) = if !lint.is_empty() {
        let fix = format!(
            "edit {}: the smallest change the runner takes",
            doc.display()
        );
        ("lint", Some((lint.join("; "), fix)))
    } else if running && held > limit + 60 {
        let p = lock::holder(&st);
        let f = format!("pid {p} has held the lock {held}s, past its {limit}s timeout");
        let x = format!(
            "ps -p {p} -o command= — its watchdog missed it: kill {p}, and the next run reaps the lock"
        );
        ("wedged", Some((f, x)))
    } else if paused.is_some() {
        ("paused", None)
    } else if let Some((_, fin, e, _)) = last.as_ref().filter(|l| l.2 != 0) {
        let logs = st.join("logs");
        let newest = (fs::read_dir(&logs).into_iter().flatten().flatten())
            .map(|e| e.path())
            .max()
            .unwrap_or(logs);
        let (n, d) = (newest.display(), doc.display());
        let (v, x) = if *e == 1 {
            let x = format!("it ran and found something — read {n}; {d} says whose move it is");
            ("finding", x)
        } else {
            let x = format!("read {n}, then {d} — its prose is the repair manual");
            ("failed", x)
        };
        let v = if ack.as_ref().is_some_and(|a| a.0 == *e) {
            "acknowledged"
        } else {
            v
        };
        (v, Some((format!("exit {e} at {fin}"), x)))
    } else if let Some((d, s)) = since.zip(swept).filter(|(d, s)| !running && s - d > 1800) {
        let f = format!("due since {}, swept {} without a run", iso(d), iso(s));
        let log = states.join("tick.log");
        let x = format!(
            "routine run {name}, and read {} for its refusal",
            log.display()
        );
        ("stale", Some((f, x)))
    } else {
        ("ok", None)
    };
    let said = match (
        &says,
        ack.as_ref().filter(|_| verdict == "acknowledged"),
        paused.as_ref().filter(|_| verdict == "paused"),
    ) {
        (_, Some((e, at, r)), _) => Some((
            2,
            format!("acknowledged: {name} — exit {e} since {at}: {r}\n"),
        )),
        (_, _, Some((at, r))) => Some((2, format!("paused: {name} — since {at}: {r}\n"))),
        (Some((f, x)), ..) => Some((
            usize::from(verdict == "finding"),
            report(verdict, name, f, x),
        )),
        _ => None,
    };
    let degraded = st.join("degraded");
    let row = format!(
        "{{\"name\":{},\"schedule\":{},\"next\":{},\"due\":{due},\"running\":{running},\"last\":{},\"degraded\":{},\"ack\":{},\"paused\":{},\"verdict\":\"{verdict}\",\"finding\":{},\"fix\":{}}}",
        json(name),
        opt((!sched.is_empty()).then(|| json(&sched))),
        opt(next.map(|n| n.to_string())),
        opt(last.map(|(s, f, e, d)| format!(
            "{{\"started\":{s},\"finished\":{},\"exit\":{e},\"duration\":{d}}}",
            json(&f)
        ))),
        opt((degraded.is_file()).then(|| json(state::read(&degraded).trim_end_matches('\n')))),
        opt(ack.map(|(e, at, r)| ack_json(e, &at, &r))),
        opt(paused.map(|(at, r)| stamp("", &at, &r))),
        opt(says.as_ref().map(|s| json(&s.0))),
        opt(says.map(|s| json(&s.1))),
    );
    (row, running, said)
}

/// When the machine last woke, `sysctl -n kern.waketime`'s `sec`; none when the probe fails.
fn wake() -> Option<i64> {
    let o = Command::new("sysctl")
        .args(["-n", "kern.waketime"])
        .output()
        .ok()?;
    let o = String::from_utf8_lossy(&o.stdout).into_owned();
    cron::num(o.strip_prefix("{ sec = ")?.split(',').next()?)
}

/// The tick's verdict and its finding: `unmanaged` with no launchctl, `not-loaded`, or `stale`
/// when tick.log has been quiet ten minutes since its last line or the last wake, whichever is
/// later (launchd ticks within a minute of a wake), and no routine runs (a long run holds the
/// tick).
fn tick(
    states: &Path,
    now: i64,
    heard: Option<i64>,
    busy: bool,
) -> (&'static str, Option<(String, String)>) {
    let target = format!("{}/{}", setup::gui(), setup::TICK);
    if !setup::launchd() {
        return ("unmanaged", None);
    } else if !setup::launchctl(&["print", &target]) {
        let f = format!("{target} is not loaded");
        return ("not-loaded", Some((f, "routine setup".into())));
    }
    let quiet = heard.max(wake());
    if busy || quiet.is_some_and(|t| now - t <= 600) {
        return ("ok", None);
    }
    let f = quiet.map_or("tick.log has no line".into(), |t| {
        format!(
            "tick.log quiet {}s since {}, its last line or the last wake",
            now - t,
            iso(t)
        )
    });
    let out = states.join("due.log");
    let x = format!(
        "launchctl kickstart -k {target} — its output is {}",
        out.display()
    );
    ("stale", Some((f, x)))
}

/// One verdict over the home, as a JSON line when `as_json` whose every finding carries the fix
/// the plain report prints; 1 when anything is red.
pub fn health(as_json: bool, now: i64) -> i32 {
    let (dir, states, _) = state::roots();
    let mut skipped = Vec::new();
    let names = state::routines(&dir, |f, why| {
        let opens = state::read(&dir.join(f)).lines().next() == Some("---");
        skipped.push((f.to_owned(), why.to_owned(), opens));
    });
    let (heard, swept) = sweeps(&states);
    let (mut rows, mut said, mut busy) = (vec![], [vec![], vec![], vec![]], false);
    for name in &names {
        let (row, running, line) = routine((&dir, &states), name, now, swept);
        rows.push(row);
        busy |= running;
        if let Some((k, l)) = line {
            said[k].push(l);
        }
    }
    let fix = "rename it to a slug, [a-z0-9][a-z0-9-]*.md: the runner never fires it";
    let skips: Vec<_> = (skipped.iter())
        .map(|(f, why, r)| {
            if *r {
                let finding = format!("{why}, yet it opens with frontmatter");
                said[0].push(report("skipped", f, &finding, fix));
            }
            let x = opt(r.then(|| json(fix)));
            format!(
                "{{\"file\":{},\"why\":{},\"red\":{r},\"fix\":{x}}}",
                json(f),
                json(why)
            )
        })
        .collect();
    let mut orphans: Vec<_> = (fs::read_dir(&states).into_iter().flatten().flatten())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.') && !dir.join(format!("{n}.md")).is_file())
        .collect();
    orphans.sort();
    let (archive, stamp) = (states.join(".archive"), iso(now).replace(['-', ':'], ""));
    let orphans: Vec<_> = (orphans.iter())
        .map(|name| {
            let (st, a) = (states.join(name).display().to_string(), archive.display());
            let finding = format!("{st} has no {name}.md");
            let fix = format!("mkdir -p {a} && mv {st} {a}/{name}-{stamp} — archive, never delete");
            said[0].push(report("orphan", name, &finding, &fix));
            let (name, finding, fix) = (json(name), json(&finding), json(&fix));
            format!("{{\"name\":{name},\"finding\":{finding},\"fix\":{fix}}}")
        })
        .collect();
    let tick = tick(&states, now, heard, busy);
    if let Some((f, x)) = &tick.1 {
        said[0].insert(0, report(tick.0, "tick", f, x));
    }
    if as_json {
        println!(
            "{{\"version\":{},\"now\":{now},\"verdict\":\"{}\",\"tick\":{{\"verdict\":\"{}\",\"swept\":{},\"finding\":{},\"fix\":{}}},\"routines\":[{}],\"skipped\":[{}],\"orphans\":[{}]}}",
            json(crate::VERSION),
            if said[0].is_empty() { "ok" } else { "red" },
            tick.0,
            opt(swept.map(|s| s.to_string())),
            opt(tick.1.as_ref().map(|t| json(&t.0))),
            opt(tick.1.map(|t| json(&t.1))),
            rows.join(","),
            skips.join(","),
            orphans.join(","),
        );
    } else {
        let close = if said[0].is_empty() {
            format!("healthy — {} routines", names.len())
        } else {
            format!("unhealthy — {} red", said[0].len())
        };
        let found = (!said[1].is_empty()).then(|| format!(" · {} finding(s)", said[1].len()));
        println!(
            "{}{close}{}",
            said.concat().concat(),
            found.unwrap_or_default()
        );
    }
    i32::from(!said[0].is_empty())
}

fn known(dir: &Path, name: &str) -> bool {
    state::slug(name) && dir.join(format!("{name}.md")).is_file()
}

/// `routine ack <name> [reason…]` holds the failed last run's exit in `<state>/<name>/ack`, and
/// `--clear` drops it: 64 for an unknown routine or a last run that is no failure, 73 when the
/// ack cannot land.
pub fn ack(args: &[&str], now: i64) -> i32 {
    let (dir, states, _) = state::roots();
    let (name, why) = (args[0], &args[1..]);
    let f = states.join(name).join("ack");
    let (rc, msg) = if !known(&dir, name) {
        (64, format!("no routine {name}"))
    } else if why == ["--clear"] {
        drop(fs::remove_file(&f));
        return 0;
    } else if let Some(e) = last_run(&states.join(name))
        .map(|l| l.2)
        .filter(|&e| e != 0)
    {
        if fs::write(&f, ack_json(e, &iso(now), &why.join(" ")) + "\n").is_ok() {
            return 0;
        }
        (73, format!("cannot write {}", f.display()))
    } else {
        (
            64,
            format!("{name}'s last run is no failure to acknowledge"),
        )
    };
    eprintln!("routine: {msg}");
    rc
}

// ※ pause
/// `routine pause <name> [reason…]` holds the routine off the tick in `<state>/<name>/paused`;
/// `routine resume <name>` drops it and stamps `resumed`, where health counts stale from: 64 for
/// an unknown routine, 73 when the marker cannot land or lift.
pub fn pause(on: bool, args: &[&str], now: i64) -> i32 {
    let (dir, states, _) = state::roots();
    let (name, st) = (args[0], states.join(args[0]));
    let f = st.join("paused");
    let (rc, msg) = if !known(&dir, name) {
        (64, format!("no routine {name}"))
    } else if on {
        let line = stamp("", &iso(now), &args[1..].join(" ")) + "\n";
        if fs::create_dir_all(&st)
            .and_then(|()| fs::write(&f, line))
            .is_ok()
        {
            return 0;
        }
        (73, format!("cannot write {}", f.display()))
    } else if !f.is_file()
        || fs::remove_file(&f).is_ok() && fs::write(st.join("resumed"), format!("{now}\n")).is_ok()
    {
        return 0;
    } else {
        (73, format!("cannot lift {}", f.display()))
    };
    eprintln!("routine: {msg}");
    rc
}
