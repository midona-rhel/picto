//! Decides what a key press or a click on the grid opens, according to the
//! Controls settings. Space toggles Quick Look or scrolls the grid by a page,
//! a double-click opens the media view or the file's default application,
//! and a middle-click opens a detail window. A viewer that is open closes on
//! Space, Enter and Escape through its own key bindings.

/// What a key press or a click on the grid asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    OpenMediaView,
    CloseMediaView,
    OpenQuickLook,
    CloseQuickLook,
    /// Open the item in a detail window.
    OpenWindow,
    /// Open the file in its default application.
    OpenDefault,
    /// Scroll the grid down by most of its height.
    ScrollPage,
    Nothing,
}

/// What the grid has open when the key press or the click arrives.
#[derive(Clone, Copy, Debug, Default)]
pub struct State {
    pub media_view: bool,
    pub quick_look: bool,
    /// Whether exactly one item is selected.
    pub single: bool,
}

/// Returns what Space (the `view.quicklook` shortcut) does. An open media
/// view closes, whatever the setting is. Otherwise the setting `scroll`
/// scrolls the grid by a page, and any other setting toggles Quick Look,
/// which opens only when exactly one item is selected.
pub fn space(setting: &str, state: State) -> Action {
    if state.media_view {
        Action::CloseMediaView
    } else if setting == "scroll" {
        Action::ScrollPage
    } else if state.quick_look {
        Action::CloseQuickLook
    } else if state.single {
        Action::OpenQuickLook
    } else {
        Action::Nothing
    }
}

/// Returns what a double-click on a tile does. With the setting `external`,
/// a media item opens in its default application. A collection, and any item
/// with the default setting `detail`, opens in the media view.
pub fn double_click(setting: &str, collection: bool) -> Action {
    if setting == "external" && !collection {
        Action::OpenDefault
    } else {
        Action::OpenMediaView
    }
}

/// Returns what a middle-click on a tile does. With the setting
/// `new_window`, which is the default, the item opens in a detail window.
/// With `none`, nothing happens.
pub fn middle_click(setting: &str) -> Action {
    if setting == "new_window" {
        Action::OpenWindow
    } else {
        Action::Nothing
    }
}

#[cfg(test)]
mod tests {
    use super::{Action, State, double_click, middle_click, space};

    const ONE: State = State {
        media_view: false,
        quick_look: false,
        single: true,
    };

    #[test]
    fn space_toggles_quick_look_by_default() {
        assert_eq!(space("quick_look", ONE), Action::OpenQuickLook);
        let open = State {
            quick_look: true,
            ..ONE
        };
        assert_eq!(space("quick_look", open), Action::CloseQuickLook);
        // With no item or several items selected, there is nothing to
        // preview.
        let several = State {
            single: false,
            ..ONE
        };
        assert_eq!(space("quick_look", several), Action::Nothing);
    }

    #[test]
    fn space_pages_the_grid_when_set_to_scroll() {
        assert_eq!(space("scroll", ONE), Action::ScrollPage);
        let several = State {
            single: false,
            ..ONE
        };
        assert_eq!(space("scroll", several), Action::ScrollPage);
    }

    #[test]
    fn space_closes_the_media_view() {
        let viewing = State {
            media_view: true,
            ..ONE
        };
        assert_eq!(space("quick_look", viewing), Action::CloseMediaView);
        assert_eq!(space("scroll", viewing), Action::CloseMediaView);
    }

    #[test]
    fn clicks_follow_their_settings() {
        assert_eq!(double_click("detail", false), Action::OpenMediaView);
        assert_eq!(double_click("external", false), Action::OpenDefault);
        // A collection has no single file to hand to another application.
        assert_eq!(double_click("external", true), Action::OpenMediaView);
        assert_eq!(middle_click("new_window"), Action::OpenWindow);
        assert_eq!(middle_click("none"), Action::Nothing);
    }
}
