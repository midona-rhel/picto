//! This module holds the app's settings. `SettingsModel` is the one model of
//! the settings, and the whole app reads it. The application settings and
//! the grid's default view are loaded from the library and saved to it, in
//! the rows where production stores them. The Settings window, which
//! corresponds to production's `Settings.tsx`, edits a draft of the model.
//! The draft reaches the rest of the app when the user applies it with the
//! Save & Close button or the Apply button, as in production. Three choices
//! are shown before that. A language is shown and stored as soon as it is
//! chosen (`SettingsModel::choose_locale`). A theme and the frosted material
//! of the sidebar and the inspector are shown as soon as they are chosen,
//! and closing the window without saving puts the saved ones back.
pub mod model;
mod panels;
mod pieces;
pub mod registry;
mod shortcuts;
mod window;

use crate::i18n::t;
use crate::session::Session;
use gpui::*;
use model::{AppSettings, GridDefaults};
use registry::{KeyboardPreset, Overrides};

actions!(settings, [OpenSettings]);

/// The settings that the Settings window edits. `SettingsModel` holds two
/// values of this type: the settings as applied, and the window's draft.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    /// The application settings. They are stored in the `application` row
    /// of the library's `setting` table.
    pub app: AppSettings,
    /// The grid's default view. It is stored in the `grid:defaults` row of
    /// the library's `view_pref` table.
    pub grid: GridDefaults,
    // Production keeps the next four fields in the renderer's localStorage,
    // under the keys `picto-keyboard-preset`, `picto:locale`,
    // `picto:audio-visualization` and `picto-shortcut-overrides`. What is in
    // localStorage belongs to one machine, so this app keeps the four in its
    // own state (`crate::state`) and not in the library.
    pub keyboard: KeyboardPreset,
    pub locale: &'static str,
    pub audio_visualization: &'static str,
    pub shortcuts: Overrides,
}

/// The values that the audio visualization setting can have. They are the
/// values of production's `AUDIO_VISUALIZATION_OPTIONS`.
const VISUALIZATIONS: [&str; 4] = ["none", "spectrum", "oscilloscope", "orbit"];

impl Settings {
    /// Returns the settings as they are before the library's settings have
    /// been read. The four settings that are kept for this machine come from
    /// the app's state, and every other setting has its default.
    fn new(cx: &App) -> Self {
        let (keyboard, shortcuts) = keyboard(cx);
        let state = crate::state::get(cx);
        Self {
            keyboard,
            shortcuts,
            // The language that the app is showing. It is the language the
            // user chose or, if the user has not chosen one, the system's
            // language.
            locale: crate::i18n::current().code(),
            audio_visualization: VISUALIZATIONS
                .into_iter()
                .find(|mode| *mode == state.audio_visualization)
                .unwrap_or("spectrum"),
            ..Self::default()
        }
    }

    /// Stores the keyboard preset, the rebound shortcuts and the audio
    /// visualization in the app's state (`crate::state`), which is kept for
    /// this machine and not in the library.
    fn keep(&self, cx: &mut App) {
        let overrides = self
            .shortcuts
            .iter()
            .map(|(id, binding)| {
                (
                    (*id).to_owned(),
                    crate::state::Binding {
                        keys: binding.keys.clone(),
                        keys2: binding.keys2.clone(),
                    },
                )
            })
            .collect();
        // The language is not stored here. `SettingsModel::choose_locale`
        // stores it in the app's state when the user chooses it.
        let (preset, mode) = (self.keyboard.value(), self.audio_visualization);
        crate::state::update(cx, |state| {
            state.keyboard_preset = preset.into();
            state.shortcut_overrides = overrides;
            state.audio_visualization = mode.into();
        });
    }
}

/// Returns the keyboard preset and the rebound shortcuts that are stored in
/// the app's state. Reading the rebound shortcuts corresponds to
/// production's `loadOverrides`. A binding of a shortcut that is no longer
/// in `registry::SHORTCUT_DEFS` is left out.
pub fn keyboard(cx: &App) -> (KeyboardPreset, Overrides) {
    let state = crate::state::get(cx);
    let overrides = state
        .shortcut_overrides
        .iter()
        .filter_map(|(id, binding)| {
            let def = registry::SHORTCUT_DEFS.iter().find(|def| def.id == id)?;
            Some((
                def.id,
                registry::Override {
                    keys: binding.keys.clone(),
                    keys2: binding.keys2.clone(),
                },
            ))
        })
        .collect();
    (
        KeyboardPreset::from_value(&state.keyboard_preset),
        overrides,
    )
}

