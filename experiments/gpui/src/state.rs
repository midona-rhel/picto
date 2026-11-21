//! This file keeps the things the user arranges that belong to this machine
//! and not to a library. They are: the position and size of the main window,
//! the widths of the sidebar and the inspector and whether each is hidden,
//! the folded sections and nodes of the sidebar, the filter bar's pinned
//! fields and saved filters, the tag picker's layout, the recent tags, the
//! recent folders and the recent Quick Search choices, the keyboard preset
//! and the changed shortcuts, the language, the audio visualization, the
//! interface zoom and the list of libraries. Production keeps most of these
//! in the renderer's localStorage, and keeps the main window's position and
//! the list of libraries in the main process's `config.json`. Both are kept
//! per machine. The settings that production keeps in the library are in the
//! settings model (`settings::model`), and they travel with the library.
//!
//! The state is one JSON file, `state.json`, in this app's own folder. That
//! folder is "Picto GPUI" in the platform's configuration folder, or the
//! folder that the environment variable `PICTO_GPUI_STATE` names. It is never
//! the Electron app's folder. Tests and the off-screen tool keep no state
//! unless `PICTO_GPUI_STATE` is set, so that their pictures never depend on
//! how the user left the app.
//!
//! The file is read once when the app starts. A change is written 250 ms
//! after the last change, off the thread that draws, and the remaining
//! changes are written at once when the app quits. Each write goes to a
//! temporary file that is then renamed over the old file. The rules for
//! reading are these:
//!
//! - When there is no file, every field has its default.
//! - A field that is missing, or whose value is of the wrong kind, has its
//!   default, and the other fields are read as usual.
//! - A file that cannot be read as JSON is renamed to `state.json.corrupt`,
//!   and every field has its default.
//! - A field that this version does not know, because a newer version wrote
//!   it, is kept and written back unchanged.
use gpui::{App, Global, Task};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// The version of the file's layout that this build writes.
pub const VERSION: u64 = 1;
const FILE: &str = "state.json";
/// The time between the last change and the write of the file.
const SAVE_DELAY: Duration = Duration::from_millis(250);

/// The smallest size of the main window. These are the `minWidth` and
/// `minHeight` that production gives its `BrowserWindow` in
/// `windowManager.mjs`. Production accepts a saved size as small as 700×500
/// (`MAIN_WINDOW_MIN_WIDTH` and `MAIN_WINDOW_MIN_HEIGHT`), but the window's
/// own minimum then enlarges it to this size, so this size is the only limit
/// that has an effect.
pub const WINDOW_MIN_WIDTH: f32 = 1000.;
pub const WINDOW_MIN_HEIGHT: f32 = 600.;

/// A rectangle, in the points of the whole screen area.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.width
            && self.x + self.width > other.x
            && self.y < other.y + other.height
            && self.y + self.height > other.y
    }
}

/// The saved position and size of the main window (`windowState.main` in
/// production). The bounds are those the window has while it is neither
/// maximised nor in full screen. `maximized` says whether the window was
/// maximised. Production does not remember full screen: a window that was
/// closed in full screen opens again at these bounds.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowPlace {
    /// The position of the window's top-left corner. When `x` or `y` is
    /// `None`, the window opens in the centre of the display.
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub width: f32,
    pub height: f32,
    pub maximized: bool,
}

impl WindowPlace {
    /// Returns the position and size to open the window with, given the
    /// usable areas of the displays. The size is at least the minimum size.
    /// The saved position is kept only when the window would overlap one of
    /// the displays there. Otherwise the position is `None`, and the window
    /// opens in the centre. This is production's `normalizeMainWindowState`.
    pub fn on_displays(&self, displays: &[Rect]) -> WindowPlace {
        let mut place = WindowPlace {
            width: self.width.round().max(WINDOW_MIN_WIDTH),
            height: self.height.round().max(WINDOW_MIN_HEIGHT),
            x: self.x.map(f32::round),
            y: self.y.map(f32::round),
            maximized: self.maximized,
        };
        if let (Some(x), Some(y)) = (place.x, place.y) {
            let rect = Rect {
                x,
                y,
                width: place.width,
                height: place.height,
            };
            if !displays.iter().any(|display| rect.intersects(display)) {
                place.x = None;
                place.y = None;
            }
        }
        place
    }
}

/// The keys of one shortcut that the user changed in Settings
/// (`ShortcutBindingOverride` in production). Each field is one of the
/// shortcut's two key slots. A slot is `None` when the user did not change
/// it, and an empty string when the user cleared it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Binding {
    pub keys: Option<String>,
    pub keys2: Option<String>,
}

