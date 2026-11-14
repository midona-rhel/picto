//! The Cloud section: the library's sync state, its pending work and
//! recovery, and the reading of that state from the library.
use crate::i18n::t;
use crate::settings::pieces::{block, card, hint, row, separator, title, value};
use crate::settings::window::{CloudInfo, SettingsWindow};
use crate::theme;
use crate::ui::{self, action_button::Look};
use gpui::*;

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
    pub(super) fn cloud(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let Some(info) = &self.cloud else {
            return Vec::new();
        };
        let configured = info.provider.is_some();
        let provider = match info.provider.as_deref() {
            Some("google_drive") => t("Google Drive"),
            Some("dropbox") => t("Dropbox"),
            _ => t("Not configured"),
        };
        let state = if info.message.is_empty() {
            info.state.clone()
        } else {
            info.message.clone()
        };
        let mut sync = card(theme)
            .child(title(theme, t("Library Sync")))
            .child(row(theme, t("Provider"), value(theme, provider)))
            .child(separator(theme))
            .child(row(
                theme,
                t("Folder"),
                value(
                    theme,
                    info.root_path.clone().unwrap_or_else(|| {
                        crate::i18n::en("Choose a cloud folder in Libraries").into()
                    }),
                ),
            ))
            .child(separator(theme))
            .child(row(theme, t("State"), value(theme, state)))
            .child(separator(theme))
            .child(row(
                theme,
                t("Last sync"),
                value(theme, cloud_date(info.last_sync_at.as_deref())),
            ))
            .child(separator(theme))
            .child(
                // TODO: sync, pause and snapshot through the cloud service.
                // `.cloudActions`' 10px top margin collapses into the
                // separator's 12px, as the card is a block.
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(
                        ui::action_button("cloud-sync", Look::Settings)
                            .label(t("Sync now"))
                            .disabled(!configured),
                    )
                    .child(
                        ui::action_button("cloud-pause", Look::Settings)
                            .label(if info.state == "paused" {
                                t("Resume")
                            } else {
                                t("Pause")
                            })
                            .disabled(!configured),
                    )
                    .child(
                        ui::action_button("cloud-snapshot", Look::Settings)
                            .label(t("Create snapshot"))
                            .disabled(!configured),
                    ),
            );
        if !configured {
            sync = sync.child(hint(
                theme,
                t("Cloud sync is enabled per library from the Libraries window. Picto uses the Google Drive or Dropbox desktop folder already installed on this device."),
            ));
        }
        let retention = info.retention;
        // TODO: write retention through `cloud.retention.update` and enable
        // the fields; until then they show the stored values, disabled.
        let retention_card = card(theme)
            .child(title(theme, t("Recovery Retention")))
            .child(row(
                theme,
                t("Daily snapshots"),
                ui::number_input("retention-daily", retention[0], 2, 365)
                    .name(t("Daily snapshots"))
                    .disabled(true),
            ))
            .child(separator(theme))
            .child(row(
                theme,
                t("Weekly snapshots"),
                ui::number_input("retention-weekly", retention[1], 0, 260)
                    .name(t("Weekly snapshots"))
                    .disabled(true),
            ))
            .child(separator(theme))
            .child(row(
                theme,
                t("Yearly snapshots"),
                ui::number_input("retention-yearly", retention[2], 0, 100)
                    .name(t("Yearly snapshots"))
                    .disabled(true),
            ))
            .child(separator(theme))
            .child(row(
                theme,
                t("Keep deleted files"),
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        ui::number_input("retention-deleted", retention[3], 0, 3650)
                            .name(t("Keep deleted files"))
                            .disabled(true),
                    )
                    .child(value(theme, t("days"))),
            ))
            .child(hint(
                theme,
                t("Recently deleted cloud files can be restored immediately during this period."),
            ));
        vec![
            block().child(sync).into_any_element(),
            block()
                .child(
                    card(theme)
                        .child(title(theme, t("Pending Work")))
                        .child(row(
                            theme,
                            t("Mutations"),
                            value(theme, info.pending_mutations.to_string()),
                        ))
                        .child(separator(theme))
                        .child(row(
                            theme,
                            t("Files"),
                            value(theme, info.pending_blobs.to_string()),
                        ))
                        .child(separator(theme))
                        .child(row(
                            theme,
                            t("Unavailable files"),
                            value(theme, info.missing_blobs.to_string()),
                        )),
                )
                .into_any_element(),
            block().child(retention_card).into_any_element(),
            block()
                .child(
                    card(theme)
                        .child(title(theme, t("Restore History")))
                        // TODO: list the recovery snapshots of a configured
                        // provider (`cloud.restore.list`) with their Restore
                        // buttons. An unconfigured library has none.
                        .child(hint(theme, t("No recovery snapshots are available yet."))),
                )
                .into_any_element(),
        ]
    }
}

/// The cloud state shown in the Cloud section.
pub fn cloud_info(library: &picto_library::Library) -> CloudInfo {
    let read = library.auxiliary_read(
        picto_library::database::WorkPriority::VisibleRead,
        |connection| {
            let status = connection.query_row(
                "SELECT state, message, last_sync_at,
                        (SELECT COUNT(*) FROM cloud_journal WHERE expanded_at_ms IS NULL),
                        (SELECT COUNT(*) FROM cloud_blob_state
                         WHERE state IN ('queued', 'downloading')
                            OR (state = 'available' AND remote_present = 0)),
                        (SELECT COUNT(*) FROM cloud_blob_state
                         WHERE state IN ('missing_remote', 'corrupt')),
                        provider, remote_root, retention_json
                 FROM cloud_state WHERE singleton = 1",
                [],
                |row| {
                    let retention: String = row.get(8)?;
                    let retention = serde_json::from_str::<serde_json::Value>(&retention)
                        .unwrap_or(serde_json::Value::Null);
                    let days = |key: &str, fallback: i64| {
                        retention
                            .get(key)
                            .and_then(serde_json::Value::as_i64)
                            .unwrap_or(fallback)
                    };
                    Ok(CloudInfo {
                        state: row.get(0)?,
                        message: row.get(1)?,
                        last_sync_at: row.get(2)?,
                        pending_mutations: row.get(3)?,
                        pending_blobs: row.get(4)?,
                        missing_blobs: row.get(5)?,
                        provider: row.get(6)?,
                        root_path: row.get(7)?,
                        retention: [
                            days("daily", 30),
                            days("weekly", 26),
                            days("yearly", 5),
                            days("deleted_blobs_days", 7),
                        ],
                    })
                },
            )?;
            Ok(status)
        },
    );
    read.unwrap_or_default()
}
