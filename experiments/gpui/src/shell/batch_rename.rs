//! `BatchRenameModal`: new names for several items at once, either from a
//! format (`*` keeps the old name, `%N`… numbers them) or by replacing a
//! piece of every name, with a preview of each old and new name. Rename
//! writes them all as one edit (`items.rename_many`), announced with Undo.
use crate::i18n::t;
use crate::session::{Report, Reread, Session};
use crate::theme::Text as _;
use crate::ui::action_button::Look as ButtonLook;
use crate::ui::field::Look as FieldLook;
use crate::ui::form::{self, help_text};
use crate::ui::text_input::{Kind, Submitted, TextInput};
use crate::{theme, ui};
use gpui::*;
use picto_library::RootId;

/// `.previewRow`: 6px above and below a 20px line, and its 1px rule.
const ROW: f32 = 33.;
/// `.preview`'s `max-height`.
const PREVIEW_MAX: f32 = 240.;
/// The format a dialog opens with.
const DEFAULT_FORMAT: &str = "New Name - %N";
/// The longest name the library takes, in UTF-16 units as production counts.
const LONGEST: usize = 255;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Format,
    Replace,
}

/// An item to rename, as the grid has it.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: RootId,
    pub name: String,
}

/// `%N`, `%NN`, `%NNN` and `%NNNN` (`/%N{1,4}/g`): the sequence number,
/// padded with zeros to as many digits as the token has `N`s.
fn numbered(text: &str, sequence: u64) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("%N") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let width = after.chars().take_while(|c| *c == 'N').count().min(4);
        out.push_str(&format!("{sequence:0width$}"));
        rest = &after[width..];
    }
    out.push_str(rest);
    out
}

/// `buildBatchRenamePreview`: each item's new name, trimmed. A format puts
/// the old name where `*` is, then numbers from `start_at`; a replacement
/// swaps every occurrence of `pattern`, and leaves names alone while there
/// is nothing to find.
pub fn preview(
    items: &[Item],
    mode: Mode,
    pattern: &str,
    replacement: &str,
    start_at: u64,
) -> Vec<String> {
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let name = match mode {
                Mode::Replace if pattern.is_empty() => item.name.clone(),
                Mode::Replace => item.name.replace(pattern, replacement),
                Mode::Format => numbered(
                    &pattern.split('*').collect::<Vec<_>>().join(&item.name),
                    start_at.saturating_add(index as u64),
                ),
            };
            name.trim().to_owned()
        })
        .collect()
}

/// Whether Rename is off: nothing to find, or a name empty or too long.
pub fn invalid(mode: Mode, pattern: &str, names: &[String]) -> bool {
    (mode == Mode::Replace && pattern.is_empty())
        || names
            .iter()
            .any(|name| name.is_empty() || name.encode_utf16().count() > LONGEST)
}

/// "Start at" as production reads it: `Math.max(0, parseInt(value) || 0)`.
pub fn start_at(text: &str) -> u64 {
    let text = text.trim_start();
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let digits: String = digits.chars().take_while(char::is_ascii_digit).collect();
    if negative || digits.is_empty() {
        return 0;
    }
    digits.parse().unwrap_or(u64::MAX)
}

struct Open {
    items: Vec<Item>,
    mode: Mode,
    /// The edit is on its way; Rename waits for it.
    busy: bool,
}

