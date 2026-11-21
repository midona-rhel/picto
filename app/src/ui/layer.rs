//! The drawing order of everything that is drawn over the rest of the window.
//! From bottom to top the main layers are: overlay, dialog, menu, tooltip, the
//! picture that follows a drag, and notification. A few more layers lie
//! between them. Every `deferred(...).with_priority(...)` takes its priority
//! from here.
//!
//! GPUI draws all deferred elements of a frame as one list sorted by priority,
//! however they are nested (`Window::paint_deferred_draws`). A deferred
//! element inside another is not drawn above it unless its priority is at
//! least as high (on a tie, the inner one is drawn later and is on top). So
//! something that can open inside another deferred element (a tooltip in a
//! dialog, a select's list in a popover) needs the higher number here. Only
//! the order of the values matters. The gaps between them leave room for
//! dialogs that are opened one over another.

/// A field's expanded overlay and the inspector's list of sources, which are
/// drawn over the page they belong to.
pub const OVERLAY: usize = 10;

/// The panel of the tag picker and of the folder picker, opened from the
/// inspector or the filter bar. It is below the dialogs, so that a dialog that
/// is opened while a pinned picker is open covers the picker.
pub const PANEL: usize = 15;

/// A dialog and its backdrop. A dialog that is opened over another one takes
/// `DIALOG + 1`, the next one `DIALOG + 2`, and so on.
pub const DIALOG: usize = 20;

/// The Inbox review pill. It is above the dialogs, so that it is drawn over
/// the dialog that asks the user to confirm a rejection while the pill waits
/// for the answer.
pub const REVIEW: usize = 24;

/// What a dialog opens over itself: the suggestions of a tag field, and a
/// picker that is opened from a button in the dialog. A dialog that such a
/// picker opens takes `OVER_DIALOG + 1`.
pub const OVER_DIALOG: usize = 25;

/// Context menus, the list of a select, and the library switcher.
pub const MENU: usize = 30;

/// The calendar of a day picker, and the icon and colour popovers of the
/// smart-folder dialog. They are above the dialogs and menus that they are
/// opened from.
pub const POPOVER: usize = 35;

/// Tooltips, which are above whatever their control is drawn on.
pub const TOOLTIP: usize = 40;

/// The picture that follows the pointer during a drag.
pub const DRAG: usize = 50;

/// The grid's hover preview.
pub const PREVIEW: usize = 55;

/// Notifications, which are above every other layer here except the window's
/// rim.
pub const NOTIFICATION: usize = 60;

/// The window's rim (`window_chrome::rim`), which is above everything else
/// that the window draws.
pub const CHROME: usize = 100;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_follow_the_earlier_versions_order() {
        let order = [
            OVERLAY,
            PANEL,
            DIALOG,
            REVIEW,
            OVER_DIALOG,
            MENU,
            POPOVER,
            TOOLTIP,
            DRAG,
            PREVIEW,
            NOTIFICATION,
            CHROME,
        ];
        assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{order:?}");
        // Three dialogs can be opened one over another before their priority
        // reaches that of the review pill.
        assert!(DIALOG + 3 < REVIEW);
        // A tooltip is drawn above every surface that its control can be on.
        for surface in [OVERLAY, PANEL, DIALOG, OVER_DIALOG, MENU, POPOVER] {
            assert!(TOOLTIP > surface);
        }
    }
}
