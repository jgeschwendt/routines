use std::{ffi::OsStr, os::unix::ffi::OsStrExt, process::Command};

// The usage text is the runner's own: every help spelling prints it byte for byte,
// and every verb not yet ported refuses with 64.
#[test]
fn help_is_bin_routines_and_other_verbs_wait() {
    fn run(args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> (Option<i32>, String, String) {
        let o = Command::new(env!("CARGO_BIN_EXE_routine"))
            .args(args)
            .output()
            .unwrap();
        (
            o.status.code(),
            String::from_utf8(o.stdout).unwrap(),
            String::from_utf8(o.stderr).unwrap(),
        )
    }
    let bash = Command::new("/bin/bash")
        .args(["bin/routine", "help"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    let usage = String::from_utf8(bash.stdout).unwrap();
    assert!(bash.status.success() && usage.starts_with("routine — "));
    for args in [&[][..], &[""], &["help"], &["-h"], &["--help"]] {
        assert_eq!(run(args), (Some(0), usage.clone(), String::new()));
    }
    // a non-UTF-8 verb is bash's 64 too, never a panic
    for (verb, shown) in [
        (OsStr::new("install"), "install"),
        (OsStr::from_bytes(b"\xff"), "\u{fffd}"),
    ] {
        assert_eq!(
            run([verb]),
            (
                Some(64),
                String::new(),
                format!("routine: {shown} is not ported yet — use bin/routine\n")
            )
        );
    }
}

// `routine status` is bin/routine's byte for byte — stdout, stderr and exit code — for every
// fixture home: missing, empty, and one holding each kind of row and each skipped name.
#[test]
fn status_is_bin_routines() {
    use std::{fs, fs::Permissions, os::unix::fs::PermissionsExt, os::unix::fs::symlink};
    let tmp = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("status");
    let lock = |mode| fs::set_permissions(tmp.join("locked"), Permissions::from_mode(mode));
    let _ = lock(0o700);
    let _ = fs::remove_dir_all(&tmp);
    let sched = |s: &str| format!("---\nschedule: \"{s}\"\n---\n");
    let hourly = sched("0 * * * *");
    let run = |epoch: &str| {
        format!(
            "{{\"exit\":0,\"finished\":\"2026-08-14T09:00:05Z\",\"duration\":5,\"started_epoch\":{epoch}}}\n"
        )
    };
    let (two, past, recent, ahead) = (
        "{\"exit\":\"\",\"duration\":3}\n{\"exit\":7,\"exit\":9\0}\n\n".to_owned(),
        run("1786690000"),
        run("1786698000"),
        run("1786699999"),
    );
    for (path, body) in [
        ("full/a-routine-name-past-21-columns.md", hourly.as_str()),
        ("full/never.md", &hourly),
        ("full/past.md", &hourly),
        ("full/.state/past/last-run.json", &past),
        ("full/recent.md", &hourly),
        ("full/.state/recent/last-run.json", &recent),
        ("full/ahead.md", &hourly),
        ("full/.state/ahead/last-run.json", &ahead),
        ("full/two.md", "no frontmatter\n"),
        ("full/.state/two/last-run.json", &two),
        ("full/.state/two/degraded", "since —\0 then\n\0\n"), // bash's $(…) drops NUL
        ("locked/a.md", &hourly),
        ("full/bad.md", &sched("@daily")),
        ("full/blank.md", &sched("")),
        ("full/AGENTS.md", &hourly),
        ("full/-flag.md", &hourly),
        ("full/.hidden.md", &hourly),
        ("full/notes.txt", &hourly),
    ] {
        let p = tmp.join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }
    fs::create_dir_all(tmp.join("full/dir.md")).unwrap();
    fs::create_dir_all(tmp.join("empty")).unwrap();
    symlink("nowhere", tmp.join("full/dangling.md")).unwrap();
    lock(0o600).unwrap(); // readable, unsearchable: a.md fails [ -e ] and [ -L ] alike
    let full = tmp.join("full").to_string_lossy().into_owned();
    let status = |prog: &str, env: &[(&str, &str)]| {
        let mut c = Command::new(prog);
        if prog == "/bin/bash" {
            c.arg(concat!(env!("CARGO_MANIFEST_DIR"), "/bin/routine"));
        }
        for k in "HOME ROUTINE_HOME ROUTINE_DIR ROUTINE_STATE ROUTINE_NOW".split(' ') {
            c.env_remove(k);
        }
        let o = c
            .arg("status")
            .envs(env.iter().copied())
            .current_dir(&tmp)
            .output()
            .unwrap();
        (
            o.status.code(),
            String::from_utf8(o.stdout).unwrap(),
            String::from_utf8(o.stderr).unwrap(),
        )
    };
    let now = ("ROUTINE_NOW", "1786699830"); // Fri 2026-08-14T09:30:30Z
    for env in [
        &[("ROUTINE_HOME", "missing"), now][..],
        &[("ROUTINE_HOME", "empty"), now],
        &[("ROUTINE_HOME", "full"), ("ROUTINE_NOW", " \t1786699830\n")],
        &[("ROUTINE_HOME", &full)],
        &[("ROUTINE_HOME", "locked"), now],
        &[
            ("HOME", ""),
            ("ROUTINE_DIR", "full"),
            ("ROUTINE_STATE", "full/.state"),
            now,
        ],
    ] {
        let rust = status(env!("CARGO_BIN_EXE_routine"), env);
        assert_eq!(rust, status("/bin/bash", env), "{env:?}");
        assert!(
            rust.1.lines().count() > 1 || !env[0].1.contains("full"),
            "{env:?}"
        );
    }
    lock(0o700).unwrap();
    // bash fails these too, naming its own line: only space, tab and newline are its blanks
    for v in ["x", "\u{a0}5", "\u{b}5", "\u{c}5", "5\r"] {
        let err = format!("routine: ROUTINE_NOW={v} is not an integer\n");
        let rust = status(env!("CARGO_BIN_EXE_routine"), &[("ROUTINE_NOW", v)]);
        assert_eq!(rust, (Some(64), String::new(), err));
    }
}
