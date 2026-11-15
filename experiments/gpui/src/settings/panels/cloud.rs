//! The Cloud section (`CloudPanel`): the library's sync, its pending work,
//! how long recovery snapshots are kept, and its restore history. Sync now,
//! Pause or Resume, Create snapshot and the retention fields act at once
//! through the core's commands, as production's do; Restore puts a
//! snapshot's database in place of the library's (`host::restore`).
use crate::cloud::{self, RestorePoint};
use crate::i18n::{en, t};
use crate::settings::pieces::{block, card, hint, row, separator, title, value};
use crate::settings::window::SettingsWindow;
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{self, action_button::Look};
use gpui::*;

/// The section's own state: the restore points, an operation under way,
/// and the restore the user is asked about.
#[derive(Default)]
pub struct CloudPanel {
    restore_points: Vec<RestorePoint>,
    busy: bool,
    asking: Option<(String, ui::modal::Presence)>,
}

/// `formatCloudDate`: a stored timestamp in the local time, as
/// `toLocaleString` writes it.
fn cloud_date(value: Option<&str>) -> String {
    let Some(value) = value else {
        return t("Never").into();
    };
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|date| {
            crate::i18n::format::date_time(date.with_timezone(&chrono::Local).naive_local())
        })
        .unwrap_or_else(|_| value.to_owned())
}

