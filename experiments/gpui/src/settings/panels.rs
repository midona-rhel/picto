//! The Settings window's sections and their rows, in production's order and
//! wording (`Settings.tsx`, `AiTaggingPanel.tsx`).
use super::model;
use super::model::NOTIFICATION_TONES;
use super::pieces::{
    block, card, edit, hint, number, pick, row, separator, swatch, three_columns, title,
    two_columns, value,
};
use super::window::SettingsWindow;
use crate::grid::format::grouped;
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{self, action_button::Look, choice};
use gpui::*;

mod about;
mod ai_models;
mod catalog;
mod cloud;
mod controls;

pub use catalog::{PANELS, matched_panels};
pub use cloud::cloud_info;

/// `ZOOM_OPTIONS`
const ZOOM: [(&str, &str); 7] = [
    ("75", "75%"),
    ("80", "80%"),
    ("90", "90%"),
    ("100", "100%"),
    ("110", "110%"),
    ("125", "125%"),
    ("150", "150%"),
];

/// `THEMES`: the name, stored value and swatch colour of each theme.
const THEMES: [(&str, &str, Option<u32>); 7] = [
    ("Auto", "auto", None),
    ("Light", "light", Some(0xffffff)),
    ("Light Gray", "lightgray", Some(0xc8cacd)),
    ("Gray", "gray", Some(0x444444)),
    ("Dark", "dark", Some(0x010101)),
    ("Blue", "blue", Some(0x28356e)),
    ("Purple", "purple", Some(0x463275)),
];

/// The installed version, from the app's `package.json`.
fn version() -> String {
    serde_json::from_str::<serde_json::Value>(include_str!("../../../../package.json"))
        .ok()
        .and_then(|package| package.get("version")?.as_str().map(str::to_owned))
        .unwrap_or_default()
}

impl SettingsWindow {
    /// The rows of the section with this id.
    pub(super) fn panel(
        &self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        match id {
            "general" => self.general(window, cx),
            "updates" => self.updates(cx),
            "library" => self.library(cx),
            "sidebar" => self.sidebar(cx),
            "controls" => self.controls(cx),
            "preview" => self.preview(cx),
            "notifications" => self.notifications(cx),
            "autoimport" => self.auto_import(cx),
            "deleting" => self.deleting(cx),
            "subscriptions" => self.subscriptions(cx),
            "cloud" => self.cloud(cx),
            "aitagging" => self.ai_models(window, cx),
            "about" => self.about(cx),
            _ => Vec::new(),
        }
    }

