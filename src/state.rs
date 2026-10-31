//! What the runner reads back: its roots, its clock, `last-run.json` and the routine listing.

use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// bash `${VAR:-}`: unset and empty alike are absent.
pub fn var(name: &str) -> Option<OsString> {
    env::var_os(name).filter(|v| !v.is_empty())
}

/// `abspath "${VAR:-default}"`: a relative root joins the working directory. bash spells these
/// paths from `$PWD` (through a symlink where getcwd resolves one) with any doubled `/`, where
/// `join` and `current_dir` normalise them — the catch prompt's doc path and the lock refusal's
/// state dir print differently.
pub fn root(name: &str, default: PathBuf) -> PathBuf {
    env::current_dir()
        .unwrap_or_default()
        .join(var(name).map_or(default, PathBuf::from))
}

/// `(ROUTINE_DIR, ROUTINE_STATE, ROUTINE_HOME)`, the first two derived from the last unless
/// overridden.
pub fn roots() -> (PathBuf, PathBuf, PathBuf) {
    let home = root(
        "ROUTINE_HOME",
        PathBuf::from(var("HOME").unwrap_or_else(|| "/tmp".into())).join(".routines"),
    );
    (
        root("ROUTINE_DIR", home.clone()),
        root("ROUTINE_STATE", home.join(".state")),
        home,
    )
}

/// `ROUTINE_NOW` freezes the clock; `Err` carries a value that is no integer.
pub fn now() -> Result<i64, String> {
    var("ROUTINE_NOW").map_or_else(
        || {
            Ok(SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs().cast_signed()))
        },
        |v| {
            let v = v.to_string_lossy();
            // bash's own blanks only: `\v`, `\f`, `\r` or NBSP fail its arithmetic as well
            v.trim_matches([' ', '\t', '\n'])
                .parse()
                .map_err(|_| v.into_owned())
        },
    )
}

/// `sed 's/.*"key":"\{0,1\}\([^",}]*\)"\{0,1\}.*/\1/p'` under `$(…)`: per line, the value after
/// the last `"key":`, one line per matching line, trailing newlines stripped; empty when none.
pub fn json_get(file: &Path, key: &str) -> String {
    let pat = format!("\"{key}\":");
    let text = read(file);
    let vals: Vec<_> = text
        .split('\n')
        .filter_map(|l| {
            let v = &l[l.rfind(&pat)? + pat.len()..];
            let v = v.strip_prefix('"').unwrap_or(v);
            Some(&v[..v.find(['"', ',', '}']).unwrap_or(v.len())])
        })
        .collect();
    vals.join("\n").trim_end_matches('\n').to_owned()
}

/// Whole seconds since `p` was modified, a future stamp 0, as macOS find's `-mmin`/`-mtime`
/// count them (whole minutes and days, floored); `None` when `p` is gone. Never follows a symlink.
pub fn age(p: &Path) -> Option<u64> {
    let m = fs::symlink_metadata(p).ok()?.modified().ok()?;
    Some(m.elapsed().map_or(0, |d| d.as_secs()))
}

/// A routine name: `[a-z0-9-]`, never empty, never `-` first (`""|-*|*[!a-z0-9-]*`).
pub fn slug(n: &str) -> bool {
    !n.is_empty()
        && !n.starts_with('-')
        && n.bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-'))
}

/// A file's text as a shell's `$(cat …)` holds it: NUL bytes dropped, empty when unreadable.
pub fn read(file: &Path) -> String {
    String::from_utf8_lossy(&fs::read(file).unwrap_or_default()).replace('\0', "")
}

/// Each slug-named `*.md` document's name, in byte order; any other name is skipped, `skip`
/// told why — a dangling symlink, or a name that is not `[a-z0-9-]`, `-` first; any
/// other unreachable name (`[ -e ]` false, `[ -L ]` too: an unsearchable dir) is skipped silently.
pub fn routines(dir: &Path, skip: impl Fn(&str)) -> Vec<String> {
    let mut files: Vec<_> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|f| !f.starts_with('.'))
        .collect();
    files.sort();
    files
        .into_iter()
        .filter_map(|f| {
            let n = f.strip_suffix(".md")?;
            let p = dir.join(&f);
            let why = if !p.exists() {
                if !p.is_symlink() {
                    return None;
                }
                "dangling symlink"
            } else if !slug(n) {
                "name not a slug"
            } else {
                return Some(n.to_owned());
            };
            skip(&format!("skip {f}: {why}"));
            None
        })
        .collect()
}
