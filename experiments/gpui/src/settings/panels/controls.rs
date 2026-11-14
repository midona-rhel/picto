//! The Controls section: the grid's default controls and how items are
//! presented.
use crate::grid::layout::ViewMode;
use crate::grid::prefs;
use crate::i18n::t;
use crate::settings::pieces::{
    block, card, control, edit, hint, label, number, pick, row, separator, title, two_columns,
    value_label,
};
use crate::settings::window::SettingsWindow;
use crate::theme;
use crate::ui::{self, action_button::Look, choice};
use gpui::*;
use picto_library::predicate::{SortDirection, SortField};

impl SettingsWindow {
    pub(super) fn controls(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let model = &self.model;
        let state = model.read(cx);
        let (app, grid) = (&state.draft.app, &state.draft.grid);
        let resetting = state.reset_views;
        const WIDTH: f32 = 220.;
        // "Mouse wheel" is read by the grid's wheel handler on each turn:
        // "Adjust thumbnail size" zooms the tiles (`grid::nav::wheel_zoom`).
        let mouse = {
            let mut content = card(theme).child(title(theme, t("Mouse"))).child(row(
                theme,
                t("Mouse wheel"),
                ui::select(
                    "wheel",
                    app.grid_wheel_action,
                    vec![
                        choice("scroll", "Scroll grid"),
                        choice("zoom", "Adjust thumbnail size"),
                    ],
                )
                .width(WIDTH)
                .on_change(pick(model, |draft, value| {
                    draft.app.grid_wheel_action = value
                })),
            ));
            if cfg!(target_os = "macos") {
                content = content.child(separator(theme)).child(row(
                    theme,
                    t("Media view gestures"),
                    ui::select(
                        "gestures",
                        if app.viewer_trackpad_gestures {
                            "trackpad"
                        } else {
                            "wheel_zoom"
                        },
                        vec![
                            choice("wheel_zoom", "Wheel zoom"),
                            choice("trackpad", "Trackpad pan + pinch zoom"),
                        ],
                    )
                    .width(WIDTH)
                    .on_change(pick(model, |draft, value| {
                        draft.app.viewer_trackpad_gestures = value == "trackpad"
                    })),
                ));
            }
            content
                .child(separator(theme))
                .child(row(
                    theme,
                    t("Double-click"),
                    ui::select(
                        "double-click",
                        app.grid_double_click_action,
                        vec![
                            choice("detail", "Open Media View"),
                            choice("external", "Open in default app"),
                        ],
                    )
                    .width(WIDTH)
                    .on_change(pick(model, |draft, value| {
                        draft.app.grid_double_click_action = value
                    })),
                ))
                .child(separator(theme))
                .child(row(
                    theme,
                    t("Middle-click"),
                    ui::select(
                        "middle-click",
                        app.grid_middle_click_action,
                        vec![
                            choice("new_window", "Open in new window"),
                            choice("none", "Do nothing"),
                        ],
                    )
                    .width(WIDTH)
                    .on_change(pick(model, |draft, value| {
                        draft.app.grid_middle_click_action = value
                    })),
                ))
        };
        let size = grid.target_size.unwrap_or(220.);
        let fit = grid.thumbnail_fit.as_deref() == Some("cover");
        let flag = |value: Option<bool>, fallback: bool| value.unwrap_or(fallback);
        let labels: [(&str, bool, fn(&mut super::model::GridDefaults)); 6] = [
            (t("Fit thumbnails"), fit, |grid| {
                grid.thumbnail_fit = Some(
                    if grid.thumbnail_fit.as_deref() == Some("cover") {
                        "contain"
                    } else {
                        "cover"
                    }
                    .into(),
                )
            }),
            (t("Show name"), flag(grid.show_name, true), |grid| {
                grid.show_name = Some(!grid.show_name.unwrap_or(true))
            }),
            (
                t("Show resolution"),
                flag(grid.show_resolution, false),
                |grid| grid.show_resolution = Some(!grid.show_resolution.unwrap_or(false)),
            ),
            (
                t("Show extension"),
                flag(grid.show_extension, false),
                |grid| grid.show_extension = Some(!grid.show_extension.unwrap_or(false)),
            ),
            (t("Show label"), flag(grid.show_label, false), |grid| {
                grid.show_label = Some(!grid.show_label.unwrap_or(false))
            }),
            (
                t("Show item count"),
                flag(grid.show_item_count, true),
                |grid| grid.show_item_count = Some(!grid.show_item_count.unwrap_or(true)),
            ),
        ];
        let mut checks = two_columns();
        for (index, (name, on, toggle)) in labels.into_iter().enumerate() {
            checks = checks.child(
                ui::check(("grid-label", index), name, on)
                    .on_toggle(edit(model, move |draft| toggle(&mut draft.grid))),
            );
        }
        let half = |name: &'static str, child: AnyElement| {
            div()
                .flex_1()
                .min_w_0()
                .min_h(px(26.))
                .flex()
                .items_center()
                .child(label(theme).child(name))
                .child(control().child(child))
        };
        let grid_defaults = card(theme)
            .child(title(theme, t("Grid Defaults")))
            .child(row(
                theme,
                t("Default layout"),
                ui::select(
                    "layout",
                    SharedString::from(
                        grid.view_mode
                            .clone()
                            .unwrap_or_else(|| prefs::mode_value(ViewMode::Waterfall).into()),
                    ),
                    crate::shell::view_menu::layout_choices(),
                )
                .on_change(pick(model, |draft, value| draft.grid.view_mode = Some(value.into()))),
            ))
            .child(separator(theme))
            .child(row(
                theme,
                t("Thumbnail size"),
                div()
                    .flex()
                    .items_center()
                    // `.settingControl`'s gap.
                    .gap(px(6.))
                    .child(
                        ui::slider("thumbnail-size", size, 100., 600., 50.).on_change(number(
                            model,
                            |draft, value: f64| draft.grid.target_size = Some(value),
                        )),
                    )
                    .child(value_label(theme, format!("{}px", size.round() as i64))),
            ))
            .child(separator(theme))
            .child(row(
                theme,
                t("Grid spacing"),
                ui::select(
                    "spacing",
                    app.grid_spacing,
                    vec![choice("wide", "Wide"), choice("tight", "Tight")],
                )
                .on_change(pick(model, |draft, value| draft.app.grid_spacing = value)),
            ))
            .child(separator(theme))
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .child(half(
                        t("Sort by"),
                        ui::select(
                            "sort-field",
                            SharedString::from(
                                grid.sort_field
                                    .clone()
                                    .unwrap_or_else(|| prefs::sort_value(SortField::ImportedAt).into()),
                            ),
                            // `SORT_FIELD_OPTIONS`: Settings lists Rating
                            // before File Size, unlike the view panel.
                            [
                                SortField::ImportedAt,
                                SortField::CapturedAt,
                                SortField::Name,
                                SortField::Rating,
                                SortField::TotalSize,
                                SortField::Random,
                            ]
                            .map(|field| choice(prefs::sort_value(field), prefs::sort_label(field)))
                            .to_vec(),
                        )
                        .on_change(pick(model, |draft, value| {
                            draft.grid.sort_field = Some(value.into())
                        }))
                        .into_any_element(),
                    ))
                    .child(
                        div()
                            .w(px(1.))
                            .h(px(26.))
                            .mx(px(12.))
                            .flex_none()
                            .self_center()
                            .bg(theme.border_secondary()),
                    )
                    .child(half(
                        t("Order"),
                        ui::select(
                            "sort-order",
                            SharedString::from(
                                grid.sort_order.clone().unwrap_or_else(|| {
                                    prefs::direction_value(SortDirection::Descending).into()
                                }),
                            ),
                            vec![
                                choice(prefs::direction_value(SortDirection::Ascending), t("Ascending"))
                                    .icon("select-sort-ascending"),
                                choice(prefs::direction_value(SortDirection::Descending), t("Descending"))
                                    .icon("select-sort-descending"),
                            ],
                        )
                        .on_change(pick(model, |draft, value| {
                            draft.grid.sort_order = Some(value.into())
                        }))
                        .into_any_element(),
                    )),
            )
            .child(separator(theme))
            .child(checks)
            .child(separator(theme))
            .child(row(
                theme,
                t("View overrides"),
                ui::action_button("reset-views", Look::Settings).label(t("Reset all views"))
                    .disabled(resetting)
                    .on_click({
                        let model = model.clone();
                        move |_, _, cx| {
                            model.update(cx, |model, cx| {
                                model.reset_views = true;
                                cx.notify();
                            })
                        }
                    }),
            ))
            .child(hint(
                theme,
                t("Clears saved layout, sorting, and display choices so every view inherits these defaults. Inbox resets to oldest items first."),
            ));
        vec![
            block().child(mouse).into_any_element(),
            block()
                .child(
                    card(theme).child(title(theme, t("Keyboard"))).child(row(
                        theme,
                        t("Space key"),
                        ui::select(
                            "space-key",
                            app.space_key_action,
                            vec![
                                choice("quick_look", "Quick Look"),
                                choice("scroll", "Scroll page"),
                            ],
                        )
                        .width(WIDTH)
                        .on_change(pick(model, |draft, value| {
                            draft.app.space_key_action = value
                        })),
                    )),
                )
                .into_any_element(),
            block().child(grid_defaults).into_any_element(),
        ]
    }
}
