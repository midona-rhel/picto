//! This file makes the app's key bindings from the shortcut registry in
//! `settings::registry`, which is a port of production's `shortcuts.ts`.
//! For every id in the registry, [`targets`] gives the actions that the
//! shortcut performs. Each action is bound in the key context that
//! corresponds to the place where production's shortcut scope handles the
//! key. An id that has no action yet is listed in [`NOT_YET`] with the
//! reason. The bindings use the keyboard preset that is applied and the
//! user's overrides, and both key slots of each shortcut are bound. The
//! bindings are made again when the Settings window applies a change.
//!
//! The scope rules of production's `shortcutRuntime.ts` are written here as
//! key-context predicates. A binding of any scope except `Anywhere` and
//! `TextField` does not run while keyboard focus is in a text field, a
//! context menu, a popover or a dialog, because each of those handles its
//! keys itself. For example, Escape closes the menu, then the popover, then
//! the dialog, and only when none of those is open does it close the viewer
//! or clear the selection. The grid's bindings need the `Grid` key context,
//! which the shell sets only while the grid itself is shown. The viewer's
//! bindings need the viewer to have keyboard focus.
use crate::screens::duplicates;
use crate::settings::registry::{self, KeyboardPreset, Overrides};
use crate::shell;
use crate::viewer;
use gpui::*;
use std::rc::Rc;

actions!(
    shortcut,
    [
        GoBack,
        GridZoomIn,
        GridZoomOut,
        GoForward,
        FocusSearch,
        QuickSearch,
        GoToFolder,
        ShowAll,
        ShowInbox,
        ShowUntagged,
        ShowTrash,
        QuickLook,
        OpenDefaultApp,
        RevealInFinder,
        CopyFilePath,
        Rename,
        DeleteItems,
        RemoveFromFolder,
        RestoreItems,
        AcceptInbox,
        RejectInbox,
        NewFolder,
        NewSubfolder,
        NewSmartFolder,
        FolderAutoTags,
        AddTags,
        AutoTag,
        AddToFolders,
        AddToFolder,
        ImportFiles,
        ImportFolder,
        PasteImport,
        ExportOriginals,
        ExportAs,
        ExportPictoPack,
        ImportPictoPack,
        CopyFiles,
        RegenerateThumbnails,
        ToggleGrayscale
    ]
);

/// The action that the shortcuts `rate.0` to `rate.5` perform. `stars` is
/// the rating to set, from 0 to 5 stars, and 0 means no rating.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = shortcut, no_json)]
pub struct Rate {
    pub stars: u8,
}

/// The part of the app in which a shortcut works. [`Scope::predicate`]
/// gives each scope as a key-context predicate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// The shortcut works in every window, including the Settings window.
    /// This scope is for the application menu's keys.
    Anywhere,
    /// The shortcut works in the main window, whatever the window shows.
    /// This corresponds to the shortcut scopes of production's `AppShell`
    /// and sidebar, and to the application menu's commands that act on the
    /// selection.
    App,
    /// The shortcut works while the grid is shown. This corresponds to the
    /// shortcut scopes of production's `GridScreen` and `useGridArrowNav`.
    Grid,
    /// The shortcut works while the media viewer has keyboard focus. This
    /// corresponds to the shortcut scope of production's `MediaView`.
    Viewer,
    /// The shortcut works in a collection's editor, which is inside the
    /// media viewer. This corresponds to the editor scope of production's
    /// `GroupSurface`. When this scope and the viewer's scope bind the same
    /// key, the key performs this scope's action.
    GroupEditor,
    /// The shortcut works on the duplicate review screen. This corresponds
    /// to the shortcut scope of production's `DuplicatesScreen`.
    Duplicates,
    /// The shortcut works on the Subscriptions screen. This corresponds to
    /// the shortcut scope of production's `SubscriptionsGrid`.
    Subscriptions,
    /// The shortcut works in a detail window. This corresponds to the
    /// shortcut scope of production's `DetailWindow`. A detail window has
    /// the viewer's keys, except that Escape is the only key that closes it.
    DetailWindow,
    /// The shortcut works while a viewer that shows the media player has
    /// keyboard focus. This corresponds to the shortcut scope of
    /// production's `VideoPlayer`. When this scope and the viewer's scope
    /// bind the same key, the key performs this scope's action.
    Media,
    /// The shortcut works while a text field has keyboard focus, in any
    /// window. The Edit menu's Undo and Redo use this scope so that in a
    /// text field they undo and redo the field's own edits, as production's
    /// `historyRuntime.ts` does.
    TextField,
}

/// The part of a scope's predicate that stops the window's shortcuts from
/// running while keyboard focus is in a text field, a menu, a dialog or a
/// popover. Every scope except `Anywhere` and `TextField` adds it.
const OWNERS: &str = "!TextInput && !Menu && !Modal && !Popover";

