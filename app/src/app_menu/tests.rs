use super::{Entry, Keys, MenuContext, Platform, ScopeKind, Status, TopMenu, accelerators, model};
use crate::settings::registry::{KeyboardPreset, Override, Overrides};

/// Rewrites keys so that the same keys are always written the same way: the
/// modifiers in a fixed order, followed by the key. The expected rows in
/// this file write `CmdOrCtrl`, `CommandOrControl` or `Command` where the
/// registry writes `Mod`.
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

/// Writes the menus as lines of text, so that a test can compare them. A
/// row is `Title|Keys|on` or `Title|Keys|off`. A separator is `---`. The
/// Services submenu is `Services`. A submenu is `Title >|on`, and its rows
/// follow it, indented.
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

/// Returns the menus that the model must build on macOS, written out by
/// hand as lines in the form that [`rows`] produces. They are the menus of
/// Picto 0.6.19 and earlier, with the Quick Search… row added to the View
/// menu. The keys are the default keys, which a row has when the user has
/// not changed them. The "Picto", Window and Help menus are the same in
/// every context and are written here. The caller gives the rows of the
/// other menus, which change with the context, in `variable`.
fn earlier_version(variable: &[(&str, Vec<&str>)]) -> Vec<(String, Vec<String>)> {
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
        // Picto 0.6.19 and earlier did not have this row.
        "Quick Search…|CmdOrCtrl+T|on".into(),
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
    for title in ["No Rating", "★", "★★", "★★★", "★★★★", "★★★★★"] {
        rows.push(format!("  {title}||{on}"));
    }
    leak(rows)
}

#[test]
fn nothing_selected_in_all_images_matches_the_earlier_version() {
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
    let expected = earlier_version(&[
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
fn several_selected_in_a_folder_match_the_earlier_version() {
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
    // These are the Edit rows that differ when three items are selected in
    // a folder.
    let edit = &menus.iter().find(|(title, _)| title == "Edit").unwrap().1;
    for row in [
        "Batch Rename…|Ctrl+R|on",
        "Copy File Paths|Mod+Alt+C|on",
        "Copy Names||on",
        "Regenerate 3 Thumbnails|Mod+Shift+T|on",
        // In a folder, the Delete key is the key of Remove from Folder.
        "Remove from Folder|Mod+Backspace|on",
        "Move 3 to Trash||on",
    ] {
        assert!(edit.iter().any(|shown| shown == row), "{row} in {edit:?}");
    }
    // When the context has no library name and no recent libraries, the
    // Library menu has no Recent Libraries submenu.
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
    // A collection can be opened in a new window, but not in the default
    // application.
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

/// Quit has ⌘Q on macOS and Ctrl+Q on Linux, and no keys on Windows.
#[test]
fn quit_takes_the_keys_the_earlier_versions_quit_row_had() {
    assert_eq!(Platform::Mac.quit_keys(), Some("Mod+Q"));
    assert_eq!(Platform::Linux.quit_keys(), Some("Mod+Q"));
    assert_eq!(Platform::Windows.quit_keys(), None);
}

/// Returns the rows of the menu with this title, each as a `title|keys`
/// line.
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

/// On Windows and Linux, the model has no "Picto" menu. The File menu ends
/// with Settings…, Check for Updates… and the Quit row, the Window menu has
/// a Close row, and the key of Toggle Full Screen is F11.
#[test]
fn windows_and_linux_take_the_earlier_versions_other_template() {
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

fn bound(preset: KeyboardPreset, overrides: &Overrides, action: &dyn gpui::Action) -> Vec<String> {
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
    // When the registry binds a row's keys to another action, the menu only
    // shows the keys.
    let new_folder = accelerators(KeyboardPreset::Us, &none)
        .into_iter()
        .find(|accelerator| accelerator.action.partial_eq(&super::NewFolder))
        .unwrap();
    assert_eq!(new_folder.keystroke, spelled("cmd-shift-n"));
    assert_eq!(new_folder.predicate.as_deref(), Some(super::SHOWN_ONLY));
    let logs = accelerators(KeyboardPreset::Us, &none)
        .into_iter()
        .find(|accelerator| accelerator.action.partial_eq(&super::LogWindow))
        .unwrap();
    // The registry binds ⌘L to the log window's action in the main window,
    // so the menu's row only shows the keys.
    assert_eq!(logs.keystroke, spelled("cmd-l"));
    assert_eq!(logs.predicate.as_deref(), Some(super::SHOWN_ONLY));
    let close = bound(KeyboardPreset::Us, &none, &super::CloseWindow);
    assert_eq!(close, [spelled("cmd-w")]);
    // The Delete key is bound to Move to Trash outside a folder and to
    // Remove from Folder inside a folder. It is not bound to the variant of
    // either row that shows no keys.
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
    // After the user changes a shortcut's keys in Settings, the row shows
    // the new keys. After the user clears them, the row shows no keys.
    let mut overrides = Overrides::new();
    overrides.insert(
        "file.newFolder",
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
        bound(KeyboardPreset::Us, &overrides, &super::NewFolder),
        ["alt-r"]
    );
    assert!(bound(KeyboardPreset::Us, &overrides, &super::LogWindow).is_empty());
}

/// Every row that shows keys has a key binding for its own action. The
/// binding is one that the menu adds, or, for a row whose action is the
/// registry's action, one that the registry adds.
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

/// Every binding that the menu adds can be parsed as a keystroke. A binding
/// that the menu adds for the window does not use keys that the registry
/// binds in the same scope.
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
