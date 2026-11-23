//! `routine sync-ci` — writes this repo's own CI tick, compiled in, so every repo's copy is the
//! one this repo runs.

use std::{
    env,
    ffi::OsString,
    fs,
    os::unix::fs::MetadataExt as _,
    path::{Path, PathBuf},
};

const WORKFLOW: &str = include_str!("../.github/workflows/routines.yaml");

/// A shell's `$PWD`: the inherited `PWD` when absolute and naming the working directory — a
/// symlinked path, a trailing `/`, stays as given — else the resolved one. With no working
/// directory to resolve (deleted), the inherited `PWD`; with none of that either, `.`, never an
/// empty prefix that would put the workflow under `/`.
#[expect(
    clippy::single_call_fn,
    reason = "a shell's $PWD, its rules documented on their own"
)]
fn pwd() -> OsString {
    let id = |path: &Path| {
        fs::metadata(path)
            .map(|metadata| (metadata.dev(), metadata.ino()))
            .ok()
    };
    let pwd = env::var_os("PWD").filter(|inherited| !inherited.is_empty());
    match env::current_dir() {
        Err(_) => pwd.unwrap_or_else(|| ".".into()),
        Ok(cwd) => pwd
            .filter(|inherited| {
                Path::new(inherited).is_absolute()
                    && id(inherited.as_ref()).is_some_and(|i| id(&cwd) == Some(i))
            })
            .unwrap_or_else(|| cwd.into()),
    }
}

/// Writes `.github/workflows/routines.yaml` under the working directory and drops the pre-.yaml
/// `routines.yml`, which would double-tick. A failed write warns and still exits 0.
#[expect(clippy::single_call_fn, reason = "a module per verb")]
pub fn sync_ci() {
    let mut dir = pwd();
    dir.push("/.github/workflows"); // appended as a shell does: a trailing `/` doubles
    let dir = PathBuf::from(dir);
    let wf = dir.join("routines.yaml");
    drop(fs::create_dir_all(&dir));
    drop(fs::remove_file(dir.join("routines.yml")));
    if let Err(error) = fs::write(&wf, WORKFLOW) {
        eprintln!("routine: {}: {error}", wf.display());
    }
    println!("wrote {}", wf.display());
}