impl Scope {
    pub fn predicate(self) -> Option<String> {
        let within = match self {
            Scope::Anywhere => return None,
            Scope::TextField => return Some("TextInput".into()),
            Scope::App => "Browser",
            // i18n: a key context
            Scope::Grid => "Grid",
            // While a Flash movie has keyboard focus, the keys go to the
            // movie and the viewer's shortcuts do not run.
            Scope::Viewer => "Viewer && !Flash",
            Scope::GroupEditor => viewer::group_editor::CONTEXT,
            // i18n: a key context
            Scope::Duplicates => "Duplicates",
            // i18n: a key context
            Scope::Subscriptions => "Subscriptions",
            Scope::DetailWindow => "DetailWindow && !Flash",
            Scope::Media => "Media && !Flash",
        };
        Some(format!("{within} && {OWNERS}"))
    }
}

/// One action that a shortcut performs, and the scope in which the shortcut
/// performs it.
pub struct Target {
    pub scope: Scope,
    pub action: fn() -> Box<dyn Action>,
    /// When this is true, the same keys with Shift held are bound to the
    /// action as well, and they extend the selection. This is production's
    /// `allowExtraShift` option.
    pub extends: bool,
}

macro_rules! to {
    ($scope:ident, $action:expr) => {
        Target {
            scope: Scope::$scope,
            action: || -> Box<dyn Action> { Box::new($action) },
            extends: false,
        }
    };
    ($scope:ident, $action:expr, extends) => {
        Target {
            scope: Scope::$scope,
            action: || -> Box<dyn Action> { Box::new($action) },
            extends: true,
        }
    };
}