/// Everything that the file holds. The comment on each field names the
/// storage key under which production keeps the same thing.
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    /// The main window's position and size (`windowState.main` in
    /// `config.json`).
    pub window: Option<WindowPlace>,
    /// The sidebar's width, and whether the sidebar is hidden
    /// (`picto-sidebar-width` and `picto-sidebar-collapsed-panel`).
    pub sidebar_width: f32,
    pub sidebar_collapsed: bool,
    /// The inspector's width, and whether the inspector is hidden
    /// (`picto-inspector-width` and `picto-inspector-collapsed`).
    pub inspector_width: f32,
    pub inspector_collapsed: bool,
    /// The ids of the sidebar's sections and nodes that are folded
    /// (`picto-sidebar-collapsed`).
    pub folded: Vec<String>,
    /// The filter fields that the filter bar shows while they are empty,
    /// under production's names for them, such as `color` and `tags`
    /// (`picto:grid:pinned-filters`).
    pub pinned_filters: Vec<String>,
    /// The saved filters, each with its name, in the order in which they
    /// were saved (`picto:grid:saved-filters`).
    pub saved_filters: Vec<(String, Value)>,
    /// True when the tag picker shows a list and false when it shows a
    /// grid (`picto-tag-picker-layout`).
    pub tag_picker_list: bool,
    /// Whether the tag picker shows each tag's count
    /// (`picto-tag-picker-counts`).
    pub tag_picker_counts: bool,
    /// The tags used most recently, the newest first (`picto-recent-tags`).
    pub recent_tags: Vec<String>,
    /// For each library, the folders used most recently, the newest first
    /// (`picto-recent-folders:<library>`). The map's key is the library's
    /// key ([`library_key`]).
    pub recent_folders: BTreeMap<String, Vec<u32>>,
    /// For each library, the choices made most recently in the Quick Search
    /// panel, the newest first. The map's key is the library's key
    /// ([`library_key`]). Each choice is a JSON object that
    /// `crate::shell::quick_search` writes and reads. Production has no such panel
    /// and no such list.
    pub quick_search_recent: BTreeMap<String, Vec<Value>>,
    /// The keyboard preset, `us` or `eu` (`picto-keyboard-preset`).
    pub keyboard_preset: String,
    /// The shortcuts whose keys the user changed, by shortcut id
    /// (`picto-shortcut-overrides`).
    pub shortcut_overrides: BTreeMap<String, Binding>,
    /// The language the user chose (`picto:locale`). It is empty until the
    /// user chooses one, and the interface then uses the system's language.
    pub locale: String,
    /// The audio visualization the user chose (`picto:audio-visualization`).
    pub audio_visualization: String,
    /// The interface zoom (`picto-zoom-factor`). `crate::zoom` applies it.
    pub zoom_factor: f64,
    /// The paths of the libraries the user keeps that this app has opened,
    /// the most recent first (`libraryHistory` in `config.json`).
    /// `crate::libraries` maintains the list. A disposable copy is never
    /// listed.
    pub library_history: Vec<String>,
    /// The paths of the libraries pinned to Quick Access, in the order in
    /// which they were pinned (`pinnedLibraries` in `config.json`).
    pub pinned_libraries: Vec<String>,
    /// The path of the library that the app opens when it starts
    /// (`lastLibrary` in `config.json`).
    pub last_library: Option<String>,
    /// The libraries that the user agreed this app may read and write
    /// directly. Each entry is a library's path and the identity of its
    /// database file (`libraries::Identity`). Production has no such list.
    pub library_consents: Vec<(String, String)>,
    /// True once the list of libraries has been copied from the Electron
    /// app's `config.json` (`libraries::seed`). The copy is made only once.
    pub libraries_seeded: bool,
    /// Each library's colour and icon, by the library's path, as the
    /// Library Manager sets them (`libraryMeta` in `config.json`). A
    /// library's cover is not here: it is kept in the library's own folder.
    pub library_meta: BTreeMap<String, LibraryLook>,
    /// For each cloud provider, the provider's folder that the user chose
    /// last (`cloudLocations` in `config.json`, written by production's
    /// `rememberCloudRoot`). The app offers that folder again for as long as
    /// it is still a folder of that provider.
    pub cloud_locations: BTreeMap<String, crate::cloud::Root>,
}

/// A library's colour, written as `#RRGGBB`, and its icon, which is a name
/// from the icon picker. Each is `None` when the library has the default.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LibraryLook {
    pub color: Option<String>,
    pub icon: Option<String>,
    /// The id of the cloud library that this library was opened from
    /// (`cloudLibraryId`). Open from Cloud… uses it to say that the library
    /// is already added.
    pub cloud_library_id: Option<String>,
}

