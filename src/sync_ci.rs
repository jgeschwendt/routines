//! `routine sync-ci` — `cmd_sync_ci` of bin/routine. The workflow is bin/routine's own heredoc,
//! compiled in, so the two cannot drift; the cutover moves the text here.

use std::{env, fs};

const SRC: &str = include_str!("../bin/routine");

/// Writes `.github/workflows/routines.yaml` under the working directory and drops the pre-.yaml
/// `routines.yml`, which would double-tick. A failed write warns and still exits 0, as bash's.
pub fn sync_ci() {
    let dir = env::current_dir()
        .unwrap_or_default()
        .join(".github/workflows");
    let wf = dir.join("routines.yaml");
    let body = SRC.split_once("cat > \"$wf\" <<'EOF'\n").unwrap().1;
    let _ = fs::create_dir_all(&dir);
    let _ = fs::remove_file(dir.join("routines.yml"));
    if let Err(e) = fs::write(&wf, &body[..=body.find("\nEOF\n").unwrap()]) {
        eprintln!("routine: {}: {e}", wf.display());
    }
    println!("wrote {}", wf.display());
}
