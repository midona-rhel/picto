//! The Cloud section of the Settings window. It shows the library's cloud
//! sync with its state and its message, the work that is waiting to be
//! synced, how long recovery snapshots are kept, and the recovery snapshots
//! that can be restored. Sync now,
//! Pause or Resume, Create snapshot and the retention fields take effect at
//! once through the core's commands; they do not wait for the Save & Close
//! button or the Apply button. Restore puts the database of a snapshot in
//! place of the library's database (`host::restore`).
use crate::cloud::{self, RestorePoint};
use crate::i18n::{en, t};
use crate::settings::pieces::{block, card, hint, row, separator, title, value};
use crate::settings::window::SettingsWindow;
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{self, action_button::Look};
use gpui::*;

/// The state of the Cloud section: the restore points, whether an operation
/// is under way, and the dialog that asks the user to confirm a restore.
#[derive(Default)]
pub struct CloudPanel {
    restore_points: Vec<RestorePoint>,
    busy: bool,
    asking: ui::modal::Host<String>,
}

/// Returns a stored timestamp as a date and a time in the local time zone,
/// written as the app's language writes them. Returns "Never" when there is
/// no timestamp. Returns the stored text as it is when it cannot be read as
/// a timestamp.
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
    /// Reads the restore points again with the command
    /// `cloud.restore.list`. For a library that does not sync, it empties
    /// the list instead. The command lists the provider's folder, so the
    /// restore points are read only when the window opens, after an
    /// operation and when the sync state changes, and not on a timer.
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
                if let Some(points) = points
                    .and_then(|points| serde_json::from_value::<Vec<RestorePoint>>(points).ok())
                {
                    this.cloud_panel.restore_points = points;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Runs a cloud command and then reads the restore points again. It
    /// does nothing while another cloud command of this section is still
    /// running. When the command succeeds and `success` is given, a
    /// notification with that title is shown. When the command fails, an
    /// error notification with the title "Cloud operation failed" is shown.
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

    /// Restores the snapshot `snapshot` after the user has confirmed the
    /// restore. The library is closed, its database is replaced by the
    /// database of the snapshot, and the library is opened again. The
    /// database that was replaced is kept as an emergency copy. Opening the
    /// library again also closes this window.
    fn restore(&mut self, snapshot: String, cx: &mut Context<Self>) {
        self.cloud_panel.asking.dismiss(cx);
        self.cloud_panel.busy = true;
        cx.notify();
        cx.defer(move |cx| {
            crate::libraries::host::restore(snapshot, cx, |result, cx| match result {
                Ok(()) => ui::notify::success(t("Library restored"), "", cx),
                Err(error) => ui::notify::error(t("Cloud operation failed"), error, cx),
            })
        });
    }

    /// Returns the dialog that asks the user to confirm a restore, or
    /// `None` when no restore is waiting for an answer.
    pub(in crate::settings) fn restore_question(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.cloud_panel.asking.layer(window, cx)?;
        let (snapshot, presence, focus) = (
            dialog.content.clone(),
            dialog.presence,
            dialog.focus.clone(),
        );
        let close =
            cx.listener(|this: &mut Self, _: &(), _, cx| this.cloud_panel.asking.dismiss(cx));
        let cancel = cx.listener(|this: &mut Self, _: &(), _, cx| {
            this.cloud_panel.asking.close(cx);
        });
        let accept =
            cx.listener(move |this: &mut Self, _: &(), _, cx| this.restore(snapshot.clone(), cx));
        Some(
            ui::confirm::confirm(
                "cloud-restore",
                t("Restore"),
                t("Restore this recovery snapshot? Picto will preserve the current database as an emergency copy."),
                t("Restore"),
                presence,
            )
            .focus(&focus)
            .on_close(move |window, cx| close(&(), window, cx))
            .on_cancel(move |window, cx| cancel(&(), window, cx))
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
        // The sync's message is a whole sentence or more, for example the
        // reason of a failure. It is drawn under the State row as text that
        // wraps, because the row's value is a single line.
        let message = (!status.message.is_empty()).then(|| {
            ui::form::help_text(theme, status.message.clone())
                .id("cloud-state-message")
                .mt(px(4.))
                .line_height(px(20.))
        });
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
                        // This text has no translations, so it is shown in
                        // English in every language.
                        en("Choose a cloud folder in Libraries").into()
                    }),
                )
                // A folder path that does not fit is cut short, so that it
                // is not drawn over its label.
                .min_w_0()
                .truncate(),
            ))
            .child(separator(theme))
            .child(row(theme, t("State"), value(theme, status.state.clone())))
            .children(message)
            .child(separator(theme))
            .child(row(
                theme,
                t("Last sync"),
                value(theme, cloud_date(status.last_sync_at.as_deref())),
            ))
            .child(separator(theme))
            .child(
                // The row of buttons has no space of its own above it. The
                // 12px below the separator is the space between the two.
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
                                    // This text has no translations, so it
                                    // is shown in English in every language.
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
            // A separator is drawn above each restore point.
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
                            .child(crate::text::size_to_terabytes(point.size_bytes)),
                    )
                    .child(
                        ui::action_button(("cloud-restore", index), Look::Settings)
                            .label(t("Restore"))
                            .disabled(busy || !point.verified)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.cloud_panel.asking.open(
                                    snapshot.clone(),
                                    ui::menu::Return::Opener,
                                    cx,
                                );
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
