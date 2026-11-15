//! The application menu, as production's `electron/windows/menu.mjs` builds
//! it on macOS: the same menus, titles, order, separators and accelerators,
//! with the rows that depend on the selection and the destination enabled
//! by its rules (the command context of `applicationMenuRuntime.ts`).
//!
//! AppKit asks, as a menu opens or a key equivalent arrives, whether each
//! row's action can be performed (`App::is_action_available`), and that,
//! not the row's `disabled` flag, enables the row. So a row is enabled
//! exactly when something handles its action where the keyboard is: the
//! window commands and the Edit menu's text commands are handled app-wide
//! here; the commands on the selection are handled on the main window's
//! root only while production's rule holds (`shell/menu_commands.rs`).
//!
//! The accelerators come from the shortcut registry through the keymap, so
//! a shortcut rebound in Settings shows its new keys here:
//! `crate::keymap::install` adds [`accelerators`] to its bindings and sets
//! the menu again.
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
        // The Library menu's rows (`crate::libraries`).
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
        // Shows or hides the log window (`menu:toggle-diagnostics`).
        LogWindow,
        ToggleFullScreen,
        Minimize,
        Zoom,
        BringAllToFront
    ]
);

/// Move to Trash. Its row carries the Delete key only outside a folder,
/// where Remove from Folder has it, so the two rows are told apart.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = app_menu, no_json)]
pub struct MoveToTrash {
    pub keyed: bool,
}

/// Remove from Folder, keyed inside a folder only.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = app_menu, no_json)]
pub struct RemoveFromFolder {
    pub keyed: bool,
}

/// A row of Recent Libraries: the library at `path`.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = app_menu, no_json)]
pub struct OpenRecentLibrary {
    pub path: String,
}

/// A library Recent Libraries lists (`buildRecentLibrariesMenu`).
#[derive(Clone, Debug, PartialEq)]
pub struct Recent {
    pub path: String,
    pub name: String,
    pub pinned: bool,
    pub current: bool,
}

/// Set Rating's rows, 0 for No Rating.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = app_menu, no_json)]
pub struct SetRating {
    pub stars: u8,
}

/// The system the menu is built for: production's template differs on
/// macOS (`isMac`) and, in a few role rows, between Windows and Linux.
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

    /// The row that shows a file in the system's file browser: production's
    /// "Reveal in Finder" on the Mac and "Show in Explorer" on Windows. On
    /// Linux production also writes "Show in Explorer", which names a
    /// Windows program; it says "Show in File Manager" where it means the
    /// same elsewhere (the Library Manager), which is used here.
    pub const fn reveal_title(self) -> &'static str {
        match self {
            Platform::Mac => "Reveal in Finder",
            Platform::Windows => "Show in Explorer",
            Platform::Linux => "Show in File Manager",
        }
    }

    /// Electron's `quit` role's title: "Quit Picto" in the Mac's app menu,
    /// "Exit" on Windows, "Quit" on Linux.
    pub const fn quit_title(self) -> &'static str {
        match self {
            Platform::Mac => "Quit Picto",
            Platform::Windows => "Exit",
            Platform::Linux => "Quit",
        }
    }

    /// Quit's keys, as Electron's `quit` role has them: ⌘Q on the Mac,
    /// Ctrl+Q on Linux, and none on Windows, where Alt+F4 is the system's.
    pub const fn quit_keys(self) -> Option<&'static str> {
        match self {
            Platform::Mac | Platform::Linux => Some("Mod+Q"),
            Platform::Windows => None,
        }
    }
}

/// The destination's kind (`gridScopeAtom`'s `kind`).
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

/// The lifecycle the selection shares, or the destination's
/// (`statusFilter`).
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

/// What production's renderer tells the menu (`menuContext`).
#[derive(Clone, Debug, PartialEq)]
pub struct MenuContext {
    /// How many items are selected while the grid is the destination
    /// (`selectionCount`); 0 while a screen shows.
    pub selection: u64,
    /// The one item chosen by itself, and whether it is a collection
    /// (`singleSelected`, `singleKind`).
    pub single: Option<bool>,
    pub scope: ScopeKind,
    pub status: Option<Status>,
    /// Every selected item was chosen one by one and is loaded
    /// (`canRename`, `canCopyNames`).
    pub names: bool,
    /// ...and every one is a file with a thumbnail
    /// (`canRegenerateThumbnails`).
    pub thumbnails: bool,
    /// No task holds imports back (`canImport`).
    pub import: bool,
    /// The open library's name.
    pub library: Option<String>,
    /// The libraries Recent Libraries lists, pinned ones first.
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

