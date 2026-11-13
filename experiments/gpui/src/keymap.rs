//! The app's key bindings, built from the shortcut registry
//! (`settings::registry`, ported from `shortcuts.ts`). Every registry id
//! either maps to the actions that perform it, each in the key context
//! where production's shortcut scope listens, or is listed in [`NOT_YET`]
//! with the reason. The bindings follow the applied keyboard preset and the
//! user's overrides, both slots of each shortcut, and are rebuilt when the
//! Settings window applies a change.
//!
//! The scope rules of `shortcutRuntime.ts` become context predicates:
//! nothing below fires while a text field, a context menu, a popover or a
//! dialog has the keyboard, since those own their keys (Escape closes the
//! menu, then the popover, then the dialog, before the viewer or the
//! selection hear it). The grid's keys need the `Grid` context, which the
//! shell carries only while the grid itself shows, and the viewer's keys
//! need the viewer to have the keyboard.
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
        ShowAll,
        ShowInbox,
        ShowUntagged,
        ShowTrash,
        QuickLook,
        OpenDefaultApp,
        RevealInFinder,
        CopyFilePath,
        Rename,
        Rate0,
        Rate1,
        Rate2,
        Rate3,
        Rate4,
        Rate5,
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
        AddToFolders,
        AddToFolder,
        ImportFiles,
        ImportFolder,
        PasteImport,
        ExportOriginals,
        ExportAs,
        ExportPictoPack,
        ImportPictoPack
    ]
);

/// Where a shortcut listens. Each is a key-context predicate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Every window, Settings included (the application menu's keys).
    Anywhere,
    /// The main window, whatever it shows (`AppShell`'s and the sidebar's
    /// scopes, and the application menu's selection commands).
    App,
    /// The grid while it shows (`GridScreen`'s and `useGridArrowNav`'s).
    Grid,
    /// The media viewer while it has the keyboard (`MediaView`'s).
    Viewer,
    /// The duplicate review (`DuplicatesScreen`'s).
    Duplicates,
    /// A detail window (`DetailWindow`'s), which has the viewer's keys but
    /// closes only on Escape.
    DetailWindow,
    /// The media player while a viewer that shows it has the keyboard
    /// (`VideoPlayer`'s, which wins over the viewer's own).
    Media,
    /// A focused text field, wherever it is: the Edit menu's Undo and Redo
    /// go to the field's own history there (`historyRuntime.ts`).
    TextField,
}

/// What has the keyboard instead of the window's own shortcuts.
const OWNERS: &str = "!TextInput && !Menu && !Modal && !Popover";

impl Scope {
    pub fn predicate(self) -> Option<String> {
        let within = match self {
            Scope::Anywhere => return None,
            Scope::TextField => return Some("TextInput".into()),
            Scope::App => "Browser",
            Scope::Grid => "Grid",
            // Flash keeps its keys for the movie.
            Scope::Viewer => "Viewer && !Flash",
            Scope::Duplicates => "Duplicates",
            Scope::DetailWindow => "DetailWindow && !Flash",
            Scope::Media => "Media && !Flash",
        };
        Some(format!("{within} && {OWNERS}"))
    }
}

