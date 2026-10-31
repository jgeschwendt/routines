//! `routine setup [--remove]` and `routine upgrade` — the launchd tick and hotswap's upgrade
//! agent, both on mise's shim, and the manifest hotswap swaps under (gigantic.dev/hotswap/).

use crate::state;
use std::{
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    process,
    process::Command,
};

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

/// The tick's plist: mise's shim run `--due` every 60 s, the roots stamped.
fn tick(shim: &Path, home: &Path) -> String {
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
        xml(&shim.display().to_string()),
        xml(&home.display().to_string()),
        xml(&rhome.display().to_string()),
        xml(&dir.display().to_string()),
    )
}

/// Links `~/.local/bin/routine` to mise's shim and writes and loads both agents, or boots them
/// out and deletes them, the link and the upgrade record. Both plists land aside and are renamed
/// in before any load, so one that cannot land leaves the live agents running. The tick runs the
/// shim, so a swap needs no rewrite; a loaded tick whose plist changed (a checkout's runner before
/// the cutover) is booted out and bootstrapped again, the upgrade agent never: its bootout would
/// kill the swap that runs this setup.
pub fn setup(remove: bool) -> Result<(), String> {
    let home =
        PathBuf::from(state::var("HOME").ok_or("HOME unset — the tick PATH needs ~/.local/bin")?);
    let (_, st, rhome) = state::roots();
    let agents = state::root("ROUTINE_AGENTS_DIR", home.join("Library/LaunchAgents"));
    let lctl = state::var("ROUTINE_LAUNCHCTL").unwrap_or("launchctl".into());
    let launchctl = |args: &[&str]| {
        Command::new(&lctl)
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
    let (shim, link) = (
        hotswap::shims().join("routine"),
        home.join(".local/bin/routine"),
    );
    if !remove && !shim.exists() {
        return Err(format!(
            "no mise shim at {}: mise use -g packslip:jgeschwendt/routines first",
            shim.display()
        ));
    }
    launchctl(&["bootout", &format!("{gui}/{OLD}")]);
    let _ = fs::remove_file(plist(OLD));
    if remove {
        for l in [TICK, UPGRADE] {
            launchctl(&["bootout", &format!("{gui}/{l}")]);
            let _ = fs::remove_file(plist(l));
            println!("removed {}", plist(l).display());
        }
        let _ = fs::remove_file(&link);
        hotswap::forget(TOOL);
        return Ok(());
    }
    let _ = fs::create_dir_all(link.parent().unwrap());
    let _ = fs::remove_file(&link);
    symlink(&shim, &link).map_err(|e| format!("cannot link {}: {e}", link.display()))?;
    println!("linked {} → {}", link.display(), shim.display());
    let ours = [
        (TICK, tick(&shim, &home)),
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

/// `hotswap::upgrade` under routine's manifest: the tick is the one agent to kickstart, and
/// both that kick and setup's reload kill a tick in flight, so the drain refuses while any
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
