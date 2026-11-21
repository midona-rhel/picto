//! The application menu. Its menus, their titles, their order, their
//! separators and their keys are the ones that Picto 0.6.19 and earlier
//! had. Some rows depend on the selection and on the destination that the
//! grid shows; [`MenuContext`] holds what they depend on.
//!
//! On macOS, AppKit asks whether each row's action can be performed
//! (`App::is_action_available`) when a menu opens and when a key equivalent
//! arrives. The answer enables the row; the row's `disabled` flag does not.
//! A row is therefore enabled exactly when something handles its action
//! where the keyboard focus is. This file handles the window commands and
//! the Edit menu's text commands for the whole app. The main window's root
//! element handles the commands on the selection, and only while the row's
//! rule holds (`shell/menu_commands.rs`).
//!
//! The keys of the rows come from the shortcut registry through the keymap.
//! A shortcut that the user changes in Settings therefore shows its new
//! keys here: `crate::keymap::install` adds [`accelerators`] to its
//! bindings and sets the menu again.
use crate::keymap::{self, Scope};
use crate::settings::registry::{KeyboardPreset, Overrides};
use gpui::*;

mod model;
mod presentation;
#[cfg(test)]
mod tests;

pub use model::*;
pub use presentation::*;

actions!(
    app_menu,
    [
        About,
        Hide,
        HideOthers,
        ShowAllApps,
        // The next five actions are rows of the Library menu.
        LibraryManager,
        NewLibrary,
        OpenLibrary,
        ClearRecentLibraries,
        Subscriptions,
        NewFolder,
        NewSubfolder,
        NewSmartFolder,
        CheckForUpdates,
        CloseWindow,
        EditUndo,
        EditRedo,
        EditCut,
        EditCopy,
        EditPaste,
        EditSelectAll,
        CopyNames,
        CopyLinks,
        DeselectAll,
        DeletePermanently,
        Accept,
        Reject,
        AddTags,
        ShowTags,
        OpenInNewWindow,
        ShowDuplicates,
        // Shows or hides the log window.
        LogWindow,
        ToggleFullScreen,
        Minimize,
        Zoom,
        BringAllToFront
    ]
);

/// The action of the Move to Trash row. `keyed` says whether the row shows
/// the keys of the shortcut `file.delete`. It shows them outside a folder.
/// Inside a folder the row has no keys, because by default the Remove from
/// Folder row has the same keys there. The field makes the row with keys and
/// the row without keys two different actions, so that the keys are bound
/// to the first one only.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = app_menu, no_json)]
pub struct MoveToTrash {
    pub keyed: bool,
}

/// The action of the Remove from Folder row. `keyed` says whether the row
/// shows the keys of the shortcut `file.removeFromFolder`. It shows them
/// inside a folder, unless the items shown are in the Trash.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = app_menu, no_json)]
pub struct RemoveFromFolder {
    pub keyed: bool,
}

/// The action of a row of the Recent Libraries submenu. It opens the library
/// at `path`.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = app_menu, no_json)]
pub struct OpenRecentLibrary {
    pub path: String,
}

/// One library that the Recent Libraries submenu lists.
#[derive(Clone, Debug, PartialEq)]
pub struct Recent {
    pub path: String,
    pub name: String,
    pub pinned: bool,
    pub current: bool,
}

/// The action of a row of the Set Rating submenu. `stars` is the rating that
/// the row sets. The row No Rating has 0.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = app_menu, no_json)]
pub struct SetRating {
    pub stars: u8,
}

/// The operating system that the menu is built for. The menus on macOS
/// differ from the menus on Windows and Linux. Between Windows and Linux,
/// the Quit row and the row that shows a file in the file manager differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Mac,
    Windows,
    Linux,
}

impl Platform {
    pub const CURRENT: Platform = if cfg!(target_os = "macos") {
        Platform::Mac
    } else if cfg!(target_os = "windows") {
        Platform::Windows
    } else {
        Platform::Linux
    };

    pub const fn is_mac(self) -> bool {
        matches!(self, Platform::Mac)
    }

