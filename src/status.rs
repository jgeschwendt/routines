//! `routine status` — schedule × due? × last run, one row per routine.

use crate::{cron, doc, state};
use std::{fs, path::Path};

/// Pads each text by display width as `LC_ALL=C` counts it: bytes, but `—`
/// as one column. The last text is unpadded.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "each three-byte dash takes two off the length: never below zero"
)]
fn cells(texts: [&str; 6]) -> String {
    let mut row = String::new();
    for (t, w) in texts.iter().zip([21usize, 17, 7, 6, 22]) {
        row += t;
        row += &" ".repeat(w.saturating_sub(t.len() - 2 * t.matches('—').count()));
    }
    format!("{row}{}\n", texts[5])
}

/// `name`'s schedule, empty when none; the doc keeps its NULs, where `frontmatter_get` cuts a
/// line as awk does.
pub fn schedule(dir: &Path, name: &str) -> String {
    let text = fs::read(dir.join(format!("{name}.md"))).unwrap_or_default();
    doc::frontmatter_get(&String::from_utf8_lossy(&text), "schedule").unwrap_or_default()
}

// ※ due-derivation
/// When `name` next fires: after its last start, or 0 (due now) when it never ran or its record
/// is stamped ahead of the clock; `None` for no schedule or one the parser rejects.
pub fn next_fire_of(states: &Path, name: &str, sched: &str, now: i64) -> Option<i64> {
    let se = state::json_get(&states.join(name).join("last-run.json"), "started_epoch");
    cron::num(&se).filter(|&se| se <= now).map_or_else(
        || cron::next_fire_after(now, sched).map(|_| 0),
        |se| cron::next_fire_after(se, sched),
    )
}

#[expect(clippy::single_call_fn, reason = "a module per verb")]
pub fn status(dir: &Path, states: &Path, now: i64) {
    let or_dash = |s: String| if s.is_empty() { "—".into() } else { s };
    print!(
        "{}",
        cells(["NAME", "SCHEDULE", "DUE", "EXIT", "FINISHED", "SECS"])
    );
    for name in state::routines(dir, |f, why| eprintln!("routine: skip {f}: {why}")) {
        let sched = schedule(dir, &name);
        let (st, last) = (states.join(&name), states.join(&name).join("last-run.json"));
        let due = if st.join("paused").is_file() {
            "paused"
        } else if sched.is_empty() {
            "n/a"
        } else if cron::next_fire_after(now, &sched).is_none() {
            "bad"
        } else if next_fire_of(states, &name, &sched, now).is_some_and(|n| n <= now) {
            "due"
        } else {
            "—"
        };
        let deg = st.join("degraded");
        let deg = if deg.is_file() {
            format!(
                " · degraded since {}",
                state::read(&deg).trim_end_matches('\n')
            )
        } else {
            String::new()
        };
        print!(
            "{}",
            cells([
                &name,
                &or_dash(sched),
                due,
                &or_dash(state::json_get(&last, "exit")),
                &or_dash(state::json_get(&last, "finished")),
                &format!("{}{deg}", or_dash(state::json_get(&last, "duration"))),
            ])
        );
    }
}
