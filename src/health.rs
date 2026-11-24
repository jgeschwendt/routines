//! `routine health`, `routine ack` and `routine pause` — one verdict over the home, derived from
//! the runner's own parsers and state, the ack that holds one failure until its exit changes, and
//! the pause that holds a routine off the tick until `routine resume`.

use crate::{
    cron::{self, iso},
    doc, lock, run, setup, state, status,
};
use std::{fmt::Write as _, fs, path::Path, process::Command, time::UNIX_EPOCH};

/// `text` as a JSON string: `"` and `\` escaped, every other control character `\u00XX`.
fn json(text: &str) -> String {
    let mut out = String::from('"');
    for ch in text.chars() {
        match ch {
            '"' | '\\' => out.extend(['\\', ch]),
            control if control < ' ' => _ = write!(out, "\\u{:04x}", u32::from(control)),
            other => out.push(other),
        }
    }
    format!("{out}\"")
}

/// `json`'s escapes undone: `\uXXXX` its code point, `\X` the `X`.
fn unjson(text: &str) -> String {
    let (mut cs, mut out) = (text.chars(), String::new());
    while let Some(mut ch) = cs.next() {
        if ch == '\\' {
            ch = cs.next().unwrap_or_default();
            if ch == 'u' {
                let hex: String = cs.by_ref().take(4).collect();
                let code = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32);
                ch = code.unwrap_or_default();
            }
        }
        out.push(ch);
    }
    out
}

fn opt(value: Option<String>) -> String {
    value.unwrap_or_else(|| "null".into())
}

fn mtime(path: &Path) -> Option<i64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    Some(
        modified
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_secs()
            .cast_signed(),
    )
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
    let line = state::read(file);
    let (head, rest) = line.strip_prefix('{')?.split_once("\"at\":\"")?;
    let (at, rest) = rest.split_once("\",\"reason\":\"")?;
    let rest = rest.trim_end().strip_suffix("\"}")?;
    Some((head.into(), unjson(at), unjson(rest)))
}

/// `<dir>/ack` as `(exit, at, reason)`.
#[expect(
    clippy::single_call_fn,
    reason = "the ack marker's parser, named beside paused"
)]
fn acked(dir: &Path) -> Option<(i64, String, String)> {
    let (head, at, reason) = stamped(&dir.join("ack"))?;
    Some((
        cron::num(head.strip_prefix("\"exit\":")?.strip_suffix(',')?)?,
        at,
        reason,
    ))
}

fn ack_json(exit: i64, at: &str, reason: &str) -> String {
    stamp(&format!("\"exit\":{exit},"), at, reason)
}

/// `<dir>/last-run.json` as `(started_epoch, finished, exit, duration)`, when each is there.
fn last_run(dir: &Path) -> Option<(i64, String, i64, i64)> {
    let get = |key| state::json_get(&dir.join("last-run.json"), key);
    let n = |key| cron::num(&get(key));
    Some((
        n("started_epoch")?,
        get("finished"),
        n("exit")?,
        n("duration")?,
    ))
}

