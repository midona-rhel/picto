//! The Tags screen's dialogs: the tag editor (`TagEditorModal`), the merge
//! picker (`RelationPicker`), the group dialogs and the confirmations.
use super::{TagsScreen, model};
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{
    self,
    action_button::Look as ButtonLook,
    field::Look,
    modal::{Presence, Size},
    text_input::{Submitted, TextInput},
};
use gpui::{prelude::FluentBuilder as _, *};
use picto_library::{TagNamespaceRecord, TagRecord};

/// `PICKER_PAGE_SIZE`: merge candidates shown at a time.
const PICKER_PAGE_SIZE: usize = 40;

pub enum Dialog {
    Editor {
        tag: TagRecord,
        name: Entity<TextInput>,
        _subscriptions: Vec<Subscription>,
    },
    /// Choosing the tag to merge `tag` into.
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
    /// Deleting the tags nothing carries.
    Cleanup,
}

impl Dialog {
    /// The editor and the merge and delete steps opened over it.
    pub fn belongs_to_editor(&self) -> bool {
        matches!(
            self,
            Dialog::Editor { .. } | Dialog::Merge { .. } | Dialog::DeleteTag { .. }
        )
    }

    /// The field that takes the keyboard when the dialog opens. The
    /// editor's field waits for a click: `TagEditorModal`'s does not take
    /// focus, which stays on the tag that was clicked.
    fn input(&self) -> Option<&Entity<TextInput>> {
        match self {
            Dialog::RenameGroup { name, .. } | Dialog::CreateGroup { name, .. } => Some(name),
            Dialog::Merge { search, .. } => Some(search),
            _ => None,
        }
    }
}

/// An open dialog, with its time on screen.
pub struct Layer {
    pub dialog: Dialog,
    pub presence: Presence,
    /// Holds the keyboard while no field in the dialog has it.
    pub focus: FocusHandle,
}

impl Layer {
    /// Gives the dialog the keyboard: its field, or the dialog itself.
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
        let mut input = TextInput::dialog(placeholder, cx);
        if let Some(text) = text {
            input.set_text(text, cx);
            input.caret_to_end(cx);
        }
        input
    })
}

pub fn editor(tag: TagRecord, cx: &mut Context<TagsScreen>) -> Dialog {
    let name = input_with("namespace:tag", Some(crate::tags::record_name(&tag)), cx);
    let submitted = cx.subscribe(&name, |this, _, _: &Submitted, cx| this.submit_editor(cx));
    // The rename button follows what is typed.
    let typed = cx.observe(&name, |_, _, cx| cx.notify());
    Dialog::Editor {
        tag,
        name,
        _subscriptions: vec![submitted, typed],
    }
}

pub fn merge(tag: &TagRecord, cx: &mut Context<TagsScreen>) -> Dialog {
    let search = input_with("Search existing tags", None, cx);
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
    let name = input_with("Group name", None, cx);
    let submitted = cx.subscribe(&name, |this, _, _: &Submitted, cx| {
        this.submit_group_name(cx)
    });
    let typed = cx.observe(&name, |_, _, cx| cx.notify());
    Dialog::CreateGroup {
        name,
        _subscriptions: vec![submitted, typed],
    }
}

/// `RenameTagGroupModal` and `CreateTagGroupModal`: the normalized name, if
/// the dialog may submit it.
fn group_name(input: &Entity<TextInput>, current: Option<&str>, cx: &App) -> Option<String> {
    let name = model::normalized_group_name(&input.read(cx).value());
    (model::valid_group_name(&name) && Some(name.as_str()) != current).then_some(name)
}

/// `TagChip`'s namespace prefix, shown only with `showTagPrefixes` on.
fn prefix(namespace: &str, cx: &App) -> Option<SharedString> {
    let show = crate::settings::current(cx).show_tag_prefixes;
    crate::tags::group_of(namespace)
        .filter(|_| show)
        .map(SharedString::from)
}

/// `.pickerHint` and the dialogs' help text.
fn hint(text: impl Into<SharedString>, cx: &App) -> Div {
    ui::form::help_text(theme::theme(cx), text)
}

impl TagsScreen {
    fn layer_index(&self, focus: &FocusHandle) -> Option<usize> {
        self.layers.iter().position(|layer| layer.focus == *focus)
    }

    /// The close button, Escape and the backdrop: the dialog fades out.
    fn dismiss(&mut self, focus: &FocusHandle, cx: &mut Context<Self>) {
        if let Some(index) = self.layer_index(focus) {
            self.layers[index].presence.close();
            cx.notify();
        }
    }

    /// A Cancel button: the dialog goes at once.
    fn cancel(&mut self, focus: &FocusHandle, cx: &mut Context<Self>) {
        if let Some(index) = self.layer_index(focus) {
            self.layers.remove(index);
            self.refocus = true;
            cx.notify();
        }
    }

    /// `submitRename`
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

    /// Enter or the button in a group dialog.
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