/// The window's batch rename dialog.
pub struct BatchRename {
    session: Entity<Session>,
    dialog: ui::modal::Host<Open>,
    /// Where the keyboard goes back to.
    back: FocusHandle,
    pattern: Entity<TextInput>,
    replacement: Entity<TextInput>,
    start: Entity<TextInput>,
    list: UniformListScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl BatchRename {
    pub fn new(session: Entity<Session>, back: FocusHandle, cx: &mut Context<Self>) -> Self {
        let pattern = cx.new(|cx| TextInput::new(Kind::Form, cx).submits());
        let replacement = cx.new(|cx| {
            TextInput::new(Kind::Form, cx)
                .placeholder("Replace with")
                .submits()
        });
        let start = cx.new(|cx| TextInput::new(Kind::Form, cx).submits());
        let mut subscriptions = Vec::new();
        for input in [&pattern, &replacement] {
            subscriptions.push(cx.observe(input, |_, _, cx| cx.notify()));
        }
        // The field always shows the number it is read as.
        subscriptions.push(cx.observe(&start, |_, input, cx| {
            let text = input.read(cx).value();
            let read = start_at(&text).to_string();
            if read != text {
                input.update(cx, |input, cx| {
                    input.set_text(read, cx);
                    input.caret_at_end(cx);
                });
            }
            cx.notify();
        }));
        for input in [&pattern, &replacement, &start] {
            subscriptions.push(cx.subscribe(input, |this, _, _: &Submitted, cx| this.rename(cx)));
        }
        Self {
            session,
            dialog: ui::modal::Host::default(),
            back,
            pattern,
            replacement,
            start,
            list: UniformListScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    /// Opens on `items`, from the defaults every time.
    pub fn open(&mut self, items: Vec<Item>, cx: &mut Context<Self>) {
        if items.len() < 2 {
            return;
        }
        self.pattern.update(cx, |input, cx| {
            input.set_placeholder("");
            input.set_text(DEFAULT_FORMAT.into(), cx);
        });
        self.replacement
            .update(cx, |input, cx| input.set_text(String::new(), cx));
        self.start
            .update(cx, |input, cx| input.set_text("1".into(), cx));
        self.list = UniformListScrollHandle::new();
        self.dialog.open(
            Open {
                items,
                mode: Mode::Format,
                busy: false,
            },
            ui::menu::Return::To(self.back.clone()),
            cx,
        );
    }

    fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        if let Some(open) = self.dialog.content_mut() {
            open.mode = mode;
        }
        // Find shares the format's text, as one field in production.
        self.pattern.update(cx, |input, _| {
            // i18n: a placeholder; the field shows it in the language
            input.set_placeholder(if mode == Mode::Replace { "Find" } else { "" })
        });
        cx.notify();
    }

    fn names(&self, open: &Open, cx: &App) -> Vec<String> {
        preview(
            &open.items,
            open.mode,
            &self.pattern.read(cx).value(),
            &self.replacement.read(cx).value(),
            start_at(&self.start.read(cx).value()),
        )
    }

    /// Rename: every new name in one edit; the dialog closes once it is
    /// written and stays, with a notification, when it fails.
    fn rename(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.dialog.content() else {
            return;
        };
        let names = self.names(open, cx);
        if open.busy || invalid(open.mode, &self.pattern.read(cx).value(), &names) {
            return;
        }
        let renames: Vec<(RootId, String)> =
            open.items.iter().map(|item| item.id).zip(names).collect();
        if let Some(open) = self.dialog.content_mut() {
            open.busy = true;
        }
        crate::session::edit_then(
            &self.session,
            Report::failure(t("Could not rename items")).and_announce("items.rename_many"),
            Reread::RESULTS,
            move |library| library.rename_roots(&renames, crate::now_ms()),
            cx,
            |this, renamed, cx| {
                if renamed.is_ok() {
                    this.dialog.close(cx);
                } else if let Some(open) = this.dialog.content_mut() {
                    open.busy = false;
                    cx.notify();
                }
            },
        );
    }
}

impl Render for BatchRename {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = div().absolute().inset_0();
        let Some(layer) = self.dialog.layer(window, cx) else {
            return root;
        };
        let theme = theme::theme(cx);
        let this = cx.entity().downgrade();
        let open = &layer.content;
        let pattern = self.pattern.read(cx).value();
        let names = preview(
            &open.items,
            open.mode,
            &pattern,
            &self.replacement.read(cx).value(),
            start_at(&self.start.read(cx).value()),
        );
        let disabled = open.busy || invalid(open.mode, &pattern, &names);
        let on = |action: fn(&mut BatchRename, &mut Context<BatchRename>)| {
            let this = this.clone();
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                let _ = this.update(cx, |this, cx| action(this, cx));
            }
        };
        let tabs = {
            let this = this.clone();
            ui::segmented::segmented(
                "batch-rename-mode",
                ui::segmented::Look::FilledTabs,
                vec![
                    ui::segmented::tab(t("Format")),
                    ui::segmented::tab(t("Replace")),
                ],
                Some(usize::from(open.mode == Mode::Replace)),
            )
            .on_select(move |index, _, cx| {
                let mode = if index == 0 {
                    Mode::Format
                } else {
                    Mode::Replace
                };
                let _ = this.update(cx, |this, cx| this.set_mode(mode, cx));
            })
        };
        let mut body = form::stack().child(tabs);
        body = match open.mode {
            Mode::Format => body
                .child(form::field(
                    &theme,
                    t("Name format"),
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(ui::field(
                            "batch-rename-format",
                            &self.pattern,
                            FieldLook::Modal,
                        ))
                        .child(
                            help_text(
                                theme,
                                t("* keeps the original name. %N, %NN, and %NNN add padded numbering."),
                            )
                            .line_height(px(18.)),
                        ),
                ))
                .child(form::field(
                    &theme,
                    t("Start at"),
                    div().w(px(96.)).child(ui::field(
                        "batch-rename-start",
                        &self.start,
                        FieldLook::Modal,
                    )),
                )),
            Mode::Replace => body.child(
                form::row()
                    .child(div().flex_1().min_w_0().child(ui::field(
                        "batch-rename-find",
                        &self.pattern,
                        FieldLook::Modal,
                    )))
                    .child(div().flex_1().min_w_0().child(ui::field(
                        "batch-rename-replace",
                        &self.replacement,
                        FieldLook::Modal,
                    ))),
            ),
        };
        body = body.child(preview_list(&self.list, &open.items, names, &theme));
        let modal = ui::modal(
            "batch-rename",
            crate::tf!("Batch Rename {value0} Items", open.items.len()),
            layer.presence,
        )
        .size(ui::modal::Size::Medium)
        .focus(&layer.focus)
        .on_close({
            let this = this.clone();
            move |_, cx| {
                let _ = this.update(cx, |this, cx| this.dialog.dismiss(cx));
            }
        })
        .child(body)
        .footer(
            ui::action_button("batch-rename-cancel", ButtonLook::Standard)
                .label(t("Cancel"))
                .on_click(on(|this, cx| {
                    this.dialog.close(cx);
                })),
        )
        .footer(
            ui::action_button("batch-rename-go", ButtonLook::Primary)
                .label(t("Rename"))
                .disabled(disabled)
                .on_click(on(BatchRename::rename)),
        );
        root = root.child(modal);
        root
    }
}

