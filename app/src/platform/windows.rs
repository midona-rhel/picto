//! The Win32 calls that the app makes for things that GPUI has no option
//! for.
use gpui::Window;
use wgpu::rwh::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    HWND_NOTOPMOST, HWND_TOPMOST, SW_HIDE, SW_SHOWNA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SetWindowPos, ShowWindow,
};

/// Returns the handle (`HWND`) of the window, or `None` if the window has
/// no Win32 handle.
pub fn hwnd(window: &Window) -> Option<HWND> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(win32) => Some(HWND(win32.hwnd.get() as _)),
        _ => None,
    }
}

/// Closes the window the way its close button does, by posting `WM_CLOSE`
/// to it.
#[cfg(debug_assertions)]
pub fn post_close(window: &Window) {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
    let Some(hwnd) = hwnd(window) else {
        return;
    };
    // SAFETY: `hwnd` is the handle of an open window that GPUI made.
    // `WM_CLOSE` takes no parameters. The window handles it later, when the
    // thread processes its messages.
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
    }
}

/// Keeps the window in front of every window that is not topmost
/// (`HWND_TOPMOST`), or makes it an ordinary window again
/// (`HWND_NOTOPMOST`). The window's size and position do not change, and
/// the window is not activated.
pub fn set_topmost(window: &Window, topmost: bool) {
    let Some(hwnd) = hwnd(window) else {
        return;
    };
    let after = if topmost { HWND_TOPMOST } else { HWND_NOTOPMOST };
    // SAFETY: `hwnd` is the handle of an open window that GPUI made (the
    // handle borrows the window, which owns it). `SetWindowPos` takes the
    // handle, the window to place it after (here one of the two special
    // values) and flags. The flags keep the size, the position and the
    // activation, so the four zeros are ignored.
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(after),
            0,
            0,
            0,
            0,
            SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE,
        );
    }
}

/// Shows the window without activating it (`SW_SHOWNA`), or hides it
/// (`SW_HIDE`).
pub fn show_without_activating(window: &Window, shown: bool) {
    let Some(hwnd) = hwnd(window) else {
        return;
    };
    // SAFETY: `hwnd` is the handle of an open window that GPUI made.
    // `ShowWindow` takes the handle and a show command.
    unsafe {
        let _ = ShowWindow(hwnd, if shown { SW_SHOWNA } else { SW_HIDE });
    }
}
