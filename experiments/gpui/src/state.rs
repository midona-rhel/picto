//! What the user arranges that belongs to this machine rather than to a
//! library: where the window was, the rails' widths and whether they are
//! hidden, folded sidebar sections and nodes, the filter bar's pinned fields
//! and saved filters, the tag picker's layout, recent tags and folders, the
//! keyboard preset and rebound shortcuts, the language and the audio
//! visualization. Production keeps these in the renderer's localStorage and,
//! for the main window's place, in the main process's `config.json`; both
//! are per machine. What production keeps in the library's settings stays in
//! the settings model (`settings::model`), which travels with the library.
//!
//! The state lives in one JSON file in this app's own folder (never the
//! Electron app's): the platform's configuration folder under
//! "Picto GPUI", or the folder `PICTO_GPUI_STATE` names. Tests and the
//! off-screen tool keep nothing unless `PICTO_GPUI_STATE` is set, so their
//! pictures never depend on how the user left the app.
//!
//! It is read once at start. Changes are written 250 ms after the last one,
//! off the drawing thread, through a temporary file renamed over the old
//! one, and at once when the app quits. A missing file gives the defaults;
//! a field that is missing or of the wrong kind gives that field's default
//! and leaves the others; an unreadable file is set aside as
//! `state.json.corrupt` and the defaults are used; fields written by a newer
//! version that this one does not know are kept as they were.
use gpui::{App, Global, Task};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// The version of the file's layout this build writes.
pub const VERSION: u64 = 1;
const FILE: &str = "state.json";
/// How long after the last change the file is written.
const SAVE_DELAY: Duration = Duration::from_millis(250);

/// The main window's smallest size: the `minWidth` and `minHeight` its
/// `BrowserWindow` is made with in `windowManager.mjs`. Production restores a
/// kept size down to 700×500 (`MAIN_WINDOW_MIN_WIDTH`, `_HEIGHT`), but the
/// window's own minimum then raises it to this, so this is the one limit.
pub const WINDOW_MIN_WIDTH: f32 = 1000.;
pub const WINDOW_MIN_HEIGHT: f32 = 600.;

/// A rectangle in global screen points.
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

/// The main window's place (`windowState.main`): its bounds when neither
/// maximised nor full screen, and whether it was maximised. Production does
/// not remember full screen; a window closed in full screen opens at its
/// normal bounds.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowPlace {
    /// Where it was; `None` centres it.
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub width: f32,
    pub height: f32,
    pub maximized: bool,
}

impl WindowPlace {
    /// `normalizeMainWindowState`: at least the minimum size, and placed
    /// where it was only if that still overlaps one of the displays' usable
    /// areas, else centred.
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

/// A binding the user changed in Settings (`ShortcutBindingOverride`):
/// either slot, empty when cleared.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Binding {
    pub keys: Option<String>,
    pub keys2: Option<String>,
}

/// Everything kept. Field names follow production's storage keys.
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    /// `config.json` `windowState.main`.
    pub window: Option<WindowPlace>,
    /// `picto-sidebar-width`, `picto-sidebar-collapsed-panel`.
    pub sidebar_width: f32,
    pub sidebar_collapsed: bool,
    /// `picto-inspector-width`, `picto-inspector-collapsed`.
    pub inspector_width: f32,
    pub inspector_collapsed: bool,
    /// `picto-sidebar-collapsed`: folded sections and nodes, by id.
    pub folded: Vec<String>,
    /// `picto:grid:pinned-filters`: the filter fields the bar shows while
    /// empty, by production's names (`color`, `tags`, …).
    pub pinned_filters: Vec<String>,
    /// `picto:grid:saved-filters`: name and filters, in the order saved.
    pub saved_filters: Vec<(String, Value)>,
    /// `picto-tag-picker-layout` is `list`.
    pub tag_picker_list: bool,
    /// `picto-tag-picker-counts`.
    pub tag_picker_counts: bool,
    /// `picto-recent-tags`, newest first.
    pub recent_tags: Vec<String>,
    /// `picto-recent-folders:<library>`, newest first, by library.
    pub recent_folders: BTreeMap<String, Vec<u32>>,
    /// `picto-keyboard-preset`.
    pub keyboard_preset: String,
    /// `picto-shortcut-overrides`, by shortcut id.
    pub shortcut_overrides: BTreeMap<String, Binding>,
    /// `picto:locale`.
    pub locale: String,
    /// `picto:audio-visualization`.
    pub audio_visualization: String,
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
            keyboard_preset: "us".into(),
            shortcut_overrides: BTreeMap::new(),
            locale: "en".into(),
            audio_visualization: "spectrum".into(),
        }
    }
}

/// `setSidebarWidthAtom`'s and `setInspectorWidthAtom`'s rounding and limits.
pub fn sidebar_width(width: f32) -> f32 {
    width
        .round()
        .clamp(crate::theme::SIDEBAR_MIN, crate::theme::SIDEBAR_MAX)
}

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
    /// Reads a file's contents, field by field: what is missing or not of
    /// its kind takes the default. Returns the state and the fields this
    /// version does not know, to be written back unchanged.
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
        // `loadPinnedFilters`: only the known fields.
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
        let extra = migrated
            .into_iter()
            .filter(|(key, _)| !KNOWN.contains(&key.as_str()))
            .collect();
        (state, extra)
    }

    /// The file's contents, with `extra` (fields of a newer version) kept.
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
        put("locale", json!(self.locale));
        put("audioVisualization", json!(self.audio_visualization));
        Value::Object(object)
    }

    /// Reads the file's text. `None` when it is not JSON at all.
    pub fn parse(text: &str) -> Option<(State, Map<String, Value>)> {
        let value: Value = serde_json::from_str(text).ok()?;
        value.is_object().then(|| State::from_value(&value))
    }
}