/// One action a shortcut performs, and where.
pub struct Target {
    pub scope: Scope,
    pub action: fn() -> Box<dyn Action>,
    /// Shift with the keys extends the selection (`allowExtraShift`).
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

/// The actions a registry id performs. Empty for the ids in [`NOT_YET`].
pub fn targets(id: &str) -> Vec<Target> {
    match id {
        // Navigation.
        "nav.allActive" => vec![to!(App, ShowAll)],
        "nav.inbox" => vec![to!(App, ShowInbox)],
        "nav.untagged" => vec![to!(App, ShowUntagged)],
        "nav.trash" => vec![to!(App, ShowTrash)],
        "nav.search" => vec![to!(App, FocusSearch)],
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
        // File.
        "file.settings" => vec![to!(Anywhere, crate::settings::OpenSettings)],
        "file.delete" => vec![to!(Grid, DeleteItems)],
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
        // An application menu accelerator in production, so the whole
        // window's.
        "file.import" => vec![to!(App, ImportFiles)],
        // Application menu accelerators too.
        "file.export" => vec![to!(App, ExportOriginals)],
        "file.exportAs" => vec![to!(App, ExportAs)],
        // Edit.
        "organize.addTag" => vec![to!(Grid, AddTags)],
        "organize.addFolder" => vec![to!(Grid, AddToFolders)],
        "edit.undo" => vec![
            to!(App, shell::Undo),
            to!(TextField, crate::ui::text_input::Undo),
        ],
        "edit.redo" => vec![
            to!(App, shell::Redo),
            to!(TextField, crate::ui::text_input::Redo),
        ],
        "edit.selectAll" => vec![to!(Grid, shell::SelectAll)],
        // Escape: the viewer's own binding closes it while it has the
        // keyboard; this one closes it otherwise, or clears the selection.
        "edit.deselectAll" => vec![to!(App, shell::ClosePreview)],
        "edit.rename" => vec![to!(App, Rename)],
        "edit.copyFilePath" => vec![to!(App, CopyFilePath), to!(DetailWindow, CopyFilePath)],
        "edit.pasteImport" => vec![to!(Grid, PasteImport)],
        // Inbox. The viewer's review the item it shows.
        "inbox.accept" => vec![to!(Grid, AcceptInbox), to!(Viewer, AcceptInbox)],
        "inbox.reject" => vec![to!(Grid, RejectInbox), to!(Viewer, RejectInbox)],
        // Rating: the selection in the grid, the shown item in the viewer.
        "rate.0" => vec![to!(Grid, Rate0), to!(Viewer, Rate0)],
        "rate.1" => vec![to!(Grid, Rate1), to!(Viewer, Rate1)],
        "rate.2" => vec![to!(Grid, Rate2), to!(Viewer, Rate2)],
        "rate.3" => vec![to!(Grid, Rate3), to!(Viewer, Rate3)],
        "rate.4" => vec![to!(Grid, Rate4), to!(Viewer, Rate4)],
        "rate.5" => vec![to!(Grid, Rate5), to!(Viewer, Rate5)],
        // View. Enter and Space open the viewer from the grid and close it
        // from inside, as `MediaView` treats both as its close keys.
        "view.detailView" => vec![to!(Grid, shell::Preview), to!(Viewer, viewer::Close)],
        "view.quicklook" => vec![to!(Grid, QuickLook), to!(Viewer, viewer::Close)],
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
        // Document pages, in whichever host shows the document.
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
        // Duplicates.
        "dup.keepBoth" => vec![to!(Duplicates, duplicates::KeepBoth)],
        "dup.notDuplicate" => vec![to!(Duplicates, duplicates::NotDuplicate)],
        "dup.keepLeft" => vec![to!(Duplicates, duplicates::KeepLeft)],
        "dup.keepRight" => vec![to!(Duplicates, duplicates::KeepRight)],
        "dup.smartMerge" => vec![to!(Duplicates, duplicates::SmartMerge)],
        "dup.fitToWindow" => vec![to!(Duplicates, duplicates::FitPair)],
        "dup.prevPair" => vec![to!(Duplicates, duplicates::PreviousPair)],
        "dup.nextPair" => vec![to!(Duplicates, duplicates::NextPair)],
        // Video.
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
        _ => Vec::new(),
    }
}

/// The registry ids the app cannot perform yet. They stay in the registry
/// (Settings lists those production lists) and are bound to nothing.
pub const NOT_YET: &[(&str, &str)] = &[
    (
        "nav.commandPalette",
        "deferred in production; no command palette",
    ),
    ("nav.goToFolder", "deferred in production; no folder jump"),
    (
        "file.addToLastFolder",
        "deferred in production; no last-folder memory",
    ),
    ("file.regenerateThumbnail", "no thumbnail generator linked"),
    ("organize.autoTag", "no AI tagger"),
    (
        "edit.copy",
        "GPUI's macOS clipboard cannot hold files, which this copies",
    ),
    ("edit.copyTags", "deferred in production"),
    ("edit.pasteTags", "deferred in production"),
    ("group.removeMembers", "no group surface"),
    ("view.grayscale", "the grid has no grayscale mode"),
    ("view.slideshow", "deferred in production"),
    ("view.navigator", "deferred in production"),
    ("view.layoutGrid", "deferred in production"),
    ("view.layoutWaterfall", "deferred in production"),
    ("view.layoutJustified", "deferred in production"),
    ("view.toggleTileName", "deferred in production"),
    ("view.toggleTileMetadata", "deferred in production"),
    ("view.toggleLogs", "no log window"),
    (
        "video.fullscreen",
        "the shell cannot yet give the player the whole window",
    ),
];

/// A registry key string (`Mod+Shift+Backspace`) as a GPUI keystroke
/// (`cmd-shift-backspace`), or `None` for an empty slot or a key GPUI has no
/// name for. `Mod` is Command on the Mac and Control elsewhere, as
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
        // GPUI reads an upper-case letter as Shift with it.
        single if single.chars().count() == 1 => single.to_lowercase(),
        _ => return None,
    };
    written.push(&key);
    Some(written.join("-"))
}

