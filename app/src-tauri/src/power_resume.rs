//! Resume/unlock watcher that re-claims the global PTT hotkey the instant Windows comes
//! back from sleep or the session is unlocked (hotkeys.md Rule 15). Windows can silently
//! drop a `RegisterHotKey` routing across these transitions; the idle self-heal tick in
//! `hotkey.rs` recovers it within an interval, this fires immediately so the user never
//! finds a dead hotkey on return.
//!
//! Entirely best-effort: any FFI failure just ends the watcher thread and leaves the
//! idle tick as the backstop — it never panics across the IPC boundary (ADR-006).
//! Windows-only (ADR-011); a no-op stub elsewhere so the crate still compiles.

#[cfg(windows)]
mod imp {
    use std::sync::OnceLock;

    use tauri::AppHandle;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::RemoteDesktop::{
        WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW,
        TranslateMessage, CW_USEDEFAULT, MSG, WM_POWERBROADCAST, WM_WTSSESSION_CHANGE, WNDCLASSW,
        WS_OVERLAPPED,
    };

    // Stable Win32 ABI values that windows-sys 0.59 does not re-export (winuser.h /
    // wtsapi32.h). Defined locally to avoid extra crate features for three integers.
    const PBT_APMRESUMESUSPEND: u32 = 0x0007;
    const PBT_APMRESUMEAUTOMATIC: u32 = 0x0012;
    const WTS_SESSION_UNLOCK: u32 = 0x8;

    // The watcher window's wndproc is an `extern "system"` fn with no user pointer, so it
    // reaches the app handle through this set-once global. One watcher exists app-wide.
    static APP: OnceLock<AppHandle> = OnceLock::new();

    /// Spawn the resume/unlock watcher once (idempotent).
    pub fn start(app: &AppHandle) {
        if APP.set(app.clone()).is_err() {
            return; // already started
        }
        std::thread::spawn(watcher_thread);
    }

    /// Owns a hidden top-level window (top-level so it receives `WM_POWERBROADCAST`) and
    /// pumps its messages forever, re-claiming the PTT chord on resume/unlock.
    fn watcher_thread() {
        unsafe {
            let class_name = encode_wide("mia_resume_watcher");
            let hinstance = GetModuleHandleW(std::ptr::null());
            let wnd_class = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                lpszClassName: class_name.as_ptr(),
                hInstance: hinstance,
                ..std::mem::zeroed()
            };
            RegisterClassW(&wnd_class);

            let hwnd = CreateWindowExW(
                0,
                class_name.as_ptr(),
                std::ptr::null(),
                WS_OVERLAPPED, // created but never shown
                CW_USEDEFAULT,
                0,
                CW_USEDEFAULT,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                hinstance,
                std::ptr::null_mut(),
            );
            if hwnd.is_null() {
                return; // the idle self-heal tick still covers us
            }
            // Subscribe to lock/unlock for this session (resume comes via WM_POWERBROADCAST).
            WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);

            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    /// Re-claim the PTT chord on resume-from-sleep or session-unlock; default everything
    /// else (these messages must still reach `DefWindowProcW`).
    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let recovered = match msg {
            WM_POWERBROADCAST => {
                let e = wparam as u32;
                e == PBT_APMRESUMEAUTOMATIC || e == PBT_APMRESUMESUSPEND
            }
            WM_WTSSESSION_CHANGE => wparam as u32 == WTS_SESSION_UNLOCK,
            _ => false,
        };
        if recovered {
            if let Some(app) = APP.get() {
                crate::hotkey::request_reregister(app);
            }
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    fn encode_wide(s: &str) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }
}

#[cfg(windows)]
pub use imp::start;

#[cfg(not(windows))]
pub fn start(_app: &tauri::AppHandle) {}