impl LibraryLook {
    /// Reads the colour, the icon and the cloud library id from one entry of
    /// production's `libraryMeta`. The entry's other fields are not read.
    pub fn from_value(value: &Value) -> Self {
        let text = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        };
        Self {
            color: text("color"),
            icon: text("icon"),
            cloud_library_id: text("cloudLibraryId"),
        }
    }

    pub fn is_default(&self) -> bool {
        self.color.is_none() && self.icon.is_none() && self.cloud_library_id.is_none()
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            window: None,
            sidebar_width: crate::theme::SIDEBAR_WIDTH,
            sidebar_collapsed: false,
            inspector_width: crate::theme::INSPECTOR_WIDTH,
            inspector_collapsed: false,
            folded: Vec::new(),
            pinned_filters: crate::grid::filters::Field::DEFAULT_PINNED
                .map(|field| field.id().to_owned())
                .to_vec(),
            saved_filters: Vec::new(),
            tag_picker_list: false,
            tag_picker_counts: true,
            recent_tags: Vec::new(),
            recent_folders: BTreeMap::new(),
            quick_search_recent: BTreeMap::new(),
            keyboard_preset: "us".into(),
            shortcut_overrides: BTreeMap::new(),
            locale: String::new(),
            audio_visualization: "spectrum".into(),
            zoom_factor: 1.0,
            library_history: Vec::new(),
            pinned_libraries: Vec::new(),
            last_library: None,
            library_consents: Vec::new(),
            libraries_seeded: false,
            library_meta: BTreeMap::new(),
            cloud_locations: BTreeMap::new(),
        }
    }
}

/// Rounds a sidebar width to a whole number of pixels and keeps it between
/// the sidebar's smallest and largest widths, as production's
/// `setSidebarWidthAtom` does.
pub fn sidebar_width(width: f32) -> f32 {
    width
        .round()
        .clamp(crate::theme::SIDEBAR_MIN, crate::theme::SIDEBAR_MAX)
}

/// Rounds an inspector width to a whole number of pixels and keeps it
/// between the inspector's smallest and largest widths, as production's
/// `setInspectorWidthAtom` does.
pub fn inspector_width(width: f32) -> f32 {
    width
        .round()
        .clamp(crate::theme::INSPECTOR_MIN, crate::theme::INSPECTOR_MAX)
}

fn strings(value: Option<&Value>) -> Option<Vec<String>> {
    Some(
        value?
            .as_array()?
            .iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect(),
    )
}