    fn general(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let model = &self.model;
        let draft = &model.read(cx).draft;
        let active = draft.app.color_scheme.clone();
        let zoom = draft
            .app
            .zoom_factor
            .map(|factor| (factor * 100.).round() as i64)
            .filter(|percent| [75, 80, 90, 100, 110, 125, 150].contains(percent))
            .unwrap_or(100);
        let (locale, counts, prefixes) = (
            draft.locale,
            draft.app.show_sidebar_counts,
            draft.app.show_tag_prefixes,
        );
        let mut swatches = div().flex().flex_wrap().justify_end().gap(px(8.));
        for (index, &(name, css, color)) in THEMES.iter().enumerate() {
            // The choice shows in every window at once (`previewTheme`);
            // closing without saving puts the saved theme back.
            let choose = edit(model, move |draft| draft.app.color_scheme = css.into());
            swatches = swatches.child(swatch(
                index,
                name,
                color,
                active == css,
                move |window, cx| {
                    choose(window, cx);
                    theme::show(css, cx);
                },
                window,
                cx,
            ));
        }
        vec![
            block()
                .child(
                    card(theme)
                        .child(title(theme, "Appearance"))
                        .child(row(theme, "Theme", swatches))
                        .child(separator(theme))
                        .child(row(
                            theme,
                            "Zoom Level",
                            // TODO: production saves this factor without applying
                            // it (its zoom is the separate ⌘+ and ⌘- renderer
                            // zoom), so the window is not scaled here either;
                            // scale the main window's content once it does.
                            ui::select(
                                "zoom",
                                zoom.to_string(),
                                ZOOM.iter()
                                    .map(|&(value, label)| choice(value, label))
                                    .collect(),
                            )
                            .on_change(pick(model, |draft, value| {
                                draft.app.zoom_factor =
                                    value.parse::<f64>().ok().map(|percent| percent / 100.)
                            })),
                        )),
                )
                .into_any_element(),
            block()
                .child(
                    card(theme)
                        .child(title(theme, "Interface"))
                        .child(row(
                            theme,
                            "Language",
                            ui::select(
                                "language",
                                locale,
                                crate::i18n::Locale::ALL
                                    .iter()
                                    .map(|locale| choice(locale.code(), locale.label()))
                                    .collect(),
                            )
                            .width(220.)
                            .on_change({
                                let model = model.clone();
                                move |value, _, cx| {
                                    model.update(cx, |model, cx| model.choose_locale(value, cx))
                                }
                            }),
                        ))
                        .child(separator(theme))
                        .child(
                            ui::check("sidebar-counts", "Show item counts in the sidebar", counts)
                                .on_toggle(edit(model, |draft| {
                                    draft.app.show_sidebar_counts = !draft.app.show_sidebar_counts
                                })),
                        ),
                )
                .into_any_element(),
            block()
                .child(
                    card(theme)
                        .child(title(theme, "Tags"))
                        .child(
                            ui::check("hide-prefixes", "Hide group prefixes", !prefixes).on_toggle(
                                edit(model, |draft| {
                                    draft.app.show_tag_prefixes = !draft.app.show_tag_prefixes
                                }),
                            ),
                        )
                        .child(hint(
                            theme,
                            "Show shorter tag names without changing their underlying group.",
                        )),
                )
                .into_any_element(),
        ]
    }

    fn updates(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let description = if cfg!(target_os = "macos") {
            "On macOS, Picto opens the release download because this build is not signed for automatic installation."
        } else {
            "Updates download in the background and install after Picto closes."
        };
        vec![
            block()
                .child(
                    card(theme)
                        .child(title(theme, "Software Update"))
                        .child(row(theme, "Installed version", div().child(version())))
                        .child(separator(theme))
                        .child(row(
                            theme,
                            "Check for a newer version of Picto.",
                            div().flex().gap(px(8.)).child(
                                // TODO: check the release feed and offer the
                                // download; disabled until then, as the
                                // application menu's Check for Updates is.
                                ui::action_button("check-updates", Look::Settings)
                                    .label("Check Now")
                                    .disabled(true),
                            ),
                        ))
                        .child(
                            div()
                                .mt(px(12.))
                                .text(theme::TEXT_SM)
                                .line_height(px(18.))
                                .text_color(theme.text_tertiary())
                                .child(description),
                        ),
                )
                .into_any_element(),
        ]
    }

