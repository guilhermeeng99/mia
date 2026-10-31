//! macOS-only platform glue that has no Windows counterpart.
//!
//! Synthetic keystrokes (the Cmd+V paste and typed text in inject.rs) are silently
//! dropped by macOS unless MIA is trusted under Privacy & Security → Accessibility.
//! Windows has no such gate (only UIPI for elevated targets, ADR-005), so the port
//! asks at every startup while trust is missing — before the first dictation —
//! instead of prompting mid-session.

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
