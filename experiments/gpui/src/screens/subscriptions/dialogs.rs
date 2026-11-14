//! The screen's dialogs: a new subscription (`NewSubscriptionDialog`), a
//! gallery download (`AddGalleryDialog`), a rename (`RenameDialog`), a
//! source's edit (`QueryEditModal`) and details, the confirmations the
//! screen asks before deleting or resetting, and the cover
//! (`SubscriptionCoverDialog`, in [`super::cover_dialog`]).
use super::{SubscriptionsScreen, cover_dialog, data, format, requests};
use crate::theme::Text as _;
use crate::ui::{
    self,
    action_button::Look as ButtonLook,
    field::Look,
    menu::Return,
    modal::Size,
    text_input::{Kind, Submitted, TextInput},
};
use crate::{theme, ui::confirm::confirm};
use gpui::{prelude::FluentBuilder as _, *};

/// The open dialog and what it holds.
pub enum Dialog {
    Create {
        name: Entity<TextInput>,
        _subscriptions: Vec<Subscription>,
    },
    Gallery {
        service: &'static str,
        url: Entity<TextInput>,
        _subscriptions: Vec<Subscription>,
    },
    Rename {
        id: i64,
        current: String,
        name: Entity<TextInput>,
        _subscriptions: Vec<Subscription>,
    },
    EditQuery(Box<EditQuery>),
    Details(Box<data::Query>),
    Confirm(Confirmation),
    Cover(Box<cover_dialog::Cover>),
}

/// A source being edited: its site and the fields of `QueryEditModal`.
pub struct EditQuery {
    query: i64,
    /// Kept as it is: the dialog does not edit it.
    group_posts: bool,
    site: &'static str,
    text: Entity<TextInput>,
    display_name: Entity<TextInput>,
    notes: Entity<TextInput>,
    _subscriptions: Vec<Subscription>,
}

/// A question asked before an edit that cannot be taken back.
pub struct Confirmation {
    title: String,
    message: &'static str,
    label: &'static str,
    danger: bool,
    action: Confirmed,
}

/// What a confirmation does once confirmed.
#[derive(Clone, Debug, PartialEq)]
pub enum Confirmed {
    Delete(Vec<i64>),
    Reset(i64),
    DeleteQuery(i64),
}

/// `AddGalleryDialog`'s services.
const SERVICES: [(&str, &str); 2] = [("ehentai", "E-Hentai"), ("exhentai", "ExHentai")];

/// The gallery address field's example for a service.
fn gallery_placeholder(service: &str) -> &'static str {
    if service == "exhentai" {
        "https://exhentai.org/g/12345/67890abcde/"
    } else {
        "https://e-hentai.org/g/12345/67890abcde/"
    }
}

/// `.row`: an 84px label on the right of its column, then the control.
fn form_row(label: &'static str, control: impl IntoElement, cx: &App) -> Div {
    let theme = theme::theme(cx);
    div()
        .min_w_0()
        .flex()
        .items_start()
        .gap(px(12.))
        .child(
            div()
                .w(px(84.))
                .flex_none()
                .pt(px(6.))
                .text(theme::TEXT_SM)
                // The dialog body's 20px line.
                .line_height(px(theme::LINE_HEIGHT_MD))
                .text_color(theme.text_secondary())
                .text_right()
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(control),
        )
}

/// `.formField`: a label over its control, 14px above the next.
fn form_field(label: impl Into<SharedString>, control: impl IntoElement, cx: &App) -> Div {
    div()
        .min_w_0()
        .mb(px(14.))
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(
            div()
                .text(theme::TEXT_MD)
                .line_height(px(theme::LINE_HEIGHT_MD))
                .text_color(theme::theme(cx).text())
                .child(label.into()),
        )
        .child(control)
}