/// One binding to make: the keystroke, the shortcut it comes from and what
/// it does where.
pub struct Planned {
    pub keystroke: String,
    pub id: &'static str,
    pub scope: Scope,
    pub action: fn() -> Box<dyn Action>,
}

/// The bindings for the registry as configured: both slots of every
/// shortcut, each for every target, and Shift with the keys where Shift
/// extends the selection.
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

/// Marks the bindings made here, so a rebuild replaces only them.
const FROM_REGISTRY: KeyBindingMetaIndex = KeyBindingMetaIndex(0x5052_4547);

/// The registry as last installed, which tooltips and menu rows read.
#[derive(Default)]
struct Installed {
    preset: KeyboardPreset,
    overrides: Overrides,
}

impl Global for Installed {}

/// Replaces the registry's bindings with ones for this preset and these
/// overrides. Bindings made elsewhere (text fields, menus, dialogs) stay.
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
    // The application menu's keys, after the registry's so that a row
    // whose action the registry binds shows the registry's keys.
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
    // A document's scrolling keys come after the registry's, which they
    // override where pages are shown (Space).
    bindings.extend(
        crate::viewer::document::scroll_bindings()
            .into_iter()
            .map(|binding| binding.with_meta(FROM_REGISTRY)),
    );
    // Adding bindings redraws every window, so tooltips and the
    // application menu's key equivalents follow.
    cx.bind_keys(bindings);
    crate::set_app_menus(cx);
}

/// A shortcut's first binding as installed, in registry form, or empty.
fn primary(id: &str, cx: &App) -> String {
    let shortcut = match cx.try_global::<Installed>() {
        Some(installed) => registry::shortcut(id, installed.preset, &installed.overrides),
        None => registry::shortcut(id, KeyboardPreset::Us, &Overrides::new()),
    };
    shortcut.map(|shortcut| shortcut.keys).unwrap_or_default()
}

/// The keys a tooltip shows for a shortcut, one badge each
/// (`KbdTooltip` with `shortcutId`).
pub fn badges(id: &str, cx: &App) -> Vec<SharedString> {
    registry::parts(&primary(id, cx))
        .into_iter()
        .map(SharedString::from)
        .collect()
}

/// The hint a menu row shows for a shortcut (`shortcutHint`), if it has one.
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

    /// A keystroke written for the Mac, as this platform spells `Mod`.
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

    /// `Mod` is Command on the Mac and Control elsewhere.
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
        assert!(bound(KeyboardPreset::Us, &none, "video.fullscreen").is_empty());
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
        // The new keys now conflict with nothing; the old ones are free.
        assert_eq!(
            conflict("nav.forward", "Mod+[", KeyboardPreset::Us, &overrides),
            None
        );
        assert_eq!(
            conflict("nav.forward", "Mod+Shift+B", KeyboardPreset::Us, &overrides),
            Some("Go Back")
        );
        // Resetting is removing the override.
        overrides.remove("nav.back");
        assert_eq!(bound(KeyboardPreset::Us, &overrides, "nav.back").len(), 2);
    }

    #[test]
    fn delete_and_remove_from_folder_share_their_key() {
        let none = Overrides::new();
        assert_eq!(
            bound(KeyboardPreset::Us, &none, "file.delete"),
            [(spelled("cmd-backspace"), Scope::Grid)]
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
