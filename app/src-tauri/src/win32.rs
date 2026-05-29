//! Thin Win32 FFI for focus-aware injection (text-injection.md Rules 6-7) and per-app
//! context (per-app-context.md). Three best-effort probes of the **foreground window**:
//! its process name (for per-app styles), whether it's elevated relative to MIA (UIPI
//! blocks Medium → High `SendInput`), and whether one exists at all.
//!
//! Every call is best-effort: any FFI failure returns `None`/`false`/`true` so the
//! pipeline degrades gracefully and **never panics across the IPC boundary** (ADR-006).
//! Windows-only (ADR-011); non-Windows builds get inert stubs so the crate still compiles.

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW,
        PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };

    /// PID owning the foreground window, or `None` if there is no foreground window.
    fn foreground_pid() -> Option<u32> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_null() {
                return None;
            }
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            (pid != 0).then_some(pid)
        }
    }

    /// Whether the process behind `handle` has an elevated token. `None` on any failure.
    fn process_elevated(handle: HANDLE) -> Option<bool> {
        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(handle, TOKEN_QUERY, &mut token) == 0 {
                return None;
            }
            let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
            let mut ret_len = 0u32;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                &mut elevation as *mut _ as *mut c_void,
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut ret_len,
            );
            CloseHandle(token);
            (ok != 0).then_some(elevation.TokenIsElevated != 0)
        }
    }

    fn current_elevated() -> bool {
        // GetCurrentProcess returns a pseudo-handle; no CloseHandle needed for it.
        process_elevated(unsafe { GetCurrentProcess() }).unwrap_or(false)
    }

    /// Lowercased executable stem of the foreground window's process (e.g. `code`,
    /// `chrome`, `winword`), or `None` if it can't be determined.
    pub fn foreground_process_name() -> Option<String> {
        let pid = foreground_pid()?;
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return None;
            }
            let mut buf = [0u16; 260]; // MAX_PATH
            let mut len = buf.len() as u32;
            let ok =
                QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
            CloseHandle(handle);
            if ok == 0 {
                return None;
            }
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            let file = path.rsplit(['\\', '/']).next().unwrap_or(&path).to_lowercase();
            Some(file.strip_suffix(".exe").unwrap_or(&file).to_string())
        }
    }

    /// True only when the foreground window outranks MIA (target elevated, MIA not) — the
    /// case where `SendInput` is silently dropped by UIPI (text-injection.md Rule 7).
    pub fn is_foreground_elevated() -> bool {
        let Some(pid) = foreground_pid() else {
            return false;
        };
        let target = unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return false;
            }
            let r = process_elevated(handle);
            CloseHandle(handle);
            r
        };
        target.unwrap_or(false) && !current_elevated()
    }

    /// Whether any foreground window exists (best-effort editable-target proxy, Rule 6).
    pub fn has_foreground_window() -> bool {
        unsafe { !GetForegroundWindow().is_null() }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn foreground_process_name() -> Option<String> {
        None
    }
    pub fn is_foreground_elevated() -> bool {
        false
    }
    pub fn has_foreground_window() -> bool {
        true
    }
}

pub use imp::{foreground_process_name, has_foreground_window, is_foreground_elevated};
