#![expect(
    clippy::too_many_lines,
    reason = "a test is one table of literal expectations"
)]

use std::{
    ffi::OsStr,
    os::unix::ffi::OsStrExt,
    path::Path,
    process::{Command, Output},
};

const EXE: &str = env!("CARGO_BIN_EXE_routine");

/// Exit code, stdout, stderr.
fn out(o: Output) -> (Option<i32>, String, String) {
    (
        o.status.code(),
        String::from_utf8(o.stdout).unwrap(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

/// `label` ruled out to 80 columns: bytes, a `·` one column — the runner's divider.
fn rule(label: &str) -> String {
    let n = 70_usize.saturating_sub(label.len() - label.matches('·').count());
    format!("──────── {label} {}\n", "─".repeat(n))
}

// Every help spelling prints the usage; an unknown verb names itself above it on stderr with 64,
// a non-UTF-8 one lossily; `--version` names the release.
#[test]
fn help() {
    let run = |args: &[&[u8]]| {
        out(Command::new(EXE)
            .args(args.iter().map(|a| OsStr::from_bytes(a)))
            .output()
            .unwrap())
    };
    let usage = run(&[]).1;
    assert!(usage.starts_with("routine — executable markdown on rails\n"));
    for args in [&[&b""[..]][..], &[b"help"], &[b"-h"], &[b"--help"]] {
        assert_eq!(run(args), (Some(0), usage.clone(), String::new()));
    }
    for (verb, shown) in [(&b"bogus"[..], "bogus"), (b"\xff", "\u{fffd}")] {
        let err = format!("routine: unknown command {shown}\n{usage}");
        assert_eq!(run(&[verb, b"x"]), (Some(64), String::new(), err));
    }
    let version = option_env!("ROUTINE_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));
    let v = (Some(0), format!("routine {version}\n"), String::new());
    assert_eq!((run(&[b"--version"]), run(&[b"-V"])), (v.clone(), v));
}

// `routine status` — stdout, stderr and exit code — for every fixture home: missing, empty, and
// one holding each kind of row and each skipped name.
#[test]
fn status() {
    use std::{fs, fs::Permissions, os::unix::fs::PermissionsExt, os::unix::fs::symlink};
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR")).join("status");
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
        ("full/.state/two/degraded", "since —\0 then\n\0\n"), // a NUL drops out
        ("locked/a.md", &hourly),
        ("full/bad.md", &sched("@daily")),
        ("full/nul.md", "---\nschedule: 0 * * * *\0 5\n---\n"), // the line ends at the NUL
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
    let status = |env: &[(&str, &str)]| {
        let mut c = Command::new(EXE);
        for k in "HOME ROUTINE_HOME ROUTINE_DIR ROUTINE_STATE ROUTINE_NOW".split(' ') {
            c.env_remove(k);
        }
        out(c
            .arg("status")
            .envs(env.iter().copied())
            .current_dir(&tmp)
            .output()
            .unwrap())
    };
    let now = ("ROUTINE_NOW", "1786699830"); // Fri 2026-08-14T09:30:30Z
    let head = "NAME                 SCHEDULE         DUE  EXIT  FINISHED              SECS\n";
    // without ROUTINE_NOW the clock is real, long past recent's next fire
    let rows = |recent: &str| {
        format!(
            "{head}a-routine-name-past-21-columns0 * * * *        due  —     —                     —
ahead                0 * * * *        due  0     2026-08-14T09:00:05Z  5
bad                  @daily           bad  —     —                     —
blank                —                n/a  —     —                     —
dir                  —                n/a  —     —                     —
never                0 * * * *        due  —     —                     —
nul                  0 * * * *        due  —     —                     —
past                 0 * * * *        due  0     2026-08-14T09:00:05Z  5
recent               0 * * * *        {recent}0     2026-08-14T09:00:05Z  5
two                  —                n/a  \n9    —                     3 · degraded since since — then
"
        )
    };
    let skips = "routine: skip -flag.md: name not a slug\nroutine: skip AGENTS.md: name not a slug\n\
                 routine: skip dangling.md: dangling symlink\n";
    for (env, want) in [
        (&[("ROUTINE_HOME", "missing"), now][..], None),
        (&[("ROUTINE_HOME", "empty"), now], None),
        (
            &[("ROUTINE_HOME", "full"), ("ROUTINE_NOW", " \t1786699830\n")],
            Some("—    "),
        ),
        (&[("ROUTINE_HOME", &full)], Some("due  ")),
        (&[("ROUTINE_HOME", "locked"), now], None),
        (
            &[
                ("HOME", ""),
                ("ROUTINE_DIR", "full"),
                ("ROUTINE_STATE", "full/.state"),
                now,
            ],
            Some("—    "),
        ),
    ] {
        let want = want.map_or((head.to_owned(), ""), |r| (rows(r), skips));
        assert_eq!(status(env), (Some(0), want.0, want.1.to_owned()), "{env:?}");
    }
    lock(0o700).unwrap();
    // only space, tab and newline are blanks
    for v in ["x", "\u{a0}5", "\u{b}5", "\u{c}5", "5\r"] {
        let err = format!("routine: ROUTINE_NOW={v} is not an integer\n");
        assert_eq!(
            status(&[("ROUTINE_NOW", v)]),
            (Some(64), String::new(), err)
        );
    }
}

// `routine sync-ci` writes this repo's own tick in the working directory, the legacy `.yml` gone,
// and names it — a symlinked `PWD` kept as given, a trailing `/` too — exiting 0 when the write
// fails (a dir in its place).
#[test]
fn sync_ci() {
    use std::{fs, os::unix::fs::symlink};
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR")).join("sync-ci");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(tmp.join("real")).unwrap();
    symlink("real", tmp.join("link")).unwrap();
    let run = |blocked: bool, pwd: Option<&str>| {
        let name = format!("{blocked}-{pwd:?}");
        let wfs = tmp.join("real").join(&name).join(".github/workflows");
        fs::create_dir_all(&wfs).unwrap();
        fs::write(wfs.join("routines.yml"), "").unwrap();
        if blocked {
            fs::create_dir(wfs.join("routines.yaml")).unwrap();
        }
        let mut c = Command::new(EXE);
        let cwd = tmp.join("link").join(&name);
        // without a matching PWD, the resolved path
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
        let wrote = "wrote <cwd>/.github/workflows/routines.yaml\n".into();
        let wf = include_str!("../.github/workflows/routines.yaml");
        let want = (Some(0), wrote, (!blocked).then(|| wf.into()), false);
        assert_eq!(run(blocked, pwd), want);
    }
    // a deleted working directory: the inherited PWD, else `.` — never `/`
    for (env, shown) in [("", "<cwd>"), ("-u PWD", ".")] {
        let gone = tmp.join(format!("gone{}", env.len()));
        fs::create_dir(&gone).unwrap();
        let g = gone.display();
        let o = Command::new("/bin/sh")
            .arg("-c")
            .arg(format!(
                "cd {g} && rmdir {g} && exec env {env} \"$0\" sync-ci"
            ))
            .arg(EXE)
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

// `routine setup` refuses without mise's shim, links `~/.local/bin/routine` to it, writes the tick
// on the shim beside hotswap's upgrade agent and loads both, then reloads a changed tick and
// leaves current ones be; `upgrade` swaps under the manifest through stubbed mise and launchctl,
// its drain refusing while a routine holds its lock; `setup --remove` undoes it all.
#[test]
fn setup_and_upgrade() {
    use std::{fs, os::unix::fs::PermissionsExt, os::unix::fs::symlink};
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR")).join("setup &<>");
    let (agents, bin) = (tmp.join("Library/LaunchAgents"), tmp.join("bin"));
    let (shims, local) = (tmp.join(".local/share/mise/shims"), tmp.join(".local/bin"));
    let mode = |p: &Path, m| fs::set_permissions(p, fs::Permissions::from_mode(m));
    let _ = mode(&agents, 0o755);
    let _ = mode(&local, 0o755);
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&shims).unwrap();
    fs::create_dir_all(&bin).unwrap();
    let ls =
        r#"{"packslip:jgeschwendt/routines":[{"active":true,"installed":true,"version":"1"}]}"#;
    let (log, loaded) = (tmp.join("log"), tmp.join("loaded"));
    let stub = format!(
        "#!/bin/sh\necho \"${{0##*/}} $*\" >> '{}'\n[ \"$1\" != ls ] || echo '{ls}'\n\
         [ \"$1\" != print ] || [ -e '{}' ]\n",
        log.display(),
        loaded.display()
    );
    for n in ["launchctl", "mise"] {
        fs::write(bin.join(n), &stub).unwrap();
        mode(&bin.join(n), 0o755).unwrap();
    }
    let uid = String::from_utf8(Command::new("id").arg("-u").output().unwrap().stdout).unwrap();
    // exit code, stdout, stderr, then the stubs' log
    let run = |args: &[&str], env: &[(&str, &str)]| {
        let mut c = Command::new(EXE);
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
    let (shim, link) = (shims.join("routine"), local.join("routine"));
    assert_eq!(
        run(&["setup"], &[]),
        format!(
            "1\nroutine: no mise shim at {}: mise use -g packslip:jgeschwendt/routines first\n",
            shim.display()
        )
    );
    symlink(EXE, &shim).unwrap();
    let (due, up) = (
        agents.join("dev.gigantic.routines.due.plist"),
        agents.join("dev.gigantic.routines.upgrade.plist"),
    );
    let (d, u) = (due.display(), up.display());
    let linked = format!("linked {} → {}\n", link.display(), shim.display());
    let (old, print) = (
        "launchctl bootout gui/$UID/com.routines.due\n",
        "launchctl print gui/$UID/dev.gigantic.routines.",
    );
    assert_eq!(
        run(&["setup"], &[]),
        format!(
            "0\n{linked}loaded {d}\nloaded {u}\n{old}{print}due\nlaunchctl bootstrap gui/$UID {d}\n\
             {print}upgrade\nlaunchctl bootstrap gui/$UID {u}\n"
        )
    );
    let read = |p: &Path| fs::read_to_string(p).unwrap();
    let esc = |p: &Path| {
        let p = p.display().to_string().replace('&', "&amp;");
        p.replace('<', "&lt;").replace('>', "&gt;")
    };
    let (t, s) = (esc(&tmp), esc(&shims));
    assert_eq!(
        read(&due),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>dev.gigantic.routines.due</string>
  <key>ProgramArguments</key>
  <array>
    <string>{s}/routine</string>
    <string>run</string>
    <string>--due</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key><string>{t}/.local/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
    <key>ROUTINE_HOME</key><string>{t}/home</string>
    <key>ROUTINE_DIR</key><string>{t}/home</string>
    <key>ROUTINE_STATE</key><string>{t}/home/.state</string>
  </dict>
  <key>StartInterval</key><integer>60</integer>
  <key>RunAtLoad</key><true/>
  <key>ProcessType</key><string>Background</string>
  <key>StandardOutPath</key><string>{t}/home/.state/due.log</string>
  <key>StandardErrorPath</key><string>{t}/home/.state/due.log</string>
</dict>
</plist>
"#
        )
    );
    assert!(read(&up).contains(&format!("<string>{s}/routine</string>")));
    assert_eq!(fs::read_link(&link).unwrap(), shim);
    // a checkout's tick, loaded: booted out and bootstrapped on the shim; the rest stays put
    fs::write(&loaded, "").unwrap();
    fs::write(&due, "exec bin/routine run --due").unwrap();
    let reload = format!(
        "0\n{linked}reloaded {d}\ncurrent {u}\n{old}{print}due\n\
         launchctl bootout gui/$UID/dev.gigantic.routines.due\nlaunchctl bootstrap gui/$UID {d}\n\
         {print}upgrade\n"
    );
    assert_eq!(run(&["install"], &[]), reload);
    let probes = format!("{old}{print}due\n{print}upgrade\n");
    let current = format!("{linked}current {d}\ncurrent {u}\n{probes}");
    assert_eq!(run(&["setup"], &[]), format!("0\n{current}"));
    let key = "packslip:jgeschwendt/routines";
    let look =
        format!("mise ls --json\nmise latest {key}\nmise upgrade --bump {key}\nmise ls --json\n");
    let kick = "launchctl kickstart -k gui/$UID/dev.gigantic.routines.due\n";
    let green = format!("0\nroutines 1: green, 1 smoke probes\n{look}{probes}{kick}");
    assert_eq!(run(&["upgrade"], &[]), green);
    let rec = tmp.join(".local/state/hotswap/routines.json");
    assert!(rec.exists());
    let removed = format!(
        "0\nremoved {d}\nremoved {u}\n{old}launchctl bootout gui/$UID/dev.gigantic.routines.due\n\
         launchctl bootout gui/$UID/dev.gigantic.routines.upgrade\n"
    );
    assert_eq!(run(&["uninstall"], &[]), removed);
    assert!(!due.exists() && !up.exists() && !rec.exists() && !link.exists());
    fs::create_dir_all(tmp.join("home/.state/a/lock")).unwrap();
    let refused = run(&["upgrade"], &[]);
    assert!(
        refused.starts_with("1\n")
            && refused.ends_with(&format!(" refused; nothing swapped\n{look}"))
    );
    let usage = run(&["help"], &[]).replacen('0', "64", 1);
    assert_eq!(run(&["setup", "--x"], &[]), usage);
    let fail = |args, env| run(args, &[env]);
    assert_eq!(
        fail(&["setup", "--remove"], ("HOME", "")),
        "1\nroutine: HOME unset — the tick PATH needs ~/.local/bin\n"
    );
    let lctl = ("ROUTINE_LAUNCHCTL", "/usr/bin/false");
    assert_eq!(
        fail(&["setup"], lctl),
        format!("1\n{linked}routine: launchctl bootstrap failed\n")
    );
    mode(&agents, 0o555).unwrap();
    let blocked = run(&["setup"], &[]);
    mode(&agents, 0o755).unwrap();
    assert_eq!(
        blocked,
        format!("1\n{linked}routine: cannot write {d}\n{old}")
    );
    mode(&local, 0o555).unwrap();
    let unlinked = run(&["setup"], &[]);
    mode(&local, 0o755).unwrap();
    let err = format!("cannot link {}: File exists (os error 17)", link.display());
    assert_eq!(unlinked, format!("1\nroutine: {err}\n{old}"));
}

/// A fresh dir under the target's tmp, with the claude stub every run calls: `MODE` hangs the
/// auth probe (`hang`), logs out (`out`), outlives the deadline past its TERM-ignoring probe
/// into a TERM-honouring agent (`gap`), repairs (`fix`) or fails the agent (`fails`).
fn sandbox(name: &str) -> std::path::PathBuf {
    use std::{fs, os::unix::fs::PermissionsExt};
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
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
fn routine_doc(home: &Path, name: &str, fm: &str, block: Option<&str>) {
    let fm = if fm.is_empty() {
        String::new()
    } else {
        format!("---\n{fm}\n---\n")
    };
    let block = block.map_or(String::new(), |b| format!("```sh\n{b}\n```\n"));
    std::fs::create_dir_all(home.join("tmp")).unwrap();
    std::fs::write(home.join(format!("{name}.md")), fm + "prose\n" + &block).unwrap();
}

/// `routine <args>` against `home`, the clock frozen, claude the sandbox's stub and `TMPDIR` the
/// home's own.
fn routine_cmd(home: &Path, args: &[&str], env: &[(&str, String)]) -> Command {
    let mut c = Command::new(EXE);
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

/// `b` with `home` as `<h>` and each elapsed second (`· 3s`, `elapsed=3s`) as `N`.
fn masked(b: &[u8], home: &Path) -> String {
    let s = String::from_utf8_lossy(b).replace(&*home.to_string_lossy(), "<h>");
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
fn alive(file: &Path) -> bool {
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

// `routine run <name>`: exit code, stdout (each elapsed second masked), stderr, the record, what
// the state dir holds and the temp dirs left — for every gate, a pass, a fail, each catch outcome
// and a runaway block's cut copy — then the tick journal's refusals.
#[test]
fn run() {
    use std::{fs, os::unix::fs::PermissionsExt};
    let tmp = sandbox("run");
    let home = tmp.join("home");
    let h = home.to_string_lossy().into_owned();
    let st = home.join(".state");
    let usage = String::from_utf8(Command::new(EXE).output().unwrap().stdout).unwrap();
    let got = run_cases(&h).map(|(name, fm, block, env)| {
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
        let o = routine_cmd(&home, args, &env).output().unwrap();
        let _ = ro(0o755);
        if name == "leak" {
            assert!(
                !alive(&st.join("leak/leak.pid")),
                "a block's leftover dies with it"
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
            names.join(" "),
            fs::read_dir(home.join("tmp")).unwrap().count(),
        )
    });
    let rec = |exit: i32, degraded: bool| {
        Some(format!(
            "{{\"started\":\"2026-08-14T09:00:00Z\",\"started_epoch\":1786698000,\"finished\":\
             \"2026-08-14T09:00:00Z\",\"exit\":{exit},\"duration\":0,\"degraded\":{degraded}}}\n"
        ))
    };
    // a run's dividers around `body`, which ends in block 1's own
    let ran = |name: &str, body: &str, exit: i32| {
        let open = rule(&format!("routine {name} · 2026-08-14T09:00:00Z"));
        let close = rule(&format!("routine {name} · exit {exit}"));
        (
            format!("{open}{}{body}{close}", rule("block 1")),
            String::new(),
        )
    };
    let exit = |n: i32| rule(&format!("block 1 · exit {n} · Ns"));
    let caught = |n: i32| rule(&format!("block 1 · caught exit {n} · Ns"));
    let catch = |agent: i32, n: i32| {
        format!(
            "{}{}catch: claude exit={agent} elapsed=Ns\n{}{}",
            exit(7),
            rule("block 1 · catch"),
            rule("block 1 · retry"),
            caught(n)
        )
    };
    let degraded = |why: &str| format!("{}catch degraded: claude {why}\n{}", exit(5), caught(5));
    let big = format!(
        "… 51424 bytes cut — the last 1048576 follow\n{}{}",
        "x".repeat(1_048_576),
        exit(0)
    );
    let refused = |e: &str| (String::new(), format!("routine: {e}\n"));
    let (all, deg) = (
        "last-run.json logs runs.jsonl",
        "degraded last-run.json logs runs.jsonl",
    );
    let want = [
        (
            "ok",
            0,
            ran("ok", &format!("out\nerr\n<h>\nC ok\n{}", exit(0)), 0),
            rec(0, false),
            all,
        ),
        (
            "fail",
            3,
            ran("fail", &format!("x\n{}{}", exit(3), caught(3)), 3),
            rec(3, false),
            all,
        ),
        (
            "none",
            64,
            refused("no sh or bash blocks in <h>/none.md"),
            rec(64, false),
            all,
        ),
        (
            "onerr",
            64,
            refused("on_error Fail is neither claude nor fail in <h>/onerr.md"),
            rec(64, false),
            all,
        ),
        (
            "req",
            78,
            refused("req missing required env: NOPE_X"),
            rec(78, false),
            "last-run.json runs.jsonl",
        ),
        (
            "nohome",
            64,
            refused("no routines home <h>/gone"),
            rec(64, false),
            all,
        ),
        (
            "notmp",
            73,
            refused("mktemp failed under <h>/gone"),
            rec(73, false),
            all,
        ),
        ("ro", 73, refused("cannot write <h>/.state/ro"), None, ""),
        (
            "held",
            75,
            refused("held already running (pid 1)"),
            None,
            "lock lock-blocked.log",
        ),
        (
            "Bad_Name",
            64,
            refused("invalid routine name: Bad_Name"),
            None,
            "",
        ),
        (
            "unavail",
            5,
            ran("unavail", &degraded("unavailable"), 5),
            rec(5, true),
            deg,
        ),
        (
            "loggedout",
            5,
            ran("loggedout", &degraded("not logged in"), 5),
            rec(5, true),
            deg,
        ),
        (
            "repair",
            0,
            ran("repair", &catch(0, 0), 0),
            rec(0, false),
            "fixed last-run.json logs runs.jsonl",
        ),
        (
            "unrepaired",
            7,
            ran("unrepaired", &catch(1, 7), 7),
            rec(7, true),
            deg,
        ),
        ("big", 0, ran("big", &big, 0), rec(0, false), all),
        ("leak", 0, ran("leak", &exit(0), 0), rec(0, false), all),
        ("gone", 64, refused("no routine gone"), None, ""),
        (
            "",
            64,
            (String::new(), usage),
            None,
            "big fail held leak loggedout nohome none notmp ok onerr repair req ro tick.log \
             unavail unrepaired",
        ),
    ];
    for (got, (name, code, (stdout, stderr), rec, names)) in got.iter().zip(want) {
        let want = (name, Some(code), stdout, stderr, rec, names.to_owned(), 0);
        assert_eq!(got, &want, "{name}");
    }
    assert_eq!(
        masked(&fs::read(st.join("tick.log")).unwrap(), &home),
        "2026-08-14T09:00:00Z run req refused: missing required env: NOPE_X\n\
         2026-08-14T09:00:00Z run ro refused: <h>/.state/ro not writable\n\
         2026-08-14T09:00:00Z run refused: invalid routine name: Bad_Name\n\
         2026-08-14T09:00:00Z run refused: no routine gone (<h>/gone.md)\n"
    );
}

// A HUP, INT or TERM to a running `routine run` records 129, 130 or 143, stops the block's group — its TERM-ignoring member KILLed two seconds on — and releases the
// lock; a second TERM half a second in cuts nothing short.
#[test]
fn run_stops_on_a_signal() {
    use std::{fs, thread, time::Duration};
    let tmp = sandbox("signal");
    let block =
        r#"sh -c 'trap "" TERM; echo $$ > "$ROUTINE_STATE_DIR/member.pid"; exec sleep 30' & wait"#;
    thread::scope(|s| {
        for (sig, code) in [("HUP", 129), ("INT", 130), ("TERM", 143)] {
            let home = tmp.join(sig);
            routine_doc(&home, "sig", "on_error: fail", Some(block));
            s.spawn(move || {
                let mut c = routine_cmd(&home, &["run", "sig"], &[])
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
                assert_eq!(rc, Some(code), "{sig}");
                assert!(rec.contains(&format!("\"exit\":{code},")), "{sig}");
                assert!(!st.join("lock").exists() && !alive(&st.join("member.pid")));
            });
        }
    });
}

// Past `timeout` the watchdog TERMs the block (124, its output and exit divider kept), KILLs a
// TERM-ignoring member ten seconds on, ends a hung auth probe as a timeout (no degraded marker),
// and TERMs an agent forked past the deadline, skipping the retry.
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
        for (name, block, mode, want) in cases {
            let home = tmp.join(name);
            routine_doc(&home, name, "timeout: 1", Some(block));
            s.spawn(move || {
                let env = [("MODE", mode.to_owned())];
                let o = routine_cmd(&home, &["run", name], &env).output().unwrap();
                let out = String::from_utf8_lossy(&o.stdout);
                let st = home.join(".state").join(name);
                let rec = fs::read_to_string(st.join("last-run.json")).unwrap();
                assert_eq!(o.status.code(), Some(124), "{name}");
                assert!(
                    out.contains(want) && rec.contains("\"exit\":124,"),
                    "{name}: {out}"
                );
                assert!(
                    !st.join("degraded").exists() && !out.contains("retry"),
                    "{name}"
                );
                assert!(name != "slow" || out.contains("block 1 · exit 143"));
                assert!(name != "stubborn" || !alive(&st.join("member.pid")));
            });
        }
    });
}

/// What a tick leaves in `home`'s state: tick.log, then each entry's name, and for a routine
/// its record and what its dir holds.
fn due_state(home: &Path) -> Vec<String> {
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
fn due_home(home: &Path, docs: &str) {
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

// `routine run --due`: exit code, stdout and stderr (sorted by line, the routines being
// concurrent), tick.log, and each routine's record and state — an empty home, nothing due, one due
// under a full wake, several with one failing and one held under a pmset that fails open, and a
// dark wake that defers, over every kind of skipped, bad and unscheduled doc and a due.log past
// 5 MB.
#[test]
fn due() {
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
    let ran = |name: &str, exit: i32| {
        let r = format!(
            "<h>/.state/{name} 0 [\"last-run.json\", \"logs\", \"runs.jsonl\"] {{\"started\":\"{t}\",\
             \"started_epoch\":1786698000,\"finished\":\"{t}\",\"exit\":{exit},\"duration\":0,\
             \"degraded\":false}}\n"
        );
        let lines = [
            format!("routine {name} · {t}"),
            "block 1".into(),
            format!("block 1 · exit {exit} · Ns"),
            format!("routine {name} · exit {exit}"),
        ];
        (lines.map(|l| rule(&l)).to_vec(), r)
    };
    let (ra, mut rb, hourly) = (ran("a", 0), ran("b", 3), ran("hourly", 0));
    rb.0.push(rule("block 1 · caught exit 3 · Ns"));
    let (empty, last) = (
        |n: &str, size| format!("<h>/.state/{n} {size} [] "),
        |n: &str, e| format!("<h>/.state/{n} 0 [\"last-run.json\"] {{\"started_epoch\":{e}}}\n"),
    );
    // (case, pmset, docs, exit, stdout, stderr, tick.log, each state entry)
    let cases = [
        (
            "empty",
            &dark,
            "",
            0,
            vec![],
            "",
            format!("{t} tick docs=0 due=- next=-\n"),
            vec![empty("tick.log", 46)],
        ),
        (
            "none",
            &dark,
            "manual||\nlater|0 0 1 1 *|",
            0,
            vec![],
            "",
            format!("{t} tick docs=2 due=- next=later@2027-01-01T00:00:00Z,manual@unscheduled\n"),
            vec![last("later", 1_786_698_000), empty("tick.log", 90)],
        ),
        (
            "one",
            &lit,
            "hourly|0 * * * *|echo out; echo err >&2",
            0,
            [hourly.0, vec!["out\n".into(), "err\n".into()]].concat(),
            "",
            format!(
                "{t} tick docs=1 due=hourly next=-\n{t} dispatch hourly\n\
                 {t} dispatched hourly exit=0\n"
            ),
            vec![hourly.1, empty("tick.log", 134)],
        ),
        (
            "several",
            &open,
            "a|0 * * * *|echo a\nb|0 * * * *|exit 3\nc|0 * * * *|",
            1,
            [ra.0, vec!["a\n".into()], rb.0].concat(),
            "routine: c already running (pid 1)\n",
            format!(
                "{t} tick docs=3 due=a,b,c next=-\n{t} dispatch a\n{t} dispatch b\n{t} dispatch c\n\
                 {t} dispatched a exit=0\n{t} dispatched b exit=3\n{t} dispatched c exit=75\n"
            ),
            vec![
                ra.1,
                rb.1,
                "<h>/.state/c 0 [\"lock\", \"lock-blocked.log\"] ".into(),
                empty("tick.log", 270),
            ],
        ),
        (
            "dark",
            &dark,
            "hourly|0 * * * *|\nyearly|0 0 1 1 *|\nahead|0 0 1 1 *|\nbad|@daily|\nblank||\n\
             Not_Slug|0 * * * *|",
            0,
            vec![],
            "",
            format!(
                "{t} skip Not_Slug.md: name not a slug\n{t} skip dangling.md: dangling symlink\n\
                 {t} tick docs=6 due=ahead,hourly,nul next=bad@bad,blank@unscheduled,\
                 yearly@2027-01-01T00:00:00Z\n{t} defer dark-wake due=ahead,hourly,nul\n"
            ),
            vec![
                last("ahead", 1_786_699_999),
                empty("due.log", 0),
                empty("due.log.1", 5_242_881),
                empty("tick.log", 282),
                last("yearly", 1_786_698_000),
            ],
        ),
    ];
    for (case, pmset, docs, code, mut stdout, stderr, tick, state) in cases {
        let home = tmp.join(format!("home-{case}"));
        due_home(&home, docs);
        let env = [("ROUTINE_PMSET", pmset.clone())];
        let o = routine_cmd(&home, &["run", "--due"], &env)
            .output()
            .unwrap();
        let lines = |b: &[u8]| {
            let mut l: Vec<_> = (masked(b, &home).split_inclusive('\n'))
                .map(String::from)
                .collect();
            l.sort();
            l
        };
        stdout.sort();
        let stderr = stderr.split_inclusive('\n').map(String::from).collect();
        let got = (
            o.status.code(),
            lines(&o.stdout),
            lines(&o.stderr),
            due_state(&home),
        );
        let want = (Some(code), stdout, stderr, [vec![tick], state].concat());
        assert_eq!(got, want, "{case}");
    }
}

// A TERM to the tick alone (launchd's stop), or an INT to its whole group (a ^C), reaches each
// running routine as TERM — a background routine ignores INT — which records 143 and kills its
// block; the tick exits 143 or 130 with no `dispatched` line.
#[test]
fn due_stops_on_a_signal() {
    use std::{os::unix::process::CommandExt, thread, time::Duration};
    let tmp = sandbox("due-signal");
    let block = r#"echo $$ > "$ROUTINE_STATE_DIR/pid"; exec sleep 30"#;
    let t = "2026-08-14T09:00:00Z";
    let stopped = |n: &str| {
        format!(
            "<h>/.state/{n} 0 [\"last-run.json\", \"logs\", \"runs.jsonl\"] {{\"started\":\"{t}\",\
             \"started_epoch\":1786698000,\"finished\":\"{t}\",\"exit\":143,\"duration\":0,\
             \"degraded\":false}}\n"
        )
    };
    let want = [
        format!("{t} tick docs=2 due=a,b next=-\n{t} dispatch a\n{t} dispatch b\n"),
        stopped("a"),
        stopped("b"),
        "<h>/.state/tick.log 112 [] ".into(),
    ];
    for (sig, group, code) in [("TERM", false, 143), ("INT", true, 130)] {
        let home = tmp.join(sig);
        for n in ["a", "b"] {
            routine_doc(
                &home,
                n,
                "schedule: \"* * * * *\"\non_error: fail",
                Some(block),
            );
        }
        let mut c = routine_cmd(&home, &["run", "--due"], &[])
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
        assert_eq!(c.wait().unwrap().code(), Some(code), "{sig}");
        assert!(!alive(&st.join("a/pid")) && !alive(&st.join("b/pid")));
        assert_eq!(due_state(&home), want, "{sig}");
    }
}
