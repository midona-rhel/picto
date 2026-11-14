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
//! root only while production's rule holds (`shell/menu_commands.rs`); and a
//! row whose feature this app does not have yet has no handler at all, so
//! it shows disabled.
//!
//! The accelerators come from the shortcut registry through the keymap, so
//! a shortcut rebound in Settings shows its new keys here:
//! `crate::keymap::install` adds [`accelerators`] to its bindings and sets
//! the menu again.
use crate::keymap::{self, Scope};
use crate::settings::registry::{self, KeyboardPreset, Overrides};
use gpui::*;

actions!(
    app_menu,
    [
        About,
        Hide,
        HideOthers,
        ShowAllApps,
        // TODO: open the Library Manager window, create a library, open a
        // `.library` folder, and switch to a recent one. Blocked on the
        // owner's decision on opening real libraries: this app only opens
        // the disposable copy it was started with.
        LibraryManager,
        NewLibrary,
        OpenLibrary,
        OpenRecentLibrary,
        // TODO: forget every library but the open one, then set the menu
        // again (the history is kept by the Library Manager's store).
        ClearRecentLibraries,
        Subscriptions,
        NewFolder,
        NewSubfolder,
        NewSmartFolder,
        ImportFiles,
        ImportFolder,
        // TODO: check for updates and show the updates dialog
        // (`menu:show-updates`). This app does no network work.
        CheckForUpdates,
        CloseWindow,
        Rename,
        BatchRename,
        EditUndo,
        EditRedo,
        EditCut,
        EditCopy,
        EditPaste,
        EditSelectAll,
        CopyPaths,
        CopyNames,
        CopyLinks,
        RegenerateThumbnails,
        DeselectAll,
        Restore,
        DeletePermanently,
        Accept,
        Reject,
        AddTags,
        AddToFolder,
        AutoTag,
        ShowTags,
        OpenDefault,
        Reveal,
        OpenInNewWindow,
        ShowDuplicates,
        // TODO: show or hide the log window (`menu:toggle-diagnostics`).
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
    /// The open library's name, the one row Recent Libraries has.
    pub library: Option<String>,
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

    /// Rename for one item; several have Batch Rename… instead.
    pub fn can_rename_one(&self) -> bool {
        self.names && self.selection == 1
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

/// How a row's accelerator reaches the keymap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Binding {
    /// The row's action already has these keys bound elsewhere.
    Existing,
    /// Shown by the menu but not bound in the window: the keys already do
    /// this through another action (the registry's, a text field's), or
    /// they must reach the window's own key listeners first. AppKit sends
    /// them to the row when nothing in the window took them, as production's
    /// menu accelerators fire for keys the page left alone.
    Shown,
    /// Only the menu has these keys: also bound in the window, where they
    /// listen, so they work wherever the window's shortcuts do.
    Live(Scope),
}

/// A row's accelerator.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Keys {
    None,
    /// The registry shortcut that the row's own action is bound to.
    Registry(&'static str),
    /// A registry shortcut for a row whose action is the menu's own: shown
    /// only when the registry binds the shortcut to another action, bound
    /// for real when nothing does.
    Shortcut(&'static str),
    /// Keys production's template fixes (a role's, or a literal), written
    /// as the registry writes keys.
    Fixed(&'static str, Binding),
}

pub struct Item {
    pub title: String,
    pub keys: Keys,
    /// Production's rule for the row.
    pub enabled: bool,
    /// Whether this app performs the row; one that does not has no handler
    /// and shows disabled whatever the rule says.
    pub built: bool,
    pub checked: bool,
    pub action: Box<dyn Action>,
    pub os_action: Option<OsAction>,
}

pub enum Entry {
    Item(Item),
    Separator,
    Submenu {
        title: String,
        enabled: bool,
        entries: Vec<Entry>,
    },
    /// The system's Services menu.
    Services,
}

pub struct TopMenu {
    pub title: &'static str,
    pub entries: Vec<Entry>,
}

fn item(title: impl Into<String>, action: impl Action) -> Item {
    Item {
        title: title.into(),
        keys: Keys::None,
        enabled: true,
        built: true,
        checked: false,
        action: Box::new(action),
        os_action: None,
    }
}

impl Item {
    fn keys(mut self, keys: Keys) -> Self {
        self.keys = keys;
        self
    }

    fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Not performed by this app yet; the action's `TODO` says what it
    /// should do.
    fn todo(mut self) -> Self {
        self.built = false;
        self
    }

    fn os(mut self, os_action: OsAction) -> Self {
        self.os_action = Some(os_action);
        self
    }

    /// The keys this row shows, as the registry writes them.
    pub fn accelerator(&self, preset: KeyboardPreset, overrides: &Overrides) -> Option<String> {
        let keys = match self.keys {
            Keys::None => return None,
            Keys::Registry(id) | Keys::Shortcut(id) => registry::shortcut(id, preset, overrides)
                .map(|shortcut| shortcut.keys)
                .unwrap_or_default(),
            Keys::Fixed(keys, _) => keys.to_owned(),
        };
        (!keys.is_empty()).then_some(keys)
    }
}

impl From<Item> for Entry {
    fn from(item: Item) -> Self {
        Entry::Item(item)
    }
}

/// A row's title by the selection's size (`selectionCount > 1`).
fn by_count(count: u64, one: &str, many: impl FnOnce(u64) -> String) -> String {
    if count > 1 {
        many(count)
    } else {
        one.to_owned()
    }
}

/// The menus for a context on a platform, in production's order. Off the
/// Mac there is no app menu: Settings… and Quit end the File menu, and the
/// Window menu closes the window instead of bringing all to front.
pub fn model(context: &MenuContext, platform: Platform) -> Vec<TopMenu> {
    use Entry::Separator;
    let count = context.selection;
    let has = context.has_selection();
    let fixed = |keys, binding| Keys::Fixed(keys, binding);
    // The window commands' keys reach their rows through AppKit, after the
    // window has passed them up: a field listening for keys (Settings'
    // shortcut capture) takes ⌘W before Close Window does, as a page does
    // before production's menu.
    let window_key = Binding::Shown;
    let main_window = Binding::Live(Scope::App);

    let mut library = vec![
        item("Library Manager…", LibraryManager)
            .keys(fixed("Mod+Shift+L", main_window))
            .todo()
            .into(),
        Separator,
        item("New Library…", NewLibrary)
            .keys(fixed("Mod+N", main_window))
            .todo()
            .into(),
        item("Open Library…", OpenLibrary).todo().into(),
        Separator,
    ];
    // Production lists the libraries opened before, the open one checked;
    // this app knows only the open one.
    if let Some(name) = &context.library {
        let mut current = item(name.clone(), OpenRecentLibrary).todo();
        current.checked = true;
        library.push(Entry::Submenu {
            title: "Recent Libraries".into(),
            enabled: true,
            entries: vec![
                current.into(),
                Separator,
                item("Clear History", ClearRecentLibraries).todo().into(),
            ],
        });
        library.push(Separator);
    }
    library.push(
        item("Subscriptions…", Subscriptions)
            .keys(fixed("Mod+Shift+S", main_window))
            .into(),
    );

    let in_folder = context.in_folder();
    let new_folder = if in_folder {
        item("New Subfolder", NewSubfolder).keys(Keys::Shortcut("file.newSubfolder"))
    } else {
        item("New Folder", NewFolder).keys(Keys::Shortcut("file.newFolder"))
    };
    let mut file = vec![
        new_folder.enabled(context.can_create_folder()).into(),
        item("New Smart Folder", NewSmartFolder)
            .keys(Keys::Shortcut("file.newSmartFolder"))
            .enabled(context.can_create_smart_folder())
            .into(),
        Separator,
        item("Import Files…", ImportFiles)
            .keys(Keys::Shortcut("file.import"))
            .enabled(context.import)
            .into(),
        item("Import Folder…", ImportFolder)
            .keys(fixed("Mod+Shift+I", Binding::Shown))
            .enabled(context.import)
            .into(),
        item("Import Picto Pack...", keymap::ImportPictoPack)
            .enabled(context.import)
            .into(),
        Separator,
        // The export area handles these on the window root.
        item("Export Originals…", keymap::ExportOriginals)
            .keys(Keys::Registry("file.export"))
            .enabled(has)
            .into(),
        item("Export As…", keymap::ExportAs)
            .keys(Keys::Registry("file.exportAs"))
            .enabled(has)
            .into(),
        item("Export as Picto Pack...", keymap::ExportPictoPack)
            .enabled(has)
            .into(),
    ];
    if !platform.is_mac() {
        file.extend([
            Separator,
            item("Settings…", crate::settings::OpenSettings)
                .keys(Keys::Registry("file.settings"))
                .into(),
        ]);
    }
    file.extend([
        Separator,
        item("Check for Updates…", CheckForUpdates).todo().into(),
        Separator,
        if platform.is_mac() {
            item("Close Window", CloseWindow).keys(fixed("Mod+W", window_key))
        } else {
            quit(platform)
        }
        .into(),
    ]);

    let mut edit = vec![
        if count > 1 {
            item("Batch Rename…", BatchRename)
        } else {
            item("Rename", Rename)
        }
        .keys(Keys::Shortcut("edit.rename"))
        .enabled(context.names)
        .into(),
        Separator,
        item("Undo", EditUndo)
            .keys(Keys::Shortcut("edit.undo"))
            .os(OsAction::Undo)
            .into(),
        item("Redo", EditRedo)
            .keys(Keys::Shortcut("edit.redo"))
            .os(OsAction::Redo)
            .into(),
        Separator,
        item("Cut", EditCut)
            .keys(fixed("Mod+X", Binding::Shown))
            .os(OsAction::Cut)
            .into(),
        item("Copy", EditCopy)
            .keys(fixed("Mod+C", Binding::Shown))
            .os(OsAction::Copy)
            .into(),
        item("Paste", EditPaste)
            .keys(fixed("Mod+V", Binding::Shown))
            .os(OsAction::Paste)
            .into(),
        item(
            by_count(count, "Copy File Path", |_| "Copy File Paths".into()),
            CopyPaths,
        )
        .keys(Keys::Shortcut("edit.copyFilePath"))
        .enabled(has)
        .into(),
        item(
            by_count(count, "Copy Name", |_| "Copy Names".into()),
            CopyNames,
        )
        .enabled(context.names)
        .into(),
        item(
            by_count(count, "Copy as Link", |_| "Copy as Links".into()),
            CopyLinks,
        )
        .enabled(has)
        .into(),
        Separator,
        item(
            by_count(count, "Regenerate Thumbnail", |n| {
                format!("Regenerate {n} Thumbnails")
            }),
            RegenerateThumbnails,
        )
        .keys(Keys::Shortcut("file.regenerateThumbnail"))
        .enabled(context.thumbnails)
        .into(),
        Separator,
        item("Select All", EditSelectAll)
            .keys(fixed("Mod+A", Binding::Shown))
            .os(OsAction::SelectAll)
            .into(),
        item("Deselect All", DeselectAll).enabled(has).into(),
        Separator,
    ];
    let in_trash = context.in_trash();
    let keyed = in_folder && !in_trash;
    edit.push(
        item("Remove from Folder", RemoveFromFolder { keyed })
            .keys(if keyed {
                Keys::Shortcut("file.removeFromFolder")
            } else {
                Keys::None
            })
            .enabled(in_folder && has)
            .into(),
    );
    if in_trash {
        edit.push(
            item(
                by_count(count, "Restore", |n| format!("Restore {n} Items")),
                Restore,
            )
            .keys(Keys::Shortcut("file.restore"))
            .enabled(has)
            .into(),
        );
        edit.push(
            item(
                by_count(count, "Delete Permanently", |n| {
                    format!("Delete {n} Permanently")
                }),
                DeletePermanently,
            )
            .keys(Keys::Shortcut("file.delete"))
            .enabled(has)
            .into(),
        );
    } else {
        edit.push(
            item(
                by_count(count, "Move to Trash", |n| format!("Move {n} to Trash")),
                MoveToTrash { keyed: !in_folder },
            )
            .keys(if in_folder {
                Keys::None
            } else {
                Keys::Shortcut("file.delete")
            })
            .enabled(has)
            .into(),
        );
    }

    let mut organize = Vec::new();
    if context.in_inbox() {
        organize.push(
            item(
                by_count(count, "Accept", |n| format!("Accept {n} Items")),
                Accept,
            )
            .enabled(has)
            .into(),
        );
        organize.push(
            item(
                by_count(count, "Reject", |n| format!("Reject {n} Items")),
                Reject,
            )
            .enabled(has)
            .into(),
        );
        organize.push(Separator);
    }
    organize.extend([
        item("Add Tags…", AddTags).enabled(has).into(),
        item("Add to Folder…", AddToFolder)
            .keys(Keys::Shortcut("file.addToFolder"))
            .enabled(has)
            .into(),
        item("Auto Tag…", AutoTag)
            .keys(Keys::Shortcut("organize.autoTag"))
            .enabled(has)
            .into(),
        Separator,
        Entry::Submenu {
            title: "Set Rating".into(),
            enabled: has,
            entries: (0..=5u8)
                .map(|stars| {
                    let title = if stars == 0 {
                        "No Rating".to_owned()
                    } else {
                        "★".repeat(stars.into())
                    };
                    item(title, SetRating { stars }).enabled(has).into()
                })
                .collect(),
        },
        Separator,
        // Production sends `menu:navigate` with `tags`, which its shell
        // does not map to a destination, so the row does nothing there; it
        // opens the Tags screen here.
        item("Tags", ShowTags).into(),
    ]);

    let view = vec![
        item("Open with Default App", OpenDefault)
            .keys(Keys::Shortcut("file.openDefaultApp"))
            .enabled(context.single_media())
            .into(),
        item(platform.reveal_title(), Reveal)
            .keys(Keys::Shortcut("file.revealInFolder"))
            .enabled(context.single_media())
            .into(),
        item("Open in New Window", OpenInNewWindow)
            .keys(Keys::Shortcut("file.openNewWindow"))
            .enabled(context.single())
            .into(),
        Separator,
        item("All Images", keymap::ShowAll)
            .keys(Keys::Registry("nav.allActive"))
            .into(),
        item("Inbox", keymap::ShowInbox)
            .keys(Keys::Registry("nav.inbox"))
            .into(),
        item("Untagged", keymap::ShowUntagged)
            .keys(Keys::Registry("nav.untagged"))
            .into(),
        item("Trash", keymap::ShowTrash)
            .keys(Keys::Registry("nav.trash"))
            .into(),
        Separator,
        item("Duplicates", ShowDuplicates).into(),
        Separator,
        item("Log Window", LogWindow)
            .keys(Keys::Shortcut("view.toggleLogs"))
            .todo()
            .into(),
        Separator,
        item("Toggle Full Screen", ToggleFullScreen)
            .keys(fixed(
                if platform.is_mac() {
                    "Ctrl+Mod+F"
                } else {
                    "F11"
                },
                window_key,
            ))
            .into(),
    ];

    let mut window_menu = vec![
        item("Minimize", Minimize)
            .keys(fixed("Mod+M", window_key))
            .into(),
        item("Zoom", Zoom).into(),
    ];
    if platform.is_mac() {
        window_menu.extend([
            Separator,
            item("Bring All to Front", BringAllToFront).into(),
        ]);
    } else {
        // Electron's `close` role, titled "Close" off the Mac.
        window_menu.push(
            item("Close", CloseWindow)
                .keys(fixed("Mod+W", window_key))
                .into(),
        );
    }

    let mut menus = vec![
        TopMenu {
            title: "Picto",
            entries: vec![
                item("About Picto", About).into(),
                Separator,
                item("Preferences…", crate::settings::OpenSettings)
                    .keys(Keys::Registry("file.settings"))
                    .into(),
                Separator,
                Entry::Services,
                Separator,
                item("Hide Picto", Hide)
                    .keys(fixed("Mod+H", window_key))
                    .into(),
                item("Hide Others", HideOthers)
                    .keys(fixed("Mod+Alt+H", window_key))
                    .into(),
                item("Show All", ShowAllApps).into(),
                Separator,
                quit(platform).into(),
            ],
        },
        TopMenu {
            title: "Library",
            entries: library,
        },
        TopMenu {
            title: "File",
            entries: file,
        },
        TopMenu {
            title: "Edit",
            entries: edit,
        },
        TopMenu {
            title: "Organize",
            entries: organize,
        },
        TopMenu {
            title: "View",
            entries: view,
        },
        // Titled "Window", the menu is AppKit's Windows menu, which lists
        // the app's windows below these rows.
        TopMenu {
            title: "Window",
            entries: window_menu,
        },
        TopMenu {
            title: "Help",
            entries: vec![item("About Picto", About).into()],
        },
    ];
    if !platform.is_mac() {
        menus.remove(0);
    }
    menus
}

/// Quit, as Electron's `quit` role is titled and keyed on `platform`.
fn quit(platform: Platform) -> Item {
    let quit = item(platform.quit_title(), crate::shell::Quit);
    match platform.quit_keys() {
        Some(keys) => quit.keys(Keys::Fixed(keys, Binding::Existing)),
        None => quit,
    }
}

/// Every row of `entries`, submenus included.
fn each_item(entries: &[Entry], visit: &mut impl FnMut(&Item)) {
    for entry in entries {
        match entry {
            Entry::Item(item) => visit(item),
            Entry::Submenu { entries, .. } => each_item(entries, visit),
            Entry::Separator | Entry::Services => {}
        }
    }
}

/// A key binding a row's accelerator needs.
pub struct Accelerator {
    pub keystroke: String,
    pub action: Box<dyn Action>,
    pub predicate: Option<String>,
}

/// A context no element sets: a binding under it is never taken by the
/// keyboard, but the menu shows its keys.
const SHOWN_ONLY: &str = "AppMenuKeyEquivalent";

/// The bindings the menu's rows need for their keys to show, under the
/// keyboard preset and the user's overrides.
pub fn accelerators(preset: KeyboardPreset, overrides: &Overrides) -> Vec<Accelerator> {
    let mut planned: Vec<Accelerator> = Vec::new();
    for context in MenuContext::variants() {
        for menu in model(&context, Platform::CURRENT) {
            each_item(&menu.entries, &mut |item| {
                let binding = match item.keys {
                    Keys::None | Keys::Registry(_) => return,
                    Keys::Shortcut(id) if keymap::targets(id).is_empty() => {
                        Binding::Live(Scope::App)
                    }
                    Keys::Shortcut(_) => Binding::Shown,
                    Keys::Fixed(_, binding) => binding,
                };
                let predicate = match binding {
                    Binding::Existing => return,
                    Binding::Shown => Some(SHOWN_ONLY.to_owned()),
                    Binding::Live(scope) => scope.predicate(),
                };
                let Some(keystroke) = item
                    .accelerator(preset, overrides)
                    .and_then(|keys| keymap::keystroke(&keys))
                else {
                    return;
                };
                let known = planned.iter().any(|accelerator| {
                    accelerator.keystroke == keystroke
                        && accelerator.action.partial_eq(item.action.as_ref())
                });
                if !known {
                    planned.push(Accelerator {
                        keystroke,
                        action: item.action.boxed_clone(),
                        predicate,
                    });
                }
            });
        }
    }
    planned
}

/// The menus as GPUI sets them.
fn gpui_menus(context: &MenuContext) -> Vec<Menu> {
    fn convert(entry: &Entry) -> MenuItem {
        match entry {
            Entry::Separator => MenuItem::Separator,
            Entry::Services => MenuItem::os_submenu("Services", SystemMenuType::Services),
            Entry::Submenu {
                title,
                enabled,
                entries,
            } => MenuItem::Submenu(Menu {
                name: title.clone().into(),
                items: entries.iter().map(convert).collect(),
                disabled: !enabled,
            }),
            Entry::Item(item) => MenuItem::Action {
                name: item.title.clone().into(),
                action: item.action.boxed_clone(),
                os_action: item.os_action,
                checked: item.checked,
                disabled: !(item.enabled && item.built),
            },
        }
    }
    model(context, Platform::CURRENT)
        .iter()
        .map(|menu| Menu {
            name: menu.title.into(),
            items: menu.entries.iter().map(convert).collect(),
            disabled: false,
        })
        .collect()
}

/// Whether the window carries the application menu button
/// (`ApplicationMenuButton`): off the Mac, where there is no menu bar; on
/// the Mac too with `PICTO_GPUI_APP_MENU=window`, as production's
/// development build shows it there to check the Windows and Linux menu.
pub fn in_window() -> bool {
    !cfg!(target_os = "macos") || crate::seams::app_menu_in_window()
}

/// The in-window menu's width (`width={220}`).
pub const IN_WINDOW_WIDTH: f32 = 220.;

/// The menus the in-window menu shows: this platform's, without the Mac's
/// app menu, whose rows File and Help already hold
/// (`picto:application-menu:get`).
fn in_window_menus(cx: &App) -> Vec<TopMenu> {
    let mut menus = model(&context(cx), Platform::CURRENT);
    if Platform::CURRENT.is_mac() {
        menus.remove(0);
    }
    menus
}

/// `toMenuEntries`: each menu a submenu of the button's menu; a submenu
/// inside one (Recent Libraries, Set Rating) is laid out in place after a
/// separator, as the shared menu nests only one level. A row is enabled as
/// the menu bar's would be: by production's rule, by this app performing
/// it, and by something in the window handling its action. Its value is
/// its place in the menus, which [`in_window_action`] reads back.
pub fn in_window_entries(window: &Window, cx: &mut App) -> Vec<crate::ui::menu::Entry> {
    use crate::ui::menu::{self as ui_menu, Entry as Row};
    let (preset, overrides) = crate::settings::keyboard(cx);
    fn rows(
        entries: &[Entry],
        path: &str,
        preset: KeyboardPreset,
        overrides: &Overrides,
        window: &Window,
        cx: &mut App,
    ) -> Vec<Row> {
        let mut out = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            let path = format!("{path}.{index}");
            match entry {
                Entry::Separator => out.push(Row::Separator),
                Entry::Services => {}
                Entry::Submenu { entries, .. } => {
                    out.push(Row::Separator);
                    out.extend(rows(entries, &path, preset, overrides, window, cx));
                }
                Entry::Item(item) => {
                    // Handled in the window, or app-wide (asked of the app
                    // while this window is being updated, only its own
                    // handlers count).
                    let action = item.action.as_ref();
                    let enabled = item.enabled
                        && item.built
                        && (window.is_action_available(action, cx)
                            || cx.is_action_available(action));
                    let mut row = ui_menu::item(item.action.name(), "")
                        .english(item.title.clone())
                        .value(path)
                        .disabled(!enabled);
                    if let Some(keys) = item.accelerator(preset, overrides) {
                        row = row.shortcut(registry::display(&keys));
                    }
                    if item.checked {
                        row = row.checked(true);
                    }
                    out.push(row.into());
                }
            }
        }
        out
    }
    let menus = in_window_menus(cx);
    let mut entries = Vec::new();
    for (index, menu) in menus.iter().enumerate() {
        let children = rows(
            &menu.entries,
            &index.to_string(),
            preset,
            &overrides,
            window,
            cx,
        );
        entries.push(ui_menu::submenu(menu.title, children).into());
    }
    entries
}

/// The action of the in-window menu's row whose value is `path`.
pub fn in_window_action(path: &str, cx: &App) -> Option<Box<dyn Action>> {
    let menus = in_window_menus(cx);
    let mut indices = path.split('.').map(|part| part.parse::<usize>().ok());
    let mut entries = &menus.get(indices.next()??)?.entries;
    loop {
        match entries.get(indices.next()??)? {
            Entry::Item(item) => return Some(item.action.boxed_clone()),
            Entry::Submenu { entries: inner, .. } => entries = inner,
            Entry::Separator | Entry::Services => return None,
        }
    }
}

/// The menu's state: the context it was last set for, and the main window,
/// which Close Window hides rather than closes.
#[derive(Default)]
struct Current {
    context: MenuContext,
    main: Option<WindowHandle<crate::shell::Shell>>,
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
    let context = context(cx);
    cx.set_menus(gpui_menus(&context));
}

/// The main window, for what other windows send to it.
pub fn main_window(cx: &App) -> Option<WindowHandle<crate::shell::Shell>> {
    cx.try_global::<Current>()?.main
}

/// The main window, for the rows production sends to it whichever window
/// has the keyboard, and for Close Window.
pub fn set_main_window(window: AnyWindowHandle, cx: &mut App) {
    current(cx).main = window.downcast::<crate::shell::Shell>();
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
        let is_main = main.is_some_and(|main| AnyWindowHandle::from(main) == handle);
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
        let Some(main) = current(cx).main else {
            return;
        };
        cx.defer(move |cx| {
            let _ = main.update(cx, |shell, window, cx| {
                shell.show_screen(crate::backend::Screen::Subscriptions, cx);
                window.activate_window();
            });
        });
    });
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
        "app_menu::MoveToTrash" => Some(Box::new(MoveToTrash {
            keyed: value == "true",
        })),
        "app_menu::RemoveFromFolder" => Some(Box::new(RemoveFromFolder {
            keyed: value == "true",
        })),
        _ => cx.build_action(name, None).ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Entry, Keys, MenuContext, Platform, ScopeKind, Status, TopMenu, accelerators, model,
    };
    use crate::settings::registry::{KeyboardPreset, Override, Overrides};

    /// Keys in one spelling: modifiers in a fixed order, then the key.
    /// Production writes `CmdOrCtrl` (or a role's `Command`) for `Mod`.
    fn normalized(keys: &str) -> String {
        let parts: Vec<&str> = keys.split('+').collect();
        let (key, modifiers) = parts.split_last().unwrap();
        let mut modifiers: Vec<&str> = modifiers
            .iter()
            .map(|part| match *part {
                "CmdOrCtrl" | "CommandOrControl" | "Command" | "Cmd" => "Mod",
                "Control" => "Ctrl",
                "Option" => "Alt",
                other => other,
            })
            .collect();
        let order = ["Ctrl", "Mod", "Alt", "Shift"];
        modifiers.sort_by_key(|part| order.iter().position(|known| known == part));
        modifiers.push(key);
        modifiers.join("+")
    }

    /// The menus as rows of text: `Title|Keys|on`, `---`, `Services`, and a
    /// submenu's rows indented under `Title >|on`.
    fn rows(menus: &[TopMenu]) -> Vec<(String, Vec<String>)> {
        fn entries(list: &[Entry], indent: &str, out: &mut Vec<String>) {
            for entry in list {
                match entry {
                    Entry::Separator => out.push(format!("{indent}---")),
                    Entry::Services => out.push(format!("{indent}Services")),
                    Entry::Submenu {
                        title,
                        enabled,
                        entries: children,
                    } => {
                        out.push(format!(
                            "{indent}{title} >|{}",
                            if *enabled { "on" } else { "off" }
                        ));
                        entries(children, &format!("{indent}  "), out);
                    }
                    Entry::Item(item) => {
                        let keys = item
                            .accelerator(KeyboardPreset::Us, &Overrides::new())
                            .map(|keys| normalized(&keys))
                            .unwrap_or_default();
                        out.push(format!(
                            "{indent}{}|{keys}|{}",
                            item.title,
                            if item.enabled { "on" } else { "off" }
                        ));
                    }
                }
            }
        }
        menus
            .iter()
            .map(|menu| {
                let mut out = Vec::new();
                entries(&menu.entries, "", &mut out);
                (menu.title.to_owned(), out)
            })
            .collect()
    }

    /// Production's template (`menu.mjs`, `isMac`, not a development
    /// build), transcribed: Electron's role rows with Electron's own titles
    /// and keys, the others with the registry's default keys that its
    /// `accelerator()` falls back to. `$` rows change with the context and
    /// are given by the caller.
    fn production(variable: &[(&str, Vec<&str>)]) -> Vec<(String, Vec<String>)> {
        let fixed =
            |rows: &[&str]| -> Vec<String> { rows.iter().map(|row| (*row).to_owned()).collect() };
        let picto = fixed(&[
            "About Picto||on",
            "---",
            "Preferences…|CmdOrCtrl+,|on",
            "---",
            "Services",
            "---",
            "Hide Picto|Command+H|on",
            "Hide Others|Command+Alt+H|on",
            "Show All||on",
            "---",
            "Quit Picto|Command+Q|on",
        ]);
        let window = fixed(&[
            "Minimize|CommandOrControl+M|on",
            "Zoom||on",
            "---",
            "Bring All to Front||on",
        ]);
        let help = fixed(&["About Picto||on"]);
        let mut menus = vec![("Picto".to_owned(), picto)];
        for (title, rows) in variable {
            menus.push(((*title).to_owned(), fixed(rows)));
        }
        menus.push(("Window".to_owned(), window));
        menus.push(("Help".to_owned(), help));
        menus
            .into_iter()
            .map(|(title, rows)| {
                let rows = rows
                    .into_iter()
                    .map(|row: String| {
                        let mut parts: Vec<String> = row.split('|').map(str::to_owned).collect();
                        if parts.len() == 3 && !parts[1].is_empty() {
                            parts[1] = normalized(&parts[1]);
                        }
                        parts.join("|")
                    })
                    .collect();
                (title, rows)
            })
            .collect()
    }

    fn library(recent: bool) -> Vec<&'static str> {
        let mut rows = vec![
            "Library Manager…|CmdOrCtrl+Shift+L|on",
            "---",
            "New Library…|CmdOrCtrl+N|on",
            "Open Library…||on",
            "---",
        ];
        if recent {
            rows.extend([
                "Recent Libraries >|on",
                "  Main||on",
                "  ---",
                "  Clear History||on",
                "---",
            ]);
        }
        rows.push("Subscriptions…|Shift+CmdOrCtrl+S|on");
        rows
    }

    fn view(single_media: &str, single: &str) -> Vec<String> {
        vec![
            format!("Open with Default App|Shift+Enter|{single_media}"),
            format!("Reveal in Finder|CmdOrCtrl+Enter|{single_media}"),
            format!("Open in New Window|CmdOrCtrl+O|{single}"),
            "---".into(),
            "All Images|CmdOrCtrl+1|on".into(),
            "Inbox|CmdOrCtrl+2|on".into(),
            "Untagged|CmdOrCtrl+3|on".into(),
            "Trash|CmdOrCtrl+4|on".into(),
            "---".into(),
            "Duplicates||on".into(),
            "---".into(),
            "Log Window|CmdOrCtrl+L|on".into(),
            "---".into(),
            "Toggle Full Screen|Control+Command+F|on".into(),
        ]
    }

    fn leak(rows: Vec<String>) -> Vec<&'static str> {
        rows.into_iter()
            .map(|row| &*Box::leak(row.into_boxed_str()))
            .collect()
    }

    fn rating(on: &str) -> Vec<&'static str> {
        let mut rows = vec![format!("Set Rating >|{on}")];
        for title in ["No Rating", "★", "★★", "★★★", "★★★★", "★★★★★"]
        {
            rows.push(format!("  {title}||{on}"));
        }
        leak(rows)
    }

    #[test]
    fn nothing_selected_in_all_images_matches_production() {
        let context = MenuContext {
            library: Some("Main".into()),
            ..MenuContext::default()
        };
        let mut organize = vec![
            "Add Tags…||off",
            "Add to Folder…|CmdOrCtrl+Shift+J|off",
            "Auto Tag…|CmdOrCtrl+Shift+A|off",
            "---",
        ];
        organize.extend(rating("off"));
        organize.extend(["---", "Tags||on"]);
        let expected = production(&[
            ("Library", library(true)),
            (
                "File",
                vec![
                    "New Folder|CmdOrCtrl+Shift+N|on",
                    "New Smart Folder|CmdOrCtrl+Shift+Alt+N|on",
                    "---",
                    "Import Files…|CmdOrCtrl+I|on",
                    "Import Folder…|Shift+CmdOrCtrl+I|on",
                    "Import Picto Pack...||on",
                    "---",
                    "Export Originals…|CmdOrCtrl+E|off",
                    "Export As…|Shift+CmdOrCtrl+E|off",
                    "Export as Picto Pack...||off",
                    "---",
                    "Check for Updates…||on",
                    "---",
                    "Close Window|CommandOrControl+W|on",
                ],
            ),
            (
                "Edit",
                vec![
                    "Rename|Ctrl+R|off",
                    "---",
                    "Undo|CmdOrCtrl+Z|on",
                    "Redo|Shift+Cmd+Z|on",
                    "---",
                    "Cut|CommandOrControl+X|on",
                    "Copy|CommandOrControl+C|on",
                    "Paste|CommandOrControl+V|on",
                    "Copy File Path|CmdOrCtrl+Alt+C|off",
                    "Copy Name||off",
                    "Copy as Link||off",
                    "---",
                    "Regenerate Thumbnail|CmdOrCtrl+Shift+T|off",
                    "---",
                    "Select All|CommandOrControl+A|on",
                    "Deselect All||off",
                    "---",
                    "Remove from Folder||off",
                    "Move to Trash|CmdOrCtrl+Backspace|off",
                ],
            ),
            ("Organize", organize),
            ("View", leak(view("off", "off"))),
        ]);
        assert_eq!(rows(&model(&context, Platform::Mac)), expected);
    }

    #[test]
    fn several_selected_in_a_folder_match_production() {
        let context = MenuContext {
            selection: 3,
            scope: ScopeKind::Folder,
            status: Some(Status::Active),
            names: true,
            thumbnails: true,
            ..MenuContext::default()
        };
        let menus = rows(&model(&context, Platform::Mac));
        let file = &menus.iter().find(|(title, _)| title == "File").unwrap().1;
        assert_eq!(file[0], "New Subfolder|Alt+N|on");
        assert_eq!(file[1], "New Smart Folder|Mod+Alt+Shift+N|off");
        let edit = &menus.iter().find(|(title, _)| title == "Edit").unwrap().1;
        let expected: Vec<String> = [
            "Batch Rename…|Ctrl+R|on",
            "---",
            "Undo|Mod+Z|on",
            "Redo|Mod+Shift+Z|on",
            "---",
            "Cut|Mod+X|on",
            "Copy|Mod+C|on",
            "Paste|Mod+V|on",
            "Copy File Paths|Mod+Alt+C|on",
            "Copy Names||on",
            "Copy as Links||on",
            "---",
            "Regenerate 3 Thumbnails|Mod+Shift+T|on",
            "---",
            "Select All|Mod+A|on",
            "Deselect All||on",
            "---",
            // The Delete key belongs to Remove from Folder in a folder.
            "Remove from Folder|Mod+Backspace|on",
            "Move 3 to Trash||on",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(edit, &expected);
        // No Library row is listed without a library name.
        let library = &menus
            .iter()
            .find(|(title, _)| title == "Library")
            .unwrap()
            .1;
        assert!(!library.iter().any(|row| row.starts_with("Recent")));
    }

    #[test]
    fn the_trash_restores_and_deletes_permanently() {
        let context = MenuContext {
            selection: 2,
            scope: ScopeKind::Trash,
            status: Some(Status::Trash),
            ..MenuContext::default()
        };
        let menus = rows(&model(&context, Platform::Mac));
        let edit = &menus.iter().find(|(title, _)| title == "Edit").unwrap().1;
        assert_eq!(
            &edit[edit.len() - 3..],
            [
                "Remove from Folder||off",
                "Restore 2 Items|Mod+Shift+Backspace|on",
                "Delete 2 Permanently|Mod+Backspace|on",
            ]
        );
    }

    #[test]
    fn the_inbox_accepts_and_rejects_first() {
        let context = MenuContext {
            selection: 1,
            single: Some(false),
            scope: ScopeKind::Inbox,
            status: Some(Status::Inbox),
            names: true,
            ..MenuContext::default()
        };
        let menus = rows(&model(&context, Platform::Mac));
        let organize = &menus
            .iter()
            .find(|(title, _)| title == "Organize")
            .unwrap()
            .1;
        assert_eq!(&organize[..3], ["Accept||on", "Reject||on", "---"]);
        let view = &menus.iter().find(|(title, _)| title == "View").unwrap().1;
        assert_eq!(view, &view_rows("on", "on"));
        // A collection opens in a new window but not in the default app.
        let collection = MenuContext {
            single: Some(true),
            ..context
        };
        let menus = rows(&model(&collection, Platform::Mac));
        let view = &menus.iter().find(|(title, _)| title == "View").unwrap().1;
        assert_eq!(view, &view_rows("off", "on"));
    }

    fn view_rows(single_media: &str, single: &str) -> Vec<String> {
        view(single_media, single)
            .into_iter()
            .map(|row| {
                let mut parts: Vec<String> = row.split('|').map(str::to_owned).collect();
                if parts.len() == 3 && !parts[1].is_empty() {
                    parts[1] = normalized(&parts[1]);
                }
                parts.join("|")
            })
            .collect()
    }

    #[test]
    fn smart_folders_take_no_new_folders_and_running_imports_block_imports() {
        let context = MenuContext {
            scope: ScopeKind::SmartFolder,
            import: false,
            ..MenuContext::default()
        };
        let menus = rows(&model(&context, Platform::Mac));
        let file = &menus.iter().find(|(title, _)| title == "File").unwrap().1;
        assert_eq!(file[0], "New Folder|Mod+Shift+N|off");
        assert_eq!(file[1], "New Smart Folder|Mod+Alt+Shift+N|off");
        assert_eq!(file[3], "Import Files…|Mod+I|off");
        assert_eq!(file[4], "Import Folder…|Mod+Shift+I|off");
        assert_eq!(file[5], "Import Picto Pack...||off");
    }

    /// The rows this app cannot perform yet: they have no handler, so
    /// AppKit shows them disabled.
    #[test]
    fn quit_takes_the_keys_electrons_quit_role_has() {
        assert_eq!(Platform::Mac.quit_keys(), Some("Mod+Q"));
        assert_eq!(Platform::Linux.quit_keys(), Some("Mod+Q"));
        assert_eq!(Platform::Windows.quit_keys(), None);
    }

    /// The rows of one menu as `title|keys` lines.
    fn titled(menus: &[TopMenu], title: &str) -> Vec<String> {
        let menu = menus
            .iter()
            .find(|menu| menu.title == title)
            .unwrap_or_else(|| panic!("no {title} menu"));
        menu.entries
            .iter()
            .map(|entry| match entry {
                Entry::Item(item) => {
                    let keys = match item.keys {
                        Keys::Fixed(keys, _) => keys,
                        _ => "",
                    };
                    format!("{}|{keys}", item.title)
                }
                Entry::Separator => "---".into(),
                Entry::Submenu { title, .. } => format!("{title} >"),
                Entry::Services => "Services".into(),
            })
            .collect()
    }

    /// Off the Mac, production's template has no app menu: File ends with
    /// Settings…, Check for Updates… and the quit role, the Window menu
    /// closes the window, and full screen is F11.
    #[test]
    fn windows_and_linux_take_productions_other_template() {
        let context = MenuContext::default();
        for platform in [Platform::Windows, Platform::Linux] {
            let menus = model(&context, platform);
            let titles: Vec<_> = menus.iter().map(|menu| menu.title).collect();
            assert_eq!(
                titles,
                [
                    "Library", "File", "Edit", "Organize", "View", "Window", "Help"
                ]
            );
            let file = titled(&menus, "File");
            let quit = match platform {
                Platform::Windows => "Exit|",
                _ => "Quit|Mod+Q",
            };
            assert_eq!(
                file[file.len() - 6..],
                [
                    "---",
                    "Settings…|",
                    "---",
                    "Check for Updates…|",
                    "---",
                    quit
                ]
            );
            assert_eq!(
                titled(&menus, "Window"),
                ["Minimize|Mod+M", "Zoom|", "Close|Mod+W"]
            );
            let view = titled(&menus, "View");
            assert!(view.contains(&"Toggle Full Screen|F11".to_owned()));
            assert!(view.contains(&format!("{}|", platform.reveal_title())));
        }
        assert_eq!(Platform::Windows.reveal_title(), "Show in Explorer");
        let mac = model(&context, Platform::Mac);
        assert_eq!(mac[0].title, "Picto");
        assert_eq!(
            titled(&mac, "Window"),
            ["Minimize|Mod+M", "Zoom|", "---", "Bring All to Front|"]
        );
    }

    #[test]
    fn unbuilt_rows_are_the_known_ones() {
        let context = MenuContext {
            selection: 2,
            library: Some("Main".into()),
            ..MenuContext::default()
        };
        let mut unbuilt = Vec::new();
        for menu in model(&context, Platform::CURRENT) {
            super::each_item(&menu.entries, &mut |item| {
                if !item.built {
                    unbuilt.push(item.title.clone());
                }
            });
        }
        assert_eq!(
            unbuilt,
            [
                "Library Manager…",
                "New Library…",
                "Open Library…",
                "Main",
                "Clear History",
                "Check for Updates…",
                "Log Window",
            ]
        );
    }

    fn bound(
        preset: KeyboardPreset,
        overrides: &Overrides,
        action: &dyn gpui::Action,
    ) -> Vec<String> {
        accelerators(preset, overrides)
            .into_iter()
            .filter(|accelerator| accelerator.action.partial_eq(action))
            .map(|accelerator| accelerator.keystroke)
            .collect()
    }

    fn spelled(written: &str) -> String {
        if cfg!(target_os = "macos") {
            written.to_owned()
        } else {
            written.replace("cmd-", "ctrl-")
        }
    }

    #[test]
    fn rows_bind_their_keys_and_follow_rebinding() {
        let none = Overrides::new();
        // A row whose keys the registry binds to another action only shows
        // them; one only the menu has binds them for real.
        let rename = accelerators(KeyboardPreset::Us, &none)
            .into_iter()
            .find(|accelerator| accelerator.action.partial_eq(&super::Rename))
            .unwrap();
        assert_eq!(rename.keystroke, "ctrl-r");
        assert_eq!(rename.predicate.as_deref(), Some(super::SHOWN_ONLY));
        let logs = accelerators(KeyboardPreset::Us, &none)
            .into_iter()
            .find(|accelerator| accelerator.action.partial_eq(&super::LogWindow))
            .unwrap();
        assert_eq!(logs.keystroke, spelled("cmd-l"));
        assert_eq!(logs.predicate, crate::keymap::Scope::App.predicate());
        let close = bound(KeyboardPreset::Us, &none, &super::CloseWindow);
        assert_eq!(close, [spelled("cmd-w")]);
        // The Delete key: Move to Trash outside a folder, Remove from Folder
        // inside one, and neither row's other variant.
        assert_eq!(
            bound(
                KeyboardPreset::Us,
                &none,
                &super::MoveToTrash { keyed: true }
            ),
            [spelled("cmd-backspace")]
        );
        assert!(
            bound(
                KeyboardPreset::Us,
                &none,
                &super::MoveToTrash { keyed: false }
            )
            .is_empty()
        );
        assert_eq!(
            bound(
                KeyboardPreset::Us,
                &none,
                &super::RemoveFromFolder { keyed: true }
            ),
            [spelled("cmd-backspace")]
        );
        assert!(
            bound(
                KeyboardPreset::Us,
                &none,
                &super::RemoveFromFolder { keyed: false }
            )
            .is_empty()
        );
        // Rebound in Settings, the row shows the new keys; cleared, none.
        let mut overrides = Overrides::new();
        overrides.insert(
            "edit.rename",
            Override {
                keys: Some("Alt+R".into()),
                keys2: None,
            },
        );
        overrides.insert(
            "view.toggleLogs",
            Override {
                keys: Some(String::new()),
                keys2: None,
            },
        );
        assert_eq!(
            bound(KeyboardPreset::Us, &overrides, &super::Rename),
            ["alt-r"]
        );
        assert!(bound(KeyboardPreset::Us, &overrides, &super::LogWindow).is_empty());
    }

    /// Every row with keys has a binding for its own action: the menu's, or
    /// the registry's for a row whose action is the registry's.
    #[test]
    fn every_keyed_row_has_a_binding() {
        let none = Overrides::new();
        let planned = crate::keymap::plan(KeyboardPreset::Us, &none);
        let menu = accelerators(KeyboardPreset::Us, &none);
        for context in MenuContext::variants() {
            for top in model(&context, Platform::CURRENT) {
                super::each_item(&top.entries, &mut |item| {
                    let found = match item.keys {
                        Keys::None => return,
                        Keys::Fixed(_, super::Binding::Existing) => return,
                        Keys::Registry(id) => planned.iter().any(|planned| {
                            planned.id == id && (planned.action)().partial_eq(item.action.as_ref())
                        }),
                        _ => menu
                            .iter()
                            .any(|binding| binding.action.partial_eq(item.action.as_ref())),
                    };
                    assert!(found, "{} has no binding", item.title);
                });
            }
        }
    }

    /// Every binding the menu adds parses, and a key only the menu has does
    /// not take a key the registry binds in the same place.
    #[test]
    fn menu_keys_parse_and_take_no_registry_key() {
        let none = Overrides::new();
        let planned = crate::keymap::plan(KeyboardPreset::Us, &none);
        for accelerator in accelerators(KeyboardPreset::Us, &none) {
            assert!(
                gpui::Keystroke::parse_with_os(&accelerator.keystroke, gpui::OperatingSystem::Mac)
                    .is_ok(),
                "{} does not parse",
                accelerator.keystroke
            );
            if let Some(predicate) = &accelerator.predicate {
                gpui::KeyBindingContextPredicate::parse(predicate).unwrap();
            }
            if accelerator.predicate.as_deref() != Some(super::SHOWN_ONLY) {
                assert!(
                    !planned
                        .iter()
                        .any(|planned| planned.keystroke == accelerator.keystroke),
                    "{} is also a registry key",
                    accelerator.keystroke
                );
            }
        }
    }
}
