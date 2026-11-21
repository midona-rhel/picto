//! The dialogs of the Tags screen: the tag editor, the dialog that chooses
//! the tag to merge a tag into, the dialogs that create and rename a group,
//! and the confirmations.
use super::{TagsScreen, model};
use crate::i18n::t;
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{
    self,
    action_button::Look as ButtonLook,
    field::Look,
    modal::{Presence, Size},
    text_input::{Kind, Submitted, TextInput},
};
use gpui::{prelude::FluentBuilder as _, *};
use picto_library::{TagNamespaceRecord, TagRecord};

/// The largest number of tags that the merge dialog asks the library for
/// when it searches.
const PICKER_PAGE_SIZE: usize = 40;

pub enum Dialog {
    Editor {
        tag: TagRecord,
        name: Entity<TextInput>,
        _subscriptions: Vec<Subscription>,
    },
    /// The dialog in which the user chooses the tag to merge `tag` into.
    Merge {
        tag: TagRecord,
        search: Entity<TextInput>,
        query: String,
        results: Vec<TagRecord>,
        loading: bool,
        error: Option<String>,
        generation: u64,
        _subscriptions: Vec<Subscription>,
    },
    DeleteTag {
        tag: TagRecord,
    },
    RenameGroup {
        group: TagNamespaceRecord,
        name: Entity<TextInput>,
        _subscriptions: Vec<Subscription>,
    },
    DeleteGroup {
        group: TagNamespaceRecord,
    },
    CreateGroup {
        name: Entity<TextInput>,
        _subscriptions: Vec<Subscription>,
    },
    /// The confirmation for deleting the tags that are assigned to no item.
    Cleanup,
}

impl Dialog {
    /// Returns whether this is the tag editor, or the merge or delete dialog
    /// that is opened over the editor.
    pub fn belongs_to_editor(&self) -> bool {
        matches!(
            self,
            Dialog::Editor { .. } | Dialog::Merge { .. } | Dialog::DeleteTag { .. }
        )
    }

    /// Returns the field that gets the keyboard focus when the dialog
    /// opens. The result is `None` for the tag editor and the confirmations,
    /// and the dialog itself then gets the keyboard focus. The name field of
    /// the tag editor gets it when the user clicks the field.
    fn input(&self) -> Option<&Entity<TextInput>> {
        match self {
            Dialog::RenameGroup { name, .. } | Dialog::CreateGroup { name, .. } => Some(name),
            Dialog::Merge { search, .. } => Some(search),
            _ => None,
        }
    }
}

/// An open dialog. `presence` holds the time at which it opened and, once it
/// was dismissed, the time at which it began to fade out.
pub struct Layer {
    pub dialog: Dialog,
    pub presence: Presence,
    /// The focus handle of the dialog itself. It has the keyboard focus
    /// while no field of the dialog has it.
    pub focus: FocusHandle,
}

impl Layer {
    /// Gives the keyboard focus to the field that `Dialog::input` returns,
    /// or to the dialog itself when it returns none.
    pub fn focus_in(&self, window: &mut Window, cx: &mut App) {
        match self.dialog.input() {
            Some(input) => window.focus(&input.read(cx).focus_handle(cx), cx),
            None => window.focus(&self.focus, cx),
        }
    }
}

fn input_with(
    placeholder: &'static str,
    text: Option<String>,
    cx: &mut Context<TagsScreen>,
) -> Entity<TextInput> {
    cx.new(|cx| {
        let mut input = TextInput::new(Kind::Form, cx)
            .placeholder(placeholder)
            .submits();
        if let Some(text) = text {
            input.set_text(text, cx);
            input.caret_at_end(cx);
        }
        input
    })
}

pub fn editor(tag: TagRecord, cx: &mut Context<TagsScreen>) -> Dialog {
    let name = input_with(t("namespace:tag"), Some(crate::tags::record_name(&tag)), cx);
    let submitted = cx.subscribe(&name, |this, _, _: &Submitted, cx| this.submit_editor(cx));
    // The screen is drawn again when the text changes, because the Rename
    // button is disabled while the field is empty.
    let typed = cx.observe(&name, |_, _, cx| cx.notify());
    Dialog::Editor {
        tag,
        name,
        _subscriptions: vec![submitted, typed],
    }
}

