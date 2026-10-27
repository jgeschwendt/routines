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
        run(&["status"]),
        (
            Some(64),
            String::new(),
            "routine: status is not ported yet — use bin/routine\n".into()
        )
    );
}
