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

/// A scratch HOME under the target's tmp, `&<>` in its name for the plists' escaping, holding
/// `launchctl`, `curl` and `mise` stubs in `bin/` that log each call to `log`, or to `waiter.log` when
/// hotswap's reload waiter makes it. `launchctl print` succeeds once `loaded` exists, printing
/// `print.out`, and fails for a label from its `bootout` to its `bootstrap`; `bootstrap` fails
/// while `bootstrap.code` exists, `kickstart` once per `kickstart.once`. curl's `releases/latest` lands on the tag in `tag`; `mise use`
/// writes its version to `active`, which the fake mise's shim execs.
fn host(name: &str) -> std::path::PathBuf {
    use std::{fs, os::unix::fs::PermissionsExt};
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name} &<>"));
    let _ = Command::new("chmod")
        .arg("-R")
        .arg("u+w")
        .arg(&tmp)
        .output();
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(tmp.join("bin")).unwrap();
    let stub = format!(
        "#!/bin/sh\np='{0}'; log=log\ncase \"$(ps -ww -o command= -p $PPID)\" in *bootout*) log=waiter.log;; esac\n\
         echo \"${{0##*/}} $*\" >> \"$p/$log\"\nl=${{2##*/}}\ncase \"${{0##*/}} $1\" in\n\
         'curl '*) printf 'https://github.com/jgeschwendt/routines/releases/tag/v%s' \"$(cat \"$p/tag\")\";;\n\
         'launchctl bootout') touch \"$p/booted.$l\";;\n\
         'launchctl print') [ ! -e \"$p/booted.$l\" ] && [ -e \"$p/loaded\" ] && {{ cat \"$p/print.out\" 2>/dev/null; :; }};;\n\
         'launchctl bootstrap') l=${{3##*/}}; [ ! -e \"$p/bootstrap.code\" ] && rm -f \"$p/booted.${{l%.plist}}\";;\n\
         'mise use') echo \"${{3##*@}}\" > \"$p/active\";;\n\
         'launchctl kickstart') ! rm \"$p/kickstart.once\" 2>/dev/null;;\n\
         esac\n",
        tmp.display()
    );
    for n in ["curl", "launchctl", "mise"] {
        fs::write(tmp.join("bin").join(n), &stub).unwrap();
        fs::set_permissions(tmp.join("bin").join(n), fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(tmp.join("tag"), "1").unwrap();
    tmp
}

/// `exe args` as launchd runs it under `tmp`'s HOME and stubs: exit code, stdout and stderr, then
/// the stubs' log, `tmp` read as `~` and the uid as `$UID`.
fn launchd(tmp: &Path, exe: &Path, args: &[&str], env: &[(&str, &Path)]) -> String {
    let mut c = Command::new(exe);
    for k in "MISE_DATA_DIR XDG_DATA_HOME XPC_SERVICE_NAME ROUTINE_DIR ROUTINE_STATE".split(' ') {
        c.env_remove(k);
    }
    let o = (c.args(args).env("HOME", tmp).current_dir(tmp))
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", tmp.join("bin").display()),
        )
        .env("ROUTINE_HOME", tmp.join("home"))
        .env("HOTSWAP_TEST_HOME", tmp)
        .envs(env.iter().copied())
        .output()
        .unwrap();
    let calls = std::fs::read_to_string(tmp.join("log")).unwrap_or_default();
    let _ = std::fs::remove_file(tmp.join("log"));
    let uid = String::from_utf8(Command::new("id").arg("-u").output().unwrap().stdout).unwrap();
    let out = String::from_utf8([o.stdout, o.stderr].concat()).unwrap();
    format!("{}\n{out}{calls}", o.status.code().unwrap())
        .replace(&format!("gui/{}", uid.trim()), "gui/$UID")
        .replace(&tmp.display().to_string(), "~")
}

/// `p` XML-escaped, as a plist carries it.
fn esc(p: &Path) -> String {
    let p = p.display().to_string().replace('&', "&amp;");
    p.replace('<', "&lt;").replace('>', "&gt;")
}

/// The tick's plist under `tmp`, running `~/.local/bin/routine`.
fn due_plist(tmp: &Path) -> String {
    let t = esc(tmp);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>dev.gigantic.routines.due</string>
  <key>ProgramArguments</key>
  <array>
    <string>{t}/.local/bin/routine</string>
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
}