pub fn merge(tag: &TagRecord, cx: &mut Context<TagsScreen>) -> Dialog {
    let search = input_with(t("Search existing tags"), None, cx);
    let typed = cx.observe(&search, |this, search, cx| {
        let query = search.read(cx).value();
        this.search_merge(query, cx);
    });
    Dialog::Merge {
        tag: tag.clone(),
        search,
        query: String::new(),
        results: Vec::new(),
        loading: false,
        error: None,
        generation: 0,
        _subscriptions: vec![typed],
    }
}

pub fn rename_group(group: TagNamespaceRecord, cx: &mut Context<TagsScreen>) -> Dialog {
    let name = input_with("", Some(group.name.clone()), cx);
    let submitted = cx.subscribe(&name, |this, _, _: &Submitted, cx| {
        this.submit_group_name(cx)
    });
    let typed = cx.observe(&name, |_, _, cx| cx.notify());
    Dialog::RenameGroup {
        group,
        name,
        _subscriptions: vec![submitted, typed],
    }
}

pub fn create_group(cx: &mut Context<TagsScreen>) -> Dialog {
    let name = input_with(t("Group name"), None, cx);
    let submitted = cx.subscribe(&name, |this, _, _: &Submitted, cx| {
        this.submit_group_name(cx)
    });
    let typed = cx.observe(&name, |_, _, cx| cx.notify());
    Dialog::CreateGroup {
        name,
        _subscriptions: vec![submitted, typed],
    }
}

/// Returns the normalized name that is typed in a group dialog, if the
/// dialog may submit it: the name is valid and is not `current`, the name
/// that the group already has.
fn group_name(input: &Entity<TextInput>, current: Option<&str>, cx: &App) -> Option<String> {
    let name = model::normalized_group_name(&input.read(cx).value());
    (model::valid_group_name(&name) && Some(name.as_str()) != current).then_some(name)
}

/// Returns the name of the group that a tag chip shows before the tag's
/// name. It is `None` for a tag of the General group, and when the setting
/// `show_tag_prefixes` is off.
fn prefix(namespace: &str, cx: &App) -> Option<SharedString> {
    let show = crate::settings::current(cx).show_tag_prefixes;
    crate::tags::group_of(namespace)
        .filter(|_| show)
        .map(SharedString::from)
}

/// Returns a line of help text for a dialog, such as "Searching..." in the
/// merge dialog.
fn hint(text: impl Into<SharedString>, cx: &App) -> Div {
    ui::form::help_text(theme::theme(cx), text)
}

impl TagsScreen {
    fn layer_index(&self, focus: &FocusHandle) -> Option<usize> {
        self.layers.iter().position(|layer| layer.focus == *focus)
    }

    /// Starts the fade-out of the dialog whose focus handle is `focus`. The
    /// dialog's close button, the Escape key and a click outside the dialog
    /// call this.
    fn dismiss(&mut self, focus: &FocusHandle, cx: &mut Context<Self>) {
        if let Some(index) = self.layer_index(focus) {
            self.layers[index].presence.close();
            cx.notify();
        }
    }

    /// Removes the dialog whose focus handle is `focus` without a fade-out.
    /// The Cancel buttons call this.
    fn cancel(&mut self, focus: &FocusHandle, cx: &mut Context<Self>) {
        if let Some(index) = self.layer_index(focus) {
            self.layers.remove(index);
            self.refocus = true;
            cx.notify();
        }
    }

    /// Renames the tag of the open tag editor to the text of the editor's
    /// name field. It does nothing while an edit is running, when the field
    /// is empty, or when the text is the name the tag already has.
    fn submit_editor(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some((tag, value)) = self.layers.iter().find_map(|layer| match &layer.dialog {
            Dialog::Editor { tag, name, .. } => Some((tag.clone(), name.read(cx).value())),
            _ => None,
        }) else {
            return;
        };
        let value = value.trim().to_owned();
        if !value.is_empty() && value != crate::tags::record_name(&tag) {
            self.rename_tag(&tag, value, true, cx);
        }
    }

    /// Submits the topmost dialog, if it is the Rename tag group or the
    /// Create tag group dialog and the name typed in it may be submitted.
    /// The Enter key in the dialog's field and the dialog's button call
    /// this.
    fn submit_group_name(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(top) = self.layers.last() else {
            return;
        };
        match &top.dialog {
            Dialog::RenameGroup { group, name, .. } => {
                if let Some(next) = group_name(name, Some(&group.name), cx) {
                    let group = group.clone();
                    self.rename_group(group, next, cx);
                }
            }
            Dialog::CreateGroup { name, .. } => {
                if let Some(next) = group_name(name, None, cx) {
                    self.create_group(next, cx);
                }
            }
            _ => {}
        }
    }