/// Returns the actions that the shortcut with this registry id performs.
/// The list is empty for the ids in [`NOT_YET`].
pub fn targets(id: &str) -> Vec<Target> {
    match id {
        // The shortcuts of the Navigation group.
        "nav.allActive" => vec![to!(App, ShowAll)],
        "nav.inbox" => vec![to!(App, ShowInbox)],
        "nav.untagged" => vec![to!(App, ShowUntagged)],
        "nav.trash" => vec![to!(App, ShowTrash)],
        "nav.search" => vec![to!(App, FocusSearch)],
        // Both shortcuts open the Quick Search panel (`crate::shell::quick_search`).
        // The first opens it with every kind of row and the second with
        // the places only.
        "nav.commandPalette" => vec![to!(App, QuickSearch)],
        "nav.goToFolder" => vec![to!(App, GoToFolder)],
        "nav.back" => vec![to!(App, GoBack)],
        "nav.forward" => vec![to!(App, GoForward)],
        "grid.moveLeft" => vec![to!(Grid, shell::Previous, extends)],
        "grid.moveRight" => vec![to!(Grid, shell::Next, extends)],
        "grid.moveUp" => vec![to!(Grid, shell::Up, extends)],
        "grid.moveDown" => vec![to!(Grid, shell::Down, extends)],
        "grid.first" => vec![to!(Grid, shell::First, extends)],
        "grid.last" => vec![to!(Grid, shell::Last, extends)],
        "grid.pageUp" => vec![to!(Grid, shell::PageUp, extends)],
        "grid.pageDown" => vec![to!(Grid, shell::PageDown, extends)],
        // The shortcuts of the File group.
        "file.settings" => vec![to!(Anywhere, crate::settings::OpenSettings)],
        // Delete acts on the selected items in the grid, or on the selected
        // subscriptions' cards on the Subscriptions screen.
        "file.delete" => vec![to!(Grid, DeleteItems), to!(Subscriptions, DeleteItems)],
        "file.removeFromFolder" => vec![to!(Grid, RemoveFromFolder)],
        "file.restore" => vec![to!(Grid, RestoreItems)],
        "file.newFolder" => vec![to!(App, NewFolder)],
        "file.newSubfolder" => vec![to!(App, NewSubfolder)],
        "file.newSmartFolder" => vec![to!(App, NewSmartFolder)],
        "folder.autoTags" => vec![to!(App, FolderAutoTags)],
        "file.addToFolder" => vec![to!(Grid, AddToFolder)],
        "file.openDefaultApp" => vec![to!(Grid, OpenDefaultApp)],
        "file.revealInFolder" => vec![to!(Grid, RevealInFinder)],
        "file.openNewWindow" => vec![to!(Grid, shell::OpenInNewWindow)],
        // Production binds this key as an accelerator of the application
        // menu, so it works anywhere in the main window.
        "file.import" => vec![to!(App, ImportFiles)],
        // Production binds the keys of Export Originals and Export As as
        // accelerators of the application menu too, so they also work
        // anywhere in the main window.
        "file.export" => vec![to!(App, ExportOriginals)],
        // Regenerate Thumbnail is a row of the Edit menu. Its key works
        // only while the menu's rule enables that row.
        "file.regenerateThumbnail" => vec![to!(App, RegenerateThumbnails)],
        "file.exportAs" => vec![to!(App, ExportAs)],
        // The shortcuts of the Edit group.
        "organize.addTag" => vec![to!(Grid, AddTags)],
        // Production binds this key as an accelerator of the application
        // menu, so it works anywhere in the main window. It acts on the item
        // that the viewer shows, or on the grid's selection when the viewer
        // is not open.
        "organize.autoTag" => vec![to!(App, AutoTag)],
        "organize.addFolder" => vec![to!(Grid, AddToFolders)],
        "edit.undo" => vec![
            to!(App, shell::Undo),
            to!(TextField, crate::ui::text_input::Undo),
        ],
        "edit.redo" => vec![
            to!(App, shell::Redo),
            to!(TextField, crate::ui::text_input::Redo),
        ],
        "edit.selectAll" => vec![
            to!(Grid, shell::SelectAll),
            to!(GroupEditor, shell::SelectAll),
        ],
        // Removes the selected members from the collection that the
        // collection's editor shows.
        "group.removeMembers" => vec![to!(GroupEditor, viewer::group_editor::RemoveMembers)],
        // The key is Escape. While the viewer has keyboard focus, the
        // viewer's own binding (`view.closeDetail`) closes the viewer. In
        // the rest of the main window this binding closes the viewer when
        // it is open, and clears the selection when it is not.
        "edit.deselectAll" => vec![to!(App, shell::ClosePreview)],
        "edit.rename" => vec![to!(App, Rename)],
        "edit.copyFilePath" => vec![to!(App, CopyFilePath), to!(DetailWindow, CopyFilePath)],
        // Copy copies the files themselves: the selected items in the grid,
        // the item that the viewer or Quick Look shows, the file of a
        // detail window, or the members of a collection in its editor.
        "edit.copy" => vec![
            to!(Grid, CopyFiles),
            to!(Viewer, CopyFiles),
            to!(DetailWindow, CopyFiles),
            to!(GroupEditor, CopyFiles),
        ],
        "edit.pasteImport" => vec![to!(Grid, PasteImport)],
        // The shortcuts of the Inbox group. The viewer's bindings accept or
        // reject the item that the viewer shows.
        "inbox.accept" => vec![to!(Grid, AcceptInbox), to!(Viewer, AcceptInbox)],
        "inbox.reject" => vec![to!(Grid, RejectInbox), to!(Viewer, RejectInbox)],
        // The shortcuts of the Rating group. In the grid they rate the
        // selected items, and in the viewer they rate the item that the
        // viewer shows.
        "rate.0" => vec![to!(Grid, Rate { stars: 0 }), to!(Viewer, Rate { stars: 0 })],
        "rate.1" => vec![to!(Grid, Rate { stars: 1 }), to!(Viewer, Rate { stars: 1 })],
        "rate.2" => vec![to!(Grid, Rate { stars: 2 }), to!(Viewer, Rate { stars: 2 })],
        "rate.3" => vec![to!(Grid, Rate { stars: 3 }), to!(Viewer, Rate { stars: 3 })],
        "rate.4" => vec![to!(Grid, Rate { stars: 4 }), to!(Viewer, Rate { stars: 4 })],
        "rate.5" => vec![to!(Grid, Rate { stars: 5 }), to!(Viewer, Rate { stars: 5 })],
        // The shortcuts of the View group. Enter and Space open the viewer
        // from the grid, and close the viewer when it has keyboard focus,
        // because production's `MediaView` treats both keys as its close
        // keys.
        "view.detailView" => vec![
            to!(Grid, shell::Preview),
            to!(Viewer, viewer::Close),
            // Enter is bound to `NoAction` in a collection's editor, so
            // that it does nothing there.
            to!(GroupEditor, NoAction),
        ],
        "view.quicklook" => vec![
            to!(Grid, QuickLook),
            to!(Viewer, viewer::Close),
            // In a collection's editor, Space opens Quick Look for the
            // selected member when exactly one member is selected.
            to!(GroupEditor, viewer::group_editor::QuickLookMember),
        ],
        "view.closeDetail" => vec![
            to!(Viewer, viewer::Close),
            to!(DetailWindow, viewer::detail::CloseWindow),
        ],
        "view.prevImage" => vec![
            to!(Viewer, viewer::PreviousItem),
            to!(DetailWindow, viewer::PreviousItem),
        ],
        "view.nextImage" => vec![
            to!(Viewer, viewer::NextItem),
            to!(DetailWindow, viewer::NextItem),
        ],
        "view.fitWindow" => vec![
            to!(Viewer, viewer::FitToWindow),
            to!(DetailWindow, viewer::FitToWindow),
        ],
        "view.actualSize" => vec![
            to!(Viewer, viewer::ActualSize),
            to!(Duplicates, duplicates::ActualPixels),
            to!(DetailWindow, viewer::ActualSize),
        ],
        "view.zoomIn" => vec![
            to!(Grid, GridZoomIn),
            to!(Viewer, viewer::ZoomIn),
            to!(DetailWindow, viewer::ZoomIn),
        ],
        "view.zoomOut" => vec![
            to!(Grid, GridZoomOut),
            to!(Viewer, viewer::ZoomOut),
            to!(DetailWindow, viewer::ZoomOut),
        ],
        // These two shortcuts go to the previous and the next page of a
        // document, in the viewer or in a detail window, whichever of them
        // shows the document.
        "document.previousPage" => vec![
            to!(Viewer, viewer::document::PreviousPage),
            to!(DetailWindow, viewer::document::PreviousPage),
        ],
        "document.nextPage" => vec![
            to!(Viewer, viewer::document::NextPage),
            to!(DetailWindow, viewer::document::NextPage),
        ],
        "view.alwaysOnTop" => vec![to!(DetailWindow, viewer::detail::ToggleAlwaysOnTop)],
        "view.toggleSidebar" => vec![to!(App, shell::ToggleSidebar)],
        "view.toggleInspector" => vec![to!(App, shell::ToggleInspector)],
        "view.toggleBothPanels" => vec![to!(App, shell::TogglePanels)],
        "view.grayscale" => vec![to!(Grid, ToggleGrayscale)],
        // The shortcuts of the Duplicates group.
        "dup.keepBoth" => vec![to!(Duplicates, duplicates::KeepBoth)],
        "dup.notDuplicate" => vec![to!(Duplicates, duplicates::NotDuplicate)],
        "dup.keepLeft" => vec![to!(Duplicates, duplicates::KeepLeft)],
        "dup.keepRight" => vec![to!(Duplicates, duplicates::KeepRight)],
        "dup.smartMerge" => vec![to!(Duplicates, duplicates::SmartMerge)],
        // This shortcut is not in production's registry. It shows the pixel
        // difference for as long as its key is held down.
        "dup.difference" => vec![to!(Duplicates, duplicates::ShowDifference)],
        "dup.fitToWindow" => vec![to!(Duplicates, duplicates::FitPair)],
        "dup.prevPair" => vec![to!(Duplicates, duplicates::PreviousPair)],
        "dup.nextPair" => vec![to!(Duplicates, duplicates::NextPair)],
        // Toggle Logs, and after it the shortcuts of the Video group.
        "view.toggleLogs" => vec![to!(App, crate::app_menu::LogWindow)],
        "video.togglePlay" => vec![to!(Media, viewer::video::TogglePlay)],
        "video.seekBackward" => vec![to!(Media, viewer::video::SeekBackward)],
        "video.seekForward" => vec![to!(Media, viewer::video::SeekForward)],
        "video.volumeUp" => vec![to!(Media, viewer::video::VolumeUp)],
        "video.volumeDown" => vec![to!(Media, viewer::video::VolumeDown)],
        "video.toggleMute" => vec![to!(Media, viewer::video::ToggleMute)],
        "video.toggleLoop" => vec![to!(Media, viewer::video::ToggleLoop)],
        "video.rateIncrease" => vec![to!(Media, viewer::video::RateIncrease)],
        "video.rateDecrease" => vec![to!(Media, viewer::video::RateDecrease)],
        "video.rateReset" => vec![to!(Media, viewer::video::RateReset)],
        "video.fullscreen" => vec![to!(Media, viewer::video::ToggleFullscreen)],
        _ => Vec::new(),
    }
}

