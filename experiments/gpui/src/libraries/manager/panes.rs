//! The Library Manager's panes: the header, the sidebar of libraries, the
//! library's pane with its cards and statistics, New Library, the error
//! and message line, and the footer.
use super::*;

impl Manager {
    pub(super) fn header(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        ui::drag_region("library-manager-header", window, cx)
            .h(px(HEADER))
            .flex_none()
            .pl(px(16.))
            .pr(px(12.))
            .flex()
            .items_center()
            .border_b_1()
            .border_color(theme.border_secondary())
            .child(
                div()
                    .flex_1()
                    .text(theme::TEXT_LG)
                    .font_weight(FontWeight::BOLD)
                    .child(t("Library Manager")),
            )
            .child(ui::window_chrome::close_button(
                "library-manager-close",
                |_, window, _| window.remove_window(),
            ))
    }

    pub(super) fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let mut list = div().flex().flex_col().gap(px(2.));
        if self.rows.is_empty() {
            list = list.child(
                div()
                    .px(px(8.))
                    .py(px(12.))
                    .text(theme::TEXT_SM)
                    .text_color(theme.text_tertiary())
                    .child(t("No libraries yet.")),
            );
        }
        for (index, row) in self.rows.iter().enumerate() {
            let selected = self.selected.as_deref() == Some(row.path.as_str());
            let status = if row.current {
                Some(ui::icon("check", 13., theme.text()))
            } else if row.pinned {
                Some(ui::icon("pin", 12., theme.text_tertiary()))
            } else if row.missing {
                Some(ui::icon("alert-circle", 12., theme.negative()))
            } else {
                None
            };
            let path = row.path.clone();
            let open = row.path.clone();
            let can_open = !row.current && !row.missing;
            list = list.child(
                div()
                    .id(("library-row", index))
                    .role(Role::Button)
                    .aria_label(row.name.clone())
                    .aria_selected(selected)
                    .h(px(ROW))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(theme::RADIUS_MD))
                    .cursor_pointer()
                    .when(selected, |row| row.bg(theme.surface_active()))
                    .when(!selected, |row| {
                        row.hover(move |style| style.bg(theme.surface_hover()))
                    })
                    .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                        if event.click_count() >= 2 && can_open {
                            this.open_row(open.clone(), cx);
                        } else {
                            this.choose(path.clone(), cx);
                        }
                    }))
                    .child(
                        ui::library_avatar(("library-row-avatar", index), row.cover.clone(), 42.)
                            .appearance(row.look.icon.clone(), look_color(&row.look))
                            .highlighted(selected),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text(theme::TEXT_MD)
                                    .truncate()
                                    .child(row.name.clone()),
                            )
                            .child(
                                div()
                                    .text(theme::TEXT_CAPTION)
                                    .text_color(theme.text_tertiary())
                                    .truncate()
                                    .child(row.dir.clone()),
                            ),
                    )
                    .children(row.current.then(|| self.row_sync_dot(cx)).flatten())
                    .children(status),
            );
        }
        div()
            .w(px(SIDEBAR))
            .flex_none()
            .h_full()
            .px(px(8.))
            .py(px(14.))
            .border_r_1()
            .border_color(theme.border_secondary())
            // A further 3% of the contrast colour over the window's.
            .bg(theme.rail_raised())
            .flex()
            .flex_col()
            .child(
                div()
                    .mx(px(8.))
                    .mb(px(7.))
                    .text(theme::TEXT_CAPTION)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_tertiary())
                    .child(t("On this device")),
            )
            .child(
                div()
                    .id("library-rows")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            )
    }

    pub(super) fn card(title: &'static str, cx: &App) -> Div {
        let theme = theme::theme(cx);
        div()
            .p(px(14.))
            .border_1()
            .border_color(theme.border_secondary())
            .rounded(px(theme::RADIUS_LG))
            .bg(theme.control_surface())
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .text(theme::TEXT_MD)
                    .font_weight(FontWeight::MEDIUM)
                    .child(t(title)),
            )
    }

    pub(super) fn library_pane(&self, row: &Row, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = theme::theme(cx);
        let can_open = !row.current && !row.missing;
        let open_path = row.path.clone();
        let mut actions = div().flex().items_center().gap(px(8.));
        if can_open {
            actions = actions.child(
                ui::action_button::action_button("library-open", ui::action_button::Look::Primary)
                    .label(t("Open Library"))
                    .disabled(self.busy)
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.open_row(open_path.clone(), cx)),
                    ),
            );
        }
        actions = actions.child(
            ui::icon_button("library-actions", "dots-vertical")
                .name(t("Library actions"))
                .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                    this.open_actions(event.position(), window, cx)
                })),
        );
        let title = match &self.renaming {
            Some(field) => div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        // `width: min(240px, 100%)`.
                        .flex_none()
                        .w(relative(1.))
                        .max_w(px(240.))
                        .h(px(28.))
                        .px(px(8.))
                        .rounded(px(theme::RADIUS_MD))
                        .border_1()
                        .border_color(theme.primary_at(60.))
                        .flex()
                        .items_center()
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                            match event.keystroke.key.as_str() {
                                "escape" => {
                                    this.renaming = None;
                                    cx.notify();
                                }
                                _ => {}
                            }
                        }))
                        .child(field.clone()),
                )
                .child(
                    ui::action_button::action_button(
                        "library-rename-save",
                        ui::action_button::Look::Primary,
                    )
                    .label(t("Save"))
                    .on_click(cx.listener(|this, _, _, cx| this.save_rename(cx))),
                )
                .child(
                    ui::action_button::action_button(
                        "library-rename-cancel",
                        ui::action_button::Look::Standard,
                    )
                    .label(t("Cancel"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.renaming = None;
                        cx.notify();
                    })),
                )
                .into_any_element(),
            None => div()
                .text(theme::TEXT_XL)
                .font_weight(FontWeight::BOLD)
                .truncate()
                .child(row.name.clone())
                .into_any_element(),
        };
        let hero = div()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                ui::library_avatar("library-hero-avatar", row.cover.clone(), 84.)
                    .appearance(row.look.icon.clone(), look_color(&row.look))
                    .highlighted(true),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(title)
                    .child(
                        div()
                            .text(theme::TEXT_CAPTION)
                            .text_color(theme.text_tertiary())
                            .truncate()
                            .child(row.path.clone()),
                    ),
            )
            .child(actions);
        let mut status = div()
            .min_h(px(32.))
            .flex()
            .items_center()
            .gap(px(14.))
            .text(theme::TEXT_CAPTION)
            .text_color(theme.text_secondary());
        if row.current {
            status = status.child(t("Current library"));
        }
        if row.kind == Kind::Copy {
            status = status.child(en("Library copy"));
        }
        if row.pinned {
            status = status.child(t("Pinned"));
        }
        if row.missing {
            status = status.child(
                div()
                    .text_color(theme.negative())
                    .child(t("Missing on disk")),
            );
        }
        // `Appearance`: the colour and icon (kept in this app's state, as
        // production keeps them in its `config.json`), and the cover,
        // chosen in the main window's cover dialog for the open library.
        let current = row.current;
        let label = |text: &'static str| {
            div()
                .w(px(64.))
                .flex_none()
                .text(theme::TEXT_SM)
                .text_color(theme.text_secondary())
                .child(t(text))
        };
        let colored = row.path.clone();
        let colors = ui::color_picker::color_picker("library-color", row.look.color.clone())
            .on_pick({
                let manager = cx.entity().downgrade();
                move |color, _, cx| {
                    let path = colored.clone();
                    let _ = manager.update(cx, |this, cx| {
                        this.set_look(path, |look| look.color = color.map(str::to_owned), cx)
                    });
                }
            });
        let (iconed, icon) = (row.path.clone(), row.look.icon.clone());
        // i18n: translated where drawn
        let appearance = Self::card("Appearance", cx)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(label("Color"))
                    .child(div().w(px(260.)).child(colors)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(label("Icon"))
                    .child(
                        ui::action_button::action_button(
                            "library-icon",
                            ui::action_button::Look::Standard,
                        )
                        .label(if self.icons.is_some() {
                            t("Done")
                        } else {
                            t("Change…")
                        })
                        .disabled(self.busy)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.toggle_icons(iconed.clone(), icon.clone(), cx)
                        })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(label("Cover"))
                    .child(
                        ui::action_button::action_button(
                            "library-cover",
                            ui::action_button::Look::Standard,
                        )
                        .label(t("Choose…"))
                        .disabled(self.busy || !current)
                        .tip(ui::tip(if current {
                            t("Choose a media item and crop the library cover")
                        } else {
                            t("Open this library before choosing its media")
                        }))
                        .on_click(|_, _, cx| cx.defer(choose_cover)),
                    ),
            )
            .children(self.icons.clone().map(|icons| {
                div()
                    .mt(px(2.))
                    .rounded(px(theme::RADIUS_MD))
                    .border_1()
                    .border_color(theme.border_secondary())
                    .w(px(ui::icon_picker::WIDTH))
                    .child(icons)
            }));
        // i18n: translated where drawn
        let statistics = Self::card("Library", cx)
            .mt(px(12.))
            .child(self.statistics(row, cx));
        // i18n: translated where drawn
        let cloud = Self::card("Cloud Sync", cx).mt(px(12.)).child(if current {
            self.cloud_card(cx)
        } else {
            div()
                .text(theme::TEXT_SM)
                .text_color(theme.text_tertiary())
                .child(t("Open this library to configure its cloud location."))
                .into_any_element()
        });
        let _ = window;
        div()
            .flex()
            .flex_col()
            .child(hero)
            .child(status)
            .child(appearance)
            .child(statistics)
            .child(cloud)
            .into_any_element()
    }

    pub(super) fn statistics(&self, row: &Row, cx: &App) -> AnyElement {
        let theme = theme::theme(cx);
        let line = |text: &str| {
            div()
                .text(theme::TEXT_SM)
                .text_color(theme.text_tertiary())
                .child(text.to_owned())
                .into_any_element()
        };
        if !row.current {
            return line(t("Open this library to view its statistics."));
        }
        let Some(stats) = &self.stats else {
            return line(t("Loading library statistics…"));
        };
        let count =
            |key: &str| crate::i18n::format::grouped(i128::from(stats[key].as_i64().unwrap_or(0)));
        // i18n: translated where drawn
        let fields = [
            ("All", count("active_items")),
            ("Inbox", count("inbox_items")),
            ("Trash", count("trash_items")),
            ("Media", count("media_assets")),
            ("Images", count("image_assets")),
            ("Tags", count("tags")),
            ("Subscriptions", count("subscriptions")),
            (
                "Size",
                crate::text::size_to_terabytes(stats["original_bytes"].as_u64().unwrap_or(0)),
            ),
        ];

        let mut grid = div().flex().flex_wrap().gap_y(px(12.));
        for (label, value) in fields {
            grid = grid.child(
                div()
                    .w(relative(0.25))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text(theme::TEXT_LG)
                            .font_weight(FontWeight::MEDIUM)
                            .child(value),
                    )
                    .child(
                        div()
                            .text(theme::TEXT_CAPTION)
                            .text_color(theme.text_tertiary())
                            .child(t(label)),
                    ),
            );
        }
        grid.into_any_element()
    }

    pub(super) fn create_pane(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = theme::theme(cx);
        let field = self.name.focus_handle(cx);
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::icon("library-plus", 18., theme.text()))
                    .child(
                        div()
                            .text(theme::TEXT_XL)
                            .font_weight(FontWeight::BOLD)
                            .child(t("Create a new library")),
                    ),
            )
            .child(
                div()
                    .text(theme::TEXT_MD)
                    .text_color(theme.text_secondary())
                    .child(t(
                        "Choose a name now; Picto will ask where the library should be stored.",
                    )),
            )
            .child(
                div()
                    .mt(px(6.))
                    .text(theme::TEXT_SM)
                    .text_color(theme.text_secondary())
                    .child(t("Library name")),
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
                    .cursor(CursorStyle::IBeam)
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        window.focus(&field, cx)
                    })
                    .child(self.name.clone()),
            )
            .child(
                div().mt(px(4.)).child(
                    ui::action_button::action_button(
                        "library-create",
                        ui::action_button::Look::Primary,
                    )
                    .label(if self.busy {
                        t("Creating…")
                    } else {
                        t("Choose Location…")
                    })
                    .disabled(self.busy)
                    .on_click(cx.listener(|this, _, _, cx| this.create(cx))),
                ),
            )
            .into_any_element()
    }

    pub(super) fn feedback(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = theme::theme(cx);
        if let Some(error) = &self.error {
            return Some(
                div()
                    .mt(px(14.))
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(px(theme::RADIUS_LG))
                    .border_1()
                    .border_color(theme.negative_at(45.))
                    .bg(theme.control_surface())
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .overflow_hidden()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text(theme::TEXT_SM)
                            .text_color(theme.text())
                            .child(error.clone()),
                    )
                    .child(
                        // i18n: an icon's name
                        ui::icon_button("library-error-dismiss", "x")
                            .name(t("Dismiss notification"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.error = None;
                                cx.notify();
                            })),
                    )
                    .into_any_element(),
            );
        }
        self.message.as_ref().map(|message| {
            div()
                .mt(px(14.))
                .text(theme::TEXT_SM)
                .text_color(theme.text_secondary())
                .child(message.clone())
                .into_any_element()
        })
    }

    pub(super) fn footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        div()
            .h(px(FOOTER))
            .flex_none()
            .px(px(14.))
            .flex()
            .items_center()
            .justify_end()
            .gap(px(8.))
            .border_t_1()
            .border_color(theme.border_secondary())
            .bg(theme.footer_surface())
            .child(
                ui::action_button::action_button("library-new", ui::action_button::Look::Standard)
                    .icon("plus", 14.)
                    .label(t("New Library…"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.pane = Pane::Create;
                        this.error = None;
                        window.focus(&this.name.focus_handle(cx), cx);
                        cx.notify();
                    })),
            )
            .child(
                ui::action_button::action_button(
                    "library-open-existing",
                    ui::action_button::Look::Standard,
                )
                .icon("library-folder-open", 14.)
                .label(t("Open Existing…"))
                .on_click(|_, _, cx| cx.defer(host::open_dialog)),
            )
            .child(
                ui::action_button::action_button(
                    "library-open-cloud",
                    ui::action_button::Look::Standard,
                )
                .icon("IconCloud", 14.)
                .label(t("Open from Cloud…"))
                .disabled(self.busy || self.working.is_some())
                .on_click(cx.listener(|this, _, _, cx| this.show_cloud_open(cx))),
            )
    }
}