    /// Returns the title of the row that shows a file in the system's file
    /// manager: "Reveal in Finder" on macOS, "Show in Explorer" on Windows
    /// and "Show in File Manager" on Linux.
    pub const fn reveal_title(self) -> &'static str {
        match self {
            Platform::Mac => "Reveal in Finder",
            Platform::Windows => "Show in Explorer",
            Platform::Linux => "Show in File Manager",
        }
    }

    /// Returns the title of the Quit row: "Quit Picto" on macOS, "Exit" on
    /// Windows and "Quit" on Linux.
    pub const fn quit_title(self) -> &'static str {
        match self {
            Platform::Mac => "Quit Picto",
            Platform::Windows => "Exit",
            Platform::Linux => "Quit",
        }
    }

    /// Returns the keys of the Quit row: ⌘Q on macOS and Ctrl+Q on Linux. On
    /// Windows the row has no keys, because Alt+F4 closes a window there and
    /// Windows handles those keys itself.
    pub const fn quit_keys(self) -> Option<&'static str> {
        match self {
            Platform::Mac | Platform::Linux => Some("Mod+Q"),
            Platform::Windows => None,
        }
    }
}

/// The kind of the destination that the grid shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScopeKind {
    #[default]
    All,
    Inbox,
    Trash,
    Folder,
    SmartFolder,
    /// Recently Viewed, Untagged and the other built-in destinations.
    Other,
}

/// The lifecycle state of an item: in the Inbox, in the library or in the
/// Trash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Inbox,
    Active,
    Trash,
}

impl From<picto_library::Lifecycle> for Status {
    fn from(lifecycle: picto_library::Lifecycle) -> Self {
        match lifecycle {
            picto_library::Lifecycle::Inbox => Status::Inbox,
            picto_library::Lifecycle::Active => Status::Active,
            picto_library::Lifecycle::Trash => Status::Trash,
        }
    }
}

/// What the application menu needs to know about the main window: the
/// selection, the destination and the open library. The titles of the rows
/// and the rules that enable them are worked out from it
/// (`Shell::menu_context` fills it in).
///
/// `scope` is the kind of the destination that the grid shows. `status` is
/// the lifecycle state that the rows act on. It is the Trash or the Inbox
/// when the destination is the Trash or the Inbox. Otherwise it is the
/// state that all selected items have, or `None` when they differ or
/// nothing is selected (`GridScope::menu_status`).
#[derive(Clone, Debug, PartialEq)]
pub struct MenuContext {
    /// The number of selected items while the grid is shown. It is 0 while
    /// a screen is shown in place of the grid.
    pub selection: u64,
    /// `Some` when exactly one item is selected and it was selected by
    /// itself, not by Select All or as part of a range. The value says
    /// whether that item is a collection.
    pub single: Option<bool>,
    pub scope: ScopeKind,
    pub status: Option<Status>,
    /// Whether every selected item was selected by itself and is loaded.
    /// Rename and Copy Name are enabled only then, because they need the
    /// items' names.
    pub names: bool,
    /// Whether `names` holds and every selected item is a media file with a
    /// content hash. Regenerate Thumbnail is enabled only then.
    pub thumbnails: bool,
    /// Whether an import can start now. It is false while a running task
    /// blocks imports (`crate::imports::blocked`).
    pub import: bool,
    /// The open library's name.
    pub library: Option<String>,
    /// The libraries that the Recent Libraries submenu lists, with the
    /// pinned ones first.
    pub recent: Vec<Recent>,
}

impl Default for MenuContext {
    fn default() -> Self {
        Self {
            selection: 0,
            single: None,
            scope: ScopeKind::All,
            status: None,
            names: false,
            thumbnails: false,
            import: true,
            library: None,
            recent: Vec::new(),
        }
    }
}

impl MenuContext {
    pub fn has_selection(&self) -> bool {
        self.selection > 0
    }

    /// Returns whether exactly one item is selected and it is a media file,
    /// not a collection.
    pub fn single_media(&self) -> bool {
        self.single == Some(false)
    }

    pub fn single(&self) -> bool {
        self.single.is_some()
    }

