//! The Win32 calls the app makes for what GPUI has no option for.
use gpui::Window;
use wgpu::rwh::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    HWND_NOTOPMOST, HWND_TOPMOST, SW_HIDE, SW_SHOWNA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SetWindowPos, ShowWindow,
};

/// The window's handle (`HWND`), while it is open.
pub fn hwnd(window: &Window) -> Option<HWND> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(win32) => Some(HWND(win32.hwnd.get() as _)),
        _ => None,
    }
}

/// Asks the window to close as its close button does (`WM_CLOSE`).
#[cfg(debug_assertions)]
pub fn post_close(window: &Window) {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
    let Some(hwnd) = hwnd(window) else {
        return;
    };
    // SAFETY: `hwnd` is the live window GPUI made; `WM_CLOSE` takes no
    // parameters and is handled from the thread's messages later.
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
    }
}

/// Floats the window over every window that does not float
/// (`HWND_TOPMOST`), or puts it back among them (`HWND_NOTOPMOST`), leaving
/// its size, place and activation alone.
pub fn set_topmost(window: &Window, topmost: bool) {
    let Some(hwnd) = hwnd(window) else {
        return;
    };
    let after = if topmost { HWND_TOPMOST } else { HWND_NOTOPMOST };
    // SAFETY: `hwnd` is the live window GPUI made (the handle borrows the
    // window, which owns it); `SetWindowPos` takes it, the window to place
    // it after (one of the two special values) and flags that keep its size,
    // place and activation, so the zeros are ignored.
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
    // SAFETY: `hwnd` is the live window GPUI made; `ShowWindow` takes it
    // and a show command.
    unsafe {
        let _ = ShowWindow(hwnd, if shown { SW_SHOWNA } else { SW_HIDE });
    }
}
