//! The Win32 calls `wrap` needs and std does not expose. Declared by hand
//! rather than through `windows-sys`: the crate ships with no runtime
//! dependencies.

use std::ffi::c_void;
use std::os::windows::io::{AsRawHandle, RawHandle};
use std::process::Child;

const STD_INPUT_HANDLE: u32 = -10i32 as u32;
const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
const STD_ERROR_HANDLE: u32 = -12i32 as u32;
const HANDLE_FLAG_INHERIT: u32 = 1;
const INVALID_HANDLE_VALUE: RawHandle = -1isize as RawHandle;

#[link(name = "kernel32")]
extern "system" {
    fn CreateJobObjectW(attributes: *mut c_void, name: *const u16) -> RawHandle;
    fn AssignProcessToJobObject(job: RawHandle, process: RawHandle) -> i32;
    fn TerminateJobObject(job: RawHandle, exit_code: u32) -> i32;
    fn CloseHandle(handle: RawHandle) -> i32;
    fn GetStdHandle(std_handle: u32) -> RawHandle;
    fn SetHandleInformation(handle: RawHandle, mask: u32, flags: u32) -> i32;
}

/// Stop squeez's own stdin/stdout/stderr from being inherited by the commands
/// it spawns.
///
/// Those handles are the CALLER's pipes, and they arrive inheritable. std
/// spawns with `bInheritHandles = TRUE`, so every wrapped command — and every
/// process it starts — received them as extra, non-stdio handles on top of
/// the stdio it was actually given. A shell redirection only replaces fds
/// 0-2, so a daemon started with `> log 2>&1` still held the caller's pipe
/// open and the tool call never saw EOF, long after squeez itself had exited
/// (issue #261).
///
/// A child that is meant to share squeez's stdio still does: for
/// `Stdio::inherit()` std hands over an inheritable DUPLICATE, which this
/// leaves alone.
pub fn stop_std_handle_inheritance() {
    for id in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        unsafe {
            let handle = GetStdHandle(id);
            if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
                SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

/// A job object holding one wrapped command and everything it starts.
///
/// `taskkill /T` follows parent links, and an MSYS `exec` breaks them: the
/// program a git-bash shell launches (`ssh`, `tail -f`, `sleep`) is not its
/// Windows child, so the timeout kill never reached it (issue #261). Job
/// membership is inherited through `CreateProcess` whatever the parent link
/// says, so terminating the job reaches them.
///
/// No kill-on-close limit is set on purpose: when the command finishes on its
/// own, a daemon it left behind must outlive squeez.
pub struct Job(RawHandle);

impl Job {
    /// Puts `child` in a fresh job. The child is already running, so anything
    /// it started before this call is outside the job — `taskkill /T` still
    /// runs after [`Job::terminate`] to cover that window.
    pub fn attach(child: &Child) -> Option<Job> {
        unsafe {
            let job = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
            if job.is_null() {
                return None;
            }
            if AssignProcessToJobObject(job, child.as_raw_handle()) == 0 {
                CloseHandle(job);
                return None;
            }
            Some(Job(job))
        }
    }

    pub fn terminate(&self) {
        unsafe {
            TerminateJobObject(self.0, 1);
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
