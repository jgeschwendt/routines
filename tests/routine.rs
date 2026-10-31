use std::{ffi::OsStr, os::unix::ffi::OsStrExt, process::Command};

// The usage text is the runner's own: every help spelling prints it byte for byte, and an
// unknown verb names itself above it on stderr with bash's 64.
#[test]
fn help_is_bin_routines() {
    fn run(args: &[&OsStr], bash: bool) -> (Option<i32>, String, String) {
        let mut c = Command::new(if bash { "/bin/bash" } else { PORT });
        let o = (if bash { c.arg(BASH) } else { &mut c })
            .args(args)
            .output()
            .unwrap();
        (
            o.status.code(),
            String::from_utf8(o.stdout).unwrap(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
        )
    }
    let usage = run(&[], true).1;
    assert!(usage.starts_with("routine — "));
    for args in [
        &[][..],
        &[""],
        &["help"],
        &["-h"],
        &["--help"],
        &["bogus", "x"],
    ] {
        let args: Vec<_> = args.iter().map(OsStr::new).collect();
        assert_eq!(run(&args, false), run(&args, true));
    }
    // a non-UTF-8 verb reads lossily, never a panic
    let ff = run(&[OsStr::from_bytes(b"\xff")], false);
    assert_eq!(
        ff,
        (
            Some(64),
            String::new(),
            format!("routine: unknown command \u{fffd}\n{usage}")
        )
    );
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

const BASH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/bin/routine");
const PORT: &str = env!("CARGO_BIN_EXE_routine");

/// A fresh dir under the target's tmp, with the claude stub every run calls: `MODE` hangs the
/// auth probe (`hang`), logs out (`out`), outlives the deadline past its TERM-ignoring probe
/// into a TERM-honouring agent (`gap`), repairs (`fix`) or fails the agent (`fails`).
fn sandbox(name: &str) -> std::path::PathBuf {
    use std::{fs, os::unix::fs::PermissionsExt};
    let tmp = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = Command::new("chmod")
        .arg("-R")
        .arg("u+w")
        .arg(&tmp)
        .output();
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    let stub = r#"#!/bin/sh
if [ "$1" = auth ]; then
  case "${MODE:-}" in
    hang) exec sleep 30 ;;
    gap) trap '' TERM; until ls "$TMPDIR"/*/timedout >/dev/null 2>&1; do sleep 0.1; done ;;
    out) exit 1 ;;
  esac
  exit 0
fi
cat > /dev/null
case "${MODE:-}" in gap) exec sleep 30 ;; fix) : > "$ROUTINE_STATE_DIR/fixed" ;; fails) exit 1 ;; esac
"#;
    fs::write(tmp.join("claude"), stub).unwrap();
    fs::set_permissions(tmp.join("claude"), fs::Permissions::from_mode(0o755)).unwrap();
    tmp
}

/// `<home>/<name>.md`: the frontmatter lines, then one sh block unless `None`.
fn routine_doc(home: &std::path::Path, name: &str, fm: &str, block: Option<&str>) {
    let fm = if fm.is_empty() {
        String::new()
    } else {
        format!("---\n{fm}\n---\n")
    };
    let block = block.map_or(String::new(), |b| format!("```sh\n{b}\n```\n"));
    std::fs::create_dir_all(home.join("tmp")).unwrap();
    std::fs::write(home.join(format!("{name}.md")), fm + "prose\n" + &block).unwrap();
}

/// `routine <args>` under `prog` against `home`, the clock frozen, claude the sandbox's stub
/// and `TMPDIR` the home's own.
fn routine_cmd(
    prog: &str,
    home: &std::path::Path,
    args: &[&str],
    env: &[(&str, String)],
) -> Command {
    let mut c = Command::new(if prog == BASH { "/bin/bash" } else { prog });
    if prog == BASH {
        c.arg(BASH);
    }
    for k in "ROUTINE_DIR ROUTINE_STATE MODE".split(' ') {
        c.env_remove(k);
    }
    c.args(args)
        .env("ROUTINE_HOME", home)
        .env("ROUTINE_NOW", "1786698000")
        .env("ROUTINE_CLAUDE", home.parent().unwrap().join("claude"))
        .env("TMPDIR", home.join("tmp"))
        .envs(env.iter().map(|(k, v)| (k, v)));
    c
}

/// `b` with `home` as `<h>`, bash's own error lines dropped and each elapsed second (`· 3s`,
/// `elapsed=3s`) as `N`: bash's `$SECONDS` and std's clock may straddle a second apart.
fn masked(b: &[u8], home: &std::path::Path) -> String {
    // bash's `cd` names its line and mktemp its template; src/AGENTS.md lists both
    let s = String::from_utf8_lossy(b).replace(&*home.to_string_lossy(), "<h>");
    let s: String = (s
        .split_inclusive('\n')
        .filter(|l| !l.starts_with(BASH) && !l.starts_with("mktemp:")))
    .collect();
    let (mut out, mut digits) = (String::new(), String::new());
    for c in s.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        if c == 's' && !digits.is_empty() && out.ends_with([' ', '=']) {
            digits = "N".into();
        }
        out += &digits;
        digits.clear();
        out.push(c);
    }
    out + &digits
}

/// Whether `pid` (in `file`) still runs, given three seconds to go.
fn alive(file: &std::path::Path) -> bool {
    let p = std::fs::read_to_string(file).unwrap();
    (0..30).all(|_| {
        std::thread::sleep(std::time::Duration::from_millis(100));
        Command::new("kill")
            .args(["-0", p.trim()])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
    })
}

/// (name, frontmatter, block, extra env) for a home at `h`: each gate, a pass, a fail, each
/// catch outcome, a runaway block, a leftover child, no document, no name.
type Case = (
    &'static str,
    &'static str,
    Option<&'static str>,
    Vec<(&'static str, String)>,
);

fn run_cases(h: &str) -> [Case; 18] {
    let fail = "on_error: fail";
    let fixed = r#"[ -f "$ROUTINE_STATE_DIR/fixed" ] || exit 7"#;
    let leak = r#"sleep 30 & echo $! > "$ROUTINE_STATE_DIR/leak.pid""#;
    [
        (
            "ok",
            fail,
            Some("echo out; echo err >&2; pwd; echo \"$LC_ALL $ROUTINE_NAME\""),
            vec![],
        ),
        ("fail", fail, Some("echo x; exit 3"), vec![]),
        ("none", fail, None, vec![]),
        ("onerr", "on_error: Fail", Some("true"), vec![]),
        (
            "req",
            "requires: LC_ALL ROUTINE_PMSET NOPE_X\non_error: fail",
            Some("true"),
            vec![],
        ),
        (
            "nohome",
            fail,
            Some("true"),
            vec![
                ("ROUTINE_HOME", format!("{h}/gone")),
                ("ROUTINE_DIR", h.into()),
                ("ROUTINE_STATE", format!("{h}/.state")),
            ],
        ),
        (
            "notmp",
            fail,
            Some("true"),
            vec![("TMPDIR", format!("{h}/gone"))],
        ),
        ("ro", fail, Some("true"), vec![]),
        ("held", fail, Some("true"), vec![]),
        ("Bad_Name", fail, Some("true"), vec![]),
        (
            "unavail",
            "",
            Some("exit 5"),
            vec![("ROUTINE_CLAUDE", format!("{h}/no-claude"))],
        ),
        (
            "loggedout",
            "",
            Some("exit 5"),
            vec![("MODE", "out".into())],
        ),
        ("repair", "", Some(fixed), vec![("MODE", "fix".into())]),
        (
            "unrepaired",
            "",
            Some("exit 7"),
            vec![("MODE", "fails".into())],
        ),
        (
            "big",
            fail,
            Some("head -c 1100000 /dev/zero | tr '\\0' x"),
            vec![],
        ),
        ("leak", fail, Some(leak), vec![]),
        ("gone", "", None, vec![]),
        ("", "", None, vec![]),
    ]
}

// `routine run <name>` is bin/routine's: exit code, stdout (each elapsed second masked), stderr,
// the record, the degraded marker, what the state dir holds and the tick journal — for every
// gate, a pass, a fail, each catch outcome and a runaway block's cut copy.
#[test]
fn run_is_bin_routines() {
    use std::{fs, os::unix::fs::PermissionsExt};
    let tmp = sandbox("run");
    let sides = [BASH, PORT].map(|prog| {
        let home = tmp.join(if prog == BASH { "sh" } else { "rs" });
        let h = home.to_string_lossy().into_owned();
        let cases = run_cases(&h);
        let st = home.join(".state");
        cases
            .map(|(name, fm, block, env)| {
                if !matches!(name, "gone" | "") {
                    routine_doc(&home, name, fm, block);
                }
                let ro = |m| fs::set_permissions(st.join("ro"), fs::Permissions::from_mode(m));
                match name {
                    "ro" => {
                        fs::create_dir_all(st.join("ro")).unwrap();
                        ro(0o555).unwrap();
                    }
                    "held" => {
                        fs::create_dir_all(st.join("held/lock")).unwrap();
                        fs::write(st.join("held/lock/pid"), "1\n").unwrap(); // EPERM: alive
                    }
                    "repair" | "unrepaired" => {
                        fs::create_dir_all(st.join(name)).unwrap();
                        fs::write(st.join(name).join("degraded"), "x\n").unwrap();
                    }
                    _ => {}
                }
                let args: &[&str] = if name.is_empty() {
                    &["run"]
                } else {
                    &["run", name]
                };
                let o = routine_cmd(prog, &home, args, &env).output().unwrap();
                let _ = ro(0o755);
                if name == "leak" {
                    assert!(
                        !alive(&st.join("leak/leak.pid")),
                        "{prog}: a block's leftover dies with it"
                    );
                }
                let mut names: Vec<_> = (fs::read_dir(st.join(name)).into_iter().flatten())
                    .map(|e| e.unwrap().file_name().into_string().unwrap())
                    .filter(|n| n != "leak.pid")
                    .collect();
                names.sort();
                (
                    name,
                    o.status.code(),
                    masked(&o.stdout, &home),
                    masked(&o.stderr, &home),
                    fs::read_to_string(st.join(name).join("last-run.json")).ok(),
                    names,
                    fs::read_dir(home.join("tmp")).unwrap().count(),
                )
            })
            .into_iter()
            .chain([(
                "tick.log",
                None,
                masked(&fs::read(st.join("tick.log")).unwrap(), &home),
                String::new(),
                None,
                vec![],
                0,
            )])
            .collect::<Vec<_>>()
    });
    for (sh, rs) in sides[0].iter().zip(&sides[1]) {
        assert_eq!(sh, rs);
    }
    let rs = |i: usize| &sides[1][i];
    assert_eq!(
        (rs(0).1, rs(1).1, rs(4).1, rs(8).1, rs(12).1),
        (Some(0), Some(3), Some(78), Some(75), Some(0))
    );
    assert!(rs(0).2.contains("\nout\nerr\n<h>\nC ok\n"));
    assert!(
        rs(14)
            .2
            .contains("… 51424 bytes cut — the last 1048576 follow\n")
    );
    assert_eq!(
        rs(13).5,
        ["degraded", "last-run.json", "logs", "runs.jsonl"][..]
    );
}

// A HUP, INT or TERM to a running `routine run` records 129, 130 or 143 as bin/routine does,
// stops the block's group — its TERM-ignoring member KILLed two seconds on — and releases the
// lock; a second TERM half a second in cuts nothing short.
#[test]
fn run_stops_on_a_signal() {
    use std::{fs, thread, time::Duration};
    let tmp = sandbox("signal");
    let block =
        r#"sh -c 'trap "" TERM; echo $$ > "$ROUTINE_STATE_DIR/member.pid"; exec sleep 30' & wait"#;
    thread::scope(|s| {
        for prog in [BASH, PORT] {
            for (sig, code) in [("HUP", 129), ("INT", 130), ("TERM", 143)] {
                let home = tmp.join(format!("{}-{sig}", prog.len()));
                routine_doc(&home, "sig", "on_error: fail", Some(block));
                s.spawn(move || {
                    let mut c = routine_cmd(prog, &home, &["run", "sig"], &[])
                        .stdout(std::process::Stdio::null())
                        .spawn()
                        .unwrap();
                    let st = home.join(".state/sig");
                    while !st.join("member.pid").exists() {
                        thread::sleep(Duration::from_millis(50));
                    }
                    let pid = c.id().to_string();
                    let kill = |sig: &str| {
                        Command::new("kill")
                            .args([&format!("-{sig}"), &pid])
                            .status()
                    };
                    kill(sig).unwrap();
                    if sig == "TERM" {
                        thread::sleep(Duration::from_millis(500));
                        let _ = kill(sig);
                    }
                    let rc = c.wait().unwrap().code();
                    let rec = fs::read_to_string(st.join("last-run.json")).unwrap();
                    assert_eq!(rc, Some(code), "{prog} {sig}");
                    assert!(rec.contains(&format!("\"exit\":{code},")), "{prog} {sig}");
                    assert!(!st.join("lock").exists() && !alive(&st.join("member.pid")));
                });
            }
        }
    });
}

// Past `timeout` the watchdog TERMs the block (124, its output and exit divider kept), KILLs a
// TERM-ignoring member ten seconds on, ends a hung auth probe as a timeout (no degraded marker),
// and TERMs an agent forked past the deadline, skipping the retry — bin/routine alike.
#[test]
fn run_times_out() {
    use std::{fs, thread};
    let tmp = sandbox("timeout");
    let stubborn =
        r#"sh -c 'trap "" TERM; echo $$ > "$ROUTINE_STATE_DIR/member.pid"; exec sleep 30' & wait"#;
    // (name, block, MODE, what the output must hold)
    let cases = [
        ("slow", "echo SLOW; sleep 30", "", "SLOW\n"),
        ("stubborn", stubborn, "", "escalating to KILL"),
        ("late", "exit 3", "hang", "watchdog: deadline reached"),
        ("gap", "exit 3", "gap", "claude exit=143"),
    ];
    thread::scope(|s| {
        for prog in [BASH, PORT] {
            for (name, block, mode, want) in cases {
                let home = tmp.join(format!("{}-{name}", prog.len()));
                routine_doc(&home, name, "timeout: 1", Some(block));
                s.spawn(move || {
                    let env = [("MODE", mode.to_owned())];
                    let o = routine_cmd(prog, &home, &["run", name], &env)
                        .output()
                        .unwrap();
                    let out = String::from_utf8_lossy(&o.stdout);
                    let st = home.join(".state").join(name);
                    let rec = fs::read_to_string(st.join("last-run.json")).unwrap();
                    assert_eq!(o.status.code(), Some(124), "{prog} {name}");
                    assert!(
                        out.contains(want) && rec.contains("\"exit\":124,"),
                        "{prog} {name}: {out}"
                    );
                    assert!(
                        !st.join("degraded").exists() && !out.contains("retry"),
                        "{prog} {name}"
                    );
                    assert!(name != "slow" || out.contains("block 1 · exit 143"));
                    assert!(name != "stubborn" || !alive(&st.join("member.pid")));
                });
            }
        }
    });
}

/// What a tick leaves in `home`'s state: tick.log, then each entry's name, and for a routine
/// its record and what its dir holds.
fn due_state(home: &std::path::Path) -> Vec<String> {
    use std::fs;
    let st = home.join(".state");
    let mut out = vec![masked(&fs::read(st.join("tick.log")).unwrap(), home)];
    let mut names: Vec<_> = (fs::read_dir(&st).unwrap())
        .map(|e| e.unwrap().path())
        .collect();
    names.sort();
    for p in names {
        let mut inner: Vec<_> = (fs::read_dir(&p).into_iter().flatten())
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| n != "pid")
            .collect();
        inner.sort();
        let rec = fs::read_to_string(p.join("last-run.json")).unwrap_or_default();
        let size = fs::metadata(&p).unwrap().len() * u64::from(p.is_file());
        out.push(
            format!("{} {size} {inner:?} {rec}", p.display())
                .replace(&*home.to_string_lossy(), "<h>"),
        );
    }
    out
}

/// A tick's home: `docs` a line per routine, `name|schedule|block` (an empty block `true`);
/// `later` and `yearly` ran at 09:00, `ahead` past the clock, `c`'s lock is held, and a dark
/// home holds a NUL schedule, a non-doc, a dangling link and a due.log past 5 MB.
fn due_home(home: &std::path::Path, docs: &str) {
    use std::{fs, os::unix::fs::symlink};
    let st = home.join(".state");
    for doc in docs.lines() {
        let [name, sched, block] = [0, 1, 2].map(|i| doc.split('|').nth(i).unwrap());
        let fm = format!("schedule: \"{sched}\"\non_error: fail");
        let block = if block.is_empty() { "true" } else { block };
        routine_doc(home, name, &fm, Some(block));
        let dir = st.join(name);
        let epoch = match name {
            "c" => {
                fs::create_dir_all(dir.join("lock")).unwrap();
                fs::write(dir.join("lock/pid"), "1\n").unwrap(); // EPERM: alive
                continue;
            }
            "later" | "yearly" => 1_786_698_000,
            "ahead" => 1_786_699_999,
            _ => continue,
        };
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("last-run.json"),
            format!("{{\"started_epoch\":{epoch}}}\n"),
        )
        .unwrap();
    }
    if docs.contains("Not_Slug") {
        fs::write(home.join("nul.md"), "---\nschedule: 0 * * * *\0 5\n---\n").unwrap();
        fs::write(home.join("notes.txt"), "").unwrap();
        symlink("gone.md", home.join("dangling.md")).unwrap();
        fs::write(st.join("due.log"), "x".repeat(5_242_881)).unwrap();
    }
}

// `routine run --due` is bin/routine's: exit code, stdout and stderr (line by line, the
// routines being concurrent), tick.log, and each routine's record and state — an empty home,
// nothing due, one due under a full wake, several with one failing and one held under a pmset
// that fails open, and a dark wake that defers, over every kind of skipped, bad and
// unscheduled doc and a due.log past 5 MB.
#[test]
fn due_is_bin_routines() {
    use std::{fs, os::unix::fs::PermissionsExt};
    let tmp = sandbox("due");
    let pmset = |name: &str, body: &str| {
        let p = tmp.join(name);
        fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        p.to_string_lossy().into_owned()
    };
    let caps = "Current System Capabilities are:";
    let (dark, lit, open) = (
        pmset("dark", &format!("echo '{caps} CPU Network'")),
        pmset("lit", &format!("printf 'x\\n{caps} CPU Graphics\\n'")),
        pmset("open", "echo 'Current Power State: 4'; exit 1"),
    );
    let t = "2026-08-14T09:00:00Z";
    // (case, pmset, docs, what tick.log must hold)
    let cases = [
        ("empty", &dark, "", " due=- ".into()),
        (
            "none",
            &dark,
            "manual||\nlater|0 0 1 1 *|",
            " due=- ".into(),
        ),
        (
            "one",
            &lit,
            "hourly|0 * * * *|echo out; echo err >&2",
            "dispatched hourly exit=0\n".into(),
        ),
        (
            "several",
            &open,
            "a|0 * * * *|echo a\nb|0 * * * *|exit 3\nc|0 * * * *|",
            format!("dispatched b exit=3\n{t} dispatched c exit=75\n"),
        ),
        (
            "dark",
            &dark,
            "hourly|0 * * * *|\nyearly|0 0 1 1 *|\nahead|0 0 1 1 *|\nbad|@daily|\nblank||\nNot_Slug|0 * * * *|",
            format!(
                " due=ahead,hourly,nul next=bad@bad,blank@unscheduled,yearly@2027-01-01T00:00:00Z\n\
                 {t} defer dark-wake due=ahead,hourly,nul\n"
            ),
        ),
    ];
    for (case, pmset, docs, want) in cases {
        let [sh, rs] = [BASH, PORT].map(|prog| {
            let home = tmp.join(format!("{case}-{}", prog.len()));
            due_home(&home, docs);
            let env = [("ROUTINE_PMSET", pmset.clone())];
            let o = routine_cmd(prog, &home, &["run", "--due"], &env)
                .output()
                .unwrap();
            let lines = |b: &[u8]| {
                let mut l: Vec<_> = masked(b, &home).lines().map(String::from).collect();
                l.sort();
                l
            };
            (
                o.status.code(),
                lines(&o.stdout),
                lines(&o.stderr),
                due_state(&home),
            )
        });
        assert_eq!(sh, rs, "{case}");
        assert_eq!(rs.0, Some(i32::from(case == "several")), "{case}");
        assert!(rs.3[0].contains(&want), "{case}: {}", rs.3[0]);
    }
}

// A TERM to the tick alone (launchd's stop), or an INT to its whole group (a ^C), reaches each
// running routine as TERM — a background routine ignores INT — which records 143 and kills its
// block; the tick exits 143 or 130 with no `dispatched` line, as bin/routine does.
#[test]
fn due_stops_on_a_signal() {
    use std::{os::unix::process::CommandExt, thread, time::Duration};
    let tmp = sandbox("due-signal");
    let block = r#"echo $$ > "$ROUTINE_STATE_DIR/pid"; exec sleep 30"#;
    let sides = [BASH, PORT].map(|prog| {
        [("TERM", false, 143), ("INT", true, 130)].map(|(sig, group, code)| {
            let home = tmp.join(format!("{}-{sig}", prog.len()));
            for n in ["a", "b"] {
                routine_doc(
                    &home,
                    n,
                    "schedule: \"* * * * *\"\non_error: fail",
                    Some(block),
                );
            }
            let mut c = routine_cmd(prog, &home, &["run", "--due"], &[])
                .process_group(0)
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap();
            let st = home.join(".state");
            while !(st.join("a/pid").exists() && st.join("b/pid").exists()) {
                thread::sleep(Duration::from_millis(50));
            }
            let pid = format!("{}{}", if group { "-" } else { "" }, c.id());
            Command::new("kill")
                .args([&format!("-{sig}"), "--", &pid])
                .status()
                .unwrap();
            assert_eq!(c.wait().unwrap().code(), Some(code), "{prog} {sig}");
            assert!(!alive(&st.join("a/pid")) && !alive(&st.join("b/pid")));
            let state = due_state(&home);
            assert!(state[1].contains("\"exit\":143,") && state[2].contains("\"exit\":143,"));
            state
        })
    });
    assert_eq!(sides[0], sides[1]);
    assert!(!sides[1][0][0].contains("dispatched"));
}