    pub fn in_folder(&self) -> bool {
        self.scope == ScopeKind::Folder
    }

    pub fn in_trash(&self) -> bool {
        self.status == Some(Status::Trash)
    }

    pub fn in_inbox(&self) -> bool {
        self.status == Some(Status::Inbox)
    }

    pub fn can_create_folder(&self) -> bool {
        self.scope != ScopeKind::SmartFolder
    }

    pub fn can_create_smart_folder(&self) -> bool {
        !matches!(self.scope, ScopeKind::Folder | ScopeKind::SmartFolder)
    }

    /// Returns four contexts whose menus together contain every row that
    /// the menu can have. [`accelerators`] builds the menu for each of them
    /// to find every row that needs a key binding.
    fn variants() -> [MenuContext; 4] {
        let selected = |scope, status| MenuContext {
            selection: 2,
            scope,
            status,
            ..MenuContext::default()
        };
        [
            MenuContext::default(),
            selected(ScopeKind::Folder, Some(Status::Active)),
            selected(ScopeKind::Trash, Some(Status::Trash)),
            selected(ScopeKind::Inbox, Some(Status::Inbox)),
        ]
    }
}

/// The state of the menu: the context that the menu was last set for, the
/// main window, and whether Close Window has hidden the main window. On
/// macOS, Close Window hides the main window and does not close it.
#[derive(Default)]
struct Current {
    context: MenuContext,
    main: Option<AnyWindowHandle>,
    main_hidden: bool,
}

impl Global for Current {}

fn current(cx: &mut App) -> &mut Current {
    cx.default_global::<Current>()
}

/// Returns the context that the menu was last set for.
pub fn context(cx: &App) -> MenuContext {
    cx.try_global::<Current>()
        .map(|current| current.context.clone())
        .unwrap_or_default()
}

/// Sets the menu for `context`. Returns whether the context differs from the
/// one before.
pub fn set_context(context: MenuContext, cx: &mut App) -> bool {
    if current(cx).context == context {
        return false;
    }
    current(cx).context = context;
    set_menus(cx);
    true
}

/// Sets the application menu for the current context and key bindings.
pub fn set_menus(cx: &mut App) {
    let mut context = context(cx);
    context.recent = crate::libraries::recent(cx);
    cx.set_menus(gpui_menus(&context));
}

/// Records which window is the main window. Subscriptions… shows the main
/// window whichever window has the keyboard focus, and Close Window hides
/// the main window on macOS where it closes any other window.
pub fn set_main_window(window: AnyWindowHandle, cx: &mut App) {
    current(cx).main = Some(window);
}

/// Shows the main window again if Close Window hid it. macOS calls this
/// when the user clicks the app's icon in the Dock.
pub fn reopen(cx: &mut App) {
    let current = current(cx);
    if !current.main_hidden {
        return;
    }
    current.main_hidden = false;
    let main = current.main;
    if let Some(main) = main {
        let _ = main.update(cx, |_, window, _| window.activate_window());
    }
}

/// Brings the main window to the front and gives it the keyboard focus. A
/// main window that Close Window hid is shown again, and a minimised one is
/// restored. While the environment variable `PICTO_GPUI_BACKGROUND` is set,
/// the window stays where it is and a line on standard error says so. A
/// test run sets the variable so that it does not take the keyboard focus
/// from the user.
pub fn bring_forward(cx: &mut App) {
    if crate::seams::background() {
        eprintln!("picto: the main window would come forward (held back in the background)");
        return;
    }
    current(cx).main_hidden = false;
    let main = current(cx).main;
    if let Some(main) = main {
        let _ = main.update(cx, |_, window, _| window.activate_window());
    }
    cx.activate(true);
}

/// Returns whether a text field has the keyboard focus in `window`.
pub fn in_text_field(window: &Window) -> bool {
    window
        .context_stack()
        .iter()
        .any(|context| context.contains("TextInput"))
}

