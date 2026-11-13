//! The settings: one model the whole app reads, loaded from and saved to the
//! library as production stores them, and the Settings window that edits a
//! draft of it (`Settings.tsx`). The window's changes reach the app when they
//! are applied with Save & Close or Apply, as in production.
pub mod model;
mod panels;
pub mod registry;
mod shortcuts;
mod window;

use crate::session::Session;
use gpui::*;
use model::{AppSettings, GridDefaults};
use registry::{KeyboardPreset, Overrides};
use std::time::Duration;

actions!(settings, [OpenSettings]);

/// Everything the Settings window edits.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    /// The library's `setting` row `application`.
    pub app: AppSettings,
    /// The library's `view_pref` row `grid:defaults`.
    pub grid: GridDefaults,
    // TODO: production keeps these four in the renderer's localStorage
    // (`picto-keyboard-preset`, `picto:locale`, `picto:audio-visualization`,
    // `picto-shortcut-overrides`); persist them once the app has storage of
    // its own. Until then they last for the session.
    pub keyboard: KeyboardPreset,
    pub locale: &'static str,
    pub audio_visualization: &'static str,
    pub shortcuts: Overrides,
}

impl Settings {
    fn new() -> Self {
        Self {
            locale: "en",
            audio_visualization: "spectrum",
            ..Self::default()
        }
    }
}

/// The settings as applied, and the draft the window edits.
pub struct SettingsModel {
    pub(super) session: Entity<Session>,
    /// What the app shows.
    pub saved: Settings,
    /// What the Settings window shows; applied by Save & Close or Apply.
    pub draft: Settings,
    /// "Reset all views" was pressed; the saved views are cleared on apply.
    pub reset_views: bool,
    /// Set 160 ms into a save that has not finished, for "Saving…".
    pub saving: bool,
    in_flight: bool,
    /// Why the last save failed.
    pub error: Option<String>,
}

/// The saved settings changed; views that follow a setting repaint.
pub struct Applied;

impl EventEmitter<Applied> for SettingsModel {}

impl SettingsModel {
    fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        let rows = session.read(cx).run(|library| {
            let read = |table: &str, key: &str| {
                library
                    .read_auxiliary_json(table, key)
                    .ok()
                    .flatten()
                    .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
                    .unwrap_or(serde_json::Value::Null)
            };
            (
                read("setting", "application"),
                read("view_pref", "grid:defaults"),
            )
        });
        cx.spawn(async move |this, cx| {
            if let Ok((app, grid)) = rows.await {
                let _ = this.update(cx, |this, cx| {
                    this.saved.app = AppSettings::from_json(&app);
                    this.saved.grid = GridDefaults::from_json(&grid);
                    this.draft = this.saved.clone();
                    cx.emit(Applied);
                    cx.notify();
                });
            }
        })
        .detach();
        Self {
            session,
            saved: Settings::new(),
            draft: Settings::new(),
            reset_views: false,
            saving: false,
            in_flight: false,
            error: None,
        }
    }

    /// Starts editing afresh from what is saved.
    fn begin(&mut self) {
        self.draft = self.saved.clone();
        self.reset_views = false;
        self.error = None;
    }

    /// Changes the draft.
    pub fn edit(&mut self, change: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        change(&mut self.draft);
        cx.notify();
    }

    /// Writes the draft to the library (`settings.replace`, then
    /// `settings.view.patch` for the grid defaults and `settings.view.reset`
    /// if asked), then makes it what the app shows. Resolves to whether it
    /// was saved.
    pub fn save(&mut self, cx: &mut Context<Self>) -> Task<bool> {
        if self.in_flight {
            return Task::ready(false);
        }
        self.in_flight = true;
        self.error = None;
        let draft = self.draft.clone();
        let reset = self.reset_views;
        let app = draft.app.to_json().to_string();
        let patch = draft.grid.to_patch();
        let written = self.session.read(cx).run(move |library| {
            library.replace_auxiliary_json(
                "settings.replace",
                "Replace settings",
                "setting",
                "application",
                Some(app),
            )?;
            library.update_auxiliary_json(
                "settings.view.patch",
                "Change view preferences",
                "view_pref",
                "grid:defaults",
                false,
                move |stored| Ok(Some(model::patched(stored, &patch).to_string())),
            )?;
            if reset {
                library.auxiliary_write(
                    picto_library::database::WorkPriority::ForegroundMutation,
                    ["settings".to_string()],
                    [],
                    |transaction, _| {
                        transaction
                            .execute("DELETE FROM view_pref WHERE scope <> 'grid:defaults'", [])?;
                        Ok(())
                    },
                )?;
            }
            Ok::<(), picto_library::LibraryError>(())
        });
        // "Saving…" shows only for a save that takes longer than 160 ms.
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(160))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.in_flight {
                    this.saving = true;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let result = written.await;
            this.update(cx, |this, cx| {
                this.in_flight = false;
                this.saving = false;
                let saved = match result {
                    Ok(Ok(())) => true,
                    Ok(Err(error)) => {
                        this.error = Some(error.to_string());
                        false
                    }
                    Err(_) => {
                        this.error = Some("Unable to save settings.".into());
                        false
                    }
                };
                if saved {
                    this.saved = draft;
                    this.reset_views = false;
                    // The grid reads its saved view again.
                    this.session.update(cx, |session, cx| session.reload(cx));
                    cx.emit(Applied);
                }
                cx.notify();
                saved
            })
            .unwrap_or(false)
        })
    }
}

struct SettingsGlobal {
    model: Entity<SettingsModel>,
    window: Option<WindowHandle<window::SettingsWindow>>,
}

impl Global for SettingsGlobal {}

/// Creates the settings model and starts loading what the library holds.
/// Views that follow a setting read it with [`model`].
pub fn init(session: Entity<Session>, cx: &mut App) {
    let model = cx.new(|cx| SettingsModel::new(session, cx));
    cx.set_global(SettingsGlobal {
        model,
        window: None,
    });
}

/// The app's settings model.
pub fn model(cx: &App) -> Entity<SettingsModel> {
    cx.global::<SettingsGlobal>().model.clone()
}

/// The applied settings.
pub fn current(cx: &App) -> &AppSettings {
    &cx.global::<SettingsGlobal>().model.read(cx).saved.app
}

/// Opens the Settings window, or brings it forward if it is open
/// (`openSettingsWindow`).
pub fn open(cx: &mut App) {
    if !cx.has_global::<SettingsGlobal>() {
        return;
    }
    if let Some(handle) = cx.global::<SettingsGlobal>().window
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    let model = model(cx);
    model.update(cx, |model, _| model.begin());
    let handle = window::open(model, cx);
    cx.global_mut::<SettingsGlobal>().window = handle;
}
