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
        (OsStr::new("run"), "run"),
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
        ("full/nul.md", "---\nschedule: 0 * * * *\0 5\n---\n"), // awk cuts at the NUL
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

// `routine sync-ci` is bin/routine's: the same workflow in the working directory, the legacy
// `.yml` gone, the same `wrote` line — a symlinked `PWD` kept as given, a trailing `/` too — and exit 0 when the
// write fails (a dir in its place).
#[test]
fn sync_ci_is_bin_routines() {
    use std::{fs, os::unix::fs::symlink};
    let tmp = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("sync-ci");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(tmp.join("real")).unwrap();
    symlink("real", tmp.join("link")).unwrap();
    let run = |prog: &str, blocked: bool, pwd: Option<&str>| {
        let name = format!("{}-{blocked}-{pwd:?}", prog.len());
        let wfs = tmp.join("real").join(&name).join(".github/workflows");
        fs::create_dir_all(&wfs).unwrap();
        fs::write(wfs.join("routines.yml"), "").unwrap();
        if blocked {
            fs::create_dir(wfs.join("routines.yaml")).unwrap();
        }
        let mut c = Command::new(prog);
        if prog == "/bin/bash" {
            c.arg(concat!(env!("CARGO_MANIFEST_DIR"), "/bin/routine"));
        }
        let cwd = tmp.join("link").join(&name);
        // without a matching PWD, both fall back to the resolved path
        let shown = pwd.map_or_else(
            || cwd.canonicalize().unwrap().display().to_string(),
            |suffix| format!("{}{suffix}", cwd.display()),
        );
        c.env("PWD", if pwd.is_some() { &shown } else { "relative" });
        let o = c.arg("sync-ci").current_dir(&cwd).output().unwrap();
        (
            o.status.code(),
            String::from_utf8(o.stdout)
                .unwrap()
                .replace(&shown, "<cwd>"),
            fs::read_to_string(wfs.join("routines.yaml")).ok(),
            wfs.join("routines.yml").exists(),
        )
    };
    for (blocked, pwd) in [
        (false, Some("")),
        (true, Some("")),
        (false, Some("/")),
        (false, None),
    ] {
        let rust = run(env!("CARGO_BIN_EXE_routine"), blocked, pwd);
        assert_eq!(rust, run("/bin/bash", blocked, pwd));
        assert_eq!(rust.1, "wrote <cwd>/.github/workflows/routines.yaml\n");
        assert_eq!(
            rust.2.is_some_and(|w| w.starts_with("name: routines\n")),
            !blocked
        );
    }
    // a deleted working directory: the inherited PWD as bash keeps it, else `.` — never `/`
    for (env, shown) in [("", "<cwd>"), ("-u PWD", ".")] {
        let gone = tmp.join(format!("gone{}", env.len()));
        fs::create_dir(&gone).unwrap();
        let g = gone.display();
        let o = Command::new("/bin/sh")
            .arg("-c")
            .arg(format!(
                "cd {g} && rmdir {g} && exec env {env} \"$0\" sync-ci"
            ))
            .arg(env!("CARGO_BIN_EXE_routine"))
            .output()
            .unwrap();
        let out = String::from_utf8(o.stdout)
            .unwrap()
            .replace(&*g.to_string(), "<cwd>");
        assert_eq!(
            (o.status.code(), out),
            (
                Some(0),
                format!("wrote {shown}/.github/workflows/routines.yaml\n")
            )
        );
    }
}