/// The registry ids that this app cannot perform yet, each with the reason.
/// They stay in the registry and no key is bound to them. Settings lists
/// those of them that production's Settings lists.
pub const NOT_YET: &[(&str, &str)] = &[
    ("file.addToLastFolder", "deferred in production"),
    ("edit.copyTags", "deferred in production"),
    ("edit.pasteTags", "deferred in production"),
    ("view.slideshow", "deferred in production"),
    ("view.navigator", "deferred in production"),
    ("view.layoutGrid", "deferred in production"),
    ("view.layoutWaterfall", "deferred in production"),
    ("view.layoutJustified", "deferred in production"),
    ("view.toggleTileName", "deferred in production"),
    ("view.toggleTileMetadata", "deferred in production"),
];

/// Binds `keys` to `action` in the key context `context`. `keys` is written
/// in the registry's form (for example `Mod+Q`) and is converted to this
/// platform's keys. This function is for the few bindings that are not in
/// the registry but whose keys still differ between platforms.
pub fn bind_written(keys: &str, action: Box<dyn Action>, context: Option<&str>, cx: &mut App) {
    let Some(written) = keystroke(keys) else {
        return;
    };
    let predicate = context.map(|source| {
        Rc::new(KeyBindingContextPredicate::parse(source).expect("key binding context"))
    });
    match KeyBinding::load(&written, action, predicate, false, None, cx) {
        Ok(binding) => cx.bind_keys([binding]),
        Err(error) => eprintln!("keys {keys}: {error}"),
    }
}