// `routine setup` on a host hotswap already laid out links `~/.local/bin/routine` through
// `current`, where hotswap loads its upgrade agent, then writes the tick on that link and loads
// it, reloads it once changed and leaves it be when current; `upgrade` swaps under the manifest,
// its drain refusing while a routine holds its lock; with no launchctl on PATH (Linux) setup only
// links; `setup --remove` undoes it all, the upgrade agent and the layout through hotswap.
#[test]
fn setup_and_upgrade() {
    use std::{fs, os::unix::fs::PermissionsExt, os::unix::fs::symlink};
    let tmp = host("setup");
    let run = |args: &[&str], env: &[(&str, &Path)]| launchd(&tmp, Path::new(EXE), args, env);
    let (agents, local) = (tmp.join("Library/LaunchAgents"), tmp.join(".local/bin"));
    let mode = |p: &Path, m| fs::set_permissions(p, fs::Permissions::from_mode(m));
    let layout = tmp.join(".local/share/hotswap/routines");
    fs::create_dir_all(layout.join("1")).unwrap();
    symlink(EXE, layout.join("1/routine")).unwrap();
    symlink("1", layout.join("current")).unwrap();
    let (due, up) = (
        agents.join("dev.gigantic.routines.due.plist"),
        agents.join("dev.gigantic.routines.upgrade.plist"),
    );
    let (d, u) = (
        "~/Library/LaunchAgents/dev.gigantic.routines.due.plist",
        "~/Library/LaunchAgents/dev.gigantic.routines.upgrade.plist",
    );
    let linked = "linked ~/.local/bin/routine → ~/.local/share/hotswap/routines/current/routine\n";
    let (old, print) = (
        "launchctl bootout gui/$UID/com.routines.due\n",
        "launchctl print gui/$UID/dev.gigantic.routines.",
    );
    let load_up = format!("launchctl print-disabled gui/$UID\nlaunchctl bootstrap gui/$UID {u}\n");
    assert_eq!(
        run(&["setup"], &[]),
        format!(
            "0\n{linked}loaded {d}\n{old}{print}upgrade\n{load_up}{print}due\n\
             launchctl bootstrap gui/$UID {d}\n"
        )
    );
    let read = |p: &Path| fs::read_to_string(p).unwrap();
    assert_eq!(read(&due), due_plist(&tmp));
    assert!(read(&up).contains(&format!("{}/.local/bin/routine</string>", esc(&tmp))));
    assert_eq!(
        fs::read_link(local.join("routine")).unwrap(),
        layout.join("current/routine")
    );
    // both loaded, the upgrade agent from its plist on the link; a checkout's tick: booted out and
    // bootstrapped on the link, the upgrade agent left to hotswap
    fs::write(tmp.join("loaded"), "").unwrap();
    let on_link = format!(
        "path = {}\nstate = not running\nprogram = {}\n",
        up.display(),
        local.join("routine").display()
    );
    fs::write(tmp.join("print.out"), on_link).unwrap();
    fs::write(&due, "exec bin/routine run --due").unwrap();
    let probes = format!("{old}{print}upgrade\n{print}due\n");
    assert_eq!(
        run(&["install"], &[]),
        format!(
            "0\n{linked}reloaded {d}\n{probes}\
             launchctl bootout gui/$UID/dev.gigantic.routines.due\nlaunchctl bootstrap gui/$UID {d}\n"
        )
    );
    assert_eq!(
        run(&["setup"], &[]),
        format!("0\n{linked}current {d}\n{probes}")
    );
    let latest = "curl -q -fsSIL -o /dev/null -w %{url_effective} https://github.com/jgeschwendt/routines/releases/latest\n";
    let kick = "launchctl kickstart -k gui/$UID/dev.gigantic.routines.due\n";
    // no record: the drain refuses while a routine holds its lock, then the swap goes green
    fs::create_dir_all(tmp.join("home/.state/a/lock")).unwrap();
    let drain = r#"/bin/sh -c set -- "$0"/*/lock; [ ! -e "$1" ] ~/home/.state"#;
    assert_eq!(
        run(&["upgrade"], &[]),
        format!("1\nroutines: {drain} refused; nothing swapped\n{latest}")
    );
    fs::remove_dir_all(tmp.join("home/.state/a")).unwrap();
    assert_eq!(
        run(&["upgrade"], &[]),
        format!("0\nroutines 1: green, 1 smoke probes\n{latest}{probes}{kick}")
    );
    assert_eq!(
        run(&["upgrade", "--check"], &[]),
        format!("0\nroutines 1: up to date\n{latest}")
    );
    let rec = tmp.join(".local/state/hotswap/routines.json");
    assert!(rec.exists());
    let fail = |args, env| run(args, &[env]);
    assert_eq!(
        fail(&["setup", "--remove"], ("HOME", Path::new(""))),
        "1\nroutine: HOME unset — the tick PATH needs ~/.local/bin\n"
    );
    // nothing loaded, every bootstrap failing: hotswap's refused load is one line, the tick's fails setup
    fs::write(tmp.join("bootstrap.code"), "").unwrap();
    fs::remove_file(tmp.join("loaded")).unwrap();
    assert_eq!(
        run(&["setup"], &[]),
        format!(
            "1\n{linked}hotswap: gui/$UID/dev.gigantic.routines.upgrade not loaded from {u}: \n\
             routine: launchctl bootstrap failed\n{old}{print}upgrade\n{load_up}{print}due\n\
             launchctl bootstrap gui/$UID {d}\n"
        )
    );
    fs::remove_file(tmp.join("bootstrap.code")).unwrap();
    mode(&agents, 0o555).unwrap();
    let blocked = run(&["setup"], &[]);
    mode(&agents, 0o755).unwrap();
    assert_eq!(
        blocked,
        format!("1\n{linked}routine: cannot write {d}\n{old}{print}upgrade\n{load_up}")
    );
    // the link that cannot land stops setup before any agent is written
    mode(&local, 0o555).unwrap();
    let unlinked = run(&["setup"], &[]);
    mode(&local, 0o755).unwrap();
    assert_eq!(
        unlinked,
        format!("1\nroutine: routines: Permission denied (os error 13)\n{old}")
    );
    let usage = run(&["help"], &[]).replacen('0', "64", 1);
    assert_eq!(run(&["setup", "--x"], &[]), usage);
    // Linux: no launchctl on PATH, so setup links and writes no agent, and --remove touches none
    let linux = [("PATH", &*tmp.join("linux"))];
    fs::remove_file(&due).unwrap();
    fs::remove_file(&up).unwrap();
    assert_eq!(run(&["setup"], &linux), format!("0\n{linked}"));
    assert!(!due.exists() && !up.exists());
    // the upgrade agent loaded from this HOME's plist: hotswap boots it out
    fs::write(tmp.join("loaded"), "").unwrap();
    let removed = format!(
        "0\nremoved {d}\n{old}launchctl bootout gui/$UID/dev.gigantic.routines.due\n\
         {print}upgrade\nlaunchctl bootout gui/$UID/dev.gigantic.routines.upgrade\n"
    );
    assert_eq!(run(&["uninstall"], &[]), removed);
    assert!(!due.exists() && !up.exists() && !rec.exists() && !layout.exists());
    assert!(local.join("routine").symlink_metadata().is_err());
    assert_eq!(run(&["setup", "--remove"], &linux), "0\n");
    assert_eq!(
        run(&["upgrade"], &[]),
        "2\nroutines: no version under ~/.local/share/hotswap/routines; run routine setup\n"
    );
}

