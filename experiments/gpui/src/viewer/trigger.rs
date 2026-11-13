//! Which way a key or a click on the grid opens an item, under the Controls
//! settings, as `GridScreen.tsx` decides it: Space toggles Quick Look (or
//! pages the grid), a double-click opens the media view or the default
//! application, and a middle-click a window of its own. Inside a viewer,
//! Space, Enter and Escape close it through the viewer's own keys.

/// What a gesture asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    OpenMediaView,
    CloseMediaView,
    OpenQuickLook,
    CloseQuickLook,
    /// A detail window of its own.
    OpenWindow,
    /// The default application for the file.
    OpenDefault,
    /// Scroll the grid down by most of its height.
    ScrollPage,
    Nothing,
}

/// What the grid has open when the gesture comes.
#[derive(Clone, Copy, Debug, Default)]
pub struct State {
    pub media_view: bool,
    pub quick_look: bool,
    /// Exactly one item is selected.
    pub single: bool,
}

/// `view.quicklook` (Space). The media view takes it first and closes, as
/// its own handler does; otherwise `spaceKeyAction` either pages the grid
/// or toggles Quick Look for a single selected item.
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

/// `onTileDoubleClick`: `gridDoubleClickAction` `external` opens a media item
/// in the default application; a collection, or the `detail` default, opens
/// the media view.
pub fn double_click(setting: &str, collection: bool) -> Action {
    if setting == "external" && !collection {
        Action::OpenDefault
    } else {
        Action::OpenMediaView
    }
}

/// `onTileMiddleClick`: `gridMiddleClickAction` `new_window` (the default)
/// opens the item in a window of its own; `none` does nothing.
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
        // Nothing, or several items, selected: nothing to preview.
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