    /// Searches the tags for the merge dialog. It asks the library for the
    /// best matches for `typed` and leaves out the tag that is being
    /// merged. An empty search text empties the list of results.
    fn search_merge(&mut self, typed: String, cx: &mut Context<Self>) {
        let Some(Dialog::Merge {
            tag,
            query,
            results,
            loading,
            error,
            generation,
            ..
        }) = self
            .layers
            .iter_mut()
            .map(|layer| &mut layer.dialog)
            .find(|dialog| matches!(dialog, Dialog::Merge { .. }))
        else {
            return;
        };
        if *query == typed {
            return;
        }
        *query = typed.clone();
        *generation += 1;
        let (asked, exclude) = (*generation, tag.tag_id);
        let trimmed = typed.trim().to_owned();
        *error = None;
        if trimmed.is_empty() {
            results.clear();
            *loading = false;
            cx.notify();
            return;
        }
        *loading = true;
        cx.notify();
        let answer = self.session.read(cx).run(move |library| {
            library
                .tag_page(None, Some(&trimmed), 0, PICKER_PAGE_SIZE)
                .map(|(tags, _, _)| {
                    tags.into_iter()
                        .filter(|tag| tag.tag_id != exclude)
                        .collect::<Vec<_>>()
                })
                .map_err(|error| error.to_string())
        });
        cx.spawn(async move |this, cx| {
            let Ok(answer) = answer.await else { return };
            let _ = this.update(cx, |this: &mut Self, cx| {
                for layer in &mut this.layers {
                    if let Dialog::Merge {
                        results,
                        loading,
                        error,
                        generation,
                        ..
                    } = &mut layer.dialog
                        && *generation == asked
                    {
                        match answer {
                            Ok(found) => *results = found,
                            Err(message) => *error = Some(message),
                        }
                        *loading = false;
                        cx.notify();
                        return;
                    }
                }
            });
        })
        .detach();
    }