// The move off mise. The old release's upgrade agent runs mise's shim; on the first release
// carrying hotswap's layout it bumps mise to 2, drains, then runs `routine setup` through the shim:
// 2's setup lays out every version mise holds, makes 2 current, links `~/.local/bin/routine`,
// moves the tick onto it and leaves hotswap's waiter to reload the upgrade agent once the tick
// ends. A routine holding its lock refuses that drain instead, so no setup ran: the next tick, the
// old agent running 2 through the shim, moves the host before its own drain, which the held lock
// refuses again — nothing swapped, the tick untouched on the shim and still scheduled. The tick
// after, from the reloaded agent, swaps 2 in and goes red at the tick's kickstart: rolled back
// onto 1, which predates hotswap, so mise's pin goes back to 1 before 1's setup puts the link and
// the tick back on the shim, which runs 1. A later release, 3, bumped in by 1, moves the host
// again through its setup, and its first tick swaps green.
#[test]
fn migration() {
    use std::{fs, os::unix::fs::PermissionsExt, os::unix::fs::symlink, thread, time::Duration};
    let tmp = host("migration");
    let xdg = tmp.join("xdg");
    let installs = xdg.join("mise/installs/packslip-jgeschwendt-routines");
    let (shim, link) = (
        xdg.join("mise/shims/routine"),
        tmp.join(".local/bin/routine"),
    );
    let layout = tmp.join(".local/share/hotswap/routines");
    let agents = tmp.join("Library/LaunchAgents");
    let (due, up) = (
        agents.join("dev.gigantic.routines.due.plist"),
        agents.join("dev.gigantic.routines.upgrade.plist"),
    );
    let agent_log = tmp.join("Library/Logs/dev.gigantic.routines.upgrade.log");
    let record = tmp.join(".local/state/hotswap/routines.json");
    let exe = |p: &Path, body: &str| {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
        fs::set_permissions(p, fs::Permissions::from_mode(0o755)).unwrap();
    };
    let put = |name: &str, body: &str| fs::write(tmp.join(name), body).unwrap();
    let read = |p: &Path| fs::read_to_string(p).unwrap();
    let (mise_tick, mise_up) = (
        "the tick, on mise's shim\n",
        "the upgrade agent, on mise's shim\n",
    );
    // mise's shim execs the active version; 1, the old release, logs, and its setup puts the link
    // and both plists back on the shim; 2 and 3 are this build
    exe(
        &shim,
        &format!(
            "#!/bin/sh\nexec \"{}/$(cat '{}/active')/routine\" \"$@\"\n",
            installs.display(),
            tmp.display()
        ),
    );
    exe(
        &installs.join("1/routine"),
        &format!(
            "#!/bin/sh\necho \"routine@1 $*\" >> '{}/log'\n[ \"$1\" = setup ] || exit 0\n\
             ln -sfn '{}' '{}'\nprintf %s \"{mise_tick}\" > '{}'\nprintf %s \"{mise_up}\" > '{}'\n",
            tmp.display(),
            shim.display(),
            link.display(),
            due.display(),
            up.display()
        ),
    );
    fs::create_dir_all(installs.join("2")).unwrap();
    fs::copy(EXE, installs.join("2/routine")).unwrap();
    symlink("./2", installs.join("global")).unwrap();
    let loaded = |program: &Path, state: &str| {
        let out = format!(
            "path = {}\nstate = {state}\nprogram = {}\n",
            up.display(),
            program.display()
        );
        put("print.out", &out);
    };
    // the host as the old release left it, mise bumped to 2
    let on_mise = || {
        let _ = fs::remove_dir_all(&layout);
        let _ = fs::remove_file(&link);
        let _ = fs::remove_file(tmp.join(".local/state/hotswap/routines.mise"));
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        symlink(&shim, &link).unwrap();
        fs::create_dir_all(&agents).unwrap();
        fs::write(&due, mise_tick).unwrap();
        fs::write(&up, mise_up).unwrap();
        fs::create_dir_all(record.parent().unwrap()).unwrap();
        fs::write(&record, r#"{"at":0,"ok":true,"smoke":[],"version":"1"}"#).unwrap();
        put("active", "2");
        put("tag", "2");
        put("loaded", "");
        loaded(&shim, "running");
        let _ = fs::write(&agent_log, "");
    };
    // the waiter's outcome once the agent's job stops running, the agent's log read and emptied;
    // launchd then runs the agent from the link
    let reloaded = || {
        loaded(&shim, "not running");
        for _ in 0..200 {
            if fs::read_to_string(&agent_log).is_ok_and(|l| !l.is_empty()) {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        let out = read(&agent_log).replace(&tmp.display().to_string(), "~");
        let uid = String::from_utf8(Command::new("id").arg("-u").output().unwrap().stdout).unwrap();
        fs::write(&agent_log, "").unwrap();
        let _ = fs::remove_file(tmp.join("waiter.log"));
        loaded(&link, "running");
        out.replace(&format!("gui/{}", uid.trim()), "gui/$UID")
    };
    let versions = || {
        let mut v = (fs::read_dir(&layout).unwrap().flatten())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        v.sort();
        format!(
            "{} → {}",
            v.join(" "),
            fs::read_link(layout.join("current")).unwrap().display()
        )
    };
    let on_link =
        |p: &Path| read(p).contains(&format!("{}/.local/bin/routine</string>", esc(&tmp)));
    let xdg_env = [("XDG_DATA_HOME", &*xdg)];
    let print = "launchctl print gui/$UID/dev.gigantic.routines.";
    let latest = "curl -q -fsSIL -o /dev/null -w %{url_effective} https://github.com/jgeschwendt/routines/releases/latest\n";
    let back = "hotswap: gui/$UID/dev.gigantic.routines.upgrade reloaded from ~/Library/LaunchAgents/dev.gigantic.routines.upgrade.plist\n";
    let d = "~/Library/LaunchAgents/dev.gigantic.routines.due.plist";
    let linked = "linked ~/.local/bin/routine → ~/.local/share/hotswap/routines/current/routine\n";
    let old = "launchctl bootout gui/$UID/com.routines.due\n";
    let moved = format!(
        "{old}{print}upgrade\n{print}due\nlaunchctl bootout gui/$UID/dev.gigantic.routines.due\n\
         launchctl bootstrap gui/$UID {d}\n"
    );
    let kick = "launchctl kickstart -k gui/$UID/dev.gigantic.routines.due\n";

    // green: the old release's setup step, through the shim, runs 2's setup
    on_mise();
    assert_eq!(
        launchd(&tmp, &shim, &["setup"], &xdg_env),
        format!("0\n{linked}reloaded {d}\n{moved}")
    );
    assert_eq!(versions(), "1 2 current → 2");
    assert_eq!(read(&due), due_plist(&tmp));
    assert!(on_link(&up));
    assert_eq!(reloaded(), back);
    // the old release recorded 2 green: the next tick, on the link, finds it up to date
    fs::write(&record, r#"{"at":0,"ok":true,"smoke":[],"version":"2"}"#).unwrap();
    assert_eq!(
        launchd(&tmp, &link, &["upgrade"], &[]),
        format!("0\nroutines 2: up to date\n{latest}")
    );

    // the stranded host: the next tick, through the shim, moves it and meets the held lock
    on_mise();
    fs::create_dir_all(tmp.join("home/.state/a/lock")).unwrap();
    let drain = r#"/bin/sh -c set -- "$0"/*/lock; [ ! -e "$1" ] ~/home/.state"#;
    assert_eq!(
        launchd(&tmp, &shim, &["upgrade"], &xdg_env),
        format!("1\nroutines: {drain} refused; nothing swapped\n{print}upgrade\n{latest}")
    );
    assert_eq!(versions(), "1 2 current → 2");
    assert_eq!(
        fs::read(layout.join("2/routine")).unwrap(),
        fs::read(EXE).unwrap()
    );
    assert_eq!(
        fs::read_link(&link).unwrap(),
        layout.join("current/routine")
    );
    assert!(on_link(&up));
    assert_eq!(read(&due), mise_tick);
    assert!(read(&record).contains(r#""version":"1""#));
    assert_eq!(reloaded(), back);
    // the tick after, the lock gone: 2's setup moves the tick, then its kickstart fails — rolled
    // back onto 1 with mise's pin first, so link, tick and shim all run 1
    fs::remove_dir_all(tmp.join("home/.state/a")).unwrap();
    put("kickstart.once", "");
    let pin = "mise use -g packslip:jgeschwendt/routines@1\n";
    assert_eq!(
        launchd(&tmp, &link, &["upgrade"], &[]),
        format!(
            "1\nroutines: {} failed; rolled back to 1\n{latest}{moved}{kick}{pin}routine@1 setup\n{kick}routine@1 status\n",
            kick.trim_end()
        )
    );
    assert_eq!(read(&tmp.join("active")), "1\n");
    assert_eq!(versions(), "1 2 current → 1");
    assert_eq!(fs::read_link(&link).unwrap(), shim);
    assert_eq!((read(&due), read(&up)), (mise_tick.into(), mise_up.into()));
    let rec = read(&record);
    assert!(
        rec.contains(r#""red":"2""#) && rec.contains(r#""version":"1""#),
        "{rec}"
    );
    // 3, bumped in by 1 and set up through the shim, moves the host again past the red 2, and its
    // first tick from the link finds it up to date: setup recorded 3, so nothing swaps or prunes
    fs::create_dir_all(installs.join("3")).unwrap();
    fs::copy(EXE, installs.join("3/routine")).unwrap();
    put("active", "3");
    put("tag", "3");
    loaded(&link, "running");
    assert_eq!(
        launchd(&tmp, &shim, &["setup"], &xdg_env),
        format!("0\n{linked}reloaded {d}\n{moved}")
    );
    assert_eq!(versions(), "1 2 3 current → 3");
    assert_eq!(
        fs::read_link(&link).unwrap(),
        layout.join("current/routine")
    );
    assert!(on_link(&up));
    assert_eq!(read(&due), due_plist(&tmp));
    assert_eq!(reloaded(), back);
    assert_eq!(
        launchd(&tmp, &link, &["upgrade"], &[]),
        format!("0\nroutines 3: up to date\n{latest}")
    );
    assert_eq!(versions(), "1 2 3 current → 3");
    assert!(installs.join("2/routine").exists() && installs.join("1/routine").exists());
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
// under a full wake finding something (exit 1, the tick still 0), one failing alone (exit 2, the
// tick 1), several with one failing, one finding and one held under a pmset that fails open, and a
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
    let (ra, mut rb, mut rf, mut hourly) =
        (ran("a", 0), ran("b", 2), ran("f", 1), ran("hourly", 1));
    for (r, e) in [(&mut rb, 2), (&mut rf, 1), (&mut hourly, 1)] {
        r.0.push(rule(&format!("block 1 · caught exit {e} · Ns")));
    }
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
            "hourly|0 * * * *|echo out; echo err >&2; exit 1",
            0,
            [hourly.0, vec!["out\n".into(), "err\n".into()]].concat(),
            "",
            format!(
                "{t} tick docs=1 due=hourly next=-\n{t} dispatch hourly\n\
                 {t} dispatched hourly exit=1\n"
            ),
            vec![hourly.1, empty("tick.log", 134)],
        ),
        (
            "failing",
            &lit,
            "b|0 * * * *|exit 2",
            1,
            rb.0.clone(),
            "",
            format!("{t} tick docs=1 due=b next=-\n{t} dispatch b\n{t} dispatched b exit=2\n"),
            vec![rb.1.clone(), empty("tick.log", 119)],
        ),
        (
            "several",
            &open,
            "a|0 * * * *|echo a\nb|0 * * * *|exit 2\nc|0 * * * *|\nf|0 * * * *|exit 1",
            1,
            [ra.0, vec!["a\n".into()], rb.0, rf.0].concat(),
            "routine: c already running (pid 1)\n",
            format!(
                "{t} tick docs=4 due=a,b,c,f next=-\n{t} dispatch a\n{t} dispatch b\n{t} dispatch c\n\
                 {t} dispatch f\n{t} dispatched a exit=0\n{t} dispatched b exit=2\n\
                 {t} dispatched c exit=75\n{t} dispatched f exit=1\n"
            ),
            vec![
                ra.1,
                rb.1,
                "<h>/.state/c 0 [\"lock\", \"lock-blocked.log\"] ".into(),
                rf.1,
                empty("tick.log", 345),
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

/// `routine <args>` against `home` as health sees a host: `PATH` the stub `launchctl` and `sysctl`
/// in `<home>/../bin` (loaded while `<home>/../loaded` exists, woken as `<home>/../waketime` says
/// and failing without it) when `launchd`, else neither; exit code, stdout and stderr, the home
/// read as `<h>` and the uid as `$UID`.
fn health_run(home: &Path, args: &[&str], launchd: bool) -> (Option<i32>, String, String) {
    use std::{fs, os::unix::fs::PermissionsExt};
    let bin = home.parent().unwrap().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let t = bin.parent().unwrap().display();
    for (n, stub) in [
        ("launchctl", format!("#!/bin/sh\n[ -e '{t}/loaded' ]\n")),
        (
            "sysctl",
            format!("#!/bin/sh\nexec /bin/cat '{t}/waketime'\n"),
        ),
    ] {
        fs::write(bin.join(n), stub).unwrap();
        fs::set_permissions(bin.join(n), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = if launchd {
        format!("{}:/usr/bin", bin.display())
    } else {
        "/usr/bin".into()
    };
    let o = routine_cmd(home, args, &[("PATH", path)]).output().unwrap();
    let uid = String::from_utf8(Command::new("id").arg("-u").output().unwrap().stdout).unwrap();
    let fix = |b: Vec<u8>| {
        String::from_utf8(b)
            .unwrap()
            .replace(&*home.to_string_lossy(), "<h>")
            .replace(&format!("gui/{}/", uid.trim()), "gui/$UID/")
    };
    (o.status.code(), fix(o.stdout), fix(o.stderr))
}

/// `<home>/<rel>` written with `body`, its parents made, modified at epoch `at` when given.
fn put(home: &Path, rel: &str, body: Option<&str>, at: Option<u64>) {
    use std::{
        fs,
        time::{Duration, UNIX_EPOCH},
    };
    let p = home.join(rel);
    fs::create_dir_all(if body.is_some() {
        p.parent().unwrap()
    } else {
        &p
    })
    .unwrap();
    if let Some(b) = body {
        fs::write(&p, b).unwrap();
    }
    if let Some(t) = at {
        let t = UNIX_EPOCH + Duration::from_secs(t);
        fs::File::open(&p).unwrap().set_modified(t).unwrap();
    }
}

// `routine health`, plain and `--json`, over a home holding each verdict — lint, wedged, a live
// lock inside its timeout, failed, acknowledged, an ack a new exit outlived, stale, due but not
// passed over (never run, or due this minute) — each kind of skipped doc and an orphan, the tick
// unmanaged; then each tick state under a stubbed launchctl, and the usage and clock refusals.
#[test]
fn health() {
    let tmp = sandbox("health");
    let home = tmp.join("home");
    let now = 1_786_698_000; // Fri 2026-08-14T09:00:00Z
    let rec = |started: u64, exit: i32| {
        Some(format!(
            "{{\"started\":\"x\",\"started_epoch\":{started},\"finished\":\"2026-08-14T08:59:00Z\",\
             \"exit\":{exit},\"duration\":7,\"degraded\":false}}\n"
        ))
    };
    let (blk, pid) = ("```sh\ntrue\n```\n", Some("1\n".to_owned())); // pid 1: EPERM, alive
    let (hourly, held) = (
        format!("---\nschedule: \"0 * * * *\"\n---\n{blk}"),
        format!("---\ntimeout: 30\n---\n{blk}"),
    );
    let tick = "2026-08-14T08:50:00Z tick docs=1 due=- next=-\n2026-08-14T08:55:00Z tick docs=1 \
                due=x next=-\n2026-08-14T08:55:00Z defer dark-wake due=x\n";
    let ack = r#"{"exit":5,"at":"2026-08-14T08:59:30Z","reason":"a \"q\" \\ \u0009t\u000a é"}"#;
    for (rel, body, at) in [
        (".state/.archive/old/x", Some(String::new()), None),
        (".state/acked/ack", Some(format!("{ack}\n")), None),
        (".state/acked/last-run.json", rec(now - 60, 5), None),
        (".state/busy/lock/pid", pid.clone(), None),
        (".state/busy/lock", None, Some(now - 10)),
        (
            ".state/failed/degraded",
            Some("2026-08-14T08:59:00Z\n".into()),
            None,
        ),
        (".state/failed/last-run.json", rec(now - 60, 2), None),
        (".state/found/last-run.json", rec(now - 60, 1), None),
        (
            ".state/failed/logs/20260814T080000Z-1.log",
            Some(String::new()),
            None,
        ),
        (
            ".state/failed/logs/20260814T085900Z-2.log",
            Some(String::new()),
            None,
        ),
        (".state/fresh/last-run.json", rec(1_786_695_000, 0), None),
        (".state/gone/runs.jsonl", Some(String::new()), None),
        (
            ".state/moved/ack",
            Some(r#"{"exit":5,"at":"x","reason":""}"#.into()),
            None,
        ),
        (".state/moved/last-run.json", rec(now - 60, 4), None),
        (".state/ok/last-run.json", rec(now - 1000, 0), None),
        (".state/stale/last-run.json", rec(1_786_690_000, 0), None),
        (".state/tick.log", Some(tick.into()), None),
        (".state/wedged/lock/pid", pid.clone(), None),
        (".state/wedged/lock", None, Some(now - 100)),
        ("AGENTS.md", Some("# agents\n".into()), None),
        ("Bad_Name.md", Some(hourly.clone()), None),
        ("acked.md", Some(blk.into()), None),
        ("busy.md", Some(held.clone()), None),
        ("failed.md", Some(blk.into()), None),
        ("found.md", Some(blk.into()), None),
        ("fresh.md", Some(hourly.clone()), None),
        (
            "lint.md",
            Some("---\non_error: none\ntimeout: 1m\nschedule: \"@daily\"\n---\n".into()),
            None,
        ),
        ("moved.md", Some(blk.into()), None),
        ("never.md", Some(hourly.clone()), Some(now - 600)),
        (
            "ok.md",
            Some(format!("---\nschedule: 0 0 1 1 *\n---\n{blk}")),
            None,
        ),
        ("stale.md", Some(hourly.clone()), None),
        ("wedged.md", Some(held.clone()), None),
    ] {
        put(&home, rel, body.as_deref(), at);
    }
    std::os::unix::fs::symlink("nowhere", home.join("dangling.md")).unwrap();
    let manual = "its prose is the repair manual";
    assert_eq!(
        health_run(&home, &["health"], false),
        (
            Some(1),
            format!(
                "failed: failed — exit 2 at 2026-08-14T08:59:00Z
  fix: read <h>/.state/failed/logs/20260814T085900Z-2.log, then <h>/failed.md — {manual}
lint: lint — no sh or bash block; on_error none is neither claude nor fail; timeout 1m is no positive number of seconds; schedule @daily never fires
  fix: edit <h>/lint.md: the smallest change the runner takes
failed: moved — exit 4 at 2026-08-14T08:59:00Z
  fix: read <h>/.state/moved/logs, then <h>/moved.md — {manual}
stale: stale — due since 2026-08-14T07:00:00Z, swept 2026-08-14T08:50:00Z without a run
  fix: routine run stale, and read <h>/.state/tick.log for its refusal
wedged: wedged — pid 1 has held the lock 100s, past its 30s timeout
  fix: ps -p 1 -o command= — its watchdog missed it: kill 1, and the next run reaps the lock
skipped: Bad_Name.md — name not a slug, yet it opens with frontmatter
  fix: rename it to a slug, [a-z0-9][a-z0-9-]*.md: the runner never fires it
orphan: gone — <h>/.state/gone has no gone.md
  fix: mkdir -p <h>/.state/.archive && mv <h>/.state/gone <h>/.state/.archive/gone-20260814T090000Z — archive, never delete
finding: found — exit 1 at 2026-08-14T08:59:00Z
  fix: it ran and found something — read <h>/.state/found/logs; <h>/found.md says whose move it is
acknowledged: acked — exit 5 since 2026-08-14T08:59:30Z: a \"q\" \\ \tt\n é
unhealthy — 7 red · 1 finding(s)
"
            ),
            String::new()
        )
    );
    let last = |exit: i32, started: u64| {
        format!(
            r#"{{"started":{started},"finished":"2026-08-14T08:59:00Z","exit":{exit},"duration":7}}"#
        )
    };
    let row = |name: &str, sched: &str, next: &str, flags: &str, rest: String| {
        format!(r#"{{"name":"{name}","schedule":{sched},"next":{next},{flags},{rest}}}"#)
    };
    let (idle, due, ok) = (
        r#""due":false,"running":false"#,
        r#""due":true,"running":false"#,
        r#""verdict":"ok","finding":null,"fix":null"#,
    );
    let plain = |l: &str, deg: &str, ack: &str, v: &str| {
        format!(r#""last":{l},"degraded":{deg},"ack":{ack},{v}"#)
    };
    let fixed =
        |v: &str, f: &str, x: &str| format!(r#""verdict":"{v}","finding":"{f}","fix":"{x}""#);
    let manual = |n: &str, log: &str| {
        format!("read <h>/.state/{n}/logs{log}, then <h>/{n}.md — its prose is the repair manual")
    };
    let rows = [
        row(
            "acked",
            "null",
            "null",
            idle,
            plain(
                &last(5, now - 60),
                "null",
                ack,
                &fixed(
                    "acknowledged",
                    "exit 5 at 2026-08-14T08:59:00Z",
                    &manual("acked", ""),
                ),
            ),
        ),
        row(
            "busy",
            "null",
            "null",
            r#""due":false,"running":true"#,
            plain("null", "null", "null", ok),
        ),
        row(
            "failed",
            "null",
            "null",
            idle,
            plain(
                &last(2, now - 60),
                r#""2026-08-14T08:59:00Z""#,
                "null",
                &fixed(
                    "failed",
                    "exit 2 at 2026-08-14T08:59:00Z",
                    &manual("failed", "/20260814T085900Z-2.log"),
                ),
            ),
        ),
        row(
            "found",
            "null",
            "null",
            idle,
            plain(
                &last(1, now - 60),
                "null",
                "null",
                &fixed(
                    "finding",
                    "exit 1 at 2026-08-14T08:59:00Z",
                    "it ran and found something — read <h>/.state/found/logs; <h>/found.md says whose move it is",
                ),
            ),
        ),
        row(
            "fresh",
            r#""0 * * * *""#,
            "1786698000",
            due,
            plain(&last(0, 1_786_695_000), "null", "null", ok),
        ),
        row(
            "lint",
            r#""@daily""#,
            "null",
            idle,
            plain(
                "null",
                "null",
                "null",
                &fixed(
                    "lint",
                    "no sh or bash block; on_error none is neither claude nor fail; timeout 1m is no positive number of seconds; schedule @daily never fires",
                    "edit <h>/lint.md: the smallest change the runner takes",
                ),
            ),
        ),
        row(
            "moved",
            "null",
            "null",
            idle,
            plain(
                &last(4, now - 60),
                "null",
                r#"{"exit":5,"at":"x","reason":""}"#,
                &fixed(
                    "failed",
                    "exit 4 at 2026-08-14T08:59:00Z",
                    &manual("moved", ""),
                ),
            ),
        ),
        row(
            "never",
            r#""0 * * * *""#,
            "0",
            due,
            plain("null", "null", "null", ok),
        ),
        row(
            "ok",
            r#""0 0 1 1 *""#,
            "1798761600",
            idle,
            plain(&last(0, now - 1000), "null", "null", ok),
        ),
        row(
            "stale",
            r#""0 * * * *""#,
            "1786690800",
            due,
            plain(
                &last(0, 1_786_690_000),
                "null",
                "null",
                &fixed(
                    "stale",
                    "due since 2026-08-14T07:00:00Z, swept 2026-08-14T08:50:00Z without a run",
                    "routine run stale, and read <h>/.state/tick.log for its refusal",
                ),
            ),
        ),
        row(
            "wedged",
            "null",
            "null",
            r#""due":false,"running":true"#,
            plain(
                "null",
                "null",
                "null",
                &fixed(
                    "wedged",
                    "pid 1 has held the lock 100s, past its 30s timeout",
                    "ps -p 1 -o command= — its watchdog missed it: kill 1, and the next run reaps the lock",
                ),
            ),
        ),
    ];
    let version = option_env!("ROUTINE_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));
    assert_eq!(
        health_run(&home, &["health", "--json"], false),
        (
            Some(1),
            format!(
                r#"{{"version":"{version}","now":{now},"verdict":"red","tick":{{"verdict":"unmanaged","swept":1786697400,"finding":null,"fix":null}},"routines":[{}],"skipped":[{{"file":"AGENTS.md","why":"name not a slug","red":false,"fix":null}},{{"file":"Bad_Name.md","why":"name not a slug","red":true,"fix":"rename it to a slug, [a-z0-9][a-z0-9-]*.md: the runner never fires it"}},{{"file":"dangling.md","why":"dangling symlink","red":false,"fix":null}}],"orphans":[{{"name":"gone","finding":"<h>/.state/gone has no gone.md","fix":"mkdir -p <h>/.state/.archive && mv <h>/.state/gone <h>/.state/.archive/gone-20260814T090000Z — archive, never delete"}}]}}
"#,
                rows.join(",")
            ),
            String::new()
        )
    );
    // (case, tick.log, a live lock, launchctl loaded, kern.waketime) → exit, the plain report,
    // the JSON tick
    let old = format!("{}\n2026-08-14T08:00:00Z run x\n", "x".repeat(70_000)); // the tail only
    let kick = "launchctl kickstart -k gui/$UID/dev.gigantic.routines.due — its output is <h>/.state/due.log";
    let cases = [
        ("unloaded", None, String::new(), false, false, 1, "not-loaded: tick — gui/$UID/dev.gigantic.routines.due is not loaded\n  fix: routine setup\nunhealthy — 1 red\n".into(), r#""verdict":"not-loaded","swept":null,"finding":"gui/$UID/dev.gigantic.routines.due is not loaded","fix":"routine setup""#.to_owned()),
        ("silent", None, String::new(), false, true, 1, format!("stale: tick — tick.log has no line\n  fix: {kick}\nunhealthy — 1 red\n"), format!(r#""verdict":"stale","swept":null,"finding":"tick.log has no line","fix":"{kick}""#)),
        ("quiet", None, old.clone(), false, true, 1, format!("stale: tick — tick.log quiet 3600s since 2026-08-14T08:00:00Z, its last line or the last wake\n  fix: {kick}\nunhealthy — 1 red\n"), format!(r#""verdict":"stale","swept":null,"finding":"tick.log quiet 3600s since 2026-08-14T08:00:00Z, its last line or the last wake","fix":"{kick}""#)),
        ("slept", Some("{ sec = 1786600000, usec = 5 } Thu"), old.clone(), false, true, 1, format!("stale: tick — tick.log quiet 3600s since 2026-08-14T08:00:00Z, its last line or the last wake\n  fix: {kick}\nunhealthy — 1 red\n"), format!(r#""verdict":"stale","swept":null,"finding":"tick.log quiet 3600s since 2026-08-14T08:00:00Z, its last line or the last wake","fix":"{kick}""#)),
        ("garbled", Some("{ sec = soon }"), old.clone(), false, true, 1, format!("stale: tick — tick.log quiet 3600s since 2026-08-14T08:00:00Z, its last line or the last wake\n  fix: {kick}\nunhealthy — 1 red\n"), format!(r#""verdict":"stale","swept":null,"finding":"tick.log quiet 3600s since 2026-08-14T08:00:00Z, its last line or the last wake","fix":"{kick}""#)),
        ("woke", Some("{ sec = 1786697900, usec = 5 } Fri"), old.clone(), false, true, 0, "healthy — 0 routines\n".into(), r#""verdict":"ok","swept":null,"finding":null,"fix":null"#.into()),
        ("held", None, old, true, true, 0, "healthy — 1 routines\n".into(), r#""verdict":"ok","swept":null,"finding":null,"fix":null"#.into()),
        ("heard", None, "2026-08-14T08:59:00Z tick docs=0 due=- next=-\n".into(), false, true, 0, "healthy — 0 routines\n".into(), r#""verdict":"ok","swept":1786697940,"finding":null,"fix":null"#.into()),
    ];
    for (case, wake, log, busy, loaded, code, report, tick) in cases {
        let home = tmp.join(case).join("home");
        if let Some(w) = wake {
            put(&tmp, &format!("{case}/waketime"), Some(w), None);
        }
        put(&home, ".state/tick.log", Some(&log), None);
        if busy {
            put(&home, "busy.md", Some(&held), None);
            put(&home, ".state/busy/lock/pid", pid.as_deref(), None);
            put(&home, ".state/busy/lock", None, Some(now - 10));
        }
        if loaded {
            put(&tmp, &format!("{case}/loaded"), Some(""), None);
        }
        let json = health_run(&home, &["health", "--json"], true);
        let tick = format!(r#""tick":{{{tick}}}"#);
        assert_eq!(
            health_run(&home, &["health"], true),
            (Some(code), report, String::new()),
            "{case}"
        );
        assert!(
            json.0 == Some(code) && json.1.contains(&tick),
            "{case}: {}",
            json.1
        );
    }
    let usage = String::from_utf8(Command::new(EXE).output().unwrap().stdout).unwrap();
    assert_eq!(
        health_run(&home, &["health", "x"], false),
        (Some(64), String::new(), usage)
    );
    let o = routine_cmd(&home, &["health"], &[("ROUTINE_NOW", "x".into())])
        .output()
        .unwrap();
    let err = "routine: ROUTINE_NOW=x is not an integer\n".to_owned();
    assert_eq!(out(o), (Some(64), String::new(), err));
}

// `routine ack`: refused (64) for an unknown routine and a last run that is no failure, usage
// without a name; it holds the failed exit with its reason, survives a run failing alike, and the
// runner drops it on a different failure and on a green run; `--clear` drops it, an unwritable
// state dir 73.
#[test]
fn ack() {
    use std::{fs, os::unix::fs::PermissionsExt};
    let tmp = sandbox("ack");
    let home = tmp.join("home");
    routine_doc(&home, "f", "on_error: fail", Some("exit \"$(cat code)\""));
    let (st, usage) = (
        home.join(".state/f"),
        String::from_utf8(Command::new(EXE).output().unwrap().stdout).unwrap(),
    );
    let run = |args: &[&str]| out(routine_cmd(&home, args, &[]).output().unwrap());
    let fail = |code: &str| {
        fs::write(home.join("code"), code).unwrap();
        run(&["run", "f"]).0
    };
    let ack = || fs::read_to_string(st.join("ack")).ok();
    let refused = |m: &str| (Some(64), String::new(), format!("routine: {m}\n"));
    assert_eq!(
        run(&["ack", "f"]),
        refused("f's last run is no failure to acknowledge")
    );
    for n in ["nope", "Bad_Name"] {
        assert_eq!(run(&["ack", n]), refused(&format!("no routine {n}")));
    }
    for args in [&["ack"][..], &["ack", ""]] {
        assert_eq!(run(args), (Some(64), String::new(), usage.clone()));
    }
    assert_eq!(fail("3"), Some(3));
    assert_eq!(
        run(&["ack", "f", "flaky", "\"net\""]),
        (Some(0), String::new(), String::new())
    );
    let held =
        r#"{"exit":3,"at":"2026-08-14T09:00:00Z","reason":"flaky \"net\""}"#.to_owned() + "\n";
    assert_eq!(ack(), Some(held.clone()));
    assert_eq!(
        health_run(&home, &["health"], false).1,
        "acknowledged: f — exit 3 since 2026-08-14T09:00:00Z: flaky \"net\"\nhealthy — 1 routines\n"
    );
    assert_eq!((fail("3"), ack()), (Some(3), Some(held)));
    assert_eq!((fail("4"), ack()), (Some(4), None));
    assert_eq!(run(&["ack", "f"]).0, Some(0));
    assert_eq!((fail("0"), ack()), (Some(0), None));
    // exit 1 is a finding, alone never red: exit 0, the top-level verdict ok
    assert_eq!(fail("1"), Some(1));
    let ((code, plain, _), json) = (
        health_run(&home, &["health"], false),
        health_run(&home, &["health", "--json"], false).1,
    );
    let found = "finding: f — exit 1 at 2026-08-14T09:00:00Z\n  fix: it ran and found something — read <h>/.state/f/logs/20260814T090000Z-";
    let close = ".log; <h>/f.md says whose move it is\nhealthy — 1 routines · 1 finding(s)\n";
    assert!(
        code == Some(0) && plain.starts_with(found) && plain.ends_with(close),
        "{plain}"
    );
    let top = r#""now":1786698000,"verdict":"ok""#;
    assert!(
        json.contains(top) && json.contains(r#""verdict":"finding""#),
        "{json}"
    );
    assert_eq!(run(&["ack", "f", "known"]).0, Some(0));
    assert_eq!(
        health_run(&home, &["health"], false).1,
        "acknowledged: f — exit 1 since 2026-08-14T09:00:00Z: known\nhealthy — 1 routines\n"
    );
    assert_eq!(
        (
            fail("5"),
            run(&["ack", "f"]).0,
            run(&["ack", "f", "--clear"]).0,
            ack()
        ),
        (Some(5), Some(0), Some(0), None)
    );
    assert_eq!(run(&["ack", "f", "--clear"]).0, Some(0));
    fs::set_permissions(&st, fs::Permissions::from_mode(0o555)).unwrap();
    let denied = run(&["ack", "f"]);
    fs::set_permissions(&st, fs::Permissions::from_mode(0o755)).unwrap();
    let h = home.display();
    assert_eq!(
        denied,
        (
            Some(73),
            String::new(),
            format!("routine: cannot write {h}/.state/f/ack\n")
        )
    );
}
