//! The group organizer: the dialog that groups a selection when the user has
//! to decide something first. When the selection holds collections, the
//! user chooses the one that keeps its name, cover, order, folders and
//! lifecycle, and the dialog's title is "Choose the Group to Keep". When it
//! holds none, the user types the name of the new group, and the title is
//! "Create Group". In both cases the dialog shows the notes of the selected
//! items in a field that the user can edit, and the group takes the text of
//! that field as its notes.
use crate::i18n::format::grouped;
use crate::i18n::{en, t};
use crate::session::{Report, Reread, Session};
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{
    self, check,
    field::Look,
    text_input::{Kind, TextInput},
};
use gpui::*;
use picto_library::RootId;
use picto_library::selection::{SelectionCollectionCandidate, SelectionTarget};

/// Holds the selection that the organizer groups and what the user decides
/// about: the collections in the selection (`groups`) and the notes of the
/// selected items (`notes`), which may be at most `maximum_bytes` long.
pub struct Request {
    pub target: SelectionTarget,
    /// The item whose cover the new group takes.
    pub cover: RootId,
    pub groups: Vec<SelectionCollectionCandidate>,
    pub notes: String,
    pub maximum_bytes: u64,
}

/// The event that the organizer emits when the group has been made. The
/// shell then closes the dialog.
pub struct Organized;

impl EventEmitter<Organized> for Organizer {}

pub struct Organizer {
    session: Entity<Session>,
    target: SelectionTarget,
    cover: RootId,
    groups: Vec<SelectionCollectionCandidate>,
    winner: Option<RootId>,
    name: Entity<TextInput>,
    notes: Entity<TextInput>,
    maximum_bytes: u64,
    saving: bool,
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

/// Returns whether the dialog can be submitted. It cannot while a save is
/// running, nor while the notes are longer than `maximum_bytes`. Apart from
/// that, a dialog in which the user chooses between groups (`choosing`)
/// needs a chosen group (`winner`), and a dialog that makes a new group
/// needs a name that is not blank.
pub fn submittable(
    choosing: bool,
    winner: bool,
    name: &str,
    notes_bytes: usize,
    maximum_bytes: u64,
    saving: bool,
) -> bool {
    !saving
        && notes_bytes as u64 <= maximum_bytes
        && if choosing {
            winner
        } else {
            !name.trim().is_empty()
        }
}

impl Organizer {
    pub fn new(session: Entity<Session>, request: Request, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| {
            TextInput::new(Kind::Form, cx)
                .placeholder("Group name")
                .submits()
        });
        let notes = cx.new(|cx| {
            let mut input =
                TextInput::new(Kind::Area, cx).placeholder("Optional notes for the collection");
            input.set_text(request.notes, cx);
            input
        });
        let subscriptions = vec![
            cx.observe(&name, |_, _, cx| cx.notify()),
            cx.observe(&notes, |_, _, cx| cx.notify()),
            // Enter in the name field submits the dialog.
            cx.subscribe(&name, |this, _, _: &ui::text_input::Submitted, cx| {
                this.submit(cx)
            }),
        ];
        Self {
            session,
            target: request.target,
            cover: request.cover,
            winner: request.groups.first().map(|group| group.collection_id),
            groups: request.groups,
            name,
            notes,
            maximum_bytes: request.maximum_bytes,
            saving: false,
            error: None,
            _subscriptions: subscriptions,
        }
    }

    /// Returns the field that takes the keyboard focus when the dialog
    /// opens. That is the name field when a new group is made, and `None`
    /// when the user chooses between groups.
    pub fn first_focus(&self, cx: &App) -> Option<FocusHandle> {
        (!self.choosing()).then(|| self.name.read(cx).focus_handle(cx))
    }

    fn choosing(&self) -> bool {
        !self.groups.is_empty()
    }

