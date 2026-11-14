//! The drawing order of everything that floats over the window, as
//! production's layer tokens give it (`tokens.css` `--layer-*`: overlay <
//! modal < menu < tooltip < drag overlay < notification), with the few
//! literal `z-index` values production sets outside that scale put in
//! between. Every `deferred(...).with_priority(...)` takes its priority from
//! here.
//!
//! GPUI draws all deferred elements of a frame as one list sorted by
//! priority, however they are nested (`Window::paint_deferred_draws`): a
//! deferred element inside another is not drawn above it unless its
//! priority is at least as high (on a tie, the inner one comes later and
//! wins). So a floating thing that can open inside another (a tooltip in a
//! dialog, a select's list in a popover) needs the higher layer here, as a
//! portal in production needs the higher `z-index`. The values only order;
//! the gaps leave room for dialogs stacked one over another.

/// `--layer-overlay`: a field's expanded overlay and the inspector's list
/// of sources, over the page they belong to.
pub const OVERLAY: usize = 10;

/// The tag and folder pickers' panel (`OverlayShell`) opened from the
/// inspector or the filter bar. Production draws it on `--layer-modal` with
/// the dialogs; it stays under them here, so that a dialog opened while a
/// pinned picker is up covers it, as the later portal does.
pub const PANEL: usize = 15;

/// `--layer-modal`: a dialog and its backdrop. A dialog opened over another
/// one takes `DIALOG + 1` (and so on).
pub const DIALOG: usize = 20;

/// The Inbox review pill (`InboxReviewControls`, `--layer-modal`), over
/// the dialog a reject asks, which it waits on.
pub const REVIEW: usize = 24;

/// `calc(--layer-modal + 1)`: what a dialog opens over itself: a tag
/// field's suggestions, and a picker opened from a dialog's button (with
/// the dialogs it opens at `OVER_DIALOG + 1`).
pub const OVER_DIALOG: usize = 25;

/// `--layer-menu`: context menus, a select's list, the library switcher.
pub const MENU: usize = 30;

/// A day picker's calendar and the smart-folder dialog's icon and colour
/// popovers (`z-index` 9998 to 10021 in production): over the dialogs and
/// menus they open from.
pub const POPOVER: usize = 35;

/// `--layer-tooltip`: tooltips, over whatever their control is on.
pub const TOOLTIP: usize = 40;

/// `--layer-drag-overlay`: the picture that follows a drag.
pub const DRAG: usize = 50;

/// The grid's hover preview (`HoverPreviewPortal`, `z-index` 200002).
pub const PREVIEW: usize = 55;

/// `--layer-notification`: notifications, over every other layer here.
/// Only the window's rim (`window_chrome::rim`, `.shellMac::after`, at
/// priority 100) is drawn above them.
pub const NOTIFICATION: usize = 60;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_follow_production_order() {
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
            // `window_chrome::rim`.
            100,
        ];
        assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{order:?}");
        // A few dialogs can stack before they reach the review pill.
        assert!(DIALOG + 3 < REVIEW);
        // A tooltip shows over every surface its control can sit on.
        for surface in [OVERLAY, PANEL, DIALOG, OVER_DIALOG, MENU, POPOVER] {
            assert!(TOOLTIP > surface);
        }
    }
}