impl SubscriptionsScreen {
    /// Opens `dialog`; the keyboard goes to `first` and back to the screen
    /// once it has gone.
    pub(super) fn show_dialog(
        &mut self,
        dialog: Dialog,
        first: Option<FocusHandle>,
        cx: &mut Context<Self>,
    ) {
        self.close_menu(cx);
        let opened = self.dialog.open(dialog, Return::To(self.focus.clone()), cx);
        if let Some(first) = first {
            opened.focus_first(first);
        }
    }

    fn dismiss_dialog(&mut self, cx: &mut Context<Self>) {
        self.dialog.dismiss(cx);
    }

    /// A text field of a dialog whose Enter submits it and whose changes
    /// repaint the dialog's buttons.
    fn dialog_field(
        &mut self,
        placeholder: &'static str,
        text: &str,
        submit: fn(&mut Self, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> (Entity<TextInput>, Vec<Subscription>) {
        let input = cx.new(|cx| {
            TextInput::new(Kind::Form, cx)
                .placeholder(placeholder)
                .submits()
        });
        if !text.is_empty() {
            input.update(cx, |input, cx| input.set_text(text.to_owned(), cx));
        }
        let subscriptions = vec![
            cx.subscribe(&input, move |this, _, _: &Submitted, cx| submit(this, cx)),
            cx.observe(&input, |_, _, cx| cx.notify()),
        ];
        (input, subscriptions)
    }

    // ── New subscription ────────────────────────────────────────

    /// The header's Add.
    pub(super) fn open_create(&mut self, cx: &mut Context<Self>) {
        let (name, subscriptions) =
            self.dialog_field("e.g. Favourite artists", "", Self::submit_create, cx);
        let first = name.read(cx).focus_handle(cx);
        self.show_dialog(
            Dialog::Create {
                name,
                _subscriptions: subscriptions,
            },
            Some(first),
            cx,
        );
    }

    /// Add: the subscription is made with no sources, on the schedule and
    /// run limit Settings gives new ones, and opened once it exists.
    fn submit_create(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::Create { name, .. }) = self.dialog.content() else {
            return;
        };
        let name = name.clone();
        let typed = name.read(cx).value().trim().to_owned();
        if typed.is_empty() || self.busy() {
            return;
        }
        // The field empties at once, as the Electron dialog's does.
        name.update(cx, |input, cx| input.clear(cx));
        let settings = crate::settings::current(cx);
        let request = requests::create(
            &typed,
            settings.subscription_default_schedule,
            settings.subscription_default_posts_per_run,
        );
        self.act(vec![request], cx, |this, answer, cx| {
            if let Some(id) = answer
                .ok()
                .and_then(|answer| answer.get("subscription_id")?.as_i64())
            {
                this.pending_open = Some(id);
                if matches!(this.dialog.content(), Some(Dialog::Create { .. })) {
                    this.dialog.close(cx);
                }
            }
        });
    }

    // ── Gallery ─────────────────────────────────────────────────

    /// One gallery downloads at a time.
    pub(super) fn gallery_running(&self) -> bool {
        self.list.iter().flatten().any(|subscription| {
            format::is_gallery_job(subscription)
                && subscription.run_status.is_some()
                && subscription.active_run_id.is_some()
        })
    }

    /// The header's Add Gallery.
    pub(super) fn open_gallery(&mut self, cx: &mut Context<Self>) {
        let (url, mut subscriptions) =
            self.dialog_field(gallery_placeholder("ehentai"), "", Self::submit_gallery, cx);
        // The service follows an address typed or pasted for the other.
        subscriptions.push(cx.observe(&url, |this, url, cx| {
            let typed = url.read(cx).value();
            if let Some(found) = format::gallery_service(&typed) {
                this.choose_service(found, cx);
            }
        }));
        let first = url.read(cx).focus_handle(cx);
        self.show_dialog(
            Dialog::Gallery {
                service: "ehentai",
                url,
                _subscriptions: subscriptions,
            },
            Some(first),
            cx,
        );
    }

    fn choose_service(&mut self, chosen: &'static str, cx: &mut Context<Self>) {
        if let Some(Dialog::Gallery { service, url, .. }) = self.dialog.content_mut()
            && *service != chosen
        {
            *service = chosen;
            url.update(cx, |input, cx| {
                input.set_placeholder(gallery_placeholder(chosen));
                cx.notify();
            });
            cx.notify();
        }
    }

    /// Add Gallery: the download starts, and the dialog closes once it has.
    fn submit_gallery(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::Gallery { url, .. }) = self.dialog.content() else {
            return;
        };
        let address = url.read(cx).value().trim().to_owned();
        if address.is_empty() || self.busy() || self.gallery_running() {
            return;
        }
        self.act(
            vec![requests::start_gallery(&address)],
            cx,
            |this, answer, cx| {
                if answer.is_ok() && matches!(this.dialog.content(), Some(Dialog::Gallery { .. })) {
                    this.dialog.close(cx);
                }
            },
        );
    }