    pub fn title(&self) -> &'static str {
        if self.choosing() {
            en("Choose the Group to Keep")
        } else {
            en("Create Group")
        }
    }

    pub fn submit_label(&self) -> &'static str {
        match (self.saving, self.choosing()) {
            (true, _) => t("Saving..."),
            (false, true) => en("Merge Groups"),
            (false, false) => en("Create Group"),
        }
    }

    pub fn saving(&self) -> bool {
        self.saving
    }

    pub fn can_submit(&self, cx: &App) -> bool {
        submittable(
            self.choosing(),
            self.winner.is_some(),
            &self.name.read(cx).value(),
            self.notes.read(cx).value().len(),
            self.maximum_bytes,
            self.saving,
        )
    }

    /// Groups the selection as the user decided, as one change that can be
    /// undone. A failure is shown in the dialog, and the dialog stays open.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        if !self.can_submit(cx) {
            return;
        }
        self.saving = true;
        self.error = None;
        // The grid's selection is cleared before the edit is made.
        self.session
            .update(cx, |session, cx| session.clear_selection(cx));
        let choosing = self.choosing();
        let notes = self.notes.read(cx).value().trim().to_owned();
        let request = picto_library::GroupRequest {
            target: self.target.clone(),
            cover_root_id: self.cover,
            winning_collection_id: self.winner.filter(|_| choosing),
            name: (!choosing).then(|| self.name.read(cx).value().trim().to_owned()),
            notes: (!notes.is_empty()).then_some(notes),
            modified_at_ms: crate::now_ms(),
        };
        crate::session::edit_then(
            &self.session,
            Report::announce("collections.organize"),
            Reread::RESULTS,
            move |library| library.organize_into_collection(&request),
            cx,
            |this, result, cx| {
                match result {
                    // The new group is selected alone once the results list
                    // it.
                    Ok((group, _)) => {
                        this.session
                            .update(cx, |session, cx| session.select_when_listed(group, cx));
                        cx.emit(Organized);
                    }
                    Err(error) => {
                        this.error = Some(error.to_string().into());
                        this.saving = false;
                    }
                }
                cx.notify();
            },
        );
        cx.notify();
    }

    /// Draws the row of one group that can be chosen: a radio button, the
    /// group's name, and the number of items in the group.
    fn choice(
        &self,
        group: &SelectionCollectionCandidate,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let id = group.collection_id;
        let chosen = self.winner == Some(id);
        let name = if group.label.is_empty() {
            format!("Group {}", id.0)
        } else {
            group.label.clone()
        };
        div()
            .id(("group-choice", id.0 as u64))
            .role(Role::RadioButton)
            .aria_label(name.clone())
            .aria_toggled(crate::ui::a11y::toggled(chosen))
            .min_h(px(38.))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(10.))
            .rounded(px(theme::RADIUS_SM))
            .bg(theme.surface_2())
            .hover(move |style| style.bg(theme.surface_hover()))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.winner = Some(id);
                cx.notify();
            }))
            .child(
                div().flex_none().child(
                    check::check_box(
                        if chosen {
                            check::Mark::Selected
                        } else {
                            check::Mark::Empty
                        },
                        check::Size::Compact,
                    )
                    .round(true),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text(theme::TEXT_MD)
                    .text_color(theme.text())
                    .child(name),
            )
            .child(
                div()
                    .flex_none()
                    .text(theme::TEXT_XS)
                    .text_color(theme.text_tertiary())
                    .child(format!("{} {}", grouped(group.member_count), t("items"))),
            )
    }
}

impl Render for Organizer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        // Draws the label of a field. The label stands above its control
        // with 6 px between them. It has the small text size, the secondary
        // text colour and a line 20 px high.
        let label = |text: &'static str| {
            div()
                .text(theme::TEXT_SM)
                .line_height(px(20.))
                .text_color(theme.text_secondary())
                .child(text)
        };
        let mut body = div().flex().flex_col();
        if self.choosing() {
            let mut choices = div().flex().flex_col().gap(px(6.)).child(
                // The help text has 12 px below it, in addition to the 6 px
                // gap between the choices.
                div()
                    .mb(px(12.))
                    .text(theme::TEXT_SM)
                    .line_height(px(17.4))
                    .text_color(theme.text_secondary())
                    .child(
                        t("The selected group keeps its name, cover, order, folders, and lifecycle. Every other selected item is appended to it."),
                    ),
            );
            for group in self.groups.clone() {
                let choice = self.choice(&group, cx);
                let id = group.collection_id;
                // The radio buttons are one stop for the Tab key, and the
                // stop is on the chosen one. A radio group on a web page
                // behaves the same way.
                choices = if self.winner == Some(id) {
                    choices.child(ui::focus::stop(
                        ("group-choice", id.0 as u64),
                        choice,
                        cx.listener(move |this, _, _, cx| {
                            this.winner = Some(id);
                            cx.notify();
                        }),
                    ))
                } else {
                    choices.child(choice)
                };
            }
            body = body.child(choices);
        } else {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(label(t("Name")))
                    .child(ui::field("group-organizer-name", &self.name, Look::Modal)),
            );
        }
        let bytes = self.notes.read(cx).value().len();
        let too_long = bytes as u64 > self.maximum_bytes;
        let counter_color = if too_long {
            theme.negative()
        } else {
            theme.text_tertiary()
        };
        let mut notes = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(label(t("Collection notes")))
            .child(ui::field("group-organizer-notes", &self.notes, Look::TextArea).rows(8))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .text(theme::TEXT_XS)
                    .line_height(px(20.))
                    .text_color(counter_color)
                    .child(format!(
                        "{} / {} {}",
                        grouped(bytes as u64),
                        grouped(self.maximum_bytes),
                        t("bytes")
                    )),
            );
        if too_long {
            notes = notes.child(
                div()
                    .text(theme::TEXT_SM)
                    .line_height(px(20.))
                    .text_color(theme.negative())
                    .child(t("Trim the collection notes before continuing.")),
            );
        }
        body = body.child(notes);
        if let Some(error) = self.error.clone() {
            body = body.child(
                div()
                    .mt(px(10.))
                    .text(theme::TEXT_SM)
                    .text_color(theme.negative())
                    .child(error),
            );
        }
        body
    }
}

#[cfg(test)]
mod tests {
    use super::submittable;

    #[test]
    fn a_merge_needs_a_winner_and_a_new_group_a_name() {
        assert!(submittable(true, true, "", 0, 100, false));
        assert!(!submittable(true, false, "", 0, 100, false));
        assert!(submittable(false, false, "Trip", 0, 100, false));
        assert!(!submittable(false, false, "   ", 0, 100, false));
    }

    #[test]
    fn notes_over_the_limit_or_a_save_in_flight_hold_it_back() {
        assert!(submittable(true, true, "", 100, 100, false));
        assert!(!submittable(true, true, "", 101, 100, false));
        assert!(!submittable(false, false, "Trip", 0, 100, true));
    }
}