/// Converts a key string in the registry's form (`Mod+Shift+Backspace`) to
/// a GPUI keystroke (`cmd-shift-backspace`). Returns `None` for an empty
/// slot and for a key that GPUI has no name for. `Mod` means Command on the
/// Mac and Control on other platforms, which is how production's
/// `matchesShortcut` reads it.
pub fn keystroke(keys: &str) -> Option<String> {
    if keys.is_empty() {
        return None;
    }
    let (base, trailing_plus) = if keys == "+" {
        ("", true)
    } else {
        match keys.strip_suffix("++") {
            Some(base) => (base, true),
            None => (keys, false),
        }
    };
    let mut parts: Vec<&str> = base.split('+').filter(|part| !part.is_empty()).collect();
    if trailing_plus {
        parts.push("+");
    }
    let (key, modifiers) = parts.split_last()?;
    let mut written = Vec::new();
    for modifier in modifiers {
        written.push(match *modifier {
            "Mod" if cfg!(target_os = "macos") => "cmd",
            "Mod" | "Ctrl" => "ctrl",
            "Alt" => "alt",
            "Shift" => "shift",
            _ => return None,
        });
    }
    let key = match *key {
        "ArrowLeft" => "left".to_owned(),
        "ArrowRight" => "right".to_owned(),
        "ArrowUp" => "up".to_owned(),
        "ArrowDown" => "down".to_owned(),
        "Enter" => "enter".to_owned(),
        "Escape" => "escape".to_owned(),
        "Backspace" => "backspace".to_owned(),
        "Delete" => "delete".to_owned(),
        "Space" => "space".to_owned(),
        "Tab" => "tab".to_owned(),
        "Home" => "home".to_owned(),
        "End" => "end".to_owned(),
        "PageUp" => "pageup".to_owned(),
        "PageDown" => "pagedown".to_owned(),
        function
            if function.len() > 1
                && function.starts_with('F')
                && function[1..].parse::<u8>().is_ok() =>
        {
            function.to_lowercase()
        }
        // The key is written in lower case, because GPUI reads an upper-case
        // letter as that letter with Shift held.
        single if single.chars().count() == 1 => single.to_lowercase(),
        _ => return None,
    };
    written.push(&key);
    Some(written.join("-"))
}

/// One key binding that [`install`] makes: the keystroke, the id of the
/// shortcut it comes from, the scope in which it works and the action it
/// performs.
pub struct Planned {
    pub keystroke: String,
    pub id: &'static str,
    pub scope: Scope,
    pub action: fn() -> Box<dyn Action>,
}

/// Returns the bindings for the registry with this preset and these
/// overrides applied. Each of a shortcut's two key slots is bound to every
/// target of the shortcut. For a target that extends the selection with
/// Shift, the same keys with Shift are bound as well, unless the keys
/// already contain Shift.
pub fn plan(preset: KeyboardPreset, overrides: &Overrides) -> Vec<Planned> {
    let mut planned = Vec::new();
    for shortcut in registry::all(preset, overrides) {
        let targets = targets(shortcut.id);
        for keys in [&shortcut.keys, &shortcut.keys2] {
            let Some(keystroke) = keystroke(keys) else {
                continue;
            };
            for target in &targets {
                let mut strokes = vec![keystroke.clone()];
                if target.extends && !keys.split('+').any(|part| part == "Shift") {
                    strokes.push(format!("shift-{keystroke}"));
                }
                for keystroke in strokes {
                    planned.push(Planned {
                        keystroke,
                        id: shortcut.id,
                        scope: target.scope,
                        action: target.action,
                    });
                }
            }
        }
    }
    planned
}

/// The mark that [`install`] puts on every binding it makes, so that the
/// next call to [`install`] removes only the bindings that have the mark.
const FROM_REGISTRY: KeyBindingMetaIndex = KeyBindingMetaIndex(0x5052_4547);

/// The keyboard preset and the overrides that [`install`] last applied.
/// [`badges`] and [`hint`] read them to get the keys that tooltips and menu
/// rows show.
#[derive(Default)]
struct Installed {
    preset: KeyboardPreset,
    overrides: Overrides,
}

impl Global for Installed {}