// `routine setup` writes bin/routine's tick byte for byte, bar the runner's path, beside hotswap's
// upgrade agent on the `routine` shim; `upgrade` swaps under the manifest through stubbed mise and
// launchctl, its drain refusing while a routine holds its lock; `setup --remove` undoes both.
#[test]
fn setup_and_upgrade() {
    use std::{fs, os::unix::fs::PermissionsExt, os::unix::fs::symlink, path::Path};
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR")).join("setup &<>");
    let (agents, bin) = (tmp.join("Library/LaunchAgents"), tmp.join("bin"));
    let shims = tmp.join(".local/share/mise/shims");
    let mode = |p: &Path, m| fs::set_permissions(p, fs::Permissions::from_mode(m));
    let _ = mode(&agents, 0o755);
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&shims).unwrap();
    fs::create_dir_all(&bin).unwrap();
    let exe = env!("CARGO_BIN_EXE_routine");
    symlink(exe, shims.join("routine")).unwrap();
    let ls =
        r#"{"packslip:jgeschwendt/routines":[{"active":true,"installed":true,"version":"1"}]}"#;
    let log = tmp.join("log");
    let stub = format!(
        "#!/bin/sh\necho \"${{0##*/}} $*\" >> '{}'\n[ \"$1\" != ls ] || echo '{ls}'\n",
        log.display()
    );
    for n in ["launchctl", "mise"] {
        fs::write(bin.join(n), &stub).unwrap();
        mode(&bin.join(n), 0o755).unwrap();
    }
    let uid = String::from_utf8(Command::new("id").arg("-u").output().unwrap().stdout).unwrap();
    // exit code, stdout, stderr, then the stubs' log
    let run = |prog: &str, args: &[&str], env: &[(&str, &str)]| {
        let mut c = Command::new(prog);
        for k in
            "MISE_DATA_DIR XDG_DATA_HOME ROUTINE_AGENTS_DIR ROUTINE_DIR ROUTINE_STATE".split(' ')
        {
            c.env_remove(k);
        }
        let o = (c.args(args).env("HOME", &tmp).current_dir(&tmp))
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env("ROUTINE_HOME", tmp.join("home"))
            .env("ROUTINE_LAUNCHCTL", bin.join("launchctl"))
            .envs(env.iter().copied())
            .output()
            .unwrap();
        let calls = fs::read_to_string(&log).unwrap_or_default();
        let _ = fs::remove_file(&log);
        let out = String::from_utf8([o.stdout, o.stderr].concat()).unwrap();
        format!("{}\n{out}{calls}", o.status.code().unwrap())
            .replace(&format!("gui/{}", uid.trim()), "gui/$UID")
    };
    let (due, up) = (
        agents.join("dev.gigantic.routines.due.plist"),
        agents.join("dev.gigantic.routines.upgrade.plist"),
    );
    let (d, u) = (due.display(), up.display());
    let bash = concat!(env!("CARGO_MANIFEST_DIR"), "/bin/routine");
    assert!(run(bash, &["install"], &[]).starts_with("0\n"));
    let read = |p: &Path| fs::read_to_string(p).unwrap();
    let tick = read(&due);
    let outs = "launchctl bootout gui/$UID/com.routines.due\nlaunchctl bootout gui/$UID/dev.gigantic.routines.due\n\
                launchctl bootout gui/$UID/dev.gigantic.routines.upgrade\n";
    let boot =
        format!("{outs}launchctl bootstrap gui/$UID {d}\nlaunchctl bootstrap gui/$UID {u}\n");
    assert_eq!(
        run(exe, &["setup"], &[]),
        format!("0\ninstalled {d}\ninstalled {u}\n{boot}")
    );
    assert_eq!(read(&due).replace(&format!("'{exe}'"), bash), tick);
    assert!(read(&up).contains("/shims/routine</string>"));
    let key = "packslip:jgeschwendt/routines";
    let look =
        format!("mise ls --json\nmise latest {key}\nmise upgrade --bump {key}\nmise ls --json\n");
    let kick = "launchctl kickstart -k gui/$UID/dev.gigantic.routines.due\n";
    let green = format!("0\nroutines 1: green, 1 smoke probes\n{look}{boot}{kick}");
    assert_eq!(run(exe, &["upgrade"], &[]), green);
    let rec = tmp.join(".local/state/hotswap/routines.json");
    assert!(rec.exists());
    let removed = format!("0\nremoved {d}\nremoved {u}\n{outs}");
    assert_eq!(run(exe, &["setup", "--remove"], &[]), removed);
    assert!(!due.exists() && !up.exists() && !rec.exists());
    fs::create_dir_all(tmp.join("home/.state/a/lock")).unwrap();
    let refused = run(exe, &["upgrade"], &[]);
    assert!(
        refused.starts_with("1\n")
            && refused.ends_with(&format!(" refused; nothing swapped\n{look}"))
    );
    let usage = run(exe, &["help"], &[]).replacen('0', "64", 1);
    assert_eq!(run(exe, &["setup", "--x"], &[]), usage);
    let fail = |args, env| run(exe, args, &[env]);
    assert_eq!(
        fail(&["setup"], ("HOME", "")),
        "1\nroutine: HOME unset — the tick PATH needs ~/.local/bin\n"
    );
    let lctl = ("ROUTINE_LAUNCHCTL", "/usr/bin/false");
    assert_eq!(
        fail(&["install"], lctl),
        "1\nroutine: launchctl bootstrap failed\n"
    );
    mode(&agents, 0o555).unwrap();
    let blocked = run(exe, &["setup"], &[]);
    mode(&agents, 0o755).unwrap();
    assert_eq!(blocked, format!("1\nroutine: cannot write {d}\n"));
}
