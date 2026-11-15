//! The main window's content: the gate, or the library's shell, with the
//! consent question, the missing library's question and the blocking
//! notice over it.
use super::*;

impl Main {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            shell: None,
            gate: Gate::Closed,
            asking: None,
            missing: None,
            blocked: None,
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
                // `LibraryGate` opens the Library Manager once by itself,
                // after the question about a missing library (production
                // asks it before the window opens).
                // Only once the launch has had its say and nothing is
                // opening or being asked about: a library given at launch
                // leaves the gate closed for a frame or two first.
                let waiting = !self.launched || self.asking.is_some() || host(cx).read(cx).busy;
                if !self.offered && self.missing.is_none() && !waiting {
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

    fn consent(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let asking = self.asking.as_ref()?;
        let message = consent_message(&asking.root);
        let cancel = cx.listener(|this: &mut Self, _: &(), _, cx| {
            this.asking = None;
            cx.notify();
        });
        let cancel_too = cx.listener(|this: &mut Self, _: &(), _, cx| {
            this.asking = None;
            cx.notify();
        });
        let accept = cx.listener(|this: &mut Self, _: &(), _, cx| {
            if let Some(asking) = this.asking.take() {
                cx.notify();
                cx.defer(move |cx| consented(asking, cx));
            }
        });
        Some(
            ui::confirm::confirm(
                "library-consent",
                en("Open this library directly?"),
                message,
                t("Open Library"),
                asking.presence,
            )
            .layer(ui::layer::OVER_DIALOG)
            .on_close(move |window, cx| cancel(&(), window, cx))
            .on_cancel(move |window, cx| cancel_too(&(), window, cx))
            .on_confirm(move |window, cx| accept(&(), window, cx))
            .into_any_element(),
        )
    }

    /// `handleMissingLibrary`'s message box: Locate…, Remove from List,
    /// Cancel.
    fn missing(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (path, presence) = self.missing.as_ref()?;
        let theme = theme::theme(cx);
        let name = crate::libraries::name(&path.to_string_lossy());
        let dismiss = |this: &mut Self, cx: &mut Context<Self>| {
            this.missing = None;
            cx.notify();
        };
        let close = cx.listener(move |this: &mut Self, _: &(), _, cx| dismiss(this, cx));
        let cancel = cx.listener(move |this: &mut Self, _: &ClickEvent, _, cx| dismiss(this, cx));
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
        // Production's main process writes these in English, without keys.
        let message = format!(
            "Library \"{name}\" could not be found.\n\nPath: {}",
            path.display()
        );
        Some(
            ui::modal::modal("library-missing", en("Library Not Found"), *presence)
                .size(ui::modal::Size::Small)
                .layer(ui::layer::OVER_DIALOG)
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

    fn notice(&self) -> Option<AnyElement> {
        let (_, presence) = self.blocked.as_ref()?;
        Some(
            ui::confirm::confirm(
                "library-picto-running",
                en("Quit Picto first"),
                en("Picto is running while this app has a library you keep open. Both must never have a library open at the same time: quit Picto, then choose Check Again, or close the library here."),
                t("Check Again"),
                *presence,
            )
            .cancel_label(en("Close Library"))
            .layer(ui::layer::OVER_DIALOG)
            // It stays until Picto has quit or the library is closed.
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
            .children(self.missing(cx))
            .children(self.consent(cx))
            .children(self.notice());
        // The shell draws its own notifications; the gate needs these.
        if self.shell.is_none() {
            root = root
                .child(self.notifications.clone())
                .children(ui::window_chrome::rim(window, cx));
        }
        root
    }
}