// ※ health-verdict
/// One routine's JSON object, whether it runs, and its report line with its bucket — 0 red, 1 a
/// finding, 2 acknowledged or paused. The first match wins: lint (what the runner refuses or
/// mis-reads), wedged (a live lock past its timeout and the watchdog's minute), paused (a `paused`
/// marker, whatever it holds: the tick passes it over, so it is never due), a non-zero last exit —
/// acknowledged while an ack holds it, else a finding for exit 1 (it ran and found something,
/// never red) and failed for 2 and above — stale (due, idle, and passed over by a sweep that could dispatch half an hour after it fell
/// due or was resumed — a slept machine's sweeps predate it), else ok.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "now is ROUTINE_NOW's to set: saturating would change what a hostile clock prints"
)]
#[expect(
    clippy::pattern_type_mismatch,
    reason = "a tuple's fields bind by reference only through ref, which ref_patterns forbids"
)]
#[expect(
    clippy::single_call_fn,
    reason = "one routine's verdict, the unit health loops over"
)]
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
        (run::on_error(&text).err())
            .map(|value| format!("on_error {value} is neither claude nor fail")),
        bad.map(|value| format!("timeout {value} is no positive number of seconds")),
        (!sched.is_empty() && next.is_none()).then(|| format!("schedule {sched} never fires")),
    ]
    .into_iter()
    .flatten()
    .collect();
    let (last, ack) = (last_run(&st), acked(&st));
    // `paused` as `(at, reason)`, both empty when a hand made it
    let paused = {
        let file = st.join("paused");
        (file.is_file())
            .then(|| stamped(&file).map_or_else(Default::default, |parsed| (parsed.1, parsed.2)))
    };
    let due = paused.is_none() && next.is_some_and(|n| n <= now);
    let running = st.join("lock").exists() && !lock::stale(&st);
    let held = mtime(&st.join("lock")).map_or(0, |taken| now - taken);
    let resumed = cron::num(state::read(&st.join("resumed")).trim_end()).unwrap_or(0);
    let since = (next.filter(|_| due)).and_then(|n| if n == 0 { mtime(&doc) } else { Some(n) });
    let since = since.map(|fell_due| fell_due.max(resumed));
    let (verdict, says) = if !lint.is_empty() {
        let fix = format!(
            "edit {}: the smallest change the runner takes",
            doc.display()
        );
        ("lint", Some((lint.join("; "), fix)))
    } else if running && held > limit.saturating_add(60) {
        let pid = lock::holder(&st);
        let finding = format!("pid {pid} has held the lock {held}s, past its {limit}s timeout");
        let fix = format!(
            "ps -p {pid} -o command= — its watchdog missed it: kill {pid}, and the next run reaps the lock"
        );
        ("wedged", Some((finding, fix)))
    } else if paused.is_some() {
        ("paused", None)
    } else if let Some((_, fin, exit, _)) = last.as_ref().filter(|ran| ran.2 != 0) {
        let logs = st.join("logs");
        let newest = (fs::read_dir(&logs).into_iter().flatten().flatten())
            .map(|entry| entry.path())
            .max()
            .unwrap_or(logs);
        let (log_shown, doc_shown) = (newest.display(), doc.display());
        let (kind, fix) = if *exit == 1 {
            let fix = format!(
                "it ran and found something — read {log_shown}; {doc_shown} says whose move it is"
            );
            ("finding", fix)
        } else {
            let fix =
                format!("read {log_shown}, then {doc_shown} — its prose is the repair manual");
            ("failed", fix)
        };
        let kind = if ack.as_ref().is_some_and(|marker| marker.0 == *exit) {
            "acknowledged"
        } else {
            kind
        };
        (kind, Some((format!("exit {exit} at {fin}"), fix)))
    } else if let Some((due_at, swept_at)) = since
        .zip(swept)
        .filter(|&(due_at, swept_at)| !running && swept_at - due_at > 1800)
    {
        let finding = format!(
            "due since {}, swept {} without a run",
            iso(due_at),
            iso(swept_at)
        );
        let log = states.join("tick.log");
        let fix = format!(
            "routine run {name}, and read {} for its refusal",
            log.display()
        );
        ("stale", Some((finding, fix)))
    } else {
        ("ok", None)
    };
    let said = match (
        &says,
        ack.as_ref().filter(|_| verdict == "acknowledged"),
        paused.as_ref().filter(|_| verdict == "paused"),
    ) {
        (_, Some((exit, at, reason)), _) => Some((
            2,
            format!("acknowledged: {name} — exit {exit} since {at}: {reason}\n"),
        )),
        (_, _, Some((at, reason))) => Some((2, format!("paused: {name} — since {at}: {reason}\n"))),
        (Some((finding, fix)), ..) => Some((
            usize::from(verdict == "finding"),
            report(verdict, name, finding, fix),
        )),
        _ => None,
    };
    let degraded = st.join("degraded");
    let row = format!(
        "{{\"name\":{},\"schedule\":{},\"next\":{},\"due\":{due},\"running\":{running},\"last\":{},\"degraded\":{},\"ack\":{},\"paused\":{},\"verdict\":\"{verdict}\",\"finding\":{},\"fix\":{}}}",
        json(name),
        opt((!sched.is_empty()).then(|| json(&sched))),
        opt(next.map(|n| n.to_string())),
        opt(last.map(|(started, finished, exit, duration)| format!(
            "{{\"started\":{started},\"finished\":{},\"exit\":{exit},\"duration\":{duration}}}",
            json(&finished)
        ))),
        opt((degraded.is_file()).then(|| json(state::read(&degraded).trim_end_matches('\n')))),
        opt(ack.map(|(exit, at, reason)| ack_json(exit, &at, &reason))),
        opt(paused.map(|(at, reason)| stamp("", &at, &reason))),
        opt(says.as_ref().map(|found| json(&found.0))),
        opt(says.map(|found| json(&found.1))),
    );
    (row, running, said)
}

