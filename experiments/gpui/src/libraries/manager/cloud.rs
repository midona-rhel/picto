//! The Library Manager's cloud parts (`LibraryManager.tsx`): the Cloud Sync
//! card of the open library, the cloud rows of its actions menu, the sync
//! dot on its row, Connect Google Drive… and Connect Dropbox…, and Open from
//! Cloud…. Detecting, validating and discovering provider folders need no
//! library; everything else is a command of the open library's application.
use super::{Manager, Pane, host};
use crate::cloud::{self, Choice, Root, Tone};
use crate::i18n::{en, t};
use crate::theme::Text as _;
use crate::ui::menu;
use crate::{theme, ui};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A tone's colour (`.syncState_*`).
fn tone_color(tone: Tone, theme: &theme::Theme) -> Rgba {
    match tone {
        Tone::Positive => rgb(theme::SUCCESS),
        Tone::Neutral => theme.text_secondary(),
        Tone::Warning => rgb(theme::WARNING),
        Tone::Negative => theme.negative(),
    }
}

/// `.syncDot`: 7px, in the tone's colour.
fn dot(color: Rgba) -> Div {
    div().size(px(7.)).flex_none().rounded_full().bg(color)
}

impl Manager {
    /// Follows the open library's cloud state, or lets go of it.
    pub(super) fn follow_cloud(&mut self, cx: &mut Context<Self>) {
        let cloud = host::shell(cx).map(|shell| shell.read(cx).cloud().clone());
        match (cloud, &self.cloud) {
            (Some(cloud), Some((followed, _))) if cloud == *followed => {}
            (Some(cloud), _) => {
                let subscription = cx.observe(&cloud, |this, _, cx| this.cloud_changed(cx));
                self.cloud = Some((cloud, subscription));
                self.cloud_changed(cx);
            }
            (None, _) => self.cloud = None,
        }
    }

    /// The state changed: the time estimate follows.
    fn cloud_changed(&mut self, cx: &mut Context<Self>) {
        let presentation = cloud::present(self.status(cx).as_ref());
        self.eta = cloud::track(
            &mut self.samples,
            &mut self.work_key,
            &presentation,
            std::time::Instant::now(),
        );
        cx.notify();
    }

    fn status(&self, cx: &App) -> Option<cloud::Status> {
        self.cloud
            .as_ref()
            .and_then(|(cloud, _)| cloud.read(cx).status.clone())
    }

    fn configuration(&self, cx: &App) -> Option<cloud::Configuration> {
        self.cloud
            .as_ref()
            .and_then(|(cloud, _)| cloud.read(cx).configuration.clone())
    }

    /// The folder the open library syncs to, if it does.
    fn cloud_root(&self, cx: &App) -> Option<String> {
        self.configuration(cx)?.root_path
    }

