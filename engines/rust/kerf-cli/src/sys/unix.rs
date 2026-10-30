use std::fs::File;
use std::io::{self, Read};
use std::process::{Child, Command, ExitStatus};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
    fn signal(signum: i32, handler: usize) -> usize;
}

/// Fills `buf` from the kernel's random source.
pub fn random_bytes(buf: &mut [u8]) -> bool {
    File::open("/dev/urandom").and_then(|mut f| f.read_exact(buf)).is_ok()
}

/// The agent leads its own process group, so the terminal's Ctrl-C reaches only the server.
pub fn prepare_agent(cmd: &mut Command) {
    std::os::unix::process::CommandExt::process_group(cmd, 0);
}

/// The agent and everything it started (its process group).
pub struct Group {
    pgid: u32,
}

impl Group {
    pub fn adopt(child: &Child) -> io::Result<Group> {
        Ok(Group { pgid: child.id() })
    }

    /// SIGKILL when `hard`, else SIGTERM, to the whole group.
    pub fn signal(&self, hard: bool) {
        if self.pgid > 1 {
            // SAFETY: plain syscall; a negative pid addresses the process group led by the agent.
            unsafe { kill(-(self.pgid as i32), if hard { 9 } else { 15 }) };
        }
    }
}

/// `128 + signal` for a child killed by a signal, else -1.
pub fn exit_code_of_signal(st: &ExitStatus) -> i32 {
    std::os::unix::process::ExitStatusExt::signal(st).map_or(-1, |s| 128 + s)
}

static SHUTDOWN: OnceLock<&'static AtomicU8> = OnceLock::new();

extern "C" fn on_signal(sig: i32) {
    if let Some(flag) = SHUTDOWN.get() {
        flag.store(u8::try_from(sig).unwrap_or(1), Ordering::Release);
    }
}

/// SIGHUP/SIGINT/SIGTERM store their number into `flag`.
pub fn install_shutdown_handler(flag: &'static AtomicU8) {
    let _ = SHUTDOWN.set(flag);
    for sig in [1, 2, 15] {
        // SAFETY: `on_signal` only performs an atomic load and store, which are async-signal-safe.
        unsafe { signal(sig, on_signal as extern "C" fn(i32) as usize) };
    }
}

/// Exclusive advisory lock (`flock`) on `f` until it is closed; a file system without locks is tolerated.
pub fn lock_exclusive(f: &File) -> io::Result<()> {
    match f.lock() {
        Err(e) if e.kind() != io::ErrorKind::Unsupported => Err(e),
        _ => Ok(()),
    }
}

/// Program name as given: the OS searches `PATH` itself.
pub fn resolve_program(bin: &str, _path: &std::ffi::OsStr) -> std::path::PathBuf {
    bin.into()
}

/// The path without the verbatim prefix Windows adds; nothing to do here.
pub fn plain_path(p: std::path::PathBuf) -> std::path::PathBuf {
    p
}