    /// One file, not a collection (`singleMedia`).
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

    /// The contexts whose menus between them hold every row, for binding
    /// every row's keys.
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

/// The menu's state: the context it was last set for, and the main window,
/// which Close Window hides rather than closes.
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

/// The context the menu shows.
pub fn context(cx: &App) -> MenuContext {
    cx.try_global::<Current>()
        .map(|current| current.context.clone())
        .unwrap_or_default()
}

/// Sets the menu for this context. Returns whether it changed.
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

/// The main window, for the rows production sends to it whichever window
/// has the keyboard, and for Close Window.
pub fn set_main_window(window: AnyWindowHandle, cx: &mut App) {
    current(cx).main = Some(window);
}

/// The Dock icon was clicked: the main window that Close Window hid comes
/// back, as production makes a main window again.
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

/// `focusMainWindow`: the main window comes back if Close Window hid it,
/// out of the Dock or taskbar if minimised, and in front with the
/// keyboard, as when the app is asked to open files. With
/// `PICTO_GPUI_BACKGROUND` it stays behind and says so, so a check never
/// takes the keyboard from the user.
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

/// Whether a text field has the window's keyboard.
pub fn in_text_field(window: &Window) -> bool {
    window
        .context_stack()
        .iter()
        .any(|context| context.contains("TextInput"))
}

/// Whether a key the menu received was typed into a text field: the field
/// keeps its editing keys (⌘⌫ deletes to the line's start), as a Chromium
/// field does before production's menu sees them.
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

/// An Edit command: to the focused text field, as the native Edit menu's
/// are, or else to the window.
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

/// Close Window: Settings closes as its close button closes it, dropping
/// its draft; a detail window closes; on macOS the main window is hidden,
/// as production's closes while the app keeps running (it does not quit
/// when its last window closes, and the Dock icon brings a main window
/// back). On Windows and Linux the main window closes, which quits the app
/// ([`quit_with_main_window`]).
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

/// Off macOS the app ends with its main window, however that closes (its
/// close button, Close, Alt+F4, the window manager), as production quits
/// when its windows are all closed there (`window-all-closed`). Production
/// leaves a detail or Settings window open on its own after the main one
/// closes; here they go with it, since they show the main window's library.
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

/// The app-wide commands: the Picto menu's, the window commands and the
/// Edit menu's text commands.
pub fn bind(cx: &mut App) {
    cx.default_global::<Current>();
    cx.on_action(|_: &About, cx| cx.defer(|cx| crate::settings::open_at("about", cx)));
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAllApps, cx| cx.unhide_other_apps());
    cx.on_action(|_: &CloseWindow, cx| close_window(cx));
    cx.on_action(|_: &Minimize, cx| on_active(cx, |window, _| window.minimize_window()));
    // Settings is neither zoomable nor full-screen in production
    // (`maximizable: false`, `fullscreenable: false`).
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
    // Subscriptions… goes to the main window from any window
    // (`sendToMainWindow`).
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
    // The Library menu, from any window.
    cx.on_action(|_: &LibraryManager, cx| cx.defer(crate::libraries::manager::open));
    // `New Library…` opens the Library Manager, as production's does.
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
    // A check, then the update dialog (`checkForUpdates`), from any window.
    cx.on_action(|_: &CheckForUpdates, cx| cx.defer(crate::updates::check_from_menu));
    cx.on_action(|_: &EditCut, cx| edit(cx, Some(Box::new(crate::ui::text_input::Cut)), None));
    // Outside a text field, Copy puts the files themselves on the
    // clipboard, as ⌘C over the grid, the viewer and a detail window does.
    // (Production's row only copies text; chosen with the pointer over the
    // grid it copies nothing.)
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
    // In a text field Undo and Redo are the field's own; elsewhere they
    // walk the library's history.
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

/// An action by its name, for the off-screen tool's `action` lines: the
/// menu's rows that carry a value take it after the name
/// (`app_menu::SetRating 3`, `app_menu::MoveToTrash true`).
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