    // ── Rename ──────────────────────────────────────────────────

    /// The menu's Rename and a double-click on the rail's name.
    pub(super) fn open_rename(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let _ = window;
        let Some(current) = self.subscription(id).map(|entry| entry.name.clone()) else {
            return;
        };
        let (name, subscriptions) = self.dialog_field("Name", &current, Self::submit_rename, cx);
        name.update(cx, |input, cx| input.select_everything(cx));
        let first = name.read(cx).focus_handle(cx);
        self.show_dialog(
            Dialog::Rename {
                id,
                current,
                name,
                _subscriptions: subscriptions,
            },
            Some(first),
            cx,
        );
    }

    /// Rename: an empty or unchanged name closes the dialog; another is
    /// saved, and the dialog closes once it has been.
    fn submit_rename(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::Rename {
            id, current, name, ..
        }) = self.dialog.content()
        else {
            return;
        };
        let (id, typed) = (*id, name.read(cx).value().trim().to_owned());
        if self.busy() {
            return;
        }
        if typed.is_empty() || typed == *current {
            self.dismiss_dialog(cx);
            return;
        }
        self.act(vec![requests::rename(id, &typed)], cx, |this, _, cx| {
            if matches!(this.dialog.content(), Some(Dialog::Rename { .. })) {
                this.dialog.close(cx);
            }
        });
    }

    // ── Sources ─────────────────────────────────────────────────

    /// A source's Edit.
    pub(super) fn open_edit_query(&mut self, query: &data::Query, cx: &mut Context<Self>) {
        let site = format::site(&query.site_id).map_or("", |site| site.id);
        let example = format::site(site).map_or("", |site| site.example_query);
        let (text, mut subscriptions) =
            self.dialog_field(example, &query.text, Self::save_query, cx);
        let (display_name, more) = self.dialog_field(
            "Shown instead of the query text",
            query.display_name.as_deref().unwrap_or_default(),
            Self::save_query,
            cx,
        );
        subscriptions.extend(more);
        let (notes, more) = self.dialog_field(
            "",
            query.notes.as_deref().unwrap_or_default(),
            Self::save_query,
            cx,
        );
        subscriptions.extend(more);
        self.show_dialog(
            Dialog::EditQuery(Box::new(EditQuery {
                query: query.id,
                group_posts: query.group_posts,
                site,
                text,
                display_name,
                notes,
                _subscriptions: subscriptions,
            })),
            None,
            cx,
        );
    }

    fn choose_query_site(&mut self, site: &'static str, cx: &mut Context<Self>) {
        if let Some(Dialog::EditQuery(edit)) = self.dialog.content_mut() {
            edit.site = site;
            let example = format::site(site).map_or("", |site| site.example_query);
            edit.text.update(cx, |input, cx| {
                input.set_placeholder(example);
                cx.notify();
            });
            cx.notify();
        }
    }

    /// Save: the source is updated, and the dialog closes once it has been.
    fn save_query(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::EditQuery(edit)) = self.dialog.content() else {
            return;
        };
        let optional = |input: &Entity<TextInput>| {
            Some(input.read(cx).value().trim().to_owned()).filter(|text| !text.is_empty())
        };
        let source = requests::Source {
            site: edit.site.to_owned(),
            text: edit.text.read(cx).value().trim().to_owned(),
            display_name: optional(&edit.display_name),
            notes: optional(&edit.notes),
            group_posts: edit.group_posts,
        };
        if source.site.is_empty() || source.text.is_empty() || self.busy() {
            return;
        }
        let query = edit.query;
        self.act(
            vec![requests::update_query(query, &source)],
            cx,
            |this, _, cx| {
                if matches!(this.dialog.content(), Some(Dialog::EditQuery(_))) {
                    this.dialog.close(cx);
                }
            },
        );
    }

    /// A double-click on a source's row.
    pub(super) fn open_details(&mut self, query: &data::Query, cx: &mut Context<Self>) {
        self.show_dialog(Dialog::Details(Box::new(query.clone())), None, cx);
    }

    // ── Confirmations ───────────────────────────────────────────

    fn ask(&mut self, confirmation: Confirmation, cx: &mut Context<Self>) {
        self.show_dialog(Dialog::Confirm(confirmation), None, cx);
    }

    pub(super) fn confirm_delete(&mut self, id: i64, cx: &mut Context<Self>) {
        self.ask(
            Confirmation {
                title: "Delete Subscription".into(),
                message: "Downloaded files stay in your library. The subscription and its queries are removed.",
                label: "Delete",
                danger: true,
                action: Confirmed::Delete(vec![id]),
            },
            cx,
        );
    }

    /// The bulk menu's Delete and ⌘⌫ (`deleteSubscriptions`).
    pub(super) fn confirm_delete_all(&mut self, ids: Vec<i64>, cx: &mut Context<Self>) {
        let total = ids.len();
        self.ask(
            Confirmation {
                title: format!(
                    "Delete {total} Subscription{}",
                    if total == 1 { "" } else { "s" }
                ),
                message: "Downloaded files stay in your library. The selected subscriptions and their queries are removed.",
                label: "Delete",
                danger: true,
                action: Confirmed::Delete(ids),
            },
            cx,
        );
    }

    pub(super) fn confirm_reset(&mut self, id: i64, cx: &mut Context<Self>) {
        self.ask(
            Confirmation {
                title: "Reset Sync History".into(),
                message: "Picto will clear this subscription's handled-file history and scan every query from the beginning. Existing media and account login remain untouched; previously deleted source media can be downloaded again.",
                label: "Reset",
                danger: false,
                action: Confirmed::Reset(id),
            },
            cx,
        );
    }

    pub(super) fn confirm_delete_query(&mut self, query: i64, cx: &mut Context<Self>) {
        self.ask(
            Confirmation {
                title: "Delete Query".into(),
                message: "This query stops syncing. Downloaded files stay in your library.",
                label: "Delete",
                danger: true,
                action: Confirmed::DeleteQuery(query),
            },
            cx,
        );
    }

    /// The confirming button: the dialog goes and the edit is made.
    fn confirmed(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::Confirm(confirmation)) = self.dialog.content() else {
            return;
        };
        let action = confirmation.action.clone();
        self.dismiss_dialog(cx);
        let requests = match &action {
            Confirmed::Delete(ids) => ids.iter().map(|id| requests::delete(*id)).collect(),
            Confirmed::Reset(id) => vec![requests::reset(*id)],
            Confirmed::DeleteQuery(query) => vec![requests::delete_query(*query)],
        };
        if let Confirmed::Delete(ids) = &action {
            self.selected.retain(|id| !ids.contains(id));
        }
        self.act(requests, cx, |_, _, _| {});
    }

    // ── Drawing ─────────────────────────────────────────────────

    /// The open dialog, drawn over the window.
    pub(super) fn dialog_layer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let busy = self.busy();
        let gallery_running = self.gallery_running();
        let view = cx.entity().downgrade();
        let dismiss = {
            let view = view.clone();
            move |_: &mut Window, cx: &mut App| {
                let _ = view.update(cx, |this, cx| this.dismiss_dialog(cx));
            }
        };
        let close_now = |view: WeakEntity<Self>| {
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                let _ = view.update(cx, |this, cx| {
                    this.dialog.close(cx);
                });
            }
        };
        let dialog = self.dialog.layer(window, cx)?;
        let (presence, focus) = (dialog.presence, dialog.focus.clone());
        if matches!(dialog.content, Dialog::Cover(_)) {
            return Some(cover_dialog::cover_layer(self, presence, focus, window, cx));
        }
        let dialog = self.dialog.content()?;
        let theme = theme::theme(cx);
        let element = match dialog {
            Dialog::Create { name, .. } => {
                let typed = !name.read(cx).value().trim().is_empty();
                ui::modal("subscription-create", "Add subscription", presence)
                    .size(Size::Small)
                    .focus(&focus)
                    .on_close(dismiss)
                    .child(div().flex().flex_col().gap(px(12.)).child(form_row(
                        "Name",
                        ui::field("subscription-create-name", name, Look::FormRow),
                        cx,
                    )))
                    .footer(
                        ui::action_button("subscription-create-cancel", ButtonLook::Standard)
                            .label("Cancel")
                            .on_click(close_now(view.clone())),
                    )
                    .footer(
                        ui::action_button("subscription-create-add", ButtonLook::Primary)
                            .label("Add")
                            .disabled(!typed || busy)
                            .on_click(cx.listener(|this, _, _, cx| this.submit_create(cx))),
                    )
                    .into_any_element()
            }
            Dialog::Gallery { service, url, .. } => {
                let typed = !url.read(cx).value().trim().is_empty();
                let choose = view.clone();
                ui::modal("subscription-gallery", "Add gallery", presence)
                    .size(Size::Small)
                    .focus(&focus)
                    .on_close(dismiss)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .child(form_row(
                                "Service",
                                ui::select(
                                    "subscription-gallery-service",
                                    *service,
                                    SERVICES
                                        .iter()
                                        .map(|(id, label)| ui::choice(id, *label))
                                        .collect(),
                                )
                                .width(220.)
                                .on_change(
                                    move |service, _, cx| {
                                        let _ = choose.update(cx, |this, cx| {
                                            this.choose_service(service, cx)
                                        });
                                    },
                                ),
                                cx,
                            ))
                            .child(form_row(
                                "Gallery URL",
                                ui::field("subscription-gallery-url", url, Look::FormRow),
                                cx,
                            )),
                    )
                    .footer(
                        ui::action_button("subscription-gallery-cancel", ButtonLook::Standard)
                            .label("Cancel")
                            .on_click(close_now(view.clone())),
                    )
                    .footer(
                        ui::action_button("subscription-gallery-add", ButtonLook::Primary)
                            .label("Add Gallery")
                            .disabled(busy || gallery_running || !typed)
                            .on_click(cx.listener(|this, _, _, cx| this.submit_gallery(cx))),
                    )
                    .into_any_element()
            }
            Dialog::Rename { name, .. } => {
                let typed = !name.read(cx).value().trim().is_empty();
                ui::modal("subscription-rename", "Rename Subscription", presence)
                    .size(Size::Small)
                    .focus(&focus)
                    .on_close(dismiss)
                    .child(ui::field("subscription-rename-name", name, Look::Modal))
                    .footer(
                        ui::action_button("subscription-rename-cancel", ButtonLook::Standard)
                            .label("Cancel")
                            .disabled(busy)
                            .on_click(close_now(view.clone())),
                    )
                    .footer(
                        ui::action_button("subscription-rename-submit", ButtonLook::Primary)
                            .label("Rename")
                            .disabled(busy || !typed)
                            .on_click(cx.listener(|this, _, _, cx| this.submit_rename(cx))),
                    )
                    .into_any_element()
            }
            Dialog::EditQuery(edit) => {
                let site = format::site(edit.site);
                let can_save =
                    !edit.site.is_empty() && !edit.text.read(cx).value().trim().is_empty();
                let choose = view.clone();
                ui::modal("subscription-edit-query", "Edit query", presence)
                    .size(Size::Medium)
                    .focus(&focus)
                    .on_close(dismiss)
                    .child(form_field(
                        "Site",
                        div().flex().child(
                            ui::select(
                                "subscription-edit-site",
                                edit.site,
                                format::sites_by_name()
                                    .into_iter()
                                    .map(|site| ui::choice(site.id, site.name))
                                    .collect(),
                            )
                            .on_change(move |site, _, cx| {
                                let _ =
                                    choose.update(cx, |this, cx| this.choose_query_site(site, cx));
                            }),
                        ),
                        cx,
                    ))
                    .child(form_field(
                        if site.is_some_and(|site| site.supports_query) {
                            "Search"
                        } else {
                            "Account"
                        },
                        ui::field("subscription-edit-text", &edit.text, Look::Modal),
                        cx,
                    ))
                    .child(form_field(
                        "Display name (optional)",
                        ui::field("subscription-edit-name", &edit.display_name, Look::Modal),
                        cx,
                    ))
                    .child(form_field(
                        "Notes (optional)",
                        ui::field("subscription-edit-notes", &edit.notes, Look::Modal),
                        cx,
                    ))
                    .footer(
                        ui::action_button("subscription-edit-cancel", ButtonLook::Standard)
                            .label("Cancel")
                            .on_click(close_now(view.clone())),
                    )
                    .footer(
                        // `pending`: a save on its way blocks another without
                        // dimming the button.
                        ui::action_button("subscription-edit-save", ButtonLook::Primary)
                            .label("Save")
                            .disabled(!can_save)
                            .on_click(cx.listener(|this, _, _, cx| this.save_query(cx))),
                    )
                    .into_any_element()
            }
            Dialog::Details(query) => {
                let now = chrono::Local::now();
                let mut rows = vec![
                    ("Source", format::site_label(&query.site_id)),
                    (
                        "Query",
                        query
                            .display_name
                            .as_deref()
                            .map(str::trim)
                            .filter(|name| !name.is_empty())
                            .unwrap_or(&query.text)
                            .to_owned(),
                    ),
                    ("Posts", crate::grid::format::grouped(query.posts)),
                    ("Media", crate::grid::format::grouped(query.media)),
                    (
                        "Last check",
                        match format::query_last_check(query) {
                            Some(time) => format::relative_time(Some(time), now),
                            None => "Never".to_owned(),
                        },
                    ),
                    ("State", format::query_state(query).to_owned()),
                ];
                if let Some(error) = &query.last_failure_message {
                    rows.push(("Last error", error.clone()));
                }
                let last = rows.len() - 1;
                let mut table = div().flex().flex_col();
                for (index, (label, value)) in rows.into_iter().enumerate() {
                    // `.queryStats > div`: a 92px label column and the value
                    // on the right, ruled between rows.
                    table = table.child(
                        div()
                            .min_h(px(28.))
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .when(index != last, |row| {
                                row.border_b_1().border_color(theme.border_secondary())
                            })
                            .text(theme::TEXT_SM)
                            .text_color(theme.text_tertiary())
                            .child(div().w(px(92.)).flex_none().child(label))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_right()
                                    .text_color(theme.text())
                                    .child(value),
                            ),
                    );
                }
                ui::modal("subscription-source-details", "Source details", presence)
                    .size(Size::Small)
                    .focus(&focus)
                    .on_close(dismiss)
                    .child(table)
                    .into_any_element()
            }
            Dialog::Confirm(confirmation) => confirm(
                "subscription-confirm",
                confirmation.title.clone(),
                confirmation.message,
                confirmation.label,
                presence,
            )
            .danger(confirmation.danger)
            .focus(&focus)
            .on_close(dismiss)
            .on_confirm({
                let view = view.clone();
                move |_, cx| {
                    let _ = view.update(cx, |this, cx| this.confirmed(cx));
                }
            })
            .into_any_element(),
            Dialog::Cover(_) => unreachable!("drawn above"),
        };
        Some(element)
    }
}
