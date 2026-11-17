//! `routine setup [--remove]` and `routine upgrade` — the launchd tick and hotswap's upgrade
//! agent, both on `~/.local/bin/routine`, and the manifest hotswap installs and swaps under
//! (gigantic.dev/hotswap/).

use crate::state;
use std::{
    fs,
    path::{Path, PathBuf},
    process,
    process::Command,
};

const OLD: &str = "com.routines.due"; // the label until 2026-10-20
const TICK: &str = "dev.gigantic.routines.due";
/// hotswap's tool name, the binary being `routine`: it labels `dev.gigantic.routines.upgrade`,
/// keeps `~/.local/share/hotswap/routines/` and `~/.local/state/hotswap/routines.json`, and finds
/// what mise installed under `packslip-jgeschwendt-routines`.
const TOOL: &str = "routines";
const UPGRADE: &str = "dev.gigantic.routines.upgrade";

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The tick's plist: `exe run --due` every 60 s, the roots stamped.
fn tick(exe: &Path, home: &Path) -> String {
    let (dir, st, rhome) = state::roots();
    let st_x = xml(&st.display().to_string());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{TICK}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
    <string>run</string>
    <string>--due</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key><string>{}/.local/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
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
        xml(&exe.display().to_string()),
        xml(&home.display().to_string()),
        xml(&rhome.display().to_string()),
        xml(&dir.display().to_string()),
    )
}

/// `hotswap::ensure` brings the release into hotswap's layout and links `~/.local/bin/routine`
/// through its `current`; both agents run that link, never this process's binary, so a setup run
/// from a checkout wires the installed release (none installed: packslip installs the latest).
/// `--remove` boots both agents out and deletes them, the link and what hotswap keeps. Both plists
/// land aside and are renamed in before any load, so one that cannot land leaves the live agents
/// running; a loaded tick whose plist changed is booted out and bootstrapped again, the upgrade
/// agent never: its bootout would kill the swap that runs this setup (hotswap reloads it, once the
/// run ends, when it moved off mise's shim).
pub fn setup(remove: bool) -> Result<(), String> {
    let home =
        PathBuf::from(state::var("HOME").ok_or("HOME unset — the tick PATH needs ~/.local/bin")?);
    let (_, st, rhome) = state::roots();
    let agents = home.join("Library/LaunchAgents");
    let launchctl = |args: &[&str]| {
        Command::new("launchctl")
            .args(args)
            .output()
            .is_ok_and(|o| o.status.success())
    };
    let uid = Command::new("id").arg("-u").output().map(|o| o.stdout);
    let gui = format!(
        "gui/{}",
        String::from_utf8_lossy(&uid.unwrap_or_default()).trim()
    );
    let plist = |l: &str| agents.join(format!("{l}.plist"));
    launchctl(&["bootout", &format!("{gui}/{OLD}")]);
    let _ = fs::remove_file(plist(OLD));
    if remove {
        for l in [TICK, UPGRADE] {
            launchctl(&["bootout", &format!("{gui}/{l}")]);
            let _ = fs::remove_file(plist(l));
            println!("removed {}", plist(l).display());
        }
        let _ = fs::remove_file(home.join(".local/bin/routine"));
        return hotswap::forget(TOOL);
    }
    let link = hotswap::ensure(TOOL, &manifest())?;
    let to = fs::read_link(&link).unwrap_or_default();
    println!("linked {} → {}", link.display(), to.display());
    let ours = [
        (TICK, tick(&link, &home)),
        (UPGRADE, hotswap::agent_plist(TOOL, "routine")),
    ];
    for p in [&agents, &rhome, &st] {
        let _ = fs::create_dir_all(p);
    }
    let mut fresh = vec![];
    for (l, body) in &ours {
        let (p, tmp) = (plist(l), plist(&format!("{l}.{}", process::id())));
        fresh.push(fs::read_to_string(&p).ok().as_ref() != Some(body));
        if fs::write(&tmp, body)
            .and_then(|()| fs::rename(&tmp, &p))
            .is_err()
        {
            let _ = fs::remove_file(&tmp);
            return Err(format!("cannot write {}", p.display()));
        }
    }
    for ((l, _), fresh) in ours.iter().zip(fresh) {
        let (p, target) = (plist(l), format!("{gui}/{l}"));
        let loaded = launchctl(&["print", &target]);
        let reload = loaded && fresh && *l == TICK;
        if reload {
            launchctl(&["bootout", &target]);
        }
        if (!loaded || reload) && !launchctl(&["bootstrap", &gui, &p.to_string_lossy()]) {
            return Err("launchctl bootstrap failed".into());
        }
        let verb = match (loaded, reload) {
            (false, _) => "loaded",
            (true, true) => "reloaded",
            (true, false) => "current",
        };
        println!("{verb} {}", p.display());
    }
    Ok(())
}

/// routine's manifest: releases from jgeschwendt/routines under its packslip signer; the tick is
/// the one agent to kickstart, and both that kick and setup's reload kill a tick in flight, so the
/// drain refuses while any routine holds its `lock/`; `routine status` proves the new binary reads
/// the home.
fn manifest() -> hotswap::Manifest {
    let st = state::roots().1.display().to_string();
    let strs = |a: &[&str]| a.iter().map(|&s| s.to_owned()).collect::<Vec<_>>();
    hotswap::Manifest {
        agents: strs(&[TICK]),
        bin: "routine".into(),
        drain: vec![strs(&[
            "/bin/sh",
            "-c",
            r#"set -- "$0"/*/lock; [ ! -e "$1" ]"#,
            &st,
        ])],
        repo: "jgeschwendt/routines".into(),
        setup: strs(&["routine", "setup"]),
        signer: "ps1_t5o72jatmrlk56zmnxvdyjrvry".into(),
        smoke: vec![strs(&["routine", "status"])],
        ..Default::default() // the crate's 600 s fetch and 120 s step deadlines
    }
}

pub fn upgrade(args: &[String]) -> i32 {
    hotswap::upgrade(TOOL, &manifest(), args)
}