impl SettingsWindow {
    /// Reads the restore points again (`cloud.restore.list`), for a library
    /// that syncs: they list the provider's folder, so they are read when
    /// the window opens, after an operation and when the sync state
    /// changes, never on a timer.
    pub(in crate::settings) fn refresh_restore_points(&mut self, cx: &mut Context<Self>) {
        let configured = self
            .cloud
            .as_ref()
            .is_some_and(|cloud| cloud.read(cx).configured());
        if !configured {
            self.cloud_panel.restore_points.clear();
            cx.notify();
            return;
        }
        let answer = self
            .model
            .read(cx)
            .session
            .read(cx)
            .dispatch("cloud.restore.list", serde_json::json!({}));
        cx.spawn(async move |this, cx| {
            let points = answer.await.ok().and_then(Result::ok);
            let _ = this.update(cx, |this, cx| {
                if let Some(points) =
                    points.and_then(|points| serde_json::from_value::<Vec<RestorePoint>>(points).ok())
                {
                    this.cloud_panel.restore_points = points;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Runs a cloud command while nothing else runs, then reads the state
    /// again; a failure is told as production tells it.
    fn cloud_run(
        &mut self,
        command: &'static str,
        args: serde_json::Value,
        success: Option<&'static str>,
        cx: &mut Context<Self>,
    ) {
        let Some(cloud) = self.cloud.clone() else {
            return;
        };
        if self.cloud_panel.busy {
            return;
        }
        self.cloud_panel.busy = true;
        cx.notify();
        let this = cx.entity().downgrade();
        cloud.update(cx, |cloud, cx| {
            cloud.run(command, args, cx, move |result, cx| {
                match result {
                    Ok(_) => {
                        if let Some(success) = success {
                            ui::notify::success(success, "", cx);
                        }
                    }
                    Err(error) => ui::notify::error(t("Cloud operation failed"), error, cx),
                }
                let _ = this.update(cx, |this, cx| {
                    this.cloud_panel.busy = false;
                    this.refresh_restore_points(cx);
                });
            })
        });
    }

    /// The user confirmed a restore: the library is closed, its database
    /// replaced by the snapshot's (the current one kept as an emergency
    /// copy) and opened again, which closes this window too.
    fn restore(&mut self, snapshot: String, cx: &mut Context<Self>) {
        if let Some((_, presence)) = &mut self.cloud_panel.asking {
            presence.close();
        }
        self.cloud_panel.asking = None;
        self.cloud_panel.busy = true;
        cx.notify();
        cx.defer(move |cx| {
            crate::libraries::host::restore(snapshot, cx, |result, cx| match result {
                Ok(()) => ui::notify::success(t("Library restored"), "", cx),
                Err(error) => ui::notify::error(t("Cloud operation failed"), error, cx),
            })
        });
    }

    /// The question before a restore (`window.confirm`).
    pub(in crate::settings) fn restore_question(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (snapshot, presence) = self.cloud_panel.asking.as_ref()?;
        let cancel = cx.listener(|this: &mut Self, _: &(), _, cx| {
            this.cloud_panel.asking = None;
            cx.notify();
        });
        let cancel_too = cx.listener(|this: &mut Self, _: &(), _, cx| {
            this.cloud_panel.asking = None;
            cx.notify();
        });
        let snapshot = snapshot.clone();
        let accept =
            cx.listener(move |this: &mut Self, _: &(), _, cx| this.restore(snapshot.clone(), cx));
        Some(
            ui::confirm::confirm(
                "cloud-restore",
                t("Restore"),
                t("Restore this recovery snapshot? Picto will preserve the current database as an emergency copy."),
                t("Restore"),
                *presence,
            )
            .on_close(move |window, cx| cancel(&(), window, cx))
            .on_cancel(move |window, cx| cancel_too(&(), window, cx))
            .on_confirm(move |window, cx| accept(&(), window, cx))
            .into_any_element(),
        )
    }

    pub(super) fn cloud(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let Some(cloud) = &self.cloud else {
            return Vec::new();
        };
        let (Some(configuration), Some(status)) = (
            cloud.read(cx).configuration.clone(),
            cloud.read(cx).status.clone(),
        ) else {
            return Vec::new();
        };
        let configured = configuration.provider.is_some();
        let busy = self.cloud_panel.busy;
        let provider = match configuration.provider.as_deref() {
            Some(provider) => cloud::provider_name(provider),
            None => t("Not configured"),
        };
        let state = if status.message.is_empty() {
            status.state.clone()
        } else {
            status.message.clone()
        };
        let paused = status.state == "paused";
        let mut sync = card(theme)
            .child(title(theme, t("Library Sync")))
            .child(row(theme, t("Provider"), value(theme, provider)))
            .child(separator(theme))
            .child(row(
                theme,
                t("Folder"),
                value(
                    theme,
                    configuration.root_path.clone().unwrap_or_else(|| {
                        // Production writes it without a key.
                        en("Choose a cloud folder in Libraries").into()
                    }),
                )
                // A long folder is cut short rather than drawn over its label.
                .min_w_0()
                .truncate(),
            ))
            .child(separator(theme))
            .child(row(theme, t("State"), value(theme, state)))
            .child(separator(theme))
            .child(row(
                theme,
                t("Last sync"),
                value(theme, cloud_date(status.last_sync_at.as_deref())),
            ))
            .child(separator(theme))
            .child(
                // `.cloudActions`' 10px top margin collapses into the
                // separator's 12px, as the card is a block.
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(
                        ui::action_button("cloud-sync", Look::Settings)
                            .label(t("Sync now"))
                            .disabled(!configured || busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cloud_run("cloud.reconcile", serde_json::json!({}), None, cx)
                            })),
                    )
                    .child(
                        ui::action_button("cloud-pause", Look::Settings)
                            .label(if paused { t("Resume") } else { t("Pause") })
                            .disabled(!configured || busy)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.cloud_run(
                                    "cloud.pause",
                                    serde_json::json!({ "paused": !paused }),
                                    None,
                                    cx,
                                )
                            })),
                    )
                    .child(
                        ui::action_button("cloud-snapshot", Look::Settings)
                            .label(t("Create snapshot"))
                            .disabled(!configured || busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cloud_run(
                                    "cloud.snapshot.create",
                                    serde_json::json!({}),
                                    // Production writes it without a key.
                                    Some(en("Recovery snapshot created")),
                                    cx,
                                )
                            })),
                    ),
            );
        if !configured {
            sync = sync.child(hint(
                theme,
                t("Cloud sync is enabled per library from the Libraries window. Picto uses the Google Drive or Dropbox desktop folder already installed on this device."),
            ));
        }
        let retention = cloud::retention(&configuration);
        let this = cx.entity().downgrade();
        let field = |id: &'static str, label: &'static str, at: usize, min: i64, max: i64| {
            let this = this.clone();
            ui::number_input(id, retention[at], min, max)
                .name(t(label))
                .on_commit(move |value, cx| {
                    let mut next = retention;
                    next[at] = value;
                    let _ = this.update(cx, |this, cx| {
                        this.cloud_run(
                            "cloud.retention.update",
                            serde_json::json!({ "value": {
                                "daily": next[0],
                                "weekly": next[1],
                                "yearly": next[2],
                                "deleted_blobs_days": next[3],
                            }}),
                            None,
                            cx,
                        )
                    });
                })
        };
        // i18n: translated where drawn
        let retention_card = card(theme)
            .child(title(theme, t("Recovery Retention")))
            .child(row(
                theme,
                t("Daily snapshots"),
                field("retention-daily", "Daily snapshots", 0, 2, 365),
            ))
            .child(separator(theme))
            .child(row(
                theme,
                t("Weekly snapshots"),
                field("retention-weekly", "Weekly snapshots", 1, 0, 260),
            ))
            .child(separator(theme))
            .child(row(
                theme,
                t("Yearly snapshots"),
                field("retention-yearly", "Yearly snapshots", 2, 0, 100),
            ))
            .child(separator(theme))
            .child(row(
                theme,
                t("Keep deleted files"),
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(field("retention-deleted", "Keep deleted files", 3, 0, 3650))
                    .child(value(theme, t("days"))),
            ))
            .child(hint(
                theme,
                t("Recently deleted cloud files can be restored immediately during this period."),
            ));
        let mut history = card(theme).child(title(theme, t("Restore History")));
        if self.cloud_panel.restore_points.is_empty() {
            history = history.child(hint(theme, t("No recovery snapshots are available yet.")));
        }
        for (index, point) in self.cloud_panel.restore_points.iter().enumerate() {
            let snapshot = point.snapshot_id.clone();
            // Each point follows a separator (`Row sep`).
            history = history.child(separator(theme)).child(row(
                theme,
                cloud_date(Some(&point.created_at)),
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .text(theme::TEXT_SM)
                            .text_color(theme.text_secondary())
                            .font_features(theme::tabular_figures())
                            .child(cloud::bytes(point.size_bytes as i64)),
                    )
                    .child(
                        ui::action_button(("cloud-restore", index), Look::Settings)
                            .label(t("Restore"))
                            .disabled(busy || !point.verified)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.cloud_panel.asking =
                                    Some((snapshot.clone(), ui::modal::Presence::new()));
                                cx.notify();
                            })),
                    ),
            ));
        }
        vec![
            block().child(sync).into_any_element(),
            block()
                .child(
                    card(theme)
                        .child(title(theme, t("Pending Work")))
                        .child(row(
                            theme,
                            t("Mutations"),
                            value(theme, status.pending_mutations.to_string()),
                        ))
                        .child(separator(theme))
                        .child(row(
                            theme,
                            t("Files"),
                            value(theme, status.pending_blobs.to_string()),
                        ))
                        .child(separator(theme))
                        .child(row(
                            theme,
                            t("Unavailable files"),
                            value(theme, status.missing_blobs.to_string()),
                        )),
                )
                .into_any_element(),
            block().child(retention_card).into_any_element(),
            block().child(history).into_any_element(),
        ]
    }
}
