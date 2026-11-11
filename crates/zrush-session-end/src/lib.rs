//! Windows asking a window to go, heard.
//!
//! An installer replacing a program through the Restart Manager, a sign-out
//! and a shutdown all ask a window to close with `WM_QUERYENDSESSION` then
//! `WM_ENDSESSION`. winit passes neither on, so a winit window stays open:
//! the installer reports it could not close it, and a shutdown kills it
//! before it saves anything. [`on_session_end`] listens to the window
//! directly; [`restart_after_update`] asks to be started again when the
//! Restart Manager closed the program for an update.
//!
//! The one crate of the workspace allowed `unsafe`: a window's procedure
//! and these calls are the Windows API, with no safe wrapper in reach. Kept
//! apart so every other crate keeps `forbid(unsafe_code)`. Elsewhere than
//! Windows both functions do nothing.

use raw_window_handle::HasWindowHandle;

/// Call `then` when Windows says the session is ending for `window`, after
/// letting it end. `then` runs on the window's thread, inside its message
/// handling: it should ask the window to close, not do the closing.
///
/// Only the first call does anything: one window, one listener.
pub fn on_session_end(window: &impl HasWindowHandle, then: impl Fn() + Send + Sync + 'static) {
    #[cfg(windows)]
    windows::on_session_end(window, Box::new(then));
    #[cfg(not(windows))]
    let _ = (window, then);
}

/// Be started again, with the same command line, when the Restart Manager
/// closed this program to update it; not after a crash, a hang or a reboot.
pub fn restart_after_update() {
    #[cfg(windows)]
    windows::restart_after_update();
}

#[cfg(windows)]
mod windows {
    use std::sync::OnceLock;

    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::Recovery::{
        RESTART_NO_CRASH, RESTART_NO_HANG, RESTART_NO_REBOOT, RegisterApplicationRestart,
    };
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::WM_ENDSESSION;

    type Then = Box<dyn Fn() + Send + Sync>;

    /// What to do: the procedure below has nowhere else to find it.
    static THEN: OnceLock<Then> = OnceLock::new();

    pub fn on_session_end(window: &impl HasWindowHandle, then: Then) {
        let Ok(handle) = window.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(win) = handle.as_raw() else {
            return;
        };
        if THEN.set(then).is_err() {
            return;
        }
        let hwnd = win.hwnd.get() as HWND;
        // SAFETY: `hwnd` comes from a live window handle borrowed for this
        // call; `procedure` has the signature a subclass procedure needs.
        unsafe {
            SetWindowSubclass(hwnd, Some(procedure), 1, 0);
        }
    }

    pub fn restart_after_update() {
        // SAFETY: a null command line is allowed, and means the same one.
        unsafe {
            RegisterApplicationRestart(
                std::ptr::null(),
                RESTART_NO_CRASH | RESTART_NO_HANG | RESTART_NO_REBOOT,
            );
        }
    }

    /// `WM_ENDSESSION` with a yes calls `THEN`; every message, that one
    /// included, then goes on to the window as before.
    unsafe extern "system" fn procedure(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        if msg == WM_ENDSESSION
            && wparam != 0
            && let Some(then) = THEN.get()
        {
            then();
        }
        // SAFETY: the arguments are the ones Windows handed this procedure.
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }
}