/// Returns whether the keys that ran a row of the menu were typed into a
/// text field. That is the case when a text field has the keyboard focus
/// and the platform modifier (⌘ on macOS) is held. The caller then leaves
/// the keys to the field, where ⌘⌫ deletes to the start of the line, and
/// does not delete the selected items.
pub fn typed_in_text_field(window: &Window) -> bool {
    in_text_field(window) && window.modifiers().platform
}

/// Runs `work` on the window that has the keyboard, once the action that
/// asked for it has finished with that window.
fn on_active(cx: &mut App, work: impl FnOnce(&mut Window, &mut App) + 'static) {
    if let Some(window) = cx.active_window() {
        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, cx| work(window, cx));
        });
    }
}

/// Performs a row of the Edit menu. When a text field has the keyboard
/// focus, it dispatches `in_field`, which is the field's own action. When
/// none has, it dispatches `otherwise`, which acts on the window's content.
fn edit(cx: &mut App, in_field: Option<Box<dyn Action>>, otherwise: Option<Box<dyn Action>>) {
    on_active(cx, move |window, cx| {
        let action = if in_text_field(window) {
            in_field
        } else {
            otherwise
        };
        if let Some(action) = action {
            window.dispatch_action(action, cx);
        }
    });
}

/// Performs Close Window on the window that has the keyboard focus. The
/// Settings window closes as its close button closes it, which discards its
/// unsaved changes. A detail window closes. On macOS the main window is
/// hidden and the app keeps running; a click on the Dock icon shows the
/// window again ([`reopen`]). On Windows and Linux the main window closes,
/// which quits the app ([`quit_with_main_window`]).
fn close_window(cx: &mut App) {
    let Some(handle) = cx.active_window() else {
        return;
    };
    cx.defer(move |cx| {
        if crate::settings::close_window(handle, cx) {
            return;
        }
        let main = current(cx).main;
        let is_main = main == Some(handle);
        if is_main && Platform::CURRENT.is_mac() {
            current(cx).main_hidden = true;
            let _ = handle.update(cx, |_, window, _| {
                crate::ui::window_chrome::order_out(window)
            });
        } else {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    });
}

/// Makes the app quit when its main window closes, on Windows and Linux. It
/// does not matter how the window closes: by its close button, by the Close
/// row, by Alt+F4 or by the window manager. A detail window or the Settings
/// window that is still open closes with the app, because it shows the
/// library of the main window. On macOS this function does nothing.
pub fn quit_with_main_window(cx: &mut App) {
    if Platform::CURRENT.is_mac() {
        return;
    }
    cx.on_window_closed(|cx, closed| {
        let main = current(cx).main;
        if main.is_some_and(|main| main.window_id() == closed) {
            cx.quit();
        }
    })
    .detach();
}

/// Registers the handlers of the actions that work in every window: the
/// rows of the "Picto" menu, the rows of the Library menu, Check for
/// Updates…, the window commands and the Edit menu's text commands.
pub fn bind(cx: &mut App) {
    cx.default_global::<Current>();
    cx.on_action(|_: &About, cx| cx.defer(|cx| crate::settings::open_at("about", cx)));
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAllApps, cx| cx.unhide_other_apps());
    cx.on_action(|_: &CloseWindow, cx| close_window(cx));
    cx.on_action(|_: &Minimize, cx| on_active(cx, |window, _| window.minimize_window()));
    // The Settings window can be neither zoomed nor shown full screen, so
    // Zoom and Toggle Full Screen do nothing while it has the keyboard
    // focus.
    cx.on_action(|_: &Zoom, cx| {
        let settings = cx
            .active_window()
            .is_some_and(|window| crate::settings::is_window(window, cx));
        if !settings {
            on_active(cx, |window, _| window.zoom_window());
        }
    });
    cx.on_action(|_: &ToggleFullScreen, cx| {
        let settings = cx
            .active_window()
            .is_some_and(|window| crate::settings::is_window(window, cx));
        if !settings {
            on_active(cx, |window, _| window.toggle_fullscreen());
        }
    });
    cx.on_action(|_: &BringAllToFront, cx| {
        crate::ui::window_chrome::bring_all_to_front(cx);
    });
    // Subscriptions… shows the Subscriptions screen in the main window and
    // brings that window to the front, whichever window the row was chosen
    // in.
    cx.on_action(|_: &Subscriptions, cx| {
        cx.defer(move |cx| {
            let Some(shell) = crate::libraries::host::shell(cx) else {
                return;
            };
            shell.update(cx, |shell, cx| {
                shell.show_screen(crate::backend::Screen::Subscriptions, cx)
            });
            if let Some(main) = current(cx).main {
                let _ = main.update(cx, |_, window, _| window.activate_window());
            }
        });
    });
    // The rows of the Library menu work in every window.
    cx.on_action(|_: &LibraryManager, cx| cx.defer(crate::libraries::manager::open));
    // New Library… opens the Library Manager, where a library is created.
    cx.on_action(|_: &NewLibrary, cx| cx.defer(crate::libraries::manager::open));
    cx.on_action(|_: &OpenLibrary, cx| cx.defer(crate::libraries::host::open_dialog));
    cx.on_action(|action: &OpenRecentLibrary, cx| {
        if action.path.is_empty() {
            return;
        }
        let path = std::path::PathBuf::from(&action.path);
        cx.defer(move |cx| {
            crate::libraries::host::request(&path, crate::libraries::Origin::Recent, cx)
        });
    });
    cx.on_action(|_: &ClearRecentLibraries, cx| cx.defer(crate::libraries::clear_history));
    // Check for Updates… looks for a newer version and then opens the
    // update dialog. It works in every window.
    cx.on_action(|_: &CheckForUpdates, cx| cx.defer(crate::updates::check_from_menu));
    cx.on_action(|_: &EditCut, cx| edit(cx, Some(Box::new(crate::ui::text_input::Cut)), None));
    // Outside a text field, Copy puts the selected files on the clipboard,
    // as ⌘C does over the grid, in the viewer and in a detail window.
    cx.on_action(|_: &EditCopy, cx| {
        edit(
            cx,
            Some(Box::new(crate::ui::text_input::Copy)),
            Some(Box::new(keymap::CopyFiles)),
        )
    });
    cx.on_action(|_: &EditPaste, cx| {
        edit(
            cx,
            Some(Box::new(crate::ui::text_input::Paste)),
            Some(Box::new(keymap::PasteImport)),
        )
    });
    cx.on_action(|_: &EditSelectAll, cx| {
        edit(
            cx,
            Some(Box::new(crate::ui::text_input::SelectAll)),
            Some(Box::new(crate::shell::SelectAll)),
        )
    });
    // In a text field, Undo and Redo undo and redo the typing in that
    // field. Elsewhere they undo and redo the changes to the library.
    cx.on_action(|_: &EditUndo, cx| {
        edit(
            cx,
            Some(Box::new(crate::ui::text_input::Undo)),
            Some(Box::new(crate::shell::Undo)),
        )
    });
    cx.on_action(|_: &EditRedo, cx| {
        edit(
            cx,
            Some(Box::new(crate::ui::text_input::Redo)),
            Some(Box::new(crate::shell::Redo)),
        )
    });
}

/// Returns the action that `line` names, for the `action` lines of an
/// off-screen script (`offscreen.rs`). An action that holds a value takes
/// the value after its name, as in `app_menu::SetRating 3` and
/// `app_menu::MoveToTrash true`. Returns `None` when no action has that
/// name or the value cannot be read.
#[cfg(test)]
pub fn named(line: &str, cx: &App) -> Option<Box<dyn Action>> {
    let (name, value) = line.split_once(' ').unwrap_or((line, ""));
    let value = value.trim();
    match name {
        "app_menu::SetRating" => Some(Box::new(SetRating {
            stars: value.parse().ok()?,
        })),
        "shortcut::Rate" => Some(Box::new(keymap::Rate {
            stars: value.parse().ok()?,
        })),
        "app_menu::MoveToTrash" => Some(Box::new(MoveToTrash {
            keyed: value == "true",
        })),
        "app_menu::RemoveFromFolder" => Some(Box::new(RemoveFromFolder {
            keyed: value == "true",
        })),
        _ => cx.build_action(name, None).ok(),
    }
}