/// Replaces the bindings made from the registry with bindings for this
/// preset and these overrides. Bindings made in other files, such as those
/// of text fields, menus and dialogs, are kept.
pub fn install(preset: KeyboardPreset, overrides: &Overrides, cx: &mut App) {
    if cx
        .try_global::<Installed>()
        .is_some_and(|installed| installed.preset == preset && installed.overrides == *overrides)
    {
        return;
    }
    let keymap = cx.key_bindings();
    {
        let mut keymap = keymap.borrow_mut();
        let kept: Vec<KeyBinding> = keymap
            .bindings()
            .filter(|binding| binding.meta() != Some(FROM_REGISTRY))
            .cloned()
            .collect();
        keymap.clear();
        keymap.add_bindings(kept);
    }
    for def in registry::SHORTCUT_DEFS {
        if targets(def.id).is_empty() && !NOT_YET.iter().any(|(id, _)| *id == def.id) {
            eprintln!(
                "shortcuts: {} is neither bound nor listed as not yet",
                def.id
            );
        }
    }
    let mut predicates: Vec<(Scope, Option<Rc<KeyBindingContextPredicate>>)> = Vec::new();
    let mut bindings = Vec::new();
    for planned in plan(preset, overrides) {
        let predicate = match predicates.iter().find(|(scope, _)| *scope == planned.scope) {
            Some((_, predicate)) => predicate.clone(),
            None => {
                let predicate = planned.scope.predicate().map(|source| {
                    Rc::new(KeyBindingContextPredicate::parse(&source).expect("scope predicate"))
                });
                predicates.push((planned.scope, predicate.clone()));
                predicate
            }
        };
        match KeyBinding::load(
            &planned.keystroke,
            (planned.action)(),
            predicate,
            false,
            None,
            cx,
        ) {
            Ok(binding) => bindings.push(binding.with_meta(FROM_REGISTRY)),
            Err(error) => eprintln!("shortcuts: {}: {error}", planned.id),
        }
    }
    cx.set_global(Installed {
        preset,
        overrides: overrides.clone(),
    });
    // The application menu's keys are added after the registry's bindings,
    // so that a menu row whose action the registry also binds shows the
    // keys from the registry.
    for accelerator in crate::app_menu::accelerators(preset, overrides) {
        let predicate = accelerator.predicate.map(|source| {
            Rc::new(KeyBindingContextPredicate::parse(&source).expect("menu key predicate"))
        });
        match KeyBinding::load(
            &accelerator.keystroke,
            accelerator.action,
            predicate,
            false,
            None,
            cx,
        ) {
            Ok(binding) => bindings.push(binding.with_meta(FROM_REGISTRY)),
            Err(error) => eprintln!("menu: {}: {error}", accelerator.keystroke),
        }
    }
    // The keys that scroll a document are added after the registry's
    // bindings. Space is bound by both, and while a document's pages are
    // shown Space scrolls the document.
    bindings.extend(
        crate::viewer::document::scroll_bindings()
            .into_iter()
            .map(|binding| binding.with_meta(FROM_REGISTRY)),
    );
    // Adding bindings makes GPUI draw every window again, so the tooltips
    // and the application menu's key equivalents then show the new keys.
    cx.bind_keys(bindings);
    crate::set_app_menus(cx);
}

/// Returns the keys in the first slot of the shortcut `id`, in the
/// registry's form, with the preset and the overrides that [`install`] last
/// applied. Before the first call to [`install`] it uses the US preset and
/// no overrides. Returns an empty string when there is no such shortcut.
fn primary(id: &str, cx: &App) -> String {
    let shortcut = match cx.try_global::<Installed>() {
        Some(installed) => registry::shortcut(id, installed.preset, &installed.overrides),
        None => registry::shortcut(id, KeyboardPreset::Us, &Overrides::new()),
    };
    shortcut.map(|shortcut| shortcut.keys).unwrap_or_default()
}

/// Returns the keys that a tooltip shows for the shortcut `id`, one string
/// for each key badge. This is what production's `KbdTooltip` shows when it
/// is given a `shortcutId`.
pub fn badges(id: &str, cx: &App) -> Vec<SharedString> {
    registry::parts(&primary(id, cx))
        .into_iter()
        .map(SharedString::from)
        .collect()
}

/// Returns the keys that a menu row shows for the shortcut `id`, as
/// production's `shortcutHint` does. Returns `None` when the shortcut is
/// deferred or its first slot has no keys.
pub fn hint(id: &str, cx: &App) -> Option<SharedString> {
    let deferred = registry::SHORTCUT_DEFS
        .iter()
        .any(|def| def.id == id && def.deferred);
    let keys = primary(id, cx);
    (!deferred && !keys.is_empty()).then(|| registry::display(&keys).into())
}

#[cfg(test)]
mod tests {
    use super::{NOT_YET, Scope, keystroke, plan, targets};
    use crate::settings::registry::{KeyboardPreset, Override, Overrides, SHORTCUT_DEFS, conflict};
    use gpui::{Keystroke, Modifiers, OperatingSystem};

    /// Takes a keystroke that is written with the Mac's `cmd-` and returns
    /// it as this platform writes it: unchanged on macOS, and with `cmd-`
    /// replaced by `ctrl-` on other platforms.
    fn spelled(written: &str) -> String {
        if cfg!(target_os = "macos") {
            written.to_owned()
        } else {
            written.replace("cmd-", "ctrl-")
        }
    }

