//! `routine sync-ci` — writes this repo's own CI tick, compiled in, so every repo's copy is the
//! one this repo runs.

use std::{
    env,
    ffi::OsString,
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

const WORKFLOW: &str = include_str!("../.github/workflows/routines.yaml");

/// A shell's `$PWD`: the inherited `PWD` when absolute and naming the working directory — a
/// symlinked path, a trailing `/`, stays as given — else the resolved one. With no working
/// directory to resolve (deleted), the inherited `PWD`; with none of that either, `.`, never an
/// empty prefix that would put the workflow under `/`.
fn pwd() -> OsString {
    let id = |p: &Path| fs::metadata(p).map(|m| (m.dev(), m.ino())).ok();
    let pwd = env::var_os("PWD").filter(|p| !p.is_empty());
    match env::current_dir() {
        Err(_) => pwd.unwrap_or_else(|| ".".into()),
        Ok(cwd) => pwd
            .filter(|p| {
                Path::new(p).is_absolute() && id(p.as_ref()).is_some_and(|i| id(&cwd) == Some(i))
            })
            .unwrap_or_else(|| cwd.into()),
    }
}

/// Writes `.github/workflows/routines.yaml` under the working directory and drops the pre-.yaml
/// `routines.yml`, which would double-tick. A failed write warns and still exits 0.
pub fn sync_ci() {
    let mut dir = pwd();
    dir.push("/.github/workflows"); // appended as a shell does: a trailing `/` doubles
    let dir = PathBuf::from(dir);
    let wf = dir.join("routines.yaml");
    let _ = fs::create_dir_all(&dir);
    let _ = fs::remove_file(dir.join("routines.yml"));
    if let Err(e) = fs::write(&wf, WORKFLOW) {
        eprintln!("routine: {}: {e}", wf.display());
    }
    println!("wrote {}", wf.display());
}
