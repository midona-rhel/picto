//! The group organizer (`GroupOrganizerModal.tsx`): grouping a selection
//! that needs a decision. With collections among the selection, which one
//! keeps its name, cover, order, folders and lifecycle ("Choose the Group to
//! Keep"); with none, the new group's name ("Create Group"). Either way the
//! notes the selection carries, which the group takes.
use crate::grid::format::grouped;
use crate::session::{Report, Reread, Session};
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{self, check, field::Look, text_input::TextInput};
use gpui::*;
use picto_library::RootId;
use picto_library::selection::{SelectionCollectionCandidate, SelectionTarget};

/// What the organizer groups, and what it found to decide between.
pub struct Request {
    pub target: SelectionTarget,
    /// The item whose cover the new group takes.
    pub cover: RootId,
    pub groups: Vec<SelectionCollectionCandidate>,
    pub notes: String,
    pub maximum_bytes: u64,
}

/// The group was made; the dialog closes.
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

/// `canSubmit`: not while saving, nor with notes over the limit; a winner
/// chosen, or a name typed.
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
        let name = cx.new(|cx| TextInput::dialog("Group name", cx));
        let notes = cx.new(|cx| {
            let mut input = TextInput::text_area("Optional notes for the collection", cx);
            input.set_text(request.notes, cx);
            input
        });
        let subscriptions = vec![
            cx.observe(&name, |_, _, cx| cx.notify()),
            cx.observe(&notes, |_, _, cx| cx.notify()),
            // Enter in the name submits (`onKeyDown` Enter).
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

    /// The field that takes the keyboard as the dialog opens: the name,
    /// when there is one to type (`autoFocus`).
    pub fn first_focus(&self, cx: &App) -> Option<FocusHandle> {
        (!self.choosing()).then(|| self.name.read(cx).focus_handle(cx))
    }

    fn choosing(&self) -> bool {
        !self.groups.is_empty()
    }

    pub fn title(&self) -> &'static str {
        if self.choosing() {
            "Choose the Group to Keep"
        } else {
            "Create Group"
        }
    }

    pub fn submit_label(&self) -> &'static str {
        match (self.saving, self.choosing()) {
            (true, _) => "Saving...",
            (false, true) => "Merge Groups",
            (false, false) => "Create Group",
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

    /// `submit`: groups the selection as decided, as one undoable change.
    /// A failure shows in the dialog, which stays open.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        if !self.can_submit(cx) {
            return;
        }
        self.saving = true;
        self.error = None;
        // `onBeforeSubmit`: the grid's selection clears first.
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
        let answer = self.session.update(cx, |session, cx| {
            session.edit_reporting(
                Report::announce("collections.organize"),
                move |library| library.organize_into_collection(&request),
                Reread::RESULTS,
                cx,
            )
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = answer.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                match result {
                    // TODO: select the new group once the results hold it
                    // (`onComplete`), as production does.
                    Ok(_) => cx.emit(Organized),
                    Err(error) => {
                        this.error = Some(error.to_string().into());
                        this.saving = false;
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// `.choice`: a radio, the group's name, and how many items it holds.
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
                    .child(format!("{} items", grouped(group.member_count))),
            )
    }
}

impl Render for Organizer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        // `.field`: a label over its control, 6px apart, in small
        // secondary type on the body's 20px lines.
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
                // `.help`: 12px under it, before the choices' own gap.
                div()
                    .mb(px(12.))
                    .text(theme::TEXT_SM)
                    .line_height(px(17.4))
                    .text_color(theme.text_secondary())
                    .child(
                        "The selected group keeps its name, cover, order, folders, and lifecycle. Every other selected item is appended to it.",
                    ),
            );
            for group in self.groups.clone() {
                let choice = self.choice(&group, cx);
                let id = group.collection_id;
                // The radios are one stop, at the chosen one, as a browser
                // makes a radio group.
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
                    .child(label("Name"))
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
            .child(label("Collection notes"))
            .child(ui::field("group-organizer-notes", &self.notes, Look::TextArea).rows(8))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .text(theme::TEXT_XS)
                    .line_height(px(20.))
                    .text_color(counter_color)
                    .child(format!(
                        "{} / {} bytes",
                        grouped(bytes as u64),
                        grouped(self.maximum_bytes)
                    )),
            );
        if too_long {
            notes = notes.child(
                div()
                    .text(theme::TEXT_SM)
                    .line_height(px(20.))
                    .text_color(theme.negative())
                    .child("Trim the collection notes before continuing."),
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
