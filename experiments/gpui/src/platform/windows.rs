//! The Win32 calls the app makes for what GPUI has no option for.
use gpui::Window;
use wgpu::rwh::{HasWindowHandle, RawWindowHandle};

#[link(name = "user32")]
unsafe extern "system" {
    fn SetWindowPos(
        window: isize,
        after: isize,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> i32;
    fn ShowWindow(window: isize, command: i32) -> i32;
    #[cfg(debug_assertions)]
    fn PostMessageW(window: isize, message: u32, wparam: usize, lparam: isize) -> i32;
}

/// The window's handle (`HWND`), while it is open.
pub fn hwnd(window: &Window) -> Option<isize> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(win32) => Some(win32.hwnd.get()),
        _ => None,
    }
}

/// Asks the window to close as its close button does (`WM_CLOSE`).
#[cfg(debug_assertions)]
pub fn post_close(window: &Window) {
    const WM_CLOSE: u32 = 0x0010;
    let Some(hwnd) = hwnd(window) else {
        return;
    };
    // SAFETY: `hwnd` is the live window GPUI made; `WM_CLOSE` takes no
    // parameters and is handled from the thread's messages later.
    unsafe {
        PostMessageW(hwnd, WM_CLOSE, 0, 0);
    }
}

/// Floats the window over every window that does not float
/// (`HWND_TOPMOST`), or puts it back among them (`HWND_NOTOPMOST`), leaving
/// its size, place and activation alone.
pub fn set_topmost(window: &Window, topmost: bool) {
    /// `HWND_TOPMOST` and `HWND_NOTOPMOST`.
    const TOPMOST: isize = -1;
    const NOT_TOPMOST: isize = -2;
    /// `SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE`.
    const KEEP_PLACE: u32 = 0x0001 | 0x0002 | 0x0010;
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return;
    };
    let after = if topmost { TOPMOST } else { NOT_TOPMOST };
    // SAFETY: `hwnd` is the live window GPUI made (the handle borrows the
    // window, which owns it); `SetWindowPos` takes it, the window to place
    // it after (one of the two special values) and flags that keep its size,
    // place and activation, so the zeros are ignored.
    unsafe {
        SetWindowPos(win32.hwnd.get(), after, 0, 0, 0, 0, KEEP_PLACE);
    }
}

/// Shows the window without activating it (`SW_SHOWNA`), or hides it
/// (`SW_HIDE`).
pub fn show_without_activating(window: &Window, shown: bool) {
    const SW_HIDE: i32 = 0;
    const SW_SHOWNA: i32 = 8;
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return;
    };
    // SAFETY: `hwnd` is the live window GPUI made; `ShowWindow` takes it
    // and a show command.
    unsafe {
        ShowWindow(win32.hwnd.get(), if shown { SW_SHOWNA } else { SW_HIDE });
    }
}
