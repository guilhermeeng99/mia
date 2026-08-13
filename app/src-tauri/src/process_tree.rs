//! Windows lifetime guard for whisper-server.
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

#[cfg(not(windows))]
mod platform {
    use std::process::Child;

    pub fn attach(_child: &Child) -> Result<(), String> {
        Ok(())
    }
}

pub fn attach(child: &std::process::Child) -> Result<(), String> {
    platform::attach(child)
}
