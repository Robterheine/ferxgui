//! Process identity: a PID alone is not an identity (PIDs are reused), so before any signal is
//! sent to a process we did not start in this session, its start time is compared with the one
//! recorded when it was launched. If identity cannot be established, the signal is refused.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessIdentity {
    pub pid: u32,
    /// Opaque, platform-specific start-time token (compared for equality only).
    pub start_time: String,
}

/// `1 <= pid <= i32::MAX`. Anything else would reach `kill(2)` as 0 (own process group) or a
/// negative number (a whole group), so it is never passed on.
pub fn pid_in_range(pid: u32) -> bool {
    pid >= 1 && pid <= i32::MAX as u32
}

impl ProcessIdentity {
    /// Identity of the live process `pid`, or None when its start time cannot be read.
    pub fn of(pid: u32) -> Option<Self> {
        if !pid_in_range(pid) { return None; }
        start_time(pid).map(|start_time| Self { pid, start_time })
    }

    /// True only when the process now at `self.pid` has the recorded start time.
    pub fn is_still_that_process(&self) -> bool {
        pid_in_range(self.pid) && start_time(self.pid).as_deref() == Some(self.start_time.as_str())
    }
}

/// Whether it is safe to signal `pid`: the recorded identity must exist and still match.
pub fn may_signal(identity: Option<&ProcessIdentity>, pid: u32) -> bool {
    identity.is_some_and(|i| i.pid == pid && i.is_still_that_process())
}

#[cfg(target_os = "linux")]
pub fn start_time(pid: u32) -> Option<String> {
    // /proc/<pid>/stat field 22 (starttime, clock ticks since boot). The command name in field 2
    // may contain spaces and parentheses, so split after the LAST ')'.
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = &stat[stat.rfind(')')? + 1..];
    // `rest` begins at field 3 (state); starttime is field 22, i.e. index 19 here.
    rest.split_whitespace().nth(19).map(str::to_owned)
}

#[cfg(all(unix, not(target_os = "linux")))]
pub fn start_time(pid: u32) -> Option<String> {
    // `ps -o lstart=` prints the start time at one-second resolution without any unsafe code
    // or struct layouts; it is only called when a signal is about to be sent.
    let out = std::process::Command::new("ps")
        .env("LC_ALL", "C")
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !s.is_empty()).then_some(s)
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    #[repr(C)] #[derive(Default, Clone, Copy)]
    pub struct FileTime { pub low: u32, pub high: u32 }
    #[link(name = "kernel32")]
    extern "system" {
        pub fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        pub fn CloseHandle(h: *mut c_void) -> i32;
        pub fn GetExitCodeProcess(h: *mut c_void, code: *mut u32) -> i32;
        pub fn GetProcessTimes(h: *mut c_void, creation: *mut FileTime, exit: *mut FileTime,
                               kernel: *mut FileTime, user: *mut FileTime) -> i32;
    }
    pub const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    pub const STILL_ACTIVE: u32 = 259;
}

#[cfg(windows)]
pub fn start_time(pid: u32) -> Option<String> {
    use win::*;
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() { return None; }
        let (mut c, mut e, mut k, mut u) = (FileTime::default(), FileTime::default(), FileTime::default(), FileTime::default());
        let ok = GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u);
        CloseHandle(h);
        (ok != 0).then(|| format!("{}", ((c.high as u64) << 32) | c.low as u64))
    }
}

/// Windows liveness: the process exists and has not exited (replaces spawning `tasklist`).
#[cfg(windows)]
pub fn windows_pid_alive(pid: u32) -> bool {
    use win::*;
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() { return false; }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code);
        CloseHandle(h);
        ok != 0 && code == STILL_ACTIVE
    }
}

#[cfg(not(any(unix, windows)))]
pub fn start_time(_pid: u32) -> Option<String> { None }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_of_self_is_stable() {
        let pid = std::process::id();
        let a = ProcessIdentity::of(pid).expect("own start time readable");
        let b = ProcessIdentity::of(pid).unwrap();
        assert_eq!(a, b);
        assert!(a.is_still_that_process());
    }

    #[test]
    fn pid_zero_and_huge_rejected() {
        assert!(!pid_in_range(0));
        assert!(!pid_in_range(u32::MAX));
        assert!(!pid_in_range(i32::MAX as u32 + 1));
        assert!(pid_in_range(1) && pid_in_range(i32::MAX as u32));
        assert!(ProcessIdentity::of(0).is_none());
        assert!(!may_signal(Some(&ProcessIdentity { pid: 0, start_time: "x".into() }), 0));
    }

    #[test]
    fn mismatch_or_missing_identity_refuses() {
        let me = std::process::id();
        let wrong = ProcessIdentity { pid: me, start_time: "not-my-start-time".into() };
        assert!(!may_signal(Some(&wrong), me));
        assert!(!may_signal(None, me));
        assert!(may_signal(ProcessIdentity::of(me).as_ref(), me));
    }
}