    fn parsed(keys: &str) -> Keystroke {
        let written = keystroke(keys).unwrap_or_else(|| panic!("{keys} has no keystroke"));
        Keystroke::parse_with_os(&written, OperatingSystem::Mac)
            .unwrap_or_else(|error| panic!("{keys} as {written}: {error}"))
    }

    /// Returns the modifiers that a test case expects. `mod_key` stands for
    /// `Mod`, which is Command on the Mac and Control on other platforms.
    fn modifiers(mod_key: bool, control: bool, alt: bool, shift: bool) -> Modifiers {
        let mac = cfg!(target_os = "macos");
        Modifiers {
            platform: mod_key && mac,
            control: control || (mod_key && !mac),
            alt,
            shift,
            function: false,
        }
    }

    #[test]
    fn every_id_is_bound_or_listed_as_not_yet() {
        for def in SHORTCUT_DEFS {
            let bound = !targets(def.id).is_empty();
            let waiting = NOT_YET.iter().any(|(id, _)| *id == def.id);
            assert!(
                bound != waiting,
                "{} must be bound to an action or listed in NOT_YET, not {}",
                def.id,
                if bound { "both" } else { "neither" }
            );
        }
        for (id, _) in NOT_YET {
            assert!(
                SHORTCUT_DEFS.iter().any(|def| def.id == *id),
                "{id} is not in the registry"
            );
        }
    }

    #[test]
    fn deferred_shortcuts_are_never_bound() {
        for def in SHORTCUT_DEFS.iter().filter(|def| def.deferred) {
            assert!(targets(def.id).is_empty(), "{} is deferred", def.id);
        }
    }

    #[test]
    fn every_default_binding_becomes_a_keystroke() {
        for preset in [KeyboardPreset::Us, KeyboardPreset::Eu] {
            for shortcut in crate::settings::registry::all(preset, &Overrides::new()) {
                for keys in [&shortcut.keys, &shortcut.keys2] {
                    if !keys.is_empty() {
                        parsed(keys);
                    }
                }
            }
        }
    }

    #[test]
    fn registry_keys_convert_as_gpui_writes_them() {
        let cases: &[(&str, &str, &str, Modifiers)] = &[
            (
                "Mod+Shift+Z",
                "cmd-shift-z",
                "z",
                modifiers(true, false, false, true),
            ),
            (
                "Alt+ArrowLeft",
                "alt-left",
                "left",
                modifiers(false, false, true, false),
            ),
            ("Mod+[", "cmd-[", "[", modifiers(true, false, false, false)),
            ("Mod+]", "cmd-]", "]", modifiers(true, false, false, false)),
            ("Mod+,", "cmd-,", ",", modifiers(true, false, false, false)),
            (
                "Mod+Backspace",
                "cmd-backspace",
                "backspace",
                modifiers(true, false, false, false),
            ),
            (
                "Mod+Shift+Alt+N",
                "cmd-shift-alt-n",
                "n",
                modifiers(true, false, true, true),
            ),
            (
                "Ctrl+R",
                "ctrl-r",
                "r",
                modifiers(false, true, false, false),
            ),
            (
                "Mod+Alt+1",
                "cmd-alt-1",
                "1",
                modifiers(true, false, true, false),
            ),
            (
                "Shift+Enter",
                "shift-enter",
                "enter",
                modifiers(false, false, false, true),
            ),
            ("+", "+", "+", Modifiers::none()),
            ("=", "=", "=", Modifiers::none()),
            ("-", "-", "-", Modifiers::none()),
            ("Mod++", "cmd-+", "+", modifiers(true, false, false, false)),
            ("Mod+-", "cmd--", "-", modifiers(true, false, false, false)),
            ("`", "`", "`", Modifiers::none()),
            ("\\", "\\", "\\", Modifiers::none()),
            (
                "Shift+.",
                "shift-.",
                ".",
                modifiers(false, false, false, true),
            ),
            (
                "Shift+F",
                "shift-f",
                "f",
                modifiers(false, false, false, true),
            ),
            ("T", "t", "t", Modifiers::none()),
            ("0", "0", "0", Modifiers::none()),
            ("Space", "space", "space", Modifiers::none()),
            ("Escape", "escape", "escape", Modifiers::none()),
            ("Tab", "tab", "tab", Modifiers::none()),
            ("Delete", "delete", "delete", Modifiers::none()),
            ("Backspace", "backspace", "backspace", Modifiers::none()),
            ("PageDown", "pagedown", "pagedown", Modifiers::none()),
            ("Home", "home", "home", Modifiers::none()),
            ("ArrowUp", "up", "up", Modifiers::none()),
            ("F5", "f5", "f5", Modifiers::none()),
        ];
        for (keys, written, key, expected) in cases {
            assert_eq!(keystroke(keys), Some(spelled(written)), "{keys}");
            let stroke = parsed(keys);
            assert_eq!(stroke.key, *key, "{keys}");
            assert_eq!(stroke.modifiers, *expected, "{keys}");
        }
        assert_eq!(keystroke(""), None);
        assert_eq!(keystroke("Mod+Numpad"), None);
    }

