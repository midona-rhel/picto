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
