//! `routine sync-ci` — `cmd_sync_ci` of bin/routine. The workflow is bin/routine's own heredoc,
//! compiled in, so the two cannot drift; the cutover moves the text here.

use std::{env, ffi::OsString, fs, os::unix::fs::MetadataExt, path::PathBuf};

const SRC: &str = include_str!("../bin/routine");

/// bash's `$PWD`: the inherited `PWD` when absolute and naming the working directory — a
/// symlinked path, a trailing `/`, stays as given — else the resolved one.
fn pwd() -> OsString {
    let id = |p: &PathBuf| fs::metadata(p).map(|m| (m.dev(), m.ino())).ok();
    env::var_os("PWD")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && id(p).is_some_and(|i| id(&".".into()) == Some(i)))
        .unwrap_or_else(|| env::current_dir().unwrap_or_default())
        .into_os_string()
}

/// Writes `.github/workflows/routines.yaml` under the working directory and drops the pre-.yaml
/// `routines.yml`, which would double-tick. A failed write warns and still exits 0, as bash's.
pub fn sync_ci() {
    let mut dir = pwd();
    dir.push("/.github/workflows"); // appended as bash does: `$PWD/` doubles a trailing slash
    let dir = PathBuf::from(dir);
    let wf = dir.join("routines.yaml");
    let body = SRC.split_once("cat > \"$wf\" <<'EOF'\n").unwrap().1;
    let _ = fs::create_dir_all(&dir);
    let _ = fs::remove_file(dir.join("routines.yml"));
    if let Err(e) = fs::write(&wf, &body[..=body.find("\nEOF\n").unwrap()]) {
        eprintln!("routine: {}: {e}", wf.display());
    }
    println!("wrote {}", wf.display());
}
