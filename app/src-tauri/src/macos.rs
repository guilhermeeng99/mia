//! macOS-only platform glue that has no Windows counterpart.
//!
//! Synthetic keystrokes (the Cmd+V paste and typed text in inject.rs) are silently
//! dropped by macOS unless MIA is trusted under Privacy & Security → Accessibility.
//! Windows has no such gate (only UIPI for elevated targets, ADR-005), so the port
//! asks once at startup — before the first dictation — instead of failing mid-session.

use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::string::{CFString, CFStringRef};

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    static kAXTrustedCheckOptionPrompt: CFStringRef;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
}

/// True when MIA may post synthetic input. With `prompt`, an untrusted app also gets
/// the system dialog that deep-links to the Accessibility pane (shown once per launch
/// at most; macOS de-duplicates it).
pub fn accessibility_trusted(prompt: bool) -> bool {
    let key = unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) };
    let options = CFDictionary::from_CFType_pairs(&[(key, CFBoolean::from(prompt))]);
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) }
}

/// Startup Accessibility prompt, shown on the **first launch only**. Prompting on every
/// launch nagged anyone who had not granted it yet — or whose grant went stale, since
/// an ad-hoc-signed rebuild/update is a new identity to TCC. Later launches stay
/// silent; `inject` still re-asks at the moment the user actually dictates.
pub fn prompt_accessibility_once(app: &tauri::AppHandle) {
    use tauri::Manager;
    if accessibility_trusted(false) {
        return;
    }
    let Ok(dir) = app.path().app_data_dir() else {
        return;
    };
    let marker = dir.join("accessibility-prompted");
    if marker.exists() {
        return;
    }
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(&marker, b"");
    accessibility_trusted(true);
}

/// User-presentable error for an injection attempted without Accessibility trust.
pub const ACCESSIBILITY_REQUIRED: &str = "MIA needs Accessibility permission to type into other apps. \
     Enable MIA in System Settings → Privacy & Security → Accessibility, then try again.";

/// Close-to-tray on macOS: while the Hub is hidden MIA should live only in the menu bar,
/// like Windows' tray-only state. `Accessory` drops the Dock icon and the Cmd+Tab entry;
/// `Regular` brings them back when the Hub is shown again. The HUD is unaffected — an
/// accessory app's windows still draw. Best-effort: a failure leaves the icon as it was.
pub fn set_dock_icon_visible(app: &tauri::AppHandle, visible: bool) {
    let policy = if visible {
        tauri::ActivationPolicy::Regular
    } else {
        tauri::ActivationPolicy::Accessory
    };
    let _ = app.set_activation_policy(policy);
}
