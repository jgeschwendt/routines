//! `routine status` — schedule × due? × last run, one row per routine: `cells`, `cmd_status`,
//! `next_fire_of` and `is_due` of bin/routine.

use crate::{cron, doc, state};
use std::{fs, path::Path};

/// Pads each text by display width as bin/routine counts it under `LC_ALL=C`: bytes, but `—`
/// as one column. The last text is unpadded.
fn cells(texts: [&str; 6]) -> String {
    let mut row = String::new();
    for (t, w) in texts.iter().zip([21_usize, 17, 5, 6, 22]) {
        row += t;
        row += &" ".repeat(w.saturating_sub(t.len() - 2 * t.matches('—').count()));
    }
    row + texts[5] + "\n"
}

pub fn status(dir: &Path, states: &Path, now: i64) {
    let or_dash = |s: String| if s.is_empty() { "—".into() } else { s };
    print!(
        "{}",
        cells(["NAME", "SCHEDULE", "DUE", "EXIT", "FINISHED", "SECS"])
    );
    for name in state::routines(dir) {
        let text = fs::read(dir.join(format!("{name}.md"))).unwrap_or_default();
        let sched =
            doc::frontmatter_get(&String::from_utf8_lossy(&text), "schedule").unwrap_or_default();
        let (st, last) = (states.join(&name), states.join(&name).join("last-run.json"));
        // a record stamped ahead of the clock, or none (never run ⇒ due), derives from now
        let due = || {
            cron::num(&state::json_get(&last, "started_epoch"))
                .filter(|&se| se <= now)
                .map_or(Some(0), |se| cron::next_fire_after(se, &sched))
                .is_some_and(|nxt| nxt <= now)
        };
        let due = if sched.is_empty() {
            "n/a"
        } else if cron::next_fire_after(now, &sched).is_none() {
            "bad"
        } else if due() {
            "due"
        } else {
            "—"
        };
        let deg = st.join("degraded");
        let deg = if deg.is_file() {
            let since = String::from_utf8_lossy(&fs::read(&deg).unwrap_or_default()).into_owned();
            format!(" · degraded since {}", since.trim_end_matches('\n'))
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
                &(or_dash(state::json_get(&last, "duration")) + &deg),
            ])
        );
    }
}
