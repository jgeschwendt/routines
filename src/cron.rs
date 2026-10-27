//! Five-field cron in UTC — `civil`, `iso`, `isnum`, `expand` and `next_fire_after` of bin/routine.

/// `(year, month, day, weekday)` of `epoch` in UTC, weekday 0 = Sunday — Hinnant's `civil_from_days`.
pub fn civil(epoch: i64) -> (i64, i64, i64, i64) {
    let mut z = epoch / 86400 + 719_468; // days from 0000-03-01, a Wednesday
    let (wday, era) = ((z + 3) % 7, z / 146_097);
    z %= 146_097;
    let mut year = (z - z / 1460 + z / 36524 - z / 146_096) / 365;
    z -= 365 * year + year / 4 - year / 100;
    let mp = (5 * z + 2) / 153;
    let month = (mp + 2) % 12 + 1;
    year += era * 400 + i64::from(month < 3);
    (year, month, z - (153 * mp + 2) / 5 + 1, wday)
}

/// The one timestamp shape every log line, log name and JSON field carries.
#[cfg_attr(not(test), expect(dead_code, reason = "stage 3 (run <name>) wires it"))]
pub fn iso(epoch: i64) -> String {
    let (y, m, d, _) = civil(epoch);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        epoch % 86400 / 3600,
        epoch % 3600 / 60,
        epoch % 60
    )
}

/// 1-18 ASCII digits: bash `$((…))` wraps from 2^63, so 18 is the safe cap.
pub fn num(s: &str) -> Option<i64> {
    (s.len() < 19 && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok())?
}

/// The values one field allows, or `None` when it is no field this parser takes.
fn expand(field: &str, lo: i64, hi: i64) -> Option<Vec<i64>> {
    // bash's `IFS=, set -- $field`: one trailing comma splits to nothing
    let mut parts: Vec<_> = field.split(',').collect();
    if parts.last() == Some(&"") {
        parts.pop();
    }
    let mut out = Vec::new();
    for part in parts {
        // N/step runs N to the field's end, N/1 too — the slash, not the step, says so;
        // the head ends at the first slash and the step starts after the last
        let (head, step, end) = match (part.split_once('/'), part.rsplit_once('/')) {
            (Some((head, _)), Some((_, step))) => (head, step, Some(hi)),
            _ => (part, "1", None),
        };
        let step = num(step).filter(|&s| s >= 1)?;
        let (s, e) = match head.split_once('-') {
            _ if head == "*" => (lo, hi),
            Some((s, e)) => (num(s)?, num(e)?),
            None => num(head).map(|s| (s, end.unwrap_or(s)))?,
        };
        if s < lo || e > hi || s > e {
            return None;
        }
        out.extend((s..=e).step_by(usize::try_from(step).ok()?));
    }
    (!out.is_empty()).then_some(out)
}

