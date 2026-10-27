use std::process::Command;

// The usage text is the runner's own: every help spelling prints it byte for byte,
// and every verb not yet ported refuses with 64.
#[test]
fn help_is_bin_routines_and_other_verbs_wait() {
    let run = |args: &[&str]| {
        let o = Command::new(env!("CARGO_BIN_EXE_routine"))
            .args(args)
            .output()
            .unwrap();
        (
            o.status.code(),
            String::from_utf8(o.stdout).unwrap(),
            String::from_utf8(o.stderr).unwrap(),
        )
    };
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
    assert_eq!(
        run(&["install"]),
        (
            Some(64),
            String::new(),
            "routine: install is not ported yet — use bin/routine\n".into()
        )
    );
}

// `routine status` is bin/routine's byte for byte — stdout, stderr and exit code — for every
// fixture home: missing, empty, and one holding each kind of row and each skipped name.
#[test]
fn status_is_bin_routines() {
    use std::{fs, os::unix::fs::symlink};
    let tmp = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("status");
    let _ = fs::remove_dir_all(&tmp);
    let sched = |s: &str| format!("---\nschedule: \"{s}\"\n---\n");
    let hourly = sched("0 * * * *");
    let run = |epoch: &str| {
        format!(
            "{{\"exit\":0,\"finished\":\"2026-08-14T09:00:05Z\",\"duration\":5,\"started_epoch\":{epoch}}}\n"
        )
    };
    let (two, past, recent, ahead) = (
        "{\"exit\":\"\",\"duration\":3}\n{\"exit\":7,\"exit\":9}\n\n".to_owned(),
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
        ("full/.state/two/degraded", "since — then\n\n"),
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
    let full = tmp.join("full").to_string_lossy().into_owned();
    let status = |prog: &str, env: &[(&str, &str)]| {
        let mut c = Command::new(prog);
        if prog == "/bin/bash" {
            c.arg(concat!(env!("CARGO_MANIFEST_DIR"), "/bin/routine"));
        }
        for k in [
            "HOME",
            "ROUTINE_HOME",
            "ROUTINE_DIR",
            "ROUTINE_STATE",
            "ROUTINE_NOW",
        ] {
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
        &[("ROUTINE_HOME", "full"), now],
        &[("ROUTINE_HOME", &full)],
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
    assert_eq!(
        status(env!("CARGO_BIN_EXE_routine"), &[("ROUTINE_NOW", "x")]),
        (
            Some(64),
            String::new(),
            "routine: ROUTINE_NOW=x is not an integer\n".into()
        )
    );
}
