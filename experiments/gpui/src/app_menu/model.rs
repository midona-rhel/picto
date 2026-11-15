//! The menus' rows, titles, keys and rules for a context, as production's
//! template builds them, and the key bindings their accelerators need.
use super::*;
use crate::settings::registry;

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
pub(super) fn by_count(count: u64, one: &str, many: impl FnOnce(u64) -> String) -> String {
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
            .into(),
        Separator,
        item("New Library…", NewLibrary)
            .keys(fixed("Mod+N", main_window))
            .into(),
        item("Open Library…", OpenLibrary).into(),
        Separator,
    ];
    // The libraries opened before, pinned ones first (marked with a pin, as
    // production writes them), the open one checked.
    // Production's history always holds the open library; a context that
    // lists none still shows it.
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
        // `crate::imports::bind` binds its keys.
        item("Import Folder…", keymap::ImportFolder)
            .keys(fixed("Mod+Shift+I", Binding::Existing))
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
        // Production sends `menu:navigate` with `tags`, which its shell
        // does not map to a destination, so the row does nothing there; it
        // opens the Tags screen here.
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
pub(super) fn quit(platform: Platform) -> Item {
    let quit = item(platform.quit_title(), crate::shell::Quit);
    match platform.quit_keys() {
        Some(keys) => quit.keys(Keys::Fixed(keys, Binding::Existing)),
        None => quit,
    }
}

/// Every row of `entries`, submenus included.
pub(super) fn each_item(entries: &[Entry], visit: &mut impl FnMut(&Item)) {
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
pub(super) const SHOWN_ONLY: &str = "AppMenuKeyEquivalent";

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
