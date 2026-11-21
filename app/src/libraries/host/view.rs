//! Draws the main window's content: the gate or the library's shell, and
//! over it the consent question, the "Library Not Found" question and the
//! notice that blocks the window while another Picto is running.
use super::*;

impl Main {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            shell: None,
            gate: Gate::Closed,
            asking: Default::default(),
            missing: Default::default(),
            blocked: Default::default(),
            focus: cx.focus_handle(),
            notifications: cx.new(ui::notify::Host::new),
            offered: false,
            launched: false,
        }
    }

    fn gate(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = theme::theme(cx);
        let titlebar = ui::drag_region("library-gate-titlebar", window, cx)
            .h(px(theme::TITLEBAR_HEIGHT))
            .flex_none()
            .pl(px(74.))
            .pr(px(8.))
            .flex()
            .items_center()
            .justify_end()
            .border_b_1()
            .border_color(theme.border_secondary())
            .children(ui::window_chrome::caption_buttons("library-gate", window));
        let content = match &self.gate {
            Gate::Closed => {
                // The gate opens the Library Manager by itself, once. It
                // waits until the launch has asked for its library
                // (`launched`), until neither the consent question nor the
                // "Library Not Found" question is open, and until no library
                // is being opened. The window draws one or two frames with
                // the gate in this state before a library named at launch
                // starts to open, and the Library Manager must not open then.
                let waiting = !self.launched || self.asking.is_open() || host(cx).read(cx).busy;
                if !self.offered && !self.missing.is_open() && !waiting {
                    self.offered = true;
                    cx.defer(|cx| crate::libraries::manager::open(cx));
                }
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(10.))
                    .text_color(theme.text_tertiary())
                    .child(ui::icon("books", 30., theme.text_tertiary()))
                    .child(
                        div()
                            .mt(px(4.))
                            .text(theme::TEXT_XL)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text())
                            .child(t("Open a library to start")),
                    )
                    .child(
                        div()
                            .text(theme::TEXT_MD)
                            .text_color(theme.text_secondary())
                            .child(t(
                                "Create a new Picto library or open one already on this device.",
                            )),
                    )
                    .child(
                        div().mt(px(8.)).child(
                            ui::action_button::action_button(
                                "library-gate-choose",
                                ui::action_button::Look::Primary,
                            )
                            .label(t("Choose Library…"))
                            .on_click(|_, _, cx| crate::libraries::manager::open(cx)),
                        ),
                    )
                    .into_any_element()
            }
            Gate::Loading(_) => div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(10.))
                .child(
                    div()
                        .mt(px(4.))
                        .text(theme::TEXT_XL)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text())
                        .child(t("Opening your library")),
                )
                .child(
                    div()
                        .mt(px(8.))
                        .w(px(320.))
                        .child(ui::progress_bar::progress_bar().height(3.)),
                )
                .into_any_element(),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background())
            .text_color(theme.text())
            .font_family(theme::FONT_UI)
            .child(titlebar)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(content),
            )
            .into_any_element()
    }

    fn consent(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.asking.layer(window, cx)?;
        let message = consent_message(&dialog.content.root);
        let (presence, focus) = (dialog.presence, dialog.focus.clone());
        let close = cx.listener(|this: &mut Self, _: &(), _, cx| this.asking.dismiss(cx));
        let cancel = cx.listener(|this: &mut Self, _: &(), _, cx| {
            this.asking.close(cx);
        });
        let accept = cx.listener(|this: &mut Self, _: &(), _, cx| {
            if let Some(asking) = this.asking.content().cloned() {
                this.asking.dismiss(cx);
                cx.defer(move |cx| consented(asking, cx));
            }
        });
        Some(
            ui::confirm::confirm(
                "library-consent",
                en("Open this library directly?"),
                message,
                t("Open Library"),
                presence,
            )
            .layer(ui::layer::OVER_DIALOG)
            .focus(&focus)
            .on_close(move |window, cx| close(&(), window, cx))
            .on_cancel(move |window, cx| cancel(&(), window, cx))
            .on_confirm(move |window, cx| accept(&(), window, cx))
            .into_any_element(),
        )
    }

    /// Draws the "Library Not Found" question, with the buttons Locate…,
    /// Remove from List and Cancel.
    fn missing(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.missing.layer(window, cx)?;
        let (path, presence, focus) = (
            dialog.content.clone(),
            dialog.presence,
            dialog.focus.clone(),
        );
        let theme = theme::theme(cx);
        let name = crate::libraries::name(&path.to_string_lossy());
        let dismiss = |this: &mut Self, cx: &mut Context<Self>| this.missing.dismiss(cx);
        let close = cx.listener(move |this: &mut Self, _: &(), _, cx| dismiss(this, cx));
        let cancel = cx.listener(move |this: &mut Self, _: &ClickEvent, _, cx| {
            this.missing.close(cx);
        });
        let located = path.clone();
        let locate = cx.listener(move |this: &mut Self, _: &ClickEvent, _, cx| {
            dismiss(this, cx);
            let located = located.clone();
            cx.defer(move |cx| locate(located, true, cx));
        });
        let removed = path.to_string_lossy().into_owned();
        let remove = cx.listener(move |this: &mut Self, _: &ClickEvent, _, cx| {
            dismiss(this, cx);
            let removed = removed.clone();
            crate::state::update(cx, |state| {
                history::remove(
                    &mut state.library_history,
                    &mut state.pinned_libraries,
                    &mut state.last_library,
                    &removed,
                )
            });
            cx.defer(crate::set_app_menus);
        });
        // The locale files have no key for this message, so it is shown in
        // English in every language.
        let message = format!(
            "Library \"{name}\" could not be found.\n\nPath: {}",
            path.display()
        );
        Some(
            ui::modal::modal("library-missing", en("Library Not Found"), presence)
                .size(ui::modal::Size::Small)
                .layer(ui::layer::OVER_DIALOG)
                .focus(&focus)
                .on_close(move |window, cx| close(&(), window, cx))
                .child(
                    div()
                        .text(theme::TEXT_MD)
                        .line_height(px(19.5))
                        .text_color(theme.text_secondary())
                        .child(message),
                )
                .footer(
                    ui::action_button::action_button(
                        "library-missing-cancel",
                        ui::action_button::Look::Standard,
                    )
                    .label(t("Cancel"))
                    .on_click(cancel),
                )
                .footer(
                    ui::action_button::action_button(
                        "library-missing-remove",
                        ui::action_button::Look::Standard,
                    )
                    .label(t("Remove from List"))
                    .on_click(remove),
                )
                .footer(
                    ui::action_button::action_button(
                        "library-missing-locate",
                        ui::action_button::Look::Primary,
                    )
                    .label(t("Locate…"))
                    .on_click(locate),
                )
                .into_any_element(),
        )
    }

    fn notice(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.blocked.layer(window, cx)?;
        let (presence, focus) = (dialog.presence, dialog.focus.clone());
        Some(
            ui::confirm::confirm(
                "library-picto-running",
                en("Quit Picto first"),
                en("Picto is running while this app has a library you keep open. Both must never have a library open at the same time: quit Picto, then choose Check Again, or close the library here."),
                t("Check Again"),
                presence,
            )
            .cancel_label(en("Close Library"))
            .layer(ui::layer::OVER_DIALOG)
            .focus(&focus)
            // The notice cannot be dismissed. It stays until the other Picto
            // has quit or the library is closed.
            .on_close(|_, _| {})
            .on_cancel(|_, cx| cx.defer(close))
            .on_confirm(|_, cx| cx.defer(check_picto))
            .into_any_element(),
        )
    }
}

impl Render for Main {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match &self.shell {
            Some(shell) => shell.clone().into_any_element(),
            None => self.gate(window, cx),
        };
        let mut root = div()
            .id("main")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .child(content)
            .children(self.missing(window, cx))
            .children(self.consent(window, cx))
            .children(self.notice(window, cx));
        // The shell draws its own notifications. While the gate is shown,
        // this view draws the notifications.
        if self.shell.is_none() {
            root = root
                .child(self.notifications.clone())
                .children(ui::window_chrome::rim(window, cx));
        }
        root
    }
}
