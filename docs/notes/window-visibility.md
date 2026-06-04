# Window visibility invariant

MIA's main window is frameless and closes to the tray: `CloseRequested` is prevented
and the Hub is hidden so the global dictation hotkey can keep running. The failure
mode was that the tray only restored the window from the context-menu item, and the
restore path called `show()` and `set_focus()` without first undoing a minimized
state. On Windows, focusing a minimized or hidden frameless WebView can look like a
no-op, so the app appears to be gone even though the process is still alive.

Keep these rules when touching window chrome or tray behavior:

- The `main` window must remain taskbar-visible when minimized.
- Every tray restore path must call `unminimize()`, then `show()`, then `set_focus()`.
- A left-click on the tray icon must restore the Hub, not only the tray menu.
- Only the explicit quit path should exit the process; regular close keeps the app in
  the tray by design.