    fn library(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let Some(stats) = &self.statistics else {
            return Vec::new();
        };
        let count = |value: i64| grouped(value.max(0) as u64);
        let section = |name: &'static str, rows: Vec<(&'static str, String)>| {
            let mut content = card(theme).child(title(theme, name));
            for (index, (label, shown)) in rows.into_iter().enumerate() {
                if index > 0 {
                    content = content.child(separator(theme));
                }
                content = content.child(row(theme, label, value(theme, shown)));
            }
            block().child(content).into_any_element()
        };
        vec![
            section(
                "Overview",
                vec![
                    (
                        "Library items",
                        count(stats.active_items + stats.inbox_items + stats.trash_items),
                    ),
                    ("Media assets", count(stats.media_assets)),
                    (
                        "Size",
                        crate::text::size_to_terabytes(stats.original_bytes.max(0) as u64),
                    ),
                ],
            ),
            section(
                "Library Items",
                vec![
                    ("All", count(stats.active_items)),
                    ("Inbox", count(stats.inbox_items)),
                    ("Trash", count(stats.trash_items)),
                    ("Standalone", count(stats.standalone_items)),
                    ("Groups", count(stats.collections)),
                ],
            ),
            section(
                "Media and Storage",
                vec![
                    ("Images", count(stats.image_assets)),
                    ("Videos", count(stats.video_assets)),
                    ("Audio", count(stats.audio_assets)),
                    ("Other media", count(stats.other_assets)),
                    ("Physical files", count(stats.physical_files)),
                ],
            ),
            section(
                "Organization",
                vec![
                    ("Tags", count(stats.tags)),
                    ("Folders", count(stats.folders)),
                    ("Smart folders", count(stats.smart_folders)),
                    ("Subscriptions", count(stats.subscriptions)),
                ],
            ),
        ]
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let model = &self.model;
        let app = &model.read(cx).draft.app;
        let rename = app.sidebar_double_click_action == "rename";
        let guides = app.show_tree_guides;
        let shown = |key: &str| app.sidebar_shows(key);
        let toggled = |index: usize, name: &'static str, key: &'static str, on: bool| {
            let check = ui::check(("sidebar-shows", index), name, on);
            if ["system:active", "system:trash"].contains(&key) {
                check.disabled(true)
            } else {
                check.on_toggle(edit(model, move |draft| {
                    if let Some(on) = draft.app.sidebar_visibility(key) {
                        *on = !*on;
                    }
                }))
            }
        };
        let mut rows = three_columns();
        for (index, (name, key)) in [
            ("All", "system:active"),
            ("Uncategorized", "system:uncategorized"),
            ("Untagged", "system:untagged"),
            ("Inbox", "system:inbox"),
            ("Random", "system:random"),
            ("Tag Manager", "system:tag_manager"),
            ("Recently Viewed", "system:recent_viewed"),
            ("Subscriptions", "system:subscriptions"),
            ("Duplicates", "system:duplicates"),
            ("Trash", "system:trash"),
        ]
        .into_iter()
        .enumerate()
        {
            rows = rows.child(toggled(index, name, key, shown(key)));
        }
        let mut section_rows = three_columns();
        for (index, (name, key)) in [
            ("Quick Access", "section:quick_access"),
            ("Smart Folders", "section:smart_folders"),
            ("Folders", "section:folders"),
        ]
        .into_iter()
        .enumerate()
        {
            section_rows = section_rows.child(toggled(10 + index, name, key, shown(key)));
        }
        vec![
            block()
                .child(
                    card(theme).child(title(theme, "Double Click")).child(
                        three_columns()
                            .child(
                                ui::check("double-click-rename", "Rename", rename)
                                    .radio()
                                    .on_toggle(edit(model, |draft| {
                                        draft.app.sidebar_double_click_action = "rename"
                                    })),
                            )
                            .child(
                                ui::check("double-click-collapse", "Expand/Collapse", !rename)
                                    .radio()
                                    .on_toggle(edit(model, |draft| {
                                        draft.app.sidebar_double_click_action = "collapse"
                                    })),
                            ),
                    ),
                )
                .into_any_element(),
            block()
                .child(
                    card(theme)
                        .child(title(theme, "Show these items in the sidebar:"))
                        .child(rows)
                        .child(separator(theme))
                        .child(section_rows),
                )
                .into_any_element(),
            block()
                .child(
                    card(theme).child(title(theme, "Folder Tree")).child(
                        ui::check("tree-guides", "Show hierarchy guides", guides)
                            .on_toggle(edit(model, |draft| {
                                draft.app.show_tree_guides = !draft.app.show_tree_guides
                            })),
                    ),
                )
                .into_any_element(),
        ]
    }

    fn preview(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let model = &self.model;
        let draft = &model.read(cx).draft;
        let app = &draft.app;
        vec![
            block()
                .child(
                    card(theme)
                        .child(title(theme, "Image"))
                        .child(row(
                            theme,
                            "Scaling",
                            ui::select(
                                "image-rendering",
                                app.image_rendering,
                                vec![choice("smooth", "Smooth"), choice("pixelated", "Pixelated")],
                            )
                            .on_change(pick(model, |draft, value| {
                                draft.app.image_rendering = value
                            })),
                        ))
                        .child(separator(theme))
                        .child(row(
                            theme,
                            "Default zoom",
                            ui::select(
                                "image-zoom",
                                app.image_default_zoom,
                                vec![
                                    choice("fit", "Fit to window"),
                                    choice("actual", "Actual size"),
                                ],
                            )
                            .on_change(pick(model, |draft, value| {
                                draft.app.image_default_zoom = value
                            })),
                        ))
                        .child(separator(theme))
                        .child(
                            ui::check(
                                "transparency",
                                "Show transparency grid",
                                app.show_transparency_grid,
                            )
                            .on_toggle(edit(model, |draft| {
                                draft.app.show_transparency_grid = !draft.app.show_transparency_grid
                            })),
                        ),
                )
                .into_any_element(),
            block()
                .child(
                    card(theme).child(title(theme, "Video")).child(
                        two_columns()
                            .child(
                                ui::check("autoplay", "Autoplay videos", app.video_auto_play)
                                    .on_toggle(edit(model, |draft| {
                                        draft.app.video_auto_play = !draft.app.video_auto_play
                                    })),
                            )
                            .child(ui::check("loop", "Loop videos", app.video_loop).on_toggle(
                                edit(model, |draft| draft.app.video_loop = !draft.app.video_loop),
                            )),
                    ),
                )
                .into_any_element(),
            block()
                .child(
                    card(theme).child(title(theme, "Audio")).child(row(
                        theme,
                        "Visualization",
                        ui::select(
                            "audio-visualization",
                            draft.audio_visualization,
                            vec![
                                choice("none", "None"),
                                choice("spectrum", "Spectrum"),
                                choice("oscilloscope", "Oscilloscope"),
                                choice("orbit", "Orbit"),
                            ],
                        )
                        .on_change(pick(model, |draft, value| {
                            draft.audio_visualization = value
                        })),
                    )),
                )
                .into_any_element(),
        ]
    }

    fn notifications(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let model = &self.model;
        let app = &model.read(cx).draft.app;
        let enabled = app.notification_popups_enabled;
        let mut content = card(theme).child(
            title(theme, "Pop-ups").child(div().ml_auto().flex().items_center().child(
                ui::toggle("popups", enabled).on_toggle(edit(model, |draft| {
                    draft.app.notification_popups_enabled = !draft.app.notification_popups_enabled
                })),
            )),
        );
        if enabled {
            let names = [
                ("success", "Successful actions"),
                ("info", "Information"),
                ("warning", "Warnings"),
                ("error", "Errors"),
            ];
            let mut grid = two_columns();
            for (tone_index, (tone, name)) in names.into_iter().enumerate() {
                let tone = NOTIFICATION_TONES
                    .iter()
                    .copied()
                    .find(|known| *known == tone)
                    .unwrap_or("info");
                grid = grid.child(
                    ui::check(
                        ("tone", tone_index),
                        name,
                        app.notification_popup_tones.contains(&tone),
                    )
                    .on_toggle(edit(model, move |draft| {
                        draft.app.toggle_notification_tone(tone)
                    })),
                );
            }
            content = content.child(grid);
        }
        vec![block().child(content).into_any_element()]
    }

    fn auto_import(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let model = &self.model;
        let app = &model.read(cx).draft.app;
        // The multiple-file prompt follows these settings (`imports`), and
        // the application's watched-folder worker reads Auto-Import.
        vec![
            block()
                .child(
                    card(theme)
                        .child(
                            title(theme, "Auto-Import").child(
                                div().ml_auto().flex().items_center().child(
                                    ui::toggle("auto-import", app.auto_import_enabled).on_toggle(
                                        edit(model, |draft| {
                                            draft.app.auto_import_enabled = !draft.app.auto_import_enabled
                                        }),
                                    ),
                                ),
                            ),
                        )
                        .child(hint(
                            theme,
                            "Import new files into Inbox from watched folders configured in the folder context menu.",
                        )),
                )
                .into_any_element(),
            block()
                .child(
                    card(theme)
                        .child(title(theme, "Manual Imports"))
                        .child(row(
                            theme,
                            "Multiple files",
                            ui::select(
                                "multi-file",
                                app.multi_file_import_behavior,
                                vec![
                                    choice("ask", "Ask every time"),
                                    choice("group", "Group as collection"),
                                    choice("separate", "Keep separate"),
                                ],
                            )
                            .on_change(pick(model, |draft, value| {
                                draft.app.multi_file_import_behavior = value
                            })),
                        ))
                        .child(hint(
                            theme,
                            "Choose whether batches become one collection, stay separate, or ask each time.",
                        )),
                )
                .into_any_element(),
        ]
    }

    fn deleting(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let model = &self.model;
        let app = &model.read(cx).draft.app;
        vec![
            block()
                .child(
                    card(theme)
                        .child(title(theme, "Rejecting"))
                        .child(row(
                            theme,
                            "Rejected items",
                            ui::select(
                                "reject-action",
                                app.reject_action,
                                vec![choice("trash", "Move to Trash"), choice("delete", "Delete permanently")],
                            )
                            .on_change(pick(model, |draft, value| draft.app.reject_action = value)),
                        ))
                        .child(hint(
                            theme,
                            "Deleting skips Trash. Only deletes within the undo limit below can be undone.",
                        )),
                )
                .into_any_element(),
            block()
                .child(
                    card(theme)
                        .child(title(theme, "Undoing deletes"))
                        .child(row(
                            theme,
                            "Undo limit (media items)",
                            ui::number_input("undo-limit", app.undo_delete_limit, 0, 100_000).on_commit(
                                number(model, |draft, value: i64| draft.app.undo_delete_limit = value),
                            ),
                        ))
                        .child(hint(
                            theme,
                            "Counts media items, including collection members. Set to 0 to disable delete undo. Deletes within the limit can be undone until Picto closes. Larger deletes ask for confirmation and cannot be undone.",
                        )),
                )
                .into_any_element(),
        ]
    }

    fn subscriptions(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let model = &self.model;
        let app = &model.read(cx).draft.app;
        // A new subscription starts from these defaults (`subscriptions`).
        vec![
            block()
                .child(
                    card(theme)
                        .child(title(theme, "Defaults for New Subscriptions"))
                        .child(row(
                            theme,
                            "Schedule",
                            ui::select(
                                "schedule",
                                app.subscription_default_schedule,
                                vec![
                                    choice("manual", "Manual"),
                                    choice("daily", "Daily"),
                                    choice("weekly", "Weekly"),
                                    choice("monthly", "Monthly"),
                                ],
                            )
                            .on_change(pick(model, |draft, value| {
                                draft.app.subscription_default_schedule = value
                            })),
                        ))
                        .child(separator(theme))
                        .child(row(
                            theme,
                            "Posts per run",
                            ui::number_input("posts-per-run", app.subscription_default_posts_per_run, 1, 10_000)
                                .on_commit(number(model, |draft, value: i64| {
                                    draft.app.subscription_default_posts_per_run = value
                                })),
                        ))
                        .child(separator(theme))
                        .child(
                            ui::check("group-posts", "Group multi-media posts", app.subscription_default_group_posts)
                                .on_toggle(edit(model, |draft| {
                                    draft.app.subscription_default_group_posts =
                                        !draft.app.subscription_default_group_posts
                                })),
                        ),
                )
                .into_any_element(),
            block()
                .child(
                    card(theme)
                        .child(title(theme, "Inbox Capacity"))
                        .child(row(
                            theme,
                            "Maximum Inbox items",
                            ui::number_input("inbox-limit", app.subscription_inbox_item_limit, 1, 1_000_000)
                                .on_commit(number(model, |draft, value: i64| {
                                    draft.app.subscription_inbox_item_limit = value
                                })),
                        ))
                        .child(hint(
                            theme,
                            "Subscription work waits when the Inbox reaches this many top-level items.",
                        )),
                )
                .into_any_element(),
        ]
    }
}
