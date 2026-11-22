//! This file builds the model of the application menu for a context: every
//! menu, and for every row its title, its keys, its action and the rule for
//! when the row is enabled. The menus, the titles and the keys are the ones
//! that Picto 0.6.19 and earlier had. The file also lists the key bindings
//! that the rows need so that their keys are shown and work.
use super::*;
use crate::settings::registry;

/// How the keys of a row are bound in the keymap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Binding {
    /// Another file already binds these keys to the row's action, so no
    /// binding is added.
    Existing,
    /// The menu shows the keys, but they are not bound in the window. This
    /// is for keys that already perform the command through another action,
    /// such as the registry's action or a text field's action, and for keys
    /// that the window's own key listeners must receive first. When nothing
    /// in the window takes the keys, AppKit sends them to the menu row.
    Shown,
    /// Only the menu has these keys. They are also bound in the window, in
    /// this scope, so that they work wherever the window's shortcuts work.
    Live(Scope),
}

/// The keys of a row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Keys {
    None,
    /// The keys of this registry shortcut. The keymap binds the shortcut to
    /// the row's own action.
    Registry(&'static str),
    /// The keys of this registry shortcut, for a row whose action is one of
    /// the menu's own actions. When the keymap binds the shortcut to another
    /// action, the menu only shows the keys. When the keymap binds the
    /// shortcut to nothing, the keys are bound to the row's action.
    Shortcut(&'static str),
    /// Keys that are written in this file and do not come from a shortcut
    /// of the registry, so the user cannot change them in Settings. They are
    /// written in the form in which the registry writes keys.
    Fixed(&'static str, Binding),
}

pub struct Item {
    pub title: String,
    pub keys: Keys,
    /// Whether the row's rule enables the row in the context that the model
    /// was built for.
    pub enabled: bool,
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
    /// The Services submenu, which macOS fills.
    Services,
}

pub struct TopMenu {
    pub title: &'static str,
    pub entries: Vec<Entry>,
}

pub(super) fn item(title: impl Into<String>, action: impl Action) -> Item {
    Item {
        title: title.into(),
        keys: Keys::None,
        enabled: true,
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

    fn os(mut self, os_action: OsAction) -> Self {
        self.os_action = Some(os_action);
        self
    }

    /// Returns the keys that this row shows, in the form in which the
    /// registry writes keys, with the keyboard preset and the user's
    /// overrides applied. Returns `None` when the row has no keys.
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

/// Returns the title of a row whose wording depends on how many items are
/// selected: `one` when at most one item is selected, and `many(count)`
/// when more are.
pub(super) fn by_count(count: u64, one: &str, many: impl FnOnce(u64) -> String) -> String {
    if count > 1 {
        many(count)
    } else {
        one.to_owned()
    }
}

/// Returns the menus for `context` on `platform`, in the order in which the
/// menu shows them. Windows and Linux have no "Picto" menu. There, the File
/// menu has a Settings… row and ends with Quit where macOS has Close
/// Window, and the Window menu has a Close row where macOS has Bring All to
/// Front.
pub fn model(context: &MenuContext, platform: Platform) -> Vec<TopMenu> {
    use Entry::Separator;
    let count = context.selection;
    let has = context.has_selection();
    let fixed = |keys, binding| Keys::Fixed(keys, binding);
    // The keys of the window commands are not bound in the window. They
    // reach their rows through AppKit after the window has not taken them.
    // A field that listens for keys, such as the shortcut capture field in
    // Settings, therefore takes ⌘W before Close Window does.
    let window_key = Binding::Shown;
    let main_window = Binding::Live(Scope::App);

    let mut library = vec![
        item("Library Manager…", LibraryManager)
            .keys(fixed("Mod+Shift+L", main_window))
            .into(),
        Separator,
        item("New Library…", NewLibrary)
            .keys(fixed("Mod+N", main_window))
            .into(),
        item("Open Library…", OpenLibrary).into(),
        Separator,
    ];
    // Recent Libraries lists the libraries that were opened before. The
    // pinned ones come first, and each has a pin character before its name.
    // The open library has a check mark. When the context lists no recent
    // library, the submenu still shows the open library, so that the open
    // library is always in the list.
    let open = context.library.as_ref().map(|name| Recent {
        path: String::new(),
        name: name.clone(),
        pinned: false,
        current: true,
    });
    let recent = if context.recent.is_empty() {
        open.into_iter().collect::<Vec<_>>()
    } else {
        context.recent.clone()
    };
    if !recent.is_empty() {
        let row = |recent: &Recent| {
            let title = if recent.pinned {
                format!("\u{1F4CC} {}", recent.name)
            } else {
                recent.name.clone()
            };
            let mut row = item(
                title,
                OpenRecentLibrary {
                    path: recent.path.clone(),
                },
            );
            row.checked = recent.current;
            Entry::from(row)
        };
        let pinned = recent.iter().filter(|recent| recent.pinned);
        let rest = recent.iter().filter(|recent| !recent.pinned);
        let mut entries = pinned.clone().map(row).collect::<Vec<_>>();
        if !entries.is_empty() && rest.clone().next().is_some() {
            entries.push(Separator);
        }
        entries.extend(rest.map(row));
        entries.push(Separator);
        entries.push(item("Clear History", ClearRecentLibraries).into());
        library.push(Entry::Submenu {
            title: "Recent Libraries".into(),
            enabled: true,
            entries,
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
        item("Import Files…", keymap::ImportFiles)
            .keys(Keys::Registry("file.import"))
            .enabled(context.import)
            .into(),
        // `crate::imports::bind` binds the keys of Import Folder….
        item("Import Folder…", keymap::ImportFolder)
            .keys(fixed("Mod+Shift+I", Binding::Existing))
            .enabled(context.import)
            .into(),
        item("Import Picto Pack...", keymap::ImportPictoPack)
            .enabled(context.import)
            .into(),
        Separator,
        // The shell's export handlers (`Shell::export_actions`) handle the
        // three export actions on the window's root element.
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
        item("Check for Updates…", CheckForUpdates).into(),
        Separator,
        if platform.is_mac() {
            item("Close Window", CloseWindow).keys(fixed("Mod+W", window_key))
        } else {
            quit(platform)
        }
        .into(),
    ]);

    let mut edit = vec![
        item(
            if count > 1 {
                "Batch Rename…"
            } else {
                "Rename"
            },
            keymap::Rename,
        )
        .keys(Keys::Registry("edit.rename"))
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
            keymap::CopyFilePath,
        )
        .keys(Keys::Registry("edit.copyFilePath"))
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
            keymap::RegenerateThumbnails,
        )
        .keys(Keys::Registry("file.regenerateThumbnail"))
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
                keymap::RestoreItems,
            )
            .keys(Keys::Registry("file.restore"))
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
        item("Add to Folder…", keymap::AddToFolder)
            .keys(Keys::Registry("file.addToFolder"))
            .enabled(has)
            .into(),
        item("Auto Tag…", keymap::AutoTag)
            .keys(Keys::Registry("organize.autoTag"))
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
        // This row opens the Tags screen.
        item("Tags", ShowTags).into(),
    ]);

    let view = vec![
        item("Open with Default App", keymap::OpenDefaultApp)
            .keys(Keys::Registry("file.openDefaultApp"))
            .enabled(context.single_media())
            .into(),
        item(platform.reveal_title(), keymap::RevealInFinder)
            .keys(Keys::Registry("file.revealInFolder"))
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
        // This row opens the Quick Search panel.
        item("Quick Search…", keymap::QuickSearch)
            .keys(Keys::Registry("nav.commandPalette"))
            .into(),
        Separator,
        item("Duplicates", ShowDuplicates).into(),
        Separator,
        item("Log Window", LogWindow)
            .keys(Keys::Shortcut("view.toggleLogs"))
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
        // On Windows and Linux the row that closes the window is in the
        // Window menu, and its title is "Close".
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
        // AppKit treats a menu with the title "Window" as its Windows menu,
        // and lists the app's windows below these rows.
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

/// Returns the Quit row, with the title and the keys that it has on
/// `platform`. `main.rs` binds the keys.
pub(super) fn quit(platform: Platform) -> Item {
    let quit = item(platform.quit_title(), crate::shell::Quit);
    match platform.quit_keys() {
        Some(keys) => quit.keys(Keys::Fixed(keys, Binding::Existing)),
        None => quit,
    }
}

/// Calls `visit` for every row of `entries`, including the rows of
/// submenus.
pub(super) fn each_item(entries: &[Entry], visit: &mut impl FnMut(&Item)) {
    for entry in entries {
        match entry {
            Entry::Item(item) => visit(item),
            Entry::Submenu { entries, .. } => each_item(entries, visit),
            Entry::Separator | Entry::Services => {}
        }
    }
}

/// One key binding that a row of the menu needs.
pub struct Accelerator {
    pub keystroke: String,
    pub action: Box<dyn Action>,
    pub predicate: Option<String>,
}

/// A key context that no element sets. A binding in this context never runs
/// from the keyboard, but the menu still shows the binding's keys.
pub(super) const SHOWN_ONLY: &str = "AppMenuKeyEquivalent";

/// Returns the bindings that the menu's rows need so that the menu shows
/// their keys, for this keyboard preset and these overrides.
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