    /// `refreshCloud`: the folders detected on this device, then those the
    /// user chose that are still provider folders, and the libraries in
    /// each, read off the thread that draws.
    pub(super) fn refresh_cloud(&mut self, cx: &mut Context<Self>) {
        let remembered: Vec<Root> = crate::state::get(cx)
            .cloud_locations
            .values()
            .cloned()
            .collect();
        let found = cx.background_executor().spawn(async move {
            let mut roots = cloud::detect();
            for root in remembered {
                if let Ok(valid) = cloud::validate(&root.provider, Path::new(&root.path))
                    && !roots.iter().any(|known| known.path == valid.path)
                {
                    roots.push(valid);
                }
            }
            let choices: Vec<Choice> = roots
                .iter()
                .flat_map(|root| cloud::discover(root).unwrap_or_default())
                .collect();
            (roots, choices)
        });
        cx.spawn(async move |this, cx| {
            let (roots, choices) = found.await;
            let _ = this.update(cx, |this, cx| {
                let chosen = this
                    .chosen
                    .and_then(|index| this.choices.get(index))
                    .map(|choice| (choice.root.path.clone(), choice.library.library_id.clone()));
                this.roots = roots;
                this.choices = choices;
                this.chosen = chosen.and_then(|(path, id)| {
                    this.choices.iter().position(|choice| {
                        choice.root.path == path && choice.library.library_id == id
                    })
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// `rememberCloudRoot`: kept as this provider's folder.
    fn remember(root: &Root, cx: &mut App) {
        crate::state::update(cx, |state| {
            state
                .cloud_locations
                .insert(root.provider.clone(), root.clone());
        });
    }

    /// Sends a cloud command for the open library while nothing else runs,
    /// then shows `success` or the error, and reads the folders again.
    fn cloud_command(
        &mut self,
        key: &'static str,
        command: &'static str,
        args: serde_json::Value,
        success: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(cloud) = self.cloud.as_ref().map(|(cloud, _)| cloud.clone()) else {
            return;
        };
        if self.working.is_some() {
            return;
        }
        self.working = Some(key);
        self.error = None;
        self.message = None;
        cx.notify();
        let manager = cx.entity().downgrade();
        cloud.update(cx, |cloud, cx| {
            cloud.run(command, args, cx, move |result, cx| {
                let _ = manager.update(cx, |this, cx| {
                    this.working = None;
                    match result {
                        Ok(_) => this.message = success,
                        Err(error) => this.error = Some(error),
                    }
                    this.refresh_cloud(cx);
                    cx.notify();
                });
            })
        });
    }

    /// `configureCloud`: the open library syncs through `root`.
    fn configure(&mut self, root: Root, cx: &mut Context<Self>) {
        Self::remember(&root, cx);
        let success = crate::tf!(
            "Cloud sync enabled through {value0}.",
            cloud::provider_name(&root.provider)
        );
        self.cloud_command(
            "cloud",
            "cloud.configure",
            serde_json::json!({
                "provider": root.provider,
                "account_label": root.account_label,
                "root_path": root.path,
            }),
            Some(success),
            cx,
        );
    }

    /// `chooseCloudRoot`: Connect Google Drive… or Connect Dropbox…. The
    /// folder chosen must be the root of that provider's installed app; it
    /// is kept, and the open library syncs through it if it does not sync
    /// yet and is the one chosen in the list. With no library chosen, Open
    /// from Cloud… shows what is in it.
    fn choose_root(&mut self, provider: &'static str, cx: &mut Context<Self>) {
        if self.working.is_some() {
            return;
        }
        let name = cloud::provider_name(provider);
        let chosen = crate::prompt::folder(
            &crate::tf!("Choose the {value0} synced folder", name),
            cx,
        );
        self.error = None;
        self.message = None;
        cx.spawn(async move |this, cx| {
            let Ok(Some(folder)) = chosen.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.working = Some("cloud-location");
                cx.notify();
            });
            let checked = cx
                .background_executor()
                .spawn(async move {
                    let root = cloud::validate(provider, &folder)?;
                    cloud::discover(&root)?;
                    Ok::<_, String>(root)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.working = None;
                let root = match checked {
                    Ok(root) => root,
                    Err(error) => {
                        this.error = Some(error);
                        cx.notify();
                        return;
                    }
                };
                Self::remember(&root, cx);
                let current = this.selected_row().is_some_and(|row| row.current);
                if current && this.cloud.is_some() && this.cloud_root(cx).is_none() {
                    this.configure(root, cx);
                    return;
                }
                if this.selected_row().is_none() {
                    this.pane = Pane::Cloud;
                }
                this.message = Some(crate::tf!("{value0} location saved.", name));
                this.refresh_cloud(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// The open library's cloud rows in the actions menu.
    pub(super) fn cloud_rows(&self, cx: &App) -> Vec<menu::Entry> {
        let busy = self.working.is_some();
        if self.cloud_root(cx).is_some() {
            let paused = self
                .status(cx)
                .is_some_and(|status| status.state == "paused");
            let active = cloud::present(self.status(cx).as_ref()).active;
            return vec![
                menu::item("cloud-sync", t("Sync Now"))
                    .icon("menu-refresh")
                    .disabled(busy || active)
                    .into(),
                menu::item(
                    "cloud-pause",
                    if paused {
                        t("Resume Sync")
                    } else {
                        t("Pause Sync")
                    },
                )
                .icon(if paused {
                    "menu-player-play"
                } else {
                    "menu-player-pause"
                })
                .disabled(busy)
                .into(),
                menu::item("cloud-snapshot", t("Create Recovery Snapshot"))
                    .icon("menu-cloud")
                    .disabled(busy || active)
                    .into(),
                menu::item("cloud-folder", t("Show Cloud Folder"))
                    .icon("menu-folder-open")
                    .into(),
                menu::item("cloud-stop", t("Stop Syncing"))
                    .icon("menu-cloud-off")
                    .disabled(busy)
                    .into(),
            ];
        }
        if self.roots.is_empty() || self.cloud.is_none() {
            return Vec::new();
        }
        // The actions are told apart by the folder's place in the list.
        let children = self
            .roots
            .iter()
            .enumerate()
            .take(ENABLE.len())
            .map(|(index, root)| {
                menu::item(
                    ENABLE[index],
                    crate::tf!(
                        "{value0} · {value1}",
                        cloud::provider_name(&root.provider),
                        root.account_label
                    ),
                )
                .into()
            })
            .collect();
        vec![
            menu::submenu(t("Enable Cloud Sync"), children)
                .icon("menu-cloud")
                .into(),
        ]
    }

    /// A cloud row of the actions menu was chosen.
    pub(super) fn cloud_action(&mut self, action: &str, cx: &mut Context<Self>) {
        match action {
            "cloud-sync" => self.cloud_command(
                "cloud-sync",
                "cloud.reconcile",
                serde_json::json!({}),
                None,
                cx,
            ),
            "cloud-pause" => {
                let paused = self
                    .status(cx)
                    .is_some_and(|status| status.state == "paused");
                self.cloud_command(
                    "cloud-pause",
                    "cloud.pause",
                    serde_json::json!({ "paused": !paused }),
                    None,
                    cx,
                )
            }
            "cloud-snapshot" => self.cloud_command(
                "cloud-snapshot",
                "cloud.snapshot.create",
                serde_json::json!({}),
                Some(t("Recovery snapshot created.").to_owned()),
                cx,
            ),
            "cloud-folder" => {
                if let Some(root) = self.cloud_root(cx) {
                    let root = PathBuf::from(root);
                    cx.defer(move |cx| crate::desktop::reveal_cloud_folder(&root, cx));
                }
            }
            "cloud-stop" => {
                self.stopping.open((), ui::menu::Return::Opener, cx);
            }
            action => {
                if let Some(root) = ENABLE
                    .iter()
                    .position(|name| *name == action)
                    .and_then(|index| self.roots.get(index).cloned())
                {
                    self.configure(root, cx);
                }
            }
        }
    }

    /// Stop Syncing's question (`window.confirm`).
    pub(super) fn stop_question(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.stopping.layer(window, cx)?;
        let (presence, focus) = (dialog.presence, dialog.focus.clone());
        let close = cx.listener(|this: &mut Self, _: &(), _, cx| this.stopping.dismiss(cx));
        let cancel = cx.listener(|this: &mut Self, _: &(), _, cx| {
            this.stopping.close(cx);
        });
        let accept = cx.listener(|this: &mut Self, _: &(), _, cx| {
            this.stopping.dismiss(cx);
            this.cloud_command(
                "cloud-disable",
                "cloud.disable",
                serde_json::json!({}),
                Some(t("Cloud sync stopped.").to_owned()),
                cx,
            );
        });
        Some(
            ui::confirm::confirm(
                "library-cloud-stop",
                t("Stop Syncing"),
                t("Stop syncing this library?\n\nLocal files and the existing cloud copy will not be deleted."),
                t("Stop Syncing"),
                presence,
            )
            .danger(true)
            .focus(&focus)
            .on_close(move |window, cx| close(&(), window, cx))
            .on_cancel(move |window, cx| cancel(&(), window, cx))
            .on_confirm(move |window, cx| accept(&(), window, cx))
            .into_any_element(),
        )
    }

    /// The dot on the open library's row while it syncs to a folder.
    pub(super) fn row_sync_dot(&self, cx: &App) -> Option<AnyElement> {
        self.cloud_root(cx)?;
        let theme = theme::theme(cx);
        let presentation = cloud::present(self.status(cx).as_ref());
        let label = crate::tf!("Cloud sync: {value0}", presentation.label);
        Some(
            div()
                .id("library-row-sync")
                .role(Role::Image)
                .aria_label(label)
                .flex()
                .items_center()
                .child(dot(tone_color(presentation.tone, &theme)))
                .into_any_element(),
        )
    }

    /// The Cloud Sync card of the open library: its sync, or the folders
    /// it can sync through.
    pub(super) fn cloud_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = theme::theme(cx);
        let Some(configuration) = self
            .configuration(cx)
            .filter(|configuration| configuration.root_path.is_some())
        else {
            return self.connect_buttons("library-cloud", cx).into_any_element();
        };
        let status = self.status(cx);
        let presentation = cloud::present(status.as_ref());
        let label = |text: &'static str| {
            div()
                .text_color(theme.text_secondary())
                .child(t(text))
                .into_any_element()
        };
        let text = |text: String| div().min_w_0().truncate().child(text).into_any_element();
        let pending = |count: i64| format!("{count} {}", t("pending"));
        let files = status.as_ref().map_or(0, |status| status.pending_blobs);
        let missing = status.as_ref().map_or(0, |status| status.missing_blobs);
        let mut files_text = pending(files);
        if missing > 0 {
            files_text.push_str(&crate::tf!(" · {value0} unavailable", missing));
        }
        let color = tone_color(presentation.tone, &theme);
        // i18n: translated where drawn
        let cells = [
            (
                label("Provider"),
                text(format!(
                    "{} · {}",
                    cloud::provider_name(configuration.provider.as_deref().unwrap_or("")),
                    configuration.account_label.clone().unwrap_or_default()
                )),
            ),
            (
                label("State"),
                div()
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .text_color(color)
                    .child(dot(color))
                    .child(presentation.label)
                    .into_any_element(),
            ),
            (
                label("Last sync"),
                text(cloud::last_sync(
                    status.as_ref().and_then(|status| status.last_sync_at.as_deref()),
                    chrono::Utc::now(),
                )),
            ),
            (
                label("Changes"),
                text(pending(status.as_ref().map_or(0, |status| status.pending_mutations))),
            ),
            (label("Files"), text(files_text)),
        ];
        let mut summary = div().flex().flex_col().gap(px(8.)).text(theme::TEXT_SM);
        for (name, value) in cells {
            summary = summary.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(div().w(px(72.)).flex_none().child(name))
                    .child(div().flex_1().min_w_0().child(value)),
            );
        }
        let message = status
            .as_ref()
            .map(|status| status.message.clone())
            .unwrap_or_default();
        let after = if presentation.active {
            let doing = if message.is_empty() {
                // Production writes these without a key.
                en("Syncing library changes").to_owned()
            } else {
                message
            };
            let remaining = cloud::remaining_time(self.eta).unwrap_or_else(|| {
                match presentation.total {
                    Some(total) => format!("{} of {total}", presentation.completed),
                    None => en("Estimating time remaining…").to_owned(),
                }
            });
            let mut bar = ui::progress_bar().height(3.);
            if let Some(total) = presentation.total {
                bar = bar.filled(
                    "library-cloud-progress",
                    ui::progress_bar::share(presentation.completed, total),
                );
            }
            Some(
                div()
                    .mt(px(13.))
                    .flex()
                    .flex_col()
                    .gap(px(7.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(16.))
                            .text(theme::TEXT_CAPTION)
                            .text_color(theme.text_secondary())
                            .child(div().min_w_0().truncate().child(doing))
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(theme.text_tertiary())
                                    .child(remaining),
                            ),
                    )
                    .child(bar)
                    .into_any_element(),
            )
        } else if !message.is_empty() {
            Some(
                div()
                    .mt(px(12.))
                    .text(theme::TEXT_CAPTION)
                    .text_color(theme.text_secondary())
                    .child(message)
                    .into_any_element(),
            )
        } else {
            None
        };
        div()
            .flex()
            .flex_col()
            .child(summary)
            .children(after)
            .into_any_element()
    }

    /// `.cloudProviderActions`: for the open library, Synchronize Library
    /// with each folder found; then Connect Google Drive… and Connect
    /// Dropbox….
    pub(super) fn connect_buttons(&self, id: &'static str, cx: &mut Context<Self>) -> Div {
        let busy = self.busy || self.working.is_some();
        let mut actions = div().w_full().flex().flex_col().gap(px(8.));
        let button = |suffix: SharedString, icon: &'static str| {
            ui::action_button::action_button(
                ElementId::NamedChild(std::sync::Arc::new(id.into()), suffix),
                ui::action_button::Look::Standard,
            )
            .icon(icon, 18.)
            .full_width()
            .height(38.)
            .padding(12.)
            .gap(8.)
            .disabled(busy)
        };
        if id == "library-cloud" && self.cloud.is_some() {
            for (index, root) in self.roots.iter().enumerate() {
                let chosen = root.clone();
                actions = actions.child(
                    button(
                        format!("sync-{index}").into(),
                        cloud::provider_icon(&root.provider),
                    )
                    .label(format!(
                        "{}{}",
                        t("Synchronize Library with "),
                        cloud::provider_name(&root.provider)
                    ))
                    .caption(root.account_label.clone())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.configure(chosen.clone(), cx)
                    })),
                );
            }
        }
        actions
            .child(
                button("drive".into(), "cloud-brand-google-drive")
                    .label(t("Connect Google Drive…"))
                    .caption(t("Choose folder"))
                    .on_click(cx.listener(|this, _, _, cx| this.choose_root("google_drive", cx))),
            )
            .child(
                button("dropbox".into(), "cloud-brand-dropbox")
                    .label(t("Connect Dropbox…"))
                    .caption(t("Choose folder"))
                    .on_click(cx.listener(|this, _, _, cx| this.choose_root("dropbox", cx))),
            )
    }

    /// Open from Cloud….
    pub(super) fn show_cloud_open(&mut self, cx: &mut Context<Self>) {
        self.pane = Pane::Cloud;
        self.icons = None;
        self.error = None;
        self.refresh_cloud(cx);
        cx.notify();
    }

    /// The cloud libraries already in the list, by id, with the name of
    /// the library they were added as.
    fn added(&self, cx: &App) -> BTreeMap<String, String> {
        let state = crate::state::get(cx);
        let mut added = BTreeMap::new();
        for row in &self.rows {
            if let Some(id) = state
                .library_meta
                .get(&row.path)
                .and_then(|look| look.cloud_library_id.clone())
            {
                added.insert(id, row.name.clone());
            }
        }
        if let (Some(current), Some(configuration)) = (
            self.rows.iter().find(|row| row.current),
            self.configuration(cx),
        ) {
            added.insert(configuration.library_id, current.name.clone());
        }
        added
    }

    /// Choose Location…: where the cloud library goes, then it is joined
    /// (`joinCloudLibrary`).
    pub(super) fn join(&mut self, cx: &mut Context<Self>) {
        let Some(choice) = self.chosen.and_then(|index| self.choices.get(index)).cloned() else {
            return;
        };
        let name = self.cloud_name.read(cx).value().trim().to_owned();
        if name.is_empty() || self.busy || self.working.is_some() {
            return;
        }
        self.error = None;
        self.message = None;
        // Production's main process writes it in English.
        let chosen = crate::prompt::folder(&format!("Choose where to store \"{name}\""), cx);
        cx.spawn(async move |this, cx| {
            let Ok(Some(parent)) = chosen.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                let target = match super::super::new_library_path(&name, &parent) {
                    Ok(target) => target,
                    Err(error) => {
                        this.error = Some(error);
                        cx.notify();
                        return;
                    }
                };
                this.working = Some("join-cloud");
                cx.notify();
                let manager = cx.entity().downgrade();
                cx.defer(move |cx| {
                    host::join(choice, target, cx, move |result, cx| {
                        let _ = manager.update(cx, |this, cx| {
                            this.working = None;
                            match result {
                                Ok(path) => {
                                    this.selected = Some(path.to_string_lossy().into_owned());
                                    this.pane = Pane::Library;
                                    this.chosen = None;
                                    this.cloud_name.update(cx, |field, cx| field.clear(cx));
                                    this.message = Some(crate::tf!(
                                        "Opened \"{value0}\" from cloud.",
                                        super::super::name(&path.to_string_lossy())
                                    ));
                                }
                                Err(error) => this.error = Some(error),
                            }
                            this.read(cx);
                            this.refresh_cloud(cx);
                        });
                    })
                });
            });
        })
        .detach();
    }

    /// Open from Cloud…'s pane: the providers to connect, the cloud
    /// libraries found, the local name, and Choose Location….
    pub(super) fn cloud_pane(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = theme::theme(cx);
        let added = self.added(cx);
        let busy = self.busy || self.working.is_some();
        let mut pane = div()
            .flex()
            .flex_col()
            .items_start()
            .child(
                // `.heroIcon`
                div()
                    .size(px(56.))
                    .mb(px(12.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(12.))
                    .bg(theme.surface_hover())
                    .border_1()
                    .border_color(theme.border_secondary())
                    .child(ui::icon("cloud-hero", 26., theme.text_secondary())),
            )
            .child(
                div()
                    .text(theme::TEXT_XL)
                    .font_weight(FontWeight::BOLD)
                    .child(t("Open a cloud library")),
            )
            .child(
                div()
                    .max_w(px(360.))
                    .mt(px(6.))
                    .mb(px(18.))
                    .text(theme::TEXT_SM)
                    .text_color(theme.text_secondary())
                    .child(t("Select your synced folder, then choose a Picto library found there.")),
            )
            .child(self.connect_buttons("library-open-cloud", cx));
        if self.choices.is_empty() {
            pane = pane.child(
                div()
                    .mt(px(12.))
                    .mb(px(14.))
                    .text(theme::TEXT_SM)
                    .text_color(theme.text_tertiary())
                    .child(t("No Picto recovery snapshots were found in the installed cloud folders.")),
            );
        } else {
            let mut list = div()
                .id("library-cloud-libraries")
                .w_full()
                .max_w(px(520.))
                .max_h(px(188.))
                .mt(px(12.))
                .mb(px(14.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(4.));
            for (index, choice) in self.choices.iter().enumerate() {
                let added_to = added.get(&choice.library.library_id).cloned();
                let unavailable = added_to.is_some();
                let selected = self.chosen == Some(index);
                let detail = match &added_to {
                    Some(name) => crate::tf!("Already added · synced to {value0}", name),
                    None => crate::tf!(
                        "{value0} · {value1}",
                        cloud::provider_name(&choice.root.provider),
                        choice.root.account_label
                    ),
                };
                let name = choice.library.name.clone();
                list = list.child(
                    div()
                        .id(("library-cloud-choice", index))
                        .role(Role::Button)
                        .aria_label(name.clone())
                        .aria_disabled(unavailable)
                        .aria_selected(selected)
                        .w_full()
                        .px(px(10.))
                        .py(px(9.))
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .rounded(px(theme::RADIUS_MD))
                        .border_1()
                        .border_color(if selected {
                            theme.primary_at(60.)
                        } else {
                            theme.border_secondary()
                        })
                        .bg(if selected {
                            theme.surface_active()
                        } else {
                            theme.control_surface()
                        })
                        .when(unavailable, |row| row.opacity(0.45))
                        .when(!unavailable, |row| {
                            row.cursor_pointer()
                                .hover(move |style| style.bg(theme.surface_hover()))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.chosen = Some(index);
                                    let name = this.choices[index].library.name.clone();
                                    this.cloud_name
                                        .update(cx, |field, cx| field.set_text(name, cx));
                                    cx.notify();
                                }))
                        })
                        .child(div().text(theme::TEXT_MD).truncate().child(name))
                        .child(
                            div()
                                .text(theme::TEXT_CAPTION)
                                .text_color(theme.text_tertiary())
                                .truncate()
                                .child(detail),
                        ),
                );
            }
            pane = pane.child(list);
        }
        let field = self.cloud_name.focus_handle(cx);
        let can_type = self.chosen.is_some();
        let named = !self.cloud_name.read(cx).value().trim().is_empty();
        pane.child(
            div()
                .mb(px(6.))
                .text(theme::TEXT_SM)
                .text_color(theme.text_secondary())
                .child(t("Local library name")),
        )
        .child(
            div()
                .w(px(360.))
                .h(px(30.))
                .px(px(10.))
                .rounded(px(theme::RADIUS_MD))
                .border_1()
                .border_color(theme.border_secondary())
                .bg(theme.control_surface())
                .flex()
                .items_center()
                .when(!can_type, |input| input.opacity(0.5))
                .when(can_type, |input| {
                    input
                        .cursor(CursorStyle::IBeam)
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            window.focus(&field, cx)
                        })
                })
                .child(self.cloud_name.clone()),
        )
        .child(
            div().mt(px(10.)).child(
                ui::action_button::action_button(
                    "library-cloud-join",
                    ui::action_button::Look::Primary,
                )
                .label(if self.working == Some("join-cloud") {
                    t("Opening…")
                } else {
                    t("Choose Location…")
                })
                .disabled(busy || !can_type || !named)
                .on_click(cx.listener(|this, _, _, cx| this.join(cx))),
            ),
        )
        .into_any_element()
    }
}

/// The actions of Enable Cloud Sync's rows, one per folder found.
const ENABLE: [&str; 8] = [
    "cloud-enable-0",
    "cloud-enable-1",
    "cloud-enable-2",
    "cloud-enable-3",
    "cloud-enable-4",
    "cloud-enable-5",
    "cloud-enable-6",
    "cloud-enable-7",
];