impl State {
    /// Reads the contents of a file, one field at a time. A field that is
    /// missing, or whose value is of the wrong kind, gets its default.
    /// Returns the state, and the fields that this version does not know,
    /// which are written back unchanged.
    pub fn from_value(value: &Value) -> (State, Map<String, Value>) {
        let mut state = State::default();
        let Some(object) = value.as_object() else {
            return (state, Map::new());
        };
        let migrated = migrate(object.clone());
        let get = |key: &str| migrated.get(key);
        let number = |key: &str| get(key).and_then(Value::as_f64).map(|n| n as f32);
        let flag = |key: &str| get(key).and_then(Value::as_bool);
        let text = |key: &str| get(key).and_then(Value::as_str).map(str::to_owned);

        state.window = get("window").and_then(|window| {
            let field = |key: &str| window.get(key).and_then(Value::as_f64).map(|n| n as f32);
            let (width, height) = (field("width")?, field("height")?);
            (width.is_finite() && height.is_finite()).then(|| WindowPlace {
                // i18n: a JSON field
                x: field("x").filter(|x| x.is_finite()),
                y: field("y").filter(|y| y.is_finite()),
                width,
                height,
                maximized: window
                    .get("maximized")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            })
        });
        if let Some(width) = number("sidebarWidth").filter(|n| n.is_finite()) {
            state.sidebar_width = sidebar_width(width);
        }
        if let Some(width) = number("inspectorWidth").filter(|n| n.is_finite()) {
            state.inspector_width = inspector_width(width);
        }
        state.sidebar_collapsed = flag("sidebarCollapsed").unwrap_or(state.sidebar_collapsed);
        state.inspector_collapsed = flag("inspectorCollapsed").unwrap_or(state.inspector_collapsed);
        if let Some(folded) = strings(get("sidebarFolded")) {
            state.folded = folded;
        }
        // Keep only the fields that this version knows, as production's
        // `loadPinnedFilters` does.
        if let Some(pinned) = strings(get("pinnedFilters")) {
            state.pinned_filters = pinned
                .into_iter()
                .filter(|field| crate::grid::filters::Field::from_id(field).is_some())
                .collect();
        }
        if let Some(saved) = get("savedFilters").and_then(Value::as_array) {
            state.saved_filters = saved
                .iter()
                .filter_map(|item| {
                    let name = item.get("name")?.as_str()?.to_owned();
                    let filters = item.get("filters")?;
                    filters.is_object().then(|| (name, filters.clone()))
                })
                .collect();
        }
        if let Some(layout) = text("tagPickerLayout") {
            state.tag_picker_list = layout == "list";
        }
        state.tag_picker_counts = flag("tagPickerCounts").unwrap_or(state.tag_picker_counts);
        if let Some(tags) = strings(get("recentTags")) {
            state.recent_tags = tags;
        }
        if let Some(libraries) = get("recentFolders").and_then(Value::as_object) {
            state.recent_folders = libraries
                .iter()
                .filter_map(|(library, ids)| {
                    let ids = ids
                        .as_array()?
                        .iter()
                        .filter_map(Value::as_u64)
                        .filter(|id| (1..=u64::from(u32::MAX)).contains(id))
                        .map(|id| id as u32)
                        .collect();
                    Some((library.clone(), ids))
                })
                .collect();
        }
        if let Some(libraries) = get("quickSearchRecent").and_then(Value::as_object) {
            state.quick_search_recent = libraries
                .iter()
                .filter_map(|(library, choices)| {
                    let choices = choices
                        .as_array()?
                        .iter()
                        .filter(|choice| choice.is_object())
                        .cloned()
                        .collect();
                    Some((library.clone(), choices))
                })
                .collect();
        }
        if let Some(preset) = text("keyboardPreset") {
            state.keyboard_preset = if preset == "eu" { "eu" } else { "us" }.into();
        }
        if let Some(overrides) = get("shortcutOverrides").and_then(Value::as_object) {
            state.shortcut_overrides = overrides
                .iter()
                .filter_map(|(id, binding)| {
                    let slot = |key: &str| binding.get(key)?.as_str().map(str::to_owned);
                    let binding = Binding {
                        keys: slot("keys"),
                        keys2: slot("keys2"),
                    };
                    (binding.keys.is_some() || binding.keys2.is_some())
                        .then(|| (id.clone(), binding))
                })
                .collect();
        }
        if let Some(locale) = text("locale") {
            state.locale = locale;
        }
        if let Some(mode) = text("audioVisualization") {
            state.audio_visualization = mode;
        }
        if let Some(zoom) = get("zoomFactor").and_then(Value::as_f64) {
            state.zoom_factor = crate::zoom::clamp(zoom);
        }
        let paths = |key: &str| {
            get(key).and_then(Value::as_array).map(|paths| {
                paths
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
        };
        if let Some(history) = paths("libraryHistory") {
            state.library_history = history;
        }
        if let Some(pinned) = paths("pinnedLibraries") {
            state.pinned_libraries = pinned;
        }
        state.last_library = text("lastLibrary");
        if let Some(consents) = get("libraryConsents").and_then(Value::as_array) {
            state.library_consents = consents
                .iter()
                .filter_map(|consent| {
                    Some((
                        consent.get("path")?.as_str()?.to_owned(),
                        consent.get("database")?.as_str()?.to_owned(),
                    ))
                })
                .collect();
        }
        if let Some(meta) = get("libraryMeta").and_then(Value::as_object) {
            state.library_meta = meta
                .iter()
                .map(|(path, look)| (path.clone(), LibraryLook::from_value(look)))
                .filter(|(_, look)| !look.is_default())
                .collect();
        }
        if let Some(locations) = get("cloudLocations").and_then(Value::as_object) {
            state.cloud_locations = locations
                .iter()
                .filter_map(|(provider, root)| {
                    let root: crate::cloud::Root = serde_json::from_value(root.clone()).ok()?;
                    (root.provider == *provider).then(|| (provider.clone(), root))
                })
                .collect();
        }
        if let Some(seeded) = get("librariesSeeded").and_then(Value::as_bool) {
            state.libraries_seeded = seeded;
        }
        let extra = migrated
            .into_iter()
            .filter(|(key, _)| !KNOWN.contains(&key.as_str()))
            .collect();
        (state, extra)
    }

    /// Returns the contents to write to the file. `extra` holds the fields
    /// that a newer version wrote, and they are written back unchanged.
    pub fn to_value(&self, extra: &Map<String, Value>) -> Value {
        let mut object = extra.clone();
        let mut put = |key: &str, value: Value| {
            object.insert(key.to_owned(), value);
        };
        put("version", json!(VERSION));
        put(
            "window",
            match &self.window {
                Some(window) => json!({
                    "x": window.x,
                    "y": window.y,
                    "width": window.width,
                    "height": window.height,
                    "maximized": window.maximized,
                }),
                None => Value::Null,
            },
        );
        put("sidebarWidth", json!(self.sidebar_width));
        put("sidebarCollapsed", json!(self.sidebar_collapsed));
        put("inspectorWidth", json!(self.inspector_width));
        put("inspectorCollapsed", json!(self.inspector_collapsed));
        put("sidebarFolded", json!(self.folded));
        put("pinnedFilters", json!(self.pinned_filters));
        put(
            "savedFilters",
            Value::Array(
                self.saved_filters
                    .iter()
                    .map(|(name, filters)| json!({ "name": name, "filters": filters }))
                    .collect(),
            ),
        );
        put(
            "tagPickerLayout",
            json!(if self.tag_picker_list { "list" } else { "grid" }),
        );
        put("tagPickerCounts", json!(self.tag_picker_counts));
        put("recentTags", json!(self.recent_tags));
        put("recentFolders", json!(self.recent_folders));
        put("quickSearchRecent", json!(self.quick_search_recent));
        put("keyboardPreset", json!(self.keyboard_preset));
        put(
            "shortcutOverrides",
            Value::Object(
                self.shortcut_overrides
                    .iter()
                    .map(|(id, binding)| {
                        let mut slots = Map::new();
                        if let Some(keys) = &binding.keys {
                            slots.insert("keys".into(), json!(keys));
                        }
                        if let Some(keys2) = &binding.keys2 {
                            slots.insert("keys2".into(), json!(keys2));
                        }
                        (id.clone(), Value::Object(slots))
                    })
                    .collect(),
            ),
        );
        if !self.locale.is_empty() {
            put("locale", json!(self.locale));
        }
        put("audioVisualization", json!(self.audio_visualization));
        put("zoomFactor", json!(self.zoom_factor));
        put("libraryHistory", json!(self.library_history));
        put("pinnedLibraries", json!(self.pinned_libraries));
        put("lastLibrary", json!(self.last_library));
        put(
            "libraryConsents",
            Value::Array(
                self.library_consents
                    .iter()
                    .map(|(path, database)| json!({ "path": path, "database": database }))
                    .collect(),
            ),
        );
        put("librariesSeeded", json!(self.libraries_seeded));
        put(
            "libraryMeta",
            Value::Object(
                self.library_meta
                    .iter()
                    .map(|(path, look)| {
                        (
                            path.clone(),
                            json!({
                                "color": look.color,
                                "icon": look.icon,
                                "cloudLibraryId": look.cloud_library_id,
                            }),
                        )
                    })
                    .collect(),
            ),
        );
        put(
            "cloudLocations",
            serde_json::to_value(&self.cloud_locations).unwrap_or_default(),
        );
        Value::Object(object)
    }

    /// Reads the text of a file. Returns `None` when the text is not a JSON
    /// object.
    pub fn parse(text: &str) -> Option<(State, Map<String, Value>)> {
        let value: Value = serde_json::from_str(text).ok()?;
        value.is_object().then(|| State::from_value(&value))
    }
}

/// The names of the fields that this version reads and writes.
const KNOWN: [&str; 26] = [
    "version",
    "window",
    "sidebarWidth",
    "sidebarCollapsed",
    "inspectorWidth",
    "inspectorCollapsed",
    "sidebarFolded",
    "pinnedFilters",
    "savedFilters",
    "tagPickerLayout",
    "tagPickerCounts",
    "recentTags",
    "recentFolders",
    "quickSearchRecent",
    "keyboardPreset",
    "shortcutOverrides",
    "locale",
    "audioVisualization",
    "zoomFactor",
    "libraryHistory",
    "pinnedLibraries",
    "lastLibrary",
    "libraryConsents",
    "librariesSeeded",
    "libraryMeta",
    "cloudLocations",
];

/// Converts the contents of a file that an earlier version wrote to this
/// version's layout, one version at a time. Version 1 is the first layout,
/// so there is no conversion yet. A file that has no `version` field is read
/// as version 1. A later layout adds its conversion here, and applies it
/// according to the file's `version` field.
fn migrate(object: Map<String, Value>) -> Map<String, Value> {
    object
}

/// Returns the folder that holds the state file, or `None` when no state is
/// kept. That is the folder `PICTO_GPUI_STATE` names when it is set. In a
/// test build without that variable no state is kept. Otherwise it is
/// "Picto GPUI" in the platform's configuration folder.
pub fn location() -> Option<PathBuf> {
    if let Some(folder) = crate::seams::state_folder() {
        return Some(folder);
    }
    if cfg!(test) {
        return None;
    }
    config_folder().map(|folder| folder.join("Picto GPUI"))
}

/// Returns the platform's configuration folder for the current user. This is
/// what `app.getPath('appData')` returns in production.
pub fn config_folder() -> Option<PathBuf> {
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        // i18n: a folder's name
        home().map(|home| home.join("Library").join("Application Support"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|folder| folder.is_absolute())
            .or_else(|| home().map(|home| home.join(".config")))
    }
}

/// Reads the state file in `folder`. A file that is not JSON is renamed to
/// `state.json.corrupt`, so that its contents are not lost, and every field
/// then has its default.
pub fn load(folder: &Path) -> (State, Map<String, Value>) {
    let path = folder.join(FILE);
    // Only a missing file means that there is no state. A file that exists
    // and cannot be read as text is renamed below, like any other file that
    // cannot be read. Otherwise the next save would write over it.
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (State::default(), Map::new());
        }
        Err(error) => {
            eprintln!("picto: {} could not be read: {error}", path.display());
            Vec::new()
        }
    };
    match std::str::from_utf8(&bytes).ok().and_then(State::parse) {
        Some(read) => read,
        None => {
            eprintln!(
                "picto: {} could not be read; starting from the defaults",
                path.display()
            );
            let _ = std::fs::rename(&path, folder.join(format!("{FILE}.corrupt")));
            (State::default(), Map::new())
        }
    }
}