    /// The merge picker's search: the library's best matches for what was
    /// typed, without the tag being merged.
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

    /// One open dialog, drawn over the whole window.
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
                .label("Cancel")
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
        // `ConfirmModal`, run by this screen: Cancel takes it away at once,
        // and both buttons wait while an edit is running.
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
                dialog("Edit tag", Size::Large).child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(18.))
                        .child(
                            // `.editorIntro`
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
                                            "tags-bookmark",
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
                                                // The modal body's 20px line.
                                                .line_height(px(20.))
                                                .child(format!(
                                                    "{} media items",
                                                    tag.assignment_count
                                                )),
                                        ),
                                ),
                        )
                        .child(
                            // `.editorSection`
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
                                                .child("Tag name"),
                                        )
                                        .child(
                                            ui::action_button(
                                                "editor-rename",
                                                ButtonLook::Standard,
                                            )
                                            .label("Rename")
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
                            // `.editorActions`
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .pt(px(2.))
                                .border_t_1()
                                .border_color(theme.border_secondary())
                                .child(
                                    ui::action_button("editor-merge", ButtonLook::Standard)
                                        .label("Merge into...")
                                        .icon("tags-git-merge", 14.)
                                        .disabled(busy)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            let step = merge(&merge_tag, cx);
                                            this.push(step, cx);
                                        })),
                                )
                                .child(
                                    ui::action_button("editor-delete", ButtonLook::Danger)
                                        .label("Delete")
                                        .icon("tags-trash", 14.)
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
                                // The picker goes, then the merge runs.
                                this.close_editor_step();
                                this.merge_tag(&source, &chosen, cx);
                                cx.notify();
                            })),
                    );
                }
                let searched = !query.trim().is_empty();
                dialog("Merge into existing tag", Size::Medium).child(
                    // `.pickerBody`
                    div()
                        .min_h(px(260.))
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(ui::field("merge-search", search, Look::ModalSearch).icon(
                            "tags-search",
                            14.,
                            10.,
                            theme::blend(
                                gpui::transparent_black().into(),
                                theme.text_tertiary(),
                                0.6,
                            ),
                        ))
                        .when(*loading, |body| body.child(hint("Searching...", cx)))
                        .children(error.as_ref().map(|error| error_banner(error.clone(), cx)))
                        .when(
                            !*loading && error.is_none() && searched && results.is_empty(),
                            |body| body.child(hint("No existing tags found.", cx)),
                        )
                        .child(list),
                )
            }
            Dialog::DeleteTag { tag } => {
                let id = tag.tag_id;
                return confirm(
                    "Delete tag",
                    format!(
                        "Delete {}? Existing media will remain untouched.",
                        crate::tags::record_name(&tag)
                    ),
                    "Delete tag",
                    true,
                    Box::new(move |this, cx| this.delete_tag(id, cx)),
                    cx,
                );
            }
            Dialog::DeleteGroup { group } => {
                let deleted = group.clone();
                return confirm(
                    "Delete tag group",
                    format!(
                        "Delete the {} group? Its tags will move to General; no tags or media assignments will be deleted.",
                        group.name
                    ),
                    "Delete group",
                    false,
                    Box::new(move |this, cx| this.delete_group(deleted.clone(), cx)),
                    cx,
                );
            }
            Dialog::Cleanup => {
                let count = self.unused;
                let focus = focus.clone();
                return confirm(
                    "Delete unused tags",
                    format!(
                        "Delete {} {} with no assignments anywhere in the library? Tags used by Inbox or Trash items are kept.",
                        crate::grid::format::grouped(count),
                        if count == 1 { "tag" } else { "tags" }
                    ),
                    "Delete unused tags",
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
                dialog("Rename tag group", Size::Small)
                    .child(ui::field(("group-name", index), name, Look::Modal))
                    .footer(cancel_button(cx))
                    .footer(
                        ui::action_button(("rename-group", index), ButtonLook::Standard)
                            .label("Rename")
                            .disabled(busy || !ready)
                            .on_click(cx.listener(|this, _, _, cx| this.submit_group_name(cx))),
                    )
            }
            Dialog::CreateGroup { name, .. } => {
                let ready = group_name(name, None, cx).is_some();
                dialog("Create tag group", Size::Small)
                    .child(ui::field(("group-name", index), name, Look::Modal))
                    .footer(cancel_button(cx))
                    .footer(
                        ui::action_button(("create-group", index), ButtonLook::Standard)
                            .label("Create")
                            .disabled(busy || !ready)
                            .on_click(cx.listener(|this, _, _, cx| this.submit_group_name(cx))),
                    )
            }
        };
        let _ = window;
        element.into_any_element()
    }
}

/// `.error`: an edit or search that failed, inset from the screen's edges.
pub fn error_banner(message: String, cx: &App) -> Div {
    ui::form::error_banner(theme::theme(cx), message)
        .mt(px(12.))
        .mx(px(24.))
        .flex_none()
}
