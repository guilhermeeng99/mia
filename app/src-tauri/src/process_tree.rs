//! Lifetime guard for whisper-server (Windows Job Object; macOS watchdog).
//!
//! `Drop` is not run when MIA is terminated forcibly. A kernel Job Object with
//! `KILL_ON_JOB_CLOSE` makes each attached whisper-server process die when the
//! last MIA-owned job handle closes, including on crash/taskkill.

#[cfg(windows)]
mod platform {
    use std::mem::{size_of, zeroed};
    use std::ptr::null;
    use std::sync::OnceLock;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;

    struct WhisperJob(HANDLE);

    // A Windows kernel handle can be closed from any thread. The value is immutable
    // after creation and remains owned by this wrapper for the lifetime of the process.
    unsafe impl Send for WhisperJob {}
    unsafe impl Sync for WhisperJob {}

    impl Drop for WhisperJob {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    static WHISPER_JOB: OnceLock<Result<WhisperJob, String>> = OnceLock::new();

    fn create_whisper_job() -> Result<WhisperJob, String> {
        unsafe {
            let job = CreateJobObjectW(null(), null());
            if job.is_null() {
                return Err(format!("failed to create whisper job: {}", std::io::Error::last_os_error()));
            }

            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
            {
                let error = std::io::Error::last_os_error();
                CloseHandle(job);
                return Err(format!("failed to configure whisper job: {error}"));
            }

            Ok(WhisperJob(job))
        }
    }

    pub fn attach(child: &Child) -> Result<(), String> {
        let job = match WHISPER_JOB.get_or_init(create_whisper_job) {
            Ok(job) => Ok(job),
            Err(error) => Err(error.clone()),
        }?;

        let process = child.as_raw_handle() as HANDLE;
        if unsafe { AssignProcessToJobObject(job.0, process) } == 0 {
            return Err(format!(
                "failed to attach whisper-server to its lifetime job: {}",
                std::io::Error::last_os_error()
            ));
        }

        Ok(())
    }
}

/// macOS has no Job Object, and a crashed or force-quit MIA would orphan the warm
/// server (holding the model in RAM) until reboot. A detached `sh` watchdog polls both
/// PIDs and kills the server once MIA is gone. It also exits as soon as the server
/// does (a normal unload), so it never outlives its child long enough to hit a reused PID.
/// The loop runs in a backgrounded subshell so the outer `sh` exits at once and is
/// reaped here — no zombie per spawn — while the loop is reparented to launchd.
#[cfg(target_os = "macos")]
mod platform {
    use std::process::{Child, Command, Stdio};

    pub fn watchdog_script(parent: u32, child: u32) -> String {
        format!(
            "(while kill -0 {parent} 2>/dev/null && kill -0 {child} 2>/dev/null; do sleep 1; done; \
             kill -0 {parent} 2>/dev/null || kill {child} 2>/dev/null) &"
        )
    }

    pub fn attach(child: &Child) -> Result<(), String> {
        Command::new("/bin/sh")
            .args(["-c", &watchdog_script(std::process::id(), child.id())])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|_| ())
            .map_err(|e| format!("failed to start the whisper-server watchdog: {e}"))
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn watchdog_kills_the_child_only_after_the_parent_is_gone() {
            let s = super::watchdog_script(10, 20);
            assert!(s.contains("kill -0 10") && s.contains("kill -0 20"));
            assert!(s.ends_with("kill -0 10 2>/dev/null || kill 20 2>/dev/null) &"));
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod platform {
    use std::process::Child;

    pub fn attach(_child: &Child) -> Result<(), String> {
        Ok(())
    }
}

pub fn attach(child: &std::process::Child) -> Result<(), String> {
    platform::attach(child)
}