/// The sequence number of the latest write that reached the disk. A write
/// with a lower number is skipped, so that an earlier write that finishes
/// late never replaces a later one.
static WRITTEN: Mutex<u64> = Mutex::new(0);

/// Writes `text` to the state file in `folder` with [`write_file`]. It does
/// nothing when a write with a higher sequence number has already reached
/// the disk.
fn write(folder: &Path, text: &str, sequence: u64) -> std::io::Result<()> {
    let mut written = WRITTEN.lock().unwrap_or_else(|poison| poison.into_inner());
    if sequence < *written {
        return Ok(());
    }
    write_file(folder, FILE, text.as_bytes())?;
    *written = sequence;
    Ok(())
}

/// Writes `bytes` to the file `name` in `folder`, and creates the folder
/// when it does not exist. The bytes are written to a temporary file, and
/// that file is flushed to the disk before it is renamed over the old file.
/// After a crash or a power cut the folder therefore holds either the whole
/// old file or the whole new file. A write that fails removes the temporary
/// file.
pub fn write_file(folder: &Path, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    std::fs::create_dir_all(folder)?;
    let temporary = folder.join(format!("{name}.{}.tmp", std::process::id()));
    let replaced = std::fs::File::create(&temporary)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.sync_all()
        })
        .and_then(|()| std::fs::rename(&temporary, folder.join(name)));
    if let Err(error) = replaced {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

/// Returns the key under which the recent folders and the recent Quick
/// Search choices of the library at `root` are kept. For a disposable copy
/// the key is the path of the library that the copy was made from, so that
/// the lists are still there when another copy of that library is opened.
/// For any other library the key is the library's own path.
pub fn library_key(root: &Path) -> String {
    std::fs::read_to_string(root.join(crate::application::MARKER))
        .ok()
        .map(|source| source.trim().to_owned())
        .filter(|source| !source.is_empty())
        .unwrap_or_else(|| root.display().to_string())
}

struct Store {
    folder: Option<PathBuf>,
    state: State,
    extra: Map<String, Value>,
    /// The contents of the file on the disk. A save whose contents are the
    /// same is skipped.
    on_disk: Value,
    sequence: u64,
    pending: Option<Task<()>>,
}

impl Global for Store {}

/// Reads the state from the folder that [`location`] returns. The app calls
/// it once when it starts, before anything reads the state.
pub fn init(cx: &mut App) {
    let folder = location();
    let (state, extra) = folder.as_deref().map(load).unwrap_or_default();
    let on_disk = state.to_value(&extra);
    crate::i18n::init(&state.locale);
    cx.set_global(Store {
        folder,
        state,
        extra,
        on_disk,
        sequence: 0,
        pending: None,
    });
}

/// Returns the state.
pub fn get(cx: &App) -> &State {
    &cx.global::<Store>().state
}

/// Changes the state with `change`. The file is written 250 ms after the
/// last change.
pub fn update(cx: &mut App, change: impl FnOnce(&mut State)) {
    if !cx.has_global::<Store>() {
        return;
    }
    let store = cx.global_mut::<Store>();
    let before = store.state.clone();
    change(&mut store.state);
    if store.state == before || store.folder.is_none() {
        return;
    }
    let task = cx.spawn(async move |cx| {
        cx.background_executor().timer(SAVE_DELAY).await;
        let _ = cx.update(|cx| save(cx, false));
    });
    cx.global_mut::<Store>().pending = Some(task);
}

/// Writes the state to the file at once when it differs from the file's
/// contents. The app calls it when it quits and when a script of the
/// off-screen tool ends.
pub fn flush(cx: &mut App) {
    if cx.has_global::<Store>() {
        save(cx, true);
    }
}

fn save(cx: &mut App, now: bool) {
    let store = cx.global_mut::<Store>();
    store.pending = None;
    let Some(folder) = store.folder.clone() else {
        return;
    };
    let value = store.state.to_value(&store.extra);
    if value == store.on_disk {
        return;
    }
    let text = serde_json::to_string_pretty(&value).unwrap_or_default();
    store.on_disk = value;
    store.sequence += 1;
    let sequence = store.sequence;
    let report = move |result: std::io::Result<()>| {
        if let Err(error) = result {
            eprintln!("picto: could not keep the window's arrangement: {error}");
        }
    };
    if now {
        report(write(&folder, &text, sequence));
    } else {
        cx.background_executor()
            .spawn(async move { report(write(&folder, &text, sequence)) })
            .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::{Binding, Rect, State, VERSION, WindowPlace, load, write};
    use serde_json::json;

    #[test]
    fn a_state_reads_back_as_it_was_written() {
        let mut state = State {
            window: Some(WindowPlace {
                x: Some(40.),
                y: Some(-900.),
                width: 1400.,
                height: 900.,
                maximized: true,
            }),
            sidebar_width: 360.,
            sidebar_collapsed: true,
            inspector_width: 410.,
            inspector_collapsed: true,
            folded: vec!["folders".into(), "folder:12".into()],
            pinned_filters: vec!["size".into(), "tags".into()],
            saved_filters: vec![("Wide".into(), json!({ "min_width": 1920 }))],
            tag_picker_list: true,
            tag_picker_counts: false,
            recent_tags: vec!["artist:someone".into(), "cat".into()],
            keyboard_preset: "eu".into(),
            locale: "fi".into(),
            audio_visualization: "orbit".into(),
            zoom_factor: 1.35,
            library_history: vec!["/b/Two.library".into(), "/a/One.library".into()],
            pinned_libraries: vec!["/a/One.library".into()],
            last_library: Some("/b/Two.library".into()),
            library_consents: vec![("/b/Two.library".into(), "16777234:991".into())],
            libraries_seeded: true,
            library_meta: std::collections::BTreeMap::from([(
                "/a/One.library".into(),
                super::LibraryLook {
                    color: Some("#FA5252".into()),
                    icon: Some("IconStar".into()),
                    cloud_library_id: None,
                },
            )]),
            ..State::default()
        };
        state
            .recent_folders
            .insert("/Users/x/Main.library".into(), vec![4, 9]);
        state.quick_search_recent.insert(
            "/Users/x/Main.library".into(),
            vec![
                json!({ "key": "tag:7" }),
                json!({ "key": "place:folder:4" }),
            ],
        );
        state.shortcut_overrides.insert(
            "view.toggleSidebar".into(),
            Binding {
                keys: Some("Mod+Shift+B".into()),
                keys2: Some(String::new()),
            },
        );
        let text = state.to_value(&Default::default()).to_string();
        let (read, extra) = State::parse(&text).unwrap();
        assert_eq!(read, state);
        assert!(extra.is_empty());
    }

    #[test]
    fn a_missing_or_wrong_field_takes_its_default_and_keeps_the_rest() {
        let (state, _) = State::from_value(&json!({
            "version": VERSION,
            "sidebarWidth": "wide",
            "inspectorWidth": 9999,
            "sidebarCollapsed": true,
            "pinnedFilters": ["size", "nonsense", 3],
            "recentFolders": { "a": [1, -2, "x", 7] },
            "window": { "width": 800 },
            "zoomFactor": "big",
        }));
        let defaults = State::default();
        assert_eq!(state.sidebar_width, defaults.sidebar_width);
        assert_eq!(state.inspector_width, crate::theme::INSPECTOR_MAX);
        assert!(state.sidebar_collapsed);
        assert_eq!(state.pinned_filters, ["size"]);
        assert_eq!(state.recent_folders["a"], [1, 7]);
        assert_eq!(state.window, None);
        assert_eq!(state.locale, "");
        assert_eq!(state.zoom_factor, 1.0);
        // A zoom outside production's limits is brought inside them.
        let (state, _) = State::from_value(&json!({ "zoomFactor": 3.5 }));
        assert_eq!(state.zoom_factor, 2.0);
    }

    #[test]
    fn a_file_without_a_version_is_read_and_stamped() {
        let (state, extra) = State::from_value(&json!({
            "window": { "x": 10, "y": 20, "width": 1000, "height": 700 },
            "sidebarWidth": 250,
            "sidebarCollapsed": true,
            "locale": "de",
        }));
        assert_eq!(
            state.window,
            Some(WindowPlace {
                x: Some(10.),
                y: Some(20.),
                width: 1000.,
                height: 700.,
                maximized: false
            })
        );
        assert_eq!((state.sidebar_width, state.sidebar_collapsed), (250., true));
        assert_eq!(state.inspector_width, crate::theme::INSPECTOR_WIDTH);
        assert_eq!(state.locale, "de");
        assert!(extra.is_empty());
        assert_eq!(state.to_value(&extra)["version"], json!(VERSION));
    }

    #[test]
    fn fields_of_a_newer_version_are_kept() {
        let (state, extra) = State::from_value(&json!({
            "version": VERSION + 1,
            "locale": "ja",
            "somethingNew": { "a": 1 },
        }));
        assert_eq!(state.locale, "ja");
        assert_eq!(state.to_value(&extra)["somethingNew"], json!({ "a": 1 }));
    }

    #[test]
    fn an_unreadable_file_is_set_aside_and_the_defaults_used() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("state.json"), "{ not json").unwrap();
        let (state, _) = load(folder.path());
        assert_eq!(state, State::default());
        assert!(folder.path().join("state.json.corrupt").exists());
        assert!(!folder.path().join("state.json").exists());
        // When there is no file at all, every field has its default too.
        assert_eq!(load(folder.path()).0, State::default());
        // A file that is not text is renamed as well, so that the next save
        // does not write over it.
        std::fs::write(folder.path().join("state.json"), b"\x00\xff{{").unwrap();
        assert_eq!(load(folder.path()).0, State::default());
        assert_eq!(
            std::fs::read(folder.path().join("state.json.corrupt")).unwrap(),
            b"\x00\xff{{"
        );
    }

    #[test]
    fn a_write_replaces_the_file_whole_and_never_goes_back() {
        let folder = tempfile::tempdir().unwrap();
        let mut state = State {
            locale: "fr".into(),
            ..State::default()
        };
        write(
            folder.path(),
            &state.to_value(&Default::default()).to_string(),
            1_000,
        )
        .unwrap();
        state.locale = "pt".into();
        // An earlier write that finishes after a later one is skipped.
        write(
            folder.path(),
            &state.to_value(&Default::default()).to_string(),
            999,
        )
        .unwrap();
        assert_eq!(load(folder.path()).0.locale, "fr");
        let leftovers = std::fs::read_dir(folder.path()).unwrap().count();
        assert_eq!(leftovers, 1);
    }

    #[test]
    fn a_window_off_every_display_is_centred_and_never_too_small() {
        let displays = [
            Rect {
                x: 0.,
                y: 0.,
                width: 1512.,
                height: 944.,
            },
            Rect {
                x: 1512.,
                y: -200.,
                width: 2560.,
                height: 1415.,
            },
        ];
        let place = |x, y, width, height| WindowPlace {
            x: Some(x),
            y: Some(y),
            width,
            height,
            maximized: false,
        };
        // The saved position is on the second display, so it is kept.
        let kept = place(2000., 100., 1200., 800.).on_displays(&displays);
        assert_eq!((kept.x, kept.y), (Some(2000.), Some(100.)));
        // The saved position is partly on a display, so it is kept, as in
        // production.
        let edge = place(-1100., 10., 1200., 800.).on_displays(&displays);
        assert_eq!(edge.x, Some(-1100.));
        // The saved position is on a display that is no longer connected, so
        // the window is centred.
        let gone = place(5000., 100., 1200., 800.).on_displays(&displays);
        assert_eq!((gone.x, gone.y), (None, None));
        // The saved size is below the minimum, so the minimum size is used.
        let small = place(10., 10., 300., 200.4).on_displays(&displays);
        assert_eq!((small.width, small.height), (1000., 600.));
        // No display is known, so the window is centred.
        assert_eq!(place(10., 10., 900., 600.).on_displays(&[]).x, None::<f32>);
    }
}