/// `.preview`: each old name, in the tertiary colour, beside its new one,
/// at most 240px tall before it scrolls.
fn preview_list(
    scroll: &UniformListScrollHandle,
    items: &[Item],
    names: Vec<String>,
    theme: &theme::Theme,
) -> Div {
    let old: Vec<String> = items.iter().map(|item| item.name.clone()).collect();
    let count = old.len();
    let height = (count as f32 * ROW - 1.).min(PREVIEW_MAX - 2.);
    // The dialog's body sets the secondary colour, which the new names
    // keep; the old ones are a step fainter.
    let (tertiary, secondary, rule) = (
        theme.text_tertiary(),
        theme.text_secondary(),
        theme.border_secondary(),
    );
    let list = uniform_list("batch-rename-preview", count, move |range, _, _| {
        range
            .map(|index| {
                let new = &names[index];
                let cell = |text: SharedString| div().flex_1().min_w_0().truncate().child(text);
                div()
                    .w_full()
                    .h(px(ROW))
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .px(px(10.))
                    .border_b_1()
                    .border_color(rule)
                    .text(theme::TEXT_SM)
                    .child(cell(old[index].clone().into()).text_color(tertiary))
                    .child(
                        cell(if new.is_empty() {
                            crate::i18n::en("Invalid empty name").into()
                        } else {
                            new.clone().into()
                        })
                        .text_color(secondary),
                    )
            })
            .collect()
    })
    .track_scroll(scroll)
    .h(px(height));
    div()
        .rounded(px(7.))
        .border_1()
        .border_color(rule)
        .overflow_hidden()
        .child(list)
}

#[cfg(test)]
mod tests {
    use super::{Item, Mode, invalid, numbered, preview, start_at};
    use picto_library::RootId;

    fn items() -> Vec<Item> {
        vec![
            Item {
                id: RootId(10),
                name: "Fox".into(),
            },
            Item {
                id: RootId(11),
                name: "Wolf".into(),
            },
        ]
    }

    #[test]
    fn a_format_keeps_original_names_and_pads_sequence_numbers() {
        assert_eq!(
            preview(&items(), Mode::Format, "* - %NN", "", 7),
            ["Fox - 07", "Wolf - 08"]
        );
        assert_eq!(
            preview(&items(), Mode::Format, "New Name - %N", "", 1),
            ["New Name - 1", "New Name - 2"]
        );
    }

    #[test]
    fn a_replacement_swaps_every_matching_piece() {
        assert_eq!(
            preview(&items(), Mode::Replace, "o", "0", 1),
            ["F0x", "W0lf"]
        );
    }

    #[test]
    fn nothing_to_find_leaves_names_alone_and_cannot_rename() {
        let names = preview(&items(), Mode::Replace, "", "x", 1);
        assert_eq!(names, ["Fox", "Wolf"]);
        assert!(invalid(Mode::Replace, "", &names));
        assert!(!invalid(Mode::Format, "", &names));
    }

    #[test]
    fn number_tokens_take_at_most_four_digits() {
        assert_eq!(numbered("%NNNN", 3), "0003");
        assert_eq!(numbered("%NNNNN", 3), "0003N");
        assert_eq!(numbered("%n %N", 12), "%n 12");
        assert_eq!(numbered("a%Nb%NNc", 5), "a5b05c");
    }

    #[test]
    fn names_are_trimmed_and_an_empty_or_long_one_cannot_be_used() {
        let names = preview(&items(), Mode::Format, "  *  ", "", 1);
        assert_eq!(names, ["Fox", "Wolf"]);
        assert!(invalid(Mode::Format, "x", &["".into()]));
        assert!(invalid(Mode::Format, "x", &["a".repeat(256)]));
        assert!(!invalid(Mode::Format, "x", &["a".repeat(255)]));
    }

    #[test]
    fn start_at_reads_as_parse_int_does_and_never_below_zero() {
        assert_eq!(start_at("12"), 12);
        assert_eq!(start_at("12abc"), 12);
        assert_eq!(start_at("abc"), 0);
        assert_eq!(start_at(""), 0);
        assert_eq!(start_at("-4"), 0);
        assert_eq!(start_at(" 7"), 7);
    }
}