    /// Builds one open dialog, which is drawn over the whole window. `index`
    /// is the dialog's position in `layers`.
    pub(super) fn layer(
        &self,
        index: usize,
        layer: &Layer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = theme::theme(cx);
        let busy = self.busy;
        let focus = layer.focus.clone();
        let dismiss = {
            let focus = focus.clone();
            std::rc::Rc::new(cx.listener(move |this, _: &(), _, cx| this.dismiss(&focus, cx)))
        };
        let cancel_button = |cx: &mut Context<Self>| {
            let focus = focus.clone();
            ui::action_button(("cancel", index), ButtonLook::Standard)
                .label(t("Cancel"))
                .disabled(busy)
                .on_click(cx.listener(move |this, _, _, cx| this.cancel(&focus, cx)))
        };
        let dialog = |title: &'static str, size: Size| {
            let dismiss = dismiss.clone();
            ui::modal(("tags-dialog", index), title, layer.presence)
                .size(size)
                .focus(&focus)
                .layer(ui::layer::DIALOG + index)
                .on_close(move |window, cx| dismiss(&(), window, cx))
        };
        // Builds a confirmation. Its Cancel button removes it without a
        // fade-out, and both of its buttons are disabled while an edit is
        // running.
        let confirm = |title: &'static str,
                       message: String,
                       label: &'static str,
                       danger: bool,
                       action: Box<dyn Fn(&mut Self, &mut Context<Self>)>,
                       cx: &mut Context<Self>| {
            let dismiss = dismiss.clone();
            let cancel = {
                let focus = focus.clone();
                cx.listener(move |this, _: &(), _, cx| this.cancel(&focus, cx))
            };
            let accept = cx.listener(move |this, _: &(), _, cx| action(this, cx));
            ui::confirm::confirm(
                ("tags-dialog", index),
                title,
                message,
                label,
                layer.presence,
            )
            .danger(danger)
            .busy(busy)
            .focus(&focus)
            .layer(ui::layer::DIALOG + index)
            .on_close(move |window, cx| dismiss(&(), window, cx))
            .on_cancel(move |window, cx| cancel(&(), window, cx))
            .on_confirm(move |window, cx| accept(&(), window, cx))
            .into_any_element()
        };
        let element = match &layer.dialog {
            Dialog::Editor { tag, name, .. } => {
                let typed = name.read(cx).value();
                let (merge_tag, delete_tag) = (tag.clone(), tag.clone());
                dialog(t("Edit tag"), Size::Large).child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(18.))
                        .child(
                            // The top of the editor: an icon, the tag as a
                            // chip, and the number of items that the tag is
                            // assigned to.
                            div()
                                .flex()
                                .items_center()
                                .gap(px(10.))
                                .child(
                                    div()
                                        .size(px(34.))
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(9.))
                                        .bg(theme.surface_2())
                                        .child(ui::icon(
                                            "pin-bookmark",
                                            18.,
                                            theme.text_secondary(),
                                        )),
                                )
                                .child(
                                    div()
                                        .child(
                                            div().flex().child(
                                                ui::chip(
                                                    ("editor-chip", tag.tag_id.0 as usize),
                                                    tag.subname.clone(),
                                                    crate::tags::chip_color(&tag.namespace),
                                                )
                                                .namespace(prefix(&tag.namespace, cx)),
                                            ),
                                        )
                                        .child(
                                            div()
                                                .mt(px(7.))
                                                .text_color(theme.text_secondary())
                                                .text(theme::TEXT_SM)
                                                // The line is 20 px high,
                                                // like the lines of text in
                                                // a dialog's body.
                                                .line_height(px(20.))
                                                .child(format!(
                                                    "{} {}",
                                                    tag.assignment_count,
                                                    t("media items")
                                                )),
                                        ),
                                ),
                        )
                        .child(
                            // The label "Tag name" with the Rename button,
                            // and under them the field with the tag's name.
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .gap(px(12.))
                                        .child(
                                            div()
                                                .text(theme::TEXT_MD)
                                                .line_height(px(theme::LINE_HEIGHT_MD))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child(t("Tag name")),
                                        )
                                        .child(
                                            ui::action_button(
                                                "editor-rename",
                                                ButtonLook::Standard,
                                            )
                                            .label(t("Rename"))
                                            .icon("tags-edit", 14.)
                                            .disabled(busy || typed.trim().is_empty())
                                            .on_click(
                                                cx.listener(|this, _, _, cx| {
                                                    this.submit_editor(cx)
                                                }),
                                            ),
                                        ),
                                )
                                .child(
                                    div()
                                        .when(busy, |field| field.opacity(0.5))
                                        .child(ui::field("editor-name", name, Look::Modal)),
                                ),
                        )
                        .child(
                            // The Merge into... and Delete buttons, with a
                            // line above them.
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .pt(px(2.))
                                .border_t_1()
                                .border_color(theme.border_secondary())
                                .child(
                                    ui::action_button("editor-merge", ButtonLook::Standard)
                                        .label(t("Merge into..."))
                                        .icon("tags-git-merge", 14.)
                                        .disabled(busy)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            let step = merge(&merge_tag, cx);
                                            this.push(step, cx);
                                        })),
                                )
                                .child(
                                    ui::action_button("editor-delete", ButtonLook::Danger)
                                        .label(t("Delete"))
                                        .icon("trash-2", 14.)
                                        .disabled(busy)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            let tag = delete_tag.clone();
                                            this.push(Dialog::DeleteTag { tag }, cx);
                                        })),
                                ),
                        ),
                )
            }
            Dialog::Merge {
                tag,
                search,
                query,
                results,
                loading,
                error,
                ..
            } => {
                let mut list = div().flex().flex_col();
                for (position, target) in results.iter().enumerate() {
                    let (source, chosen) = (tag.clone(), target.clone());
                    list = list.child(
                        div()
                            .id(("merge-result", position))
                            .role(Role::Button)
                            .aria_label(format!(
                                "{}{} {}",
                                prefix(&target.namespace, cx)
                                    .map(|p| format!("{p}:"))
                                    .unwrap_or_default(),
                                target.subname,
                                target.active_count
                            ))
                            .w_full()
                            .min_h(px(36.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .px(px(6.))
                            .py(px(4.))
                            .rounded(px(theme::RADIUS_SM))
                            .cursor_pointer()
                            .hover(move |style| style.bg(theme.subtle_overlay()))
                            .child(
                                ui::chip(
                                    ("merge-chip", position),
                                    target.subname.clone(),
                                    crate::tags::chip_color(&target.namespace),
                                )
                                .namespace(prefix(&target.namespace, cx)),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .ml_auto()
                                    .text_color(theme.text_secondary())
                                    .font_family(theme::FONT_INSPECTOR_MONO)
                                    .text(theme::TEXT_CAPTION)
                                    .child(target.active_count.to_string()),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                // The merge dialog is closed, and then the
                                // merge runs.
                                this.close_editor_step();
                                this.merge_tag(&source, &chosen, cx);
                                cx.notify();
                            })),
                    );
                }
                let searched = !query.trim().is_empty();
                dialog(t("Merge into existing tag"), Size::Medium).child(
                    // The search field, a line of help text or an error,
                    // and the list of results.
                    div()
                        .min_h(px(260.))
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(ui::field("merge-search", search, Look::ModalSearch).icon(
                            "library-search",
                            14.,
                            10.,
                            theme::blend(
                                gpui::transparent_black().into(),
                                theme.text_tertiary(),
                                0.6,
                            ),
                        ))
                        .when(*loading, |body| body.child(hint(t("Searching..."), cx)))
                        .children(error.as_ref().map(|error| error_banner(error.clone(), cx)))
                        .when(
                            !*loading && error.is_none() && searched && results.is_empty(),
                            |body| body.child(hint(t("No existing tags found."), cx)),
                        )
                        .child(list),
                )
            }
            Dialog::DeleteTag { tag } => {
                let id = tag.tag_id;
                return confirm(
                    t("Delete tag"),
                    format!(
                        "Delete {}? Existing media will remain untouched.",
                        crate::tags::record_name(&tag)
                    ),
                    t("Delete tag"),
                    true,
                    Box::new(move |this, cx| this.delete_tag(id, cx)),
                    cx,
                );
            }
            Dialog::DeleteGroup { group } => {
                let deleted = group.clone();
                return confirm(
                    t("Delete tag group"),
                    format!(
                        "Delete the {} group? Its tags will move to General; no tags or media assignments will be deleted.",
                        group.name
                    ),
                    t("Delete group"),
                    false,
                    Box::new(move |this, cx| this.delete_group(deleted.clone(), cx)),
                    cx,
                );
            }
            Dialog::Cleanup => {
                let count = self.unused;
                let focus = focus.clone();
                return confirm(
                    t("Delete unused tags"),
                    format!(
                        "Delete {} {} with no assignments anywhere in the library? Tags used by Inbox or Trash items are kept.",
                        crate::i18n::format::grouped(count),
                        if count == 1 { "tag" } else { "tags" }
                    ),
                    t("Delete unused tags"),
                    true,
                    Box::new(move |this, cx| {
                        this.cancel(&focus, cx);
                        this.delete_unused(cx);
                    }),
                    cx,
                );
            }
            Dialog::RenameGroup { group, name, .. } => {
                let ready = group_name(name, Some(&group.name), cx).is_some();
                dialog(t("Rename tag group"), Size::Small)
                    .child(ui::field(("group-name", index), name, Look::Modal))
                    .footer(cancel_button(cx))
                    .footer(
                        ui::action_button(("rename-group", index), ButtonLook::Standard)
                            .label(t("Rename"))
                            .disabled(busy || !ready)
                            .on_click(cx.listener(|this, _, _, cx| this.submit_group_name(cx))),
                    )
            }
            Dialog::CreateGroup { name, .. } => {
                let ready = group_name(name, None, cx).is_some();
                dialog(t("Create tag group"), Size::Small)
                    .child(ui::field(("group-name", index), name, Look::Modal))
                    .footer(cancel_button(cx))
                    .footer(
                        ui::action_button(("create-group", index), ButtonLook::Standard)
                            .label(t("Create"))
                            .disabled(busy || !ready)
                            .on_click(cx.listener(|this, _, _, cx| this.submit_group_name(cx))),
                    )
            }
        };
        let _ = window;
        element.into_any_element()
    }
}

/// Returns the banner that shows the message of an edit, a load or a search
/// that failed. It has a margin of 12 px above it and of 24 px on its left
/// and its right.
pub fn error_banner(message: String, cx: &App) -> Div {
    ui::form::error_banner(theme::theme(cx), message)
        .mt(px(12.))
        .mx(px(24.))
        .flex_none()
}
