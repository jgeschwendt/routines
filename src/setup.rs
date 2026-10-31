//! `routine setup [--remove]` and `routine upgrade` — `cmd_install`'s tick plus hotswap's
//! upgrade agent, and the manifest hotswap swaps under (gigantic.dev/hotswap/).

use crate::state;
use std::{env, fs, path::PathBuf, process, process::Command};

const OLD: &str = "com.routines.due"; // the label until 2026-10-20
const TICK: &str = "dev.gigantic.routines.due";
/// The crate's tool name: it finds `packslip:jgeschwendt/routines` in mise, labels
/// `dev.gigantic.routines.upgrade` and keeps `~/.local/state/hotswap/routines.json`.
const TOOL: &str = "routines";
const UPGRADE: &str = "dev.gigantic.routines.upgrade";

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Writes and bootstraps both agents, or boots them out, deletes them and forgets the upgrade
/// record. Each plist lands aside and is renamed in before any bootout, so one that cannot land
/// leaves the live agents running.
pub fn setup(remove: bool) -> Result<(), String> {
    let home = state::var("HOME").ok_or("HOME unset — the tick PATH needs ~/.local/bin")?;
    let home = home.to_string_lossy();
    let (dir, st, rhome) = state::roots();
    let agents = state::root(
        "ROUTINE_AGENTS_DIR",
        PathBuf::from(&*home).join("Library/LaunchAgents"),
    );
    let lctl = state::var("ROUTINE_LAUNCHCTL").unwrap_or("launchctl".into());
    let uid = Command::new("id").arg("-u").output().map(|o| o.stdout);
    let gui = format!(
        "gui/{}",
        String::from_utf8_lossy(&uid.unwrap_or_default()).trim()
    );
    let plist = |l: &str| agents.join(format!("{l}.plist"));
    let exe = env::current_exe().unwrap_or_default().display().to_string();
    let (st_x, home_x) = (xml(&st.display().to_string()), xml(&home));
    let tick = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{TICK}</string>
  <key>ProgramArguments</key>
  <array>
    <string>/bin/bash</string>
    <string>-lc</string>
    <string>{}</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key><string>{home_x}/.local/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
    <key>ROUTINE_HOME</key><string>{}</string>
    <key>ROUTINE_DIR</key><string>{}</string>
    <key>ROUTINE_STATE</key><string>{st_x}</string>
  </dict>
  <key>StartInterval</key><integer>60</integer>
  <key>RunAtLoad</key><true/>
  <key>ProcessType</key><string>Background</string>
  <key>StandardOutPath</key><string>{st_x}/due.log</string>
  <key>StandardErrorPath</key><string>{st_x}/due.log</string>
</dict>
</plist>
"#,
        xml(&format!("exec '{}' run --due", exe.replace('\'', r"'\''"))),
        xml(&rhome.display().to_string()),
        xml(&dir.display().to_string()),
    );
    let up = hotswap::agent_plist(TOOL, "routine");
    let ours = [(TICK, tick), (UPGRADE, up)];
    if !remove {
        for p in [&agents, &rhome, &st] {
            let _ = fs::create_dir_all(p);
        }
        for (l, body) in &ours {
            let (p, tmp) = (plist(l), plist(&format!("{l}.{}", process::id())));
            if fs::write(&tmp, body)
                .and_then(|()| fs::rename(&tmp, &p))
                .is_err()
            {
                let _ = fs::remove_file(&tmp);
                return Err(format!("cannot write {}", p.display()));
            }
        }
    }
    for l in [OLD, TICK, UPGRADE] {
        let _ = Command::new(&lctl)
            .args(["bootout", &format!("{gui}/{l}")])
            .output();
        if remove || l == OLD {
            let _ = fs::remove_file(plist(l));
        }
    }
    if remove {
        hotswap::forget(TOOL);
    }
    for (l, _) in &ours {
        let p = plist(l);
        let boot = || {
            Command::new(&lctl)
                .args(["bootstrap", &gui])
                .arg(&p)
                .status()
        };
        if !remove && !boot().is_ok_and(|s| s.success()) {
            return Err("launchctl bootstrap failed".into());
        }
        let verb = if remove { "removed" } else { "installed" };
        println!("{verb} {}", p.display());
    }
    Ok(())
}

/// `hotswap::upgrade` under routine's manifest: the tick is the one agent to kickstart, and
/// both that kick and setup's bootout kill a tick in flight, so the drain refuses while any
/// routine holds its `lock/`; `routine status` proves the new binary reads the home.
pub fn upgrade(args: &[String]) -> i32 {
    let st = state::roots().1.display().to_string();
    let strs = |a: &[&str]| a.iter().map(|&s| s.to_owned()).collect::<Vec<_>>();
    let m = hotswap::Manifest {
        agents: strs(&[TICK]),
        bin: "routine".into(),
        drain: vec![strs(&[
            "/bin/sh",
            "-c",
            r#"set -- "$0"/*/lock; [ ! -e "$1" ]"#,
            &st,
        ])],
        setup: strs(&["routine", "setup"]),
        smoke: vec![strs(&["routine", "status"])],
        timeout: None, // the crate's 120 s per step
    };
    hotswap::upgrade(TOOL, &m, args)
}