/// When the machine last woke, `sysctl -n kern.waketime`'s `sec`; none when the probe fails.
#[expect(clippy::single_call_fn, reason = "the wake-time probe")]
fn wake() -> Option<i64> {
    let output = Command::new("sysctl")
        .args(["-n", "kern.waketime"])
        .output()
        .ok()?;
    let output = String::from_utf8_lossy(&output.stdout).into_owned();
    cron::num(output.strip_prefix("{ sec = ")?.split(',').next()?)
}

/// The tick's verdict and its finding: `unmanaged` with no launchctl, `not-loaded`, or `stale`
/// when tick.log has been quiet ten minutes since its last line or the last wake, whichever is
/// later (launchd ticks within a minute of a wake), and no routine runs (a long run holds the
/// tick).
#[expect(
    clippy::arithmetic_side_effects,
    reason = "now is ROUTINE_NOW's to set: saturating would change what a hostile clock prints"
)]
#[expect(clippy::single_call_fn, reason = "the tick's verdict")]
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
        let finding = format!("{target} is not loaded");
        return ("not-loaded", Some((finding, "routine setup".into())));
    }
    let quiet = heard.max(wake());
    if busy || quiet.is_some_and(|last| now - last <= 600) {
        return ("ok", None);
    }
    let finding = quiet.map_or_else(
        || "tick.log has no line".into(),
        |last| {
            format!(
                "tick.log quiet {}s since {}, its last line or the last wake",
                now - last,
                iso(last)
            )
        },
    );
    let out = states.join("due.log");
    let fix = format!(
        "launchctl kickstart -k {target} — its output is {}",
        out.display()
    );
    ("stale", Some((finding, fix)))
}

