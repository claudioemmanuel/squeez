//! Spawning squeez's own helper processes (git, curl, interpreter probes).

use std::ffi::OsStr;
use std::process::Command;

/// Windows `CREATE_NO_WINDOW` process-creation flag.
#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// `Command::new` for a helper squeez runs for its own bookkeeping, with
/// captured or discarded stdio — never for a command the user asked for.
///
/// On Windows, a console-subsystem child whose parent has no console gets a
/// console of its own, so every `git status` from a hook flashed a window (a
/// full Windows Terminal window when that is the default terminal).
/// `CREATE_NO_WINDOW` runs the child without one (#231). Do not use it for
/// a child that must write to the user's terminal: it gets no console.
pub fn helper<S: AsRef<OsStr>>(program: S) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

#[cfg(test)]
mod tests {
    #[test]
    fn helper_still_captures_output() {
        let out = super::helper("git").arg("--version").output().expect("spawn git");
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).starts_with("git version"));
    }
}