/// The app's settings model. It holds the settings as applied and the draft
/// that the Settings window edits, and it reads the library's settings and
/// writes them.
pub struct SettingsModel {
    pub(super) session: Entity<Session>,
    /// The settings as applied. The app outside the Settings window follows
    /// these.
    pub saved: Settings,
    /// The draft, which is what the Settings window shows and edits. The
    /// Save & Close button and the Apply button write it to the library and
    /// then copy it to `saved`.
    pub draft: Settings,
    /// Whether the user has pressed "Reset all views" in this draft. When
    /// it is `true`, the next save also clears the views that the library
    /// has saved (`settings.view.reset`).
    pub reset_views: bool,
    /// Whether a save has run for 160 ms without finishing. While it is
    /// `true`, the Save & Close button reads "Saving…" and both buttons are
    /// disabled.
    pub saving: bool,
    in_flight: bool,
    /// The error message of a write to the library that failed, or `None`.
    /// It is cleared when a save starts and when the draft is started again
    /// (`begin`). The Settings window shows it to the left of its Save &
    /// Close button.
    pub error: Option<String>,
}

/// The event that `SettingsModel` emits when the applied settings have
/// changed. Views that follow a setting subscribe to it and draw again.
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
            saved: Settings::new(cx),
            draft: Settings::new(cx),
            reset_views: false,
            saving: false,
            in_flight: false,
            error: None,
        }
    }

    /// Makes the draft a copy of the applied settings again, sets
    /// `reset_views` to `false` and clears the error message. It is called
    /// when the Settings window opens and when the window closes without
    /// saving.
    fn begin(&mut self) {
        self.draft = self.saved.clone();
        self.reset_views = false;
        self.error = None;
    }

    /// Applies `change` to the draft and notifies the model's observers, so
    /// that the Settings window draws again.
    pub fn edit(&mut self, change: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        change(&mut self.draft);
        cx.notify();
    }

    /// Shows the language with the code `code` in every window at once and
    /// stores the choice in the app's state, whether or not the rest of the
    /// draft is saved. The Settings window calls this when the user chooses
    /// a language. It corresponds to production's `setLocale`. It does
    /// nothing when `code` is not the code of a language the app has.
    pub fn choose_locale(&mut self, code: &'static str, cx: &mut Context<Self>) {
        let Some(locale) = crate::i18n::Locale::normalize(code) else {
            return;
        };
        self.draft.locale = locale.code();
        self.saved.locale = locale.code();
        crate::state::update(cx, |state| state.locale = locale.code().into());
        crate::i18n::show(locale, cx);
        // The names of the sidebar's destinations were translated into the
        // previous language when they were read, so they are read again.
        self.session
            .update(cx, |session, cx| session.refresh_navigation(cx));
        cx.notify();
    }

    /// Changes an applied setting at once, without the Settings window. The
    /// sidebar's menus change settings this way, for example. It corresponds
    /// to production's `settingsController.patchSettings`. `change` is made
    /// to the applied settings and to the draft, so an open Settings window
    /// shows the same change. The fields that changed are written to the
    /// library with the command `settings.patch`, and `Applied` is emitted
    /// so that the app follows the change.
    pub fn apply_now(&mut self, change: impl Fn(&mut AppSettings), cx: &mut Context<Self>) {
        self.apply_now_reporting(change, None, cx);
    }

    /// Does what [`SettingsModel::apply_now`] does. If the write to the
    /// library fails and `failure` is given, it also shows an error
    /// notification whose title is `failure`, as production's callers of
    /// `patchSettings` do, for example "Unable to update Quick Access".
    pub fn apply_now_reporting(
        &mut self,
        change: impl Fn(&mut AppSettings),
        failure: Option<&'static str>,
        cx: &mut Context<Self>,
    ) {
        let before = self.saved.app.to_json();
        change(&mut self.saved.app);
        change(&mut self.draft.app);
        let after = self.saved.app.to_json();
        let patch = after
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(key, value)| before.get(key.as_str()) != Some(*value))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<serde_json::Map<_, _>>();
        if patch.is_empty() {
            return;
        }
        let written = self.session.read(cx).run(move |library| {
            library.update_auxiliary_json(
                "settings.patch",
                "Change settings",
                "setting",
                "application",
                true,
                move |stored| Ok(Some(model::patched(stored, &patch).to_string())),
            )
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Err(error)) = written.await {
                let _ = this.update(cx, |this, cx| {
                    if let Some(title) = failure {
                        crate::ui::notify::error(title, error.to_string(), cx);
                    }
                    this.error = Some(error.to_string());
                    cx.notify();
                });
            }
        })
        .detach();
        cx.emit(Applied);
        cx.notify();
    }

    /// Writes the draft to the library and then makes it the applied
    /// settings. The application settings are written with the command
    /// `settings.replace`, and after them the grid's default view with
    /// `settings.view.patch`. If "Reset all views" was pressed,
    /// `settings.view.reset` is sent last. The returned task resolves to
    /// `true` when all of it was saved, and to `false` when a write failed
    /// or another save was still running.
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
            Ok::<(), picto_library::LibraryError>(())
        });
        // "Saving…" shows only for a save that takes longer than this.
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(crate::motion::SAVING_MESSAGE_DELAY)
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
            let mut result = match written.await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(error)) => Err(error.to_string()),
                Err(_) => Err(t("Unable to save settings.").to_string()),
            };
            if reset && result.is_ok() {
                let answer = this.update(cx, |this, cx| {
                    this.session
                        .read(cx)
                        .dispatch("settings.view.reset", serde_json::json!({}))
                });
                result = match answer {
                    Ok(answer) => match answer.await {
                        Ok(Ok(_)) => Ok(()),
                        Ok(Err(error)) => Err(error),
                        Err(_) => Err(t("Unable to save settings.").to_string()),
                    },
                    Err(_) => return false,
                };
            }
            this.update(cx, |this, cx| {
                this.in_flight = false;
                this.saving = false;
                let saved = match result {
                    Ok(()) => true,
                    Err(error) => {
                        this.error = Some(error);
                        false
                    }
                };
                if saved {
                    this.saved = draft;
                    this.reset_views = false;
                    // The grid reads its saved view again, in place: the
                    // selection and the scroll stay.
                    this.session
                        .update(cx, |session, cx| session.refresh_results(cx));
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
    // Each time the applied settings change, the keys are bound again from
    // the applied keyboard preset and shortcut edits, every window takes the
    // applied theme, and the main window takes the applied choice for the
    // frosted material of the sidebar and the inspector. The settings that
    // are kept for this machine are also stored in the app's state then.
    cx.subscribe(&model, |model, _: &Applied, cx| {
        let saved = model.read(cx).saved.clone();
        saved.keep(cx);
        let (preset, overrides, scheme) = (
            saved.keyboard,
            saved.shortcuts.clone(),
            saved.app.color_scheme.clone(),
        );
        crate::keymap::install(preset, &overrides, cx);
        crate::theme::show(&scheme, cx);
        crate::ui::window_chrome::want_rail_material(saved.app.rail_material(), cx);
        // The applied settings also decide whether notifications pop up
        // and which tones of notification do. This corresponds to
        // production's `configureNotificationPopups`.
        let (enabled, tones) = {
            let app = &model.read(cx).saved.app;
            (
                app.notification_popups_enabled,
                app.notification_popup_tones.clone(),
            )
        };
        crate::ui::notify::configure(enabled, tones, cx);
    })
    .detach();
    cx.set_global(SettingsGlobal {
        model,
        window: None,
    });
}

/// Returns the app's settings model.
pub fn model(cx: &App) -> Entity<SettingsModel> {
    cx.global::<SettingsGlobal>().model.clone()
}

/// Returns the application settings as applied.
pub fn current(cx: &App) -> &AppSettings {
    &cx.global::<SettingsGlobal>().model.read(cx).saved.app
}

/// Opens the Settings window, or brings it forward if it is open. It
/// corresponds to production's `openSettingsWindow`.
pub fn open(cx: &mut App) {
    if !cx.has_global::<SettingsGlobal>() {
        return;
    }
    if let Some(handle) = cx.global::<SettingsGlobal>().window
        && handle
            .update(cx, |_, window, _| {
                crate::ui::window_chrome::activate(window)
            })
            .is_ok()
    {
        return;
    }
    let model = model(cx);
    model.update(cx, |model, _| model.begin());
    let handle = window::open(model, cx);
    cx.global_mut::<SettingsGlobal>().window = handle;
}

/// Opens the Settings window showing the section with the id `panel`. If
/// the window is already open, it is brought forward and changes to that
/// section. It corresponds to production's `openSettingsWindow(panel)`. The
/// About command of the app's menu opens the section `about` this way.
pub fn open_at(panel: &str, cx: &mut App) {
    open(cx);
    if let Some(handle) = cx
        .try_global::<SettingsGlobal>()
        .and_then(|settings| settings.window)
    {
        let _ = handle.update(cx, |settings, _, cx| settings.show_panel(panel, cx));
    }
}

/// Returns whether `window` is the Settings window.
pub fn is_window(window: AnyWindowHandle, cx: &App) -> bool {
    cx.try_global::<SettingsGlobal>()
        .and_then(|settings| settings.window)
        .is_some_and(|handle| AnyWindowHandle::from(handle) == window)
}

/// Closes `window` if it is the Settings window, in the way the window's
/// own close button does: the draft is discarded (`SettingsWindow::close`).
/// Returns whether `window` was the Settings window.
pub fn close_window(window: AnyWindowHandle, cx: &mut App) -> bool {
    if !is_window(window, cx) {
        return false;
    }
    if let Some(handle) = window.downcast::<window::SettingsWindow>() {
        let _ = handle.update(cx, |settings, window, cx| settings.close(window, cx));
    }
    true
}
