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

/// The first build of Windows that can show a blurred backdrop behind a
/// whole window with rounded corners (`DWMWA_SYSTEMBACKDROP_TYPE`): Windows
/// 11 version 22H2.
const SYSTEM_BACKDROP_BUILD: u32 = 22621;

/// Returns whether this Windows can show a blurred backdrop behind a whole
/// window ([`show_backdrop`]): Windows 11 version 22H2 (build 22621) and
/// later. `RtlGetVersion` gives the build number; when it fails, the answer
/// is `false`.
pub fn has_system_backdrop() -> bool {
    use windows::Wdk::System::SystemServices::RtlGetVersion;
    use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
    let mut version = OSVERSIONINFOW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // SAFETY: `RtlGetVersion` writes the version into the structure it is
    // given, whose size is stored in its first field.
    let status = unsafe { RtlGetVersion(&mut version) };
    status.is_ok() && version.dwBuildNumber >= SYSTEM_BACKDROP_BUILD
}

/// Sets one attribute of the window with `DwmSetWindowAttribute`. An
/// attribute that this Windows does not have is left out: the call fails and
/// the failure is ignored.
fn set_dwm_attribute<T>(
    hwnd: HWND,
    attribute: windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE,
    value: &T,
) {
    use windows::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    // SAFETY: `hwnd` is the handle of an open window that GPUI made. The
    // pointer and the size describe `value`, which lives until the call
    // returns.
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            attribute,
            (value as *const T).cast(),
            std::mem::size_of::<T>() as u32,
        );
    }
}

/// Takes away what Windows draws around a pop-up window: the shadow, the
/// 1px border and the rounded corners. GPUI creates a pop-up window with a
/// caption style and hides the caption, so the Desktop Window Manager draws
/// a frame around the window's rectangle, which for a transparent pop-up
/// window is a frame around nothing. The frame is switched off
/// (`DWMNCRP_DISABLED`). On Windows 11 the border is given no colour and the
/// corners are not rounded, because Windows 11 draws both for any top-level
/// window. The animation that Windows plays when a window appears is
/// switched off as well.
pub fn without_frame(window: &Window) {
    use windows::Win32::Graphics::Dwm::{
        DWMNCRP_DISABLED, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_NCRENDERING_POLICY,
        DWMWA_TRANSITIONS_FORCEDISABLED, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
    };
    let Some(hwnd) = hwnd(window) else {
        return;
    };
    set_dwm_attribute(hwnd, DWMWA_TRANSITIONS_FORCEDISABLED, &1i32);
    set_dwm_attribute(hwnd, DWMWA_NCRENDERING_POLICY, &DWMNCRP_DISABLED.0);
    set_dwm_attribute(hwnd, DWMWA_BORDER_COLOR, &DWMWA_COLOR_NONE);
    set_dwm_attribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &DWMWCP_DONOTROUND.0);
}

/// Makes Windows show its blurred backdrop (Acrylic) behind a pop-up window
/// whose background appearance is `Blurred`, on Windows 11 version 22H2 and
/// later ([`has_system_backdrop`]). The backdrop fills the whole window, so
/// such a window has exactly the rectangle of its panel. Windows rounds the
/// window's corners with a radius of 8px and draws the window's shadow.
///
/// - The backdrop shows only where the window's frame is extended into its
///   content, so the frame is extended over the whole window
///   (`DwmExtendFrameIntoClientArea` with margins of -1).
/// - Windows replaces the backdrop with a solid colour in a window that is
///   not the active one, and a pop-up window is never activated. The window
///   is therefore told to draw its frame as an active window does
///   (`WM_NCACTIVATE` with `TRUE`). This changes only how the window is
///   drawn. It does not activate the window or give it the keyboard.
/// - The 1px border that Windows 11 draws around a window is given no
///   colour, because the panel draws its own edge.
/// - The animation that Windows plays when a window appears is switched
///   off, because the panel fades in itself.
pub fn show_backdrop(window: &Window) {
    use windows::Win32::Graphics::Dwm::{
        DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_TRANSITIONS_FORCEDISABLED,
        DwmExtendFrameIntoClientArea,
    };
    use windows::Win32::UI::Controls::MARGINS;
    let Some(hwnd) = hwnd(window) else {
        return;
    };
    set_dwm_attribute(hwnd, DWMWA_TRANSITIONS_FORCEDISABLED, &1i32);
    set_dwm_attribute(hwnd, DWMWA_BORDER_COLOR, &DWMWA_COLOR_NONE);
    let whole_window = MARGINS {
        cxLeftWidth: -1,
        cxRightWidth: -1,
        cyTopHeight: -1,
        cyBottomHeight: -1,
    };
    // SAFETY: `hwnd` is the handle of an open window that GPUI made, and
    // the margins live until the call returns.
    unsafe {
        let _ = DwmExtendFrameIntoClientArea(hwnd, &whole_window);
    }
    draw_as_active(hwnd);
}

/// Tells the window to draw its frame as an active window does
/// (`WM_NCACTIVATE` with `TRUE`), without activating it.
fn draw_as_active(hwnd: HWND) {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_NCACTIVATE};
    // SAFETY: `hwnd` is the handle of an open window of this thread.
    // `WM_NCACTIVATE` takes a flag in its first parameter and nothing in its
    // second.
    unsafe {
        SendMessageW(hwnd, WM_NCACTIVATE, Some(WPARAM(1)), Some(LPARAM(0)));
    }
}

/// Moves and resizes a pop-up window that shows the system's backdrop
/// ([`show_backdrop`]) so that its content has the rectangle
/// `(x, y, width, height)`, given in device pixels on the screen. The
/// window's own rectangle is larger than its content by the borders that
/// Windows keeps around a window without drawing them. Their widths are
/// measured from the window as it is now and added to the rectangle. The
/// window is not activated and keeps its place among the windows. It is told
/// again to draw its frame as an active window does, because Windows can
/// take that back when a window's rectangle changes.
///
/// Windows sends the window its move and size messages before
/// `SetWindowPos` returns, and GPUI handles them by updating the app. The
/// caller must therefore call this function while the app is not being
/// updated, for example from a task.
pub fn set_frame(hwnd: HWND, (x, y, width, height): (i32, i32, i32, i32)) {
    use windows::Win32::Foundation::{POINT, RECT};
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, GetWindowRect, SWP_NOZORDER};
    let (mut outer, mut content, mut origin) = (RECT::default(), RECT::default(), POINT::default());
    // SAFETY: `hwnd` is the handle of an open window that GPUI made. Each
    // call writes into the structure it is given. With `SWP_NOZORDER` the
    // window to place the window after is ignored.
    unsafe {
        if GetWindowRect(hwnd, &mut outer).is_err()
            || GetClientRect(hwnd, &mut content).is_err()
            || !ClientToScreen(hwnd, &mut origin).as_bool()
        {
            return;
        }
        let left = origin.x - outer.left;
        let top = origin.y - outer.top;
        let right = outer.right - (origin.x + content.right);
        let bottom = outer.bottom - (origin.y + content.bottom);
        let _ = SetWindowPos(
            hwnd,
            None,
            x - left,
            y - top,
            width + left + right,
            height + top + bottom,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    draw_as_active(hwnd);
}