/// One verdict over the home, as a JSON line when `as_json` whose every finding carries the fix
/// the plain report prints; 1 when anything is red.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "a tuple's fields bind by reference only through ref, which ref_patterns forbids"
)]
#[expect(clippy::single_call_fn, reason = "a module per verb")]
pub fn health(as_json: bool, now: i64) -> i32 {
    let (dir, states, _) = state::roots();
    let mut skipped = Vec::new();
    let names = state::routines(&dir, |file, why| {
        let opens = state::read(&dir.join(file)).lines().next() == Some("---");
        skipped.push((file.to_owned(), why.to_owned(), opens));
    });
    // tick.log's tail: its last line's stamp, and the last sweep that could dispatch — a
    // `tick docs=` line no `defer dark-wake` follows
    let (heard, swept) = {
        let (cut, log) = state::last_bytes(&states.join("tick.log"), 1 << 16);
        let log = String::from_utf8_lossy(&log);
        let mut lines: Vec<_> = log.lines().skip(usize::from(cut > 0)).collect();
        let stamp = |line: &str| line.get(..20).and_then(cron::epoch);
        let is =
            |line: &str, marker: &str| line.get(20..).is_some_and(|rest| rest.starts_with(marker));
        let heard = lines.last().and_then(|line| stamp(line));
        lines.push("");
        let swept = (lines.iter().zip(lines.iter().skip(1)).rev())
            .filter(|&(line, next)| is(line, " tick docs=") && !is(next, " defer dark-wake"))
            .find_map(|(line, _)| stamp(line));
        (heard, swept)
    };
    let (mut rows, mut said, mut busy) = (vec![], [vec![], vec![], vec![]], false);
    for name in &names {
        let (row, running, line) = routine((&dir, &states), name, now, swept);
        rows.push(row);
        busy |= running;
        if let Some((bucket, text)) = line
            && let Some(lines) = said.get_mut(bucket)
        {
            lines.push(text);
        }
    }
    let rename_fix = "rename it to a slug, [a-z0-9][a-z0-9-]*.md: the runner never fires it";
    let skips: Vec<_> = (skipped.iter())
        .map(|(file, why, red)| {
            if *red {
                let finding = format!("{why}, yet it opens with frontmatter");
                said[0].push(report("skipped", file, &finding, rename_fix));
            }
            let fix_json = opt(red.then(|| json(rename_fix)));
            format!(
                "{{\"file\":{},\"why\":{},\"red\":{red},\"fix\":{fix_json}}}",
                json(file),
                json(why)
            )
        })
        .collect();
    let mut orphans: Vec<_> = (fs::read_dir(&states).into_iter().flatten().flatten())
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.') && !dir.join(format!("{n}.md")).is_file())
        .collect();
    orphans.sort();
    let (archive, stamp) = (states.join(".archive"), iso(now).replace(['-', ':'], ""));
    let orphans: Vec<_> = (orphans.iter())
        .map(|name| {
            let (st, shown) = (states.join(name).display().to_string(), archive.display());
            let finding = format!("{st} has no {name}.md");
            let fix = format!(
                "mkdir -p {shown} && mv {st} {shown}/{name}-{stamp} — archive, never delete"
            );
            said[0].push(report("orphan", name, &finding, &fix));
            let (name, finding, fix) = (json(name), json(&finding), json(&fix));
            format!("{{\"name\":{name},\"finding\":{finding},\"fix\":{fix}}}")
        })
        .collect();
    let tick = tick(&states, now, heard, busy);
    if let Some((finding, fix)) = &tick.1 {
        said[0].insert(0, report(tick.0, "tick", finding, fix));
    }
    if as_json {
        println!(
            "{{\"version\":{},\"now\":{now},\"verdict\":\"{}\",\"tick\":{{\"verdict\":\"{}\",\"swept\":{},\"finding\":{},\"fix\":{}}},\"routines\":[{}],\"skipped\":[{}],\"orphans\":[{}]}}",
            json(crate::VERSION),
            if said[0].is_empty() { "ok" } else { "red" },
            tick.0,
            opt(swept.map(|at| at.to_string())),
            opt(tick.1.as_ref().map(|found| json(&found.0))),
            opt(tick.1.map(|found| json(&found.1))),
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
#[expect(
    clippy::indexing_slicing,
    reason = "main's usage gate passes a name first"
)]
#[expect(clippy::single_call_fn, reason = "a module per verb")]
pub fn ack(args: &[&str], now: i64) -> i32 {
    let (dir, states, _) = state::roots();
    let (name, why) = (args[0], &args[1..]);
    let file = states.join(name).join("ack");
    let (rc, msg) = if !known(&dir, name) {
        (64, format!("no routine {name}"))
    } else if why == ["--clear"] {
        drop(fs::remove_file(&file));
        return 0;
    } else if let Some(exit) = last_run(&states.join(name))
        .map(|last| last.2)
        .filter(|&exit| exit != 0)
    {
        if fs::write(
            &file,
            format!("{}\n", ack_json(exit, &iso(now), &why.join(" "))),
        )
        .is_ok()
        {
            return 0;
        }
        (73, format!("cannot write {}", file.display()))
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
#[expect(
    clippy::indexing_slicing,
    reason = "main's usage gate passes a name first"
)]
#[expect(clippy::single_call_fn, reason = "a module per verb")]
pub fn pause(on: bool, args: &[&str], now: i64) -> i32 {
    let (dir, states, _) = state::roots();
    let (name, st) = (args[0], states.join(args[0]));
    let file = st.join("paused");
    let (rc, msg) = if !known(&dir, name) {
        (64, format!("no routine {name}"))
    } else if on {
        let line = format!("{}\n", stamp("", &iso(now), &args[1..].join(" ")));
        if fs::create_dir_all(&st)
            .and_then(|()| fs::write(&file, line))
            .is_ok()
        {
            return 0;
        }
        (73, format!("cannot write {}", file.display()))
    } else if !file.is_file()
        || fs::remove_file(&file).is_ok()
            && fs::write(st.join("resumed"), format!("{now}\n")).is_ok()
    {
        return 0;
    } else {
        (73, format!("cannot lift {}", file.display()))
    };
    eprintln!("routine: {msg}");
    rc
}