/// The fields this version reads and writes.
const KNOWN: [&str; 17] = [
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
    "keyboardPreset",
    "shortcutOverrides",
    "locale",
    "audioVisualization",
];

/// Brings a file written by an earlier version to this version's layout,
/// one version at a time. Version 1 is the first layout, so there is no
/// step yet; a file without a version stamp is read as version 1. A later
/// layout adds its step here, on the file's `version`.
fn migrate(object: Map<String, Value>) -> Map<String, Value> {
    object
}

/// This app's own folder for what it keeps per machine.
pub fn location() -> Option<PathBuf> {
    if let Some(folder) = crate::seams::state_folder() {
        return Some(folder);
    }
    if cfg!(test) {
        return None;
    }
    config_folder().map(|folder| folder.join("Picto GPUI"))
}

/// The platform's per-user configuration folder (`app.getPath('appData')`).
pub fn config_folder() -> Option<PathBuf> {
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
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

/// Reads the file in `folder`. A file that is not JSON is set aside so it is
/// not lost, and the defaults are used.
pub fn load(folder: &Path) -> (State, Map<String, Value>) {
    let path = folder.join(FILE);
    // Only a missing file is no state; one that cannot be read as text is
    // set aside below like any other unreadable file, rather than written
    // over by the next save.
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

/// The last write that reached the disk, so a slower earlier write never
/// replaces a later one.
static WRITTEN: Mutex<u64> = Mutex::new(0);

/// Writes `text` as the file in `folder` through a temporary file, unless a
/// later write has already landed. The temporary file reaches the disk
/// before it replaces the old one, so a crash or a power cut leaves the old
/// file or the new one, never a part of either; a write that fails removes
/// it.
fn write(folder: &Path, text: &str, sequence: u64) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut written = WRITTEN.lock().unwrap_or_else(|poison| poison.into_inner());
    if sequence < *written {
        return Ok(());
    }
    std::fs::create_dir_all(folder)?;
    let temporary = folder.join(format!("{FILE}.{}.tmp", std::process::id()));
    let replaced = std::fs::File::create(&temporary)
        .and_then(|mut file| {
            file.write_all(text.as_bytes())?;
            file.sync_all()
        })
        .and_then(|()| std::fs::rename(&temporary, folder.join(FILE)));
    if let Err(error) = replaced {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    *written = sequence;
    Ok(())
}

/// The library's name in the recent folders: the library the disposable
/// copy was made from, so the recents outlive the copy.
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
    /// What the file holds, to skip writing what is already there.
    on_disk: Value,
    sequence: u64,
    pending: Option<Task<()>>,
}

impl Global for Store {}

/// Reads the state from [`location`]. Called once, before anything reads it.
pub fn init(cx: &mut App) {
    let folder = location();
    let (state, extra) = folder.as_deref().map(load).unwrap_or_default();
    let on_disk = state.to_value(&extra);
    cx.set_global(Store {
        folder,
        state,
        extra,
        on_disk,
        sequence: 0,
        pending: None,
    });
}

/// The state as kept.
pub fn get(cx: &App) -> &State {
    &cx.global::<Store>().state
}

/// Changes the state; the file is written shortly after the last change.
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

/// Writes what changed now, as the app quits or a script ends.
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
            ..State::default()
        };
        state
            .recent_folders
            .insert("/Users/x/Main.library".into(), vec![4, 9]);
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
        }));
        let defaults = State::default();
        assert_eq!(state.sidebar_width, defaults.sidebar_width);
        assert_eq!(state.inspector_width, crate::theme::INSPECTOR_MAX);
        assert!(state.sidebar_collapsed);
        assert_eq!(state.pinned_filters, ["size"]);
        assert_eq!(state.recent_folders["a"], [1, 7]);
        assert_eq!(state.window, None);
        assert_eq!(state.locale, "en");
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
        // No file at all is the defaults too.
        assert_eq!(load(folder.path()).0, State::default());
        // A file that is not even text is set aside as well, not left to
        // be written over.
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
        // An earlier write finishing late is dropped.
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
        // On the second display: kept.
        let kept = place(2000., 100., 1200., 800.).on_displays(&displays);
        assert_eq!((kept.x, kept.y), (Some(2000.), Some(100.)));
        // Partly on a display: kept, as production keeps it.
        let edge = place(-1100., 10., 1200., 800.).on_displays(&displays);
        assert_eq!(edge.x, Some(-1100.));
        // On a display that is gone: centred.
        let gone = place(5000., 100., 1200., 800.).on_displays(&displays);
        assert_eq!((gone.x, gone.y), (None, None));
        // Too small: the minimum size.
        let small = place(10., 10., 300., 200.4).on_displays(&displays);
        assert_eq!((small.width, small.height), (1000., 600.));
        // No displays known: centred.
        assert_eq!(place(10., 10., 900., 600.).on_displays(&[]).x, None::<f32>);
    }
}