    fn bound(preset: KeyboardPreset, overrides: &Overrides, id: &str) -> Vec<(String, Scope)> {
        plan(preset, overrides)
            .into_iter()
            .filter(|planned| planned.id == id)
            .map(|planned| (planned.keystroke, planned.scope))
            .collect()
    }

    #[test]
    fn both_slots_bind_and_moves_extend_with_shift() {
        let none = Overrides::new();
        assert_eq!(
            bound(KeyboardPreset::Us, &none, "nav.back"),
            [
                ("alt-left".to_owned(), Scope::App),
                (spelled("cmd-["), Scope::App)
            ]
        );
        assert_eq!(
            bound(KeyboardPreset::Us, &none, "grid.moveLeft"),
            [
                ("left".to_owned(), Scope::Grid),
                ("shift-left".to_owned(), Scope::Grid),
                ("a".to_owned(), Scope::Grid),
                ("shift-a".to_owned(), Scope::Grid),
            ]
        );
        assert_eq!(
            bound(KeyboardPreset::Us, &none, "rate.3"),
            [
                ("3".to_owned(), Scope::Grid),
                ("3".to_owned(), Scope::Viewer)
            ]
        );
        assert_eq!(
            bound(KeyboardPreset::Us, &none, "video.togglePlay"),
            [("k".to_owned(), Scope::Media)]
        );
        assert_eq!(
            bound(KeyboardPreset::Us, &none, "video.fullscreen"),
            [("f".to_owned(), Scope::Media)]
        );
    }

    #[test]
    fn the_preset_swaps_the_bound_keys() {
        let none = Overrides::new();
        assert_eq!(
            bound(KeyboardPreset::Us, &none, "view.fitWindow"),
            [
                ("`".to_owned(), Scope::Viewer),
                ("`".to_owned(), Scope::DetailWindow),
                ("shift-f".to_owned(), Scope::Viewer),
                ("shift-f".to_owned(), Scope::DetailWindow)
            ]
        );
        assert_eq!(
            bound(KeyboardPreset::Eu, &none, "view.fitWindow"),
            [
                ("shift-f".to_owned(), Scope::Viewer),
                ("shift-f".to_owned(), Scope::DetailWindow),
                ("`".to_owned(), Scope::Viewer),
                ("`".to_owned(), Scope::DetailWindow)
            ]
        );
    }

    #[test]
    fn overrides_rebind_clear_and_reset() {
        let mut overrides = Overrides::new();
        overrides.insert(
            "nav.back",
            Override {
                keys: Some("Mod+Shift+B".into()),
                keys2: Some(String::new()),
            },
        );
        assert_eq!(
            bound(KeyboardPreset::Us, &overrides, "nav.back"),
            [(spelled("cmd-shift-b"), Scope::App)]
        );
        // Go Back no longer uses `Mod+[`, so that key is free for another
        // shortcut. The new keys `Mod+Shift+B` are reported as a conflict
        // with Go Back.
        assert_eq!(
            conflict("nav.forward", "Mod+[", KeyboardPreset::Us, &overrides),
            None
        );
        assert_eq!(
            conflict("nav.forward", "Mod+Shift+B", KeyboardPreset::Us, &overrides),
            Some("Go Back")
        );
        // Removing the override resets the shortcut to its two default
        // bindings.
        overrides.remove("nav.back");
        assert_eq!(bound(KeyboardPreset::Us, &overrides, "nav.back").len(), 2);
    }

    #[test]
    fn delete_and_remove_from_folder_share_their_key() {
        let none = Overrides::new();
        assert_eq!(
            bound(KeyboardPreset::Us, &none, "file.delete"),
            [
                (spelled("cmd-backspace"), Scope::Grid),
                (spelled("cmd-backspace"), Scope::Subscriptions)
            ]
        );
        assert_eq!(
            bound(KeyboardPreset::Us, &none, "file.removeFromFolder"),
            [(spelled("cmd-backspace"), Scope::Grid)]
        );
    }

    #[test]
    fn scopes_leave_text_fields_menus_popovers_and_dialogs_alone() {
        for scope in [
            Scope::App,
            Scope::Grid,
            Scope::Viewer,
            Scope::Duplicates,
            Scope::Subscriptions,
            Scope::DetailWindow,
        ] {
            let predicate = scope.predicate().unwrap();
            for owner in ["!TextInput", "!Menu", "!Modal", "!Popover"] {
                assert!(predicate.contains(owner), "{scope:?} lacks {owner}");
            }
            gpui::KeyBindingContextPredicate::parse(&predicate).unwrap();
        }
        assert_eq!(Scope::Anywhere.predicate(), None);
    }
}