/// The first fire strictly after `start`, within eight years (Feb 29 recurs every four,
/// and 2100 skips one); `None` when `expr` is no schedule this parser takes or never fires.
pub fn next_fire_after(start: i64, expr: &str) -> Option<i64> {
    // bash's default IFS: blanks around and between fields collapse
    let fields: Vec<_> = expr
        .split([' ', '\t', '\n'])
        .filter(|f| !f.is_empty())
        .collect();
    let [min, hour, dom, mon, dow] = fields[..] else {
        return None;
    };
    let (minutes, hours, days, months, mut weekdays) = (
        expand(min, 0, 59)?,
        expand(hour, 0, 23)?,
        expand(dom, 1, 31)?,
        expand(mon, 1, 12)?,
        expand(dow, 0, 7)?,
    );
    if weekdays.contains(&7) {
        weekdays.push(0);
    }
    // cronie's DOM_STAR/DOW_STAR: a day field opening with * (*/2 too) ANDs with the
    // other; only two restricted fields OR together
    let both = i64::from(dom.starts_with('*') || dow.starts_with('*'));
    let midnight = start - start % 86400;
    let mut d = 0;
    while d <= 2922 {
        let de = midnight + d * 86400;
        d += 1;
        let (_, m, md, wd) = civil(de);
        // a dead month jumps to its 28th, then walks — never past a next month's 1st
        if !months.contains(&m) {
            d += (27 - md).max(0);
            continue;
        }
        if i64::from(days.contains(&md)) + i64::from(weekdays.contains(&wd)) <= both {
            continue;
        }
        let fire = hours
            .iter()
            .flat_map(|h| minutes.iter().map(move |m| de + h * 3600 + m * 60))
            .filter(|&t| t > start)
            .min();
        if fire.is_some() {
            return fire;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{civil, iso, next_fire_after};
    use std::{io::Write, process::Command};

    #[test]
    fn civil_and_iso() {
        for (t, s, w) in [
            (0, "1970-01-01T00:00:00Z", 4),
            (951_782_400, "2000-02-29T00:00:00Z", 2), // 2000 is a leap year: 400 divides it
            (1_835_404_263, "2028-02-29T02:31:03Z", 2),
            (4_105_036_800, "2100-01-31T00:00:00Z", 0),
            (4_107_542_400, "2100-03-01T00:00:00Z", 1), // 2100 is not: Feb 28 runs into Mar 1
        ] {
            assert_eq!((iso(t).as_str(), civil(t).3), (s, w));
        }
    }

    // (start, schedule, next fire) — every case is also run through the bash functions
    // sliced out of bin/routine, so the table is the runner's own answer, not a recollection.
    const CASES: &[(i64, &str, Option<i64>)] = &[
        (1_786_698_300, "0 * * * *", Some(1_786_701_600)), // Fri 2026-08-14T09:05Z → 10:00Z
        (1_786_698_900, "0 * * * *", Some(1_786_701_600)),
        (1_786_701_600, "0 * * * *", Some(1_786_705_200)), // strictly after: a fire time is past
        (1_786_698_300, "*/15 * * * *", Some(1_786_698_900)),
        (1_786_606_260, "30 7 * * 1-5", Some(1_786_692_600)), // Thu 07:31Z → Fri 07:30Z
        (1_786_795_200, "30 7 * * 1-5", Some(1_786_951_800)), // Sat → Mon
        (1_791_763_200, "0 0 */2 * 1", Some(1_792_368_000)), // */N ANDs (cronie): Mon 19th, not Wed 14th
        (1_791_763_200, "0 0 1-31 * 1", Some(1_791_849_600)), // two restricted fields OR: every day
        (1_791_763_200, "0 0 13 * 5", Some(1_791_849_600)),  // the 13th OR a Friday
        (1_786_699_830, "30/1 * * * *", Some(1_786_699_860)), // N/step runs N to 59
        (1_786_795_200, "0 9 * * 7", Some(1_786_870_800)),   // 7 is Sunday too
        (1_786_795_200, "0 9 * * 0", Some(1_786_870_800)),
        (1_786_795_200, "0 9 * * */7", Some(1_786_870_800)),
        (1_792_800_000, "0 0 * * 7-7", Some(1_792_886_400)),
        (1_835_404_260, "30 2 29 2 *", Some(1_961_634_600)), // Feb 29 2028 → 2032
        (4_105_036_800, "0 0 29 2 *", Some(4_233_686_400)), // 2100 skips one: 2104, inside the horizon
        (1_792_800_000, "0 0 30 2 *", None), // never fires: the walk ends at eight years
        (1_792_800_000, "0 0 31 4 *", None),
        (1_792_800_000, "0 9 */31 2 3", Some(2_022_397_200)), // Wed Feb 1 2034, years out but valid
        (1_792_800_000, "0 0 * 2 1-5", Some(1_801_440_000)),  // a dead month jumps, then walks
        (1_792_800_000, "0 12 * 1,3 *", Some(1_798_804_800)),
        (1_792_800_000, "59 23 31 12 *", Some(1_798_761_540)),
        (1_792_800_000, "0-59/30 0,12 1 */3 6", Some(1_792_801_800)),
        (1_792_800_000, "0 0 1 1 1/7", Some(1_798_761_600)),
        (1_792_800_000, "5 4 * * 3-3/1", Some(1_793_160_300)),
        (1_792_800_000, "08 08 * * *", Some(1_792_829_280)), // leading zeros are decimal
        (1_792_800_000, "*/60 * * * *", Some(1_792_803_600)),
        (1_792_800_000, "1/2/3 * * * *", Some(1_792_800_060)), // head to the first slash, step after the last
        (1_792_800_000, "5, * * * *", Some(1_792_800_300)),    // a trailing comma splits to nothing
        (1_792_800_000, "  0  *\t* * *\n", Some(1_792_803_600)), // IFS blanks collapse
        (1_792_800_000, "0\r * * * *", None),                  // a CR is no blank
        (1_792_800_000, "@daily", None),
        (1_792_800_000, "18446744073709551617 * * * *", None), // 2^64+1: never wrapped to minute 1
        (1_792_800_000, "999999999999999999 * * * *", None),
        (1_792_800_000, ",5 * * * *", None),
        (1_792_800_000, "1,,2 * * * *", None),
        (1_792_800_000, ", * * * *", None),
        (1_792_800_000, "5- * * * *", None),
        (1_792_800_000, "*-5 * * * *", None),
        (1_792_800_000, "+5 * * * *", None),
        (1_792_800_000, "60 * * * *", None),
        (1_792_800_000, "0/0 * * * *", None),
        (1_792_800_000, "5-3 * * * *", None),
        (1_792_800_000, "0 24 * * *", None),
        (1_792_800_000, "0 0 0 * *", None),
        (1_792_800_000, "0 0 * 13 *", None),
        (1_792_800_000, "0 0 * * 8", None),
        (1_792_800_000, "0 * * *", None),
        (1_792_800_000, "0 * * * * *", None),
    ];

    #[test]
    fn next_fire_after_matches_the_table_and_bash() {
        let mut bash = Command::new("/bin/bash")
            .args(["-c", r#"export LC_ALL=C; eval "$(sed -n '/^civil()/,/^}/p;/^inset()/p;/^isnum()/p;/^expand()/,/^}/p;/^next_fire_after()/,/^}/p' bin/routine)"
                while IFS='|' read -r -d '' t e; do next_fire_after "$t" "$e" || echo none; done"#])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        for (t, e, _) in CASES {
            write!(bash.stdin.as_ref().unwrap(), "{t}|{e}\0").unwrap();
        }
        drop(bash.stdin.take());
        let out = String::from_utf8(bash.wait_with_output().unwrap().stdout).unwrap();
        let theirs: Vec<_> = out.lines().map(|l| l.parse().ok()).collect();
        let ours: Vec<_> = CASES
            .iter()
            .map(|&(t, e, _)| next_fire_after(t, e))
            .collect();
        let want: Vec<_> = CASES.iter().map(|c| c.2).collect();
        assert_eq!((&ours, &theirs), (&want, &want));
    }
}
