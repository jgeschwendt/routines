//! The port's one unsafe surface, decision `port-signals` A′ (2026-10-27): `sigaction` gives
//! HUP, INT and TERM a handler that only `write`s the signal number to a pipe, a thread reads
//! it, and `kill` signals a pid or a process group — the watchdog's stop. The mask is never
//! touched: a caught disposition resets to default at exec, so a block gets default TERM. A
//! tick's routine is bash's `&` child: INT and QUIT ignored from its exec on.
#![expect(
    unsafe_code,
    reason = "std cannot catch a signal; three libc calls, as std itself links libc"
)]

use std::{
    ffi::c_int,
    io::{self, Read},
    os::{fd::AsRawFd, unix::process::CommandExt},
    process::{self, Command},
    ptr,
    sync::atomic::{AtomicI32, AtomicU32, Ordering::Relaxed},
    thread,
};

pub const HUP: c_int = 1;
pub const INT: c_int = 2;
const QUIT: c_int = 3;
pub const KILL: c_int = 9;
pub const TERM: c_int = 15;

/// `struct sigaction`: macOS's, else glibc's and musl's.
#[derive(Default)]
#[repr(C)]
struct Action {
    handler: usize,
    #[cfg(target_os = "macos")]
    mask: u32,
    #[cfg(not(target_os = "macos"))]
    mask: [u64; 16],
    flags: c_int,
    #[cfg(not(target_os = "macos"))]
    restorer: usize,
}

const SIG_IGN: usize = 1;
const SA_RESTART: c_int = if cfg!(target_os = "macos") {
    2
} else {
    0x1000_0000
};

unsafe extern "C" {
    fn sigaction(sig: c_int, new: *const Action, old: *mut Action) -> c_int;
    fn write(fd: c_int, buf: *const u8, n: usize) -> isize;
    fn kill(pid: c_int, sig: c_int) -> c_int;
}

static FD: AtomicI32 = AtomicI32::new(-1);
static PID: AtomicU32 = AtomicU32::new(0);

/// Async-signal-safe: an atomic load, getpid and write. A child between fork and exec still
/// holds this handler, and its pid keeps a signal meant for it out of the runner's pipe.
extern "C" fn handler(sig: c_int) {
    if process::id() == PID.load(Relaxed) {
        // SAFETY: a one-byte write from a live local to the pipe `catch` leaked open.
        unsafe { write(FD.load(Relaxed), sig.to_le_bytes().as_ptr(), 1) };
    }
}

/// `sig`'s disposition, replaced by `new` unless null.
fn action(sig: c_int, new: *const Action) -> Action {
    let mut old = Action::default();
    // SAFETY: `new` is null or a live `Action`, `old` a live one to fill; both are C's layout.
    unsafe { sigaction(sig, new, &raw mut old) };
    old
}

/// The first HUP, INT or TERM, handed to `on` on a thread of its own; later ones are dropped.
/// A signal ignored at entry stays ignored, as bash's traps leave it (`nohup`, a background
/// job's INT).
pub fn catch(on: impl FnOnce(c_int) + Send + 'static) {
    let Ok((mut r, w)) = io::pipe() else { return };
    FD.store(w.as_raw_fd(), Relaxed);
    PID.store(process::id(), Relaxed);
    std::mem::forget(w);
    let new = Action {
        handler: handler as extern "C" fn(c_int) as usize,
        flags: SA_RESTART,
        ..Action::default()
    };
    for sig in [HUP, INT, TERM] {
        if action(sig, ptr::null()).handler != SIG_IGN {
            action(sig, &raw const new);
        }
    }
    thread::spawn(move || {
        let mut b = [0];
        let _ = r.read_exact(&mut b);
        on(b[0].into());
    });
}

/// `cmd &` without job control: INT and QUIT ignored in the child, a disposition exec keeps.
pub fn background(cmd: &mut Command) -> &mut Command {
    // SAFETY: between fork and exec only sigaction runs, async-signal-safe.
    unsafe { cmd.pre_exec(ignore) }
}

#[expect(clippy::unnecessary_wraps, reason = "pre_exec's signature")]
fn ignore() -> io::Result<()> {
    let ign = Action {
        handler: SIG_IGN,
        ..Action::default()
    };
    for sig in [INT, QUIT] {
        action(sig, &raw const ign);
    }
    Ok(())
}

/// `kill(2)`: true when delivered — `sig` 0 probes; a negative `pid` names its process group.
pub fn signal(pid: c_int, sig: c_int) -> bool {
    // SAFETY: plain integers in, an integer out.
    unsafe { kill(pid, sig) == 0 }
}

#[cfg(test)]
mod tests {
    // the pre_exec half runs past the profiler's reach; the dispositions it sets, here
    #[test]
    fn ignore_sets_sig_ign() {
        use super::{Action, INT, QUIT, SIG_IGN, action, ignore};
        assert!(ignore().is_ok());
        let dfl = Action::default();
        for sig in [INT, QUIT] {
            assert_eq!(action(sig, &raw const dfl).handler, SIG_IGN);
        }
    }
}
