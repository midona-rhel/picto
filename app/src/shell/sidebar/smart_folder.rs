//! The smart-folder editor, a dialog that creates a smart folder or edits an
//! existing one. It shows the details (name, icon, colour and notes), the
//! rules, or both: "Edit Smart Folder..." shows the details, "Edit Rules..."
//! shows the rules, and a new smart folder shows both. The rules are
//! arranged in groups. While the rules of an existing smart folder are
//! edited, the smart folder is saved 100 ms after each change, so that the
//! grid shows the items that the rules match. Cancel writes back the
//! definition that the smart folder had when the dialog opened.
mod model;

use super::rules::{self, Group, Rule, ValueType};
use crate::i18n::t;
use crate::session::{Report, Reread, Session};
use crate::theme;
use crate::theme::Text as _;
use crate::ui::color_editor::{ColorCommitted, ColorEditor};
use crate::ui::field::Look;
use crate::ui::popover::{Dismissal, Flip, Placement, Side, floating, place};
use crate::ui::tag_token_input::{self, TagTokenInput};
use crate::ui::text_input::{Kind, Submitted, TextInput};
use crate::ui::{self, action_button::Look as ButtonLook, modal::Presence, modal::Size};
use gpui::{prelude::FluentBuilder as _, *};
use picto_library::predicate::{ItemSort, ViewQuerySpec};
use picto_library::{SmartFolderId, SmartFolderInput, TagId, TagRecord};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

/// Which parts of a smart folder the dialog shows: its details (name, icon,
/// colour and notes), its rules, or both.
#[derive(Clone, Copy, PartialEq)]
pub enum Part {
    All,
    Details,
    Rules,
}

/// What the dialog edits: a new smart folder under `parent`, or the existing
/// smart folder `id`. `original` is the definition that the existing smart
/// folder had when the dialog opened.
pub enum Target {
    Create {
        parent: Option<SmartFolderId>,
    },
    Edit {
        id: SmartFolderId,
        original: SmartFolderInput,
    },
}

/// The event that the editor emits after it created a smart folder. The
/// sidebar then expands the branch that holds the new smart folder.
pub enum Event {
    Created(SmartFolderId),
}

impl EventEmitter<Event> for SmartFolderEditor {}

/// The time between the last change to the rules and the write of the rules
/// to the library, after which the grid shows the items they match.
const PREVIEW_DELAY: Duration = Duration::from_millis(100);
/// The width in pixels of the dialog when it shows only the rules. In a
/// window narrower than 808px, the dialog is 48px narrower than the window.
const RULES_WIDTH: f32 = 760.;
/// The size of the colour popover of a Color rule. The popover is 230px
/// wide, and it is placed as if it were 286px tall. `COLOR_PLACEMENT` puts
/// it 5px under its button, or above the button when there is no room
/// below, and keeps it 8px inside the window. The day picker's calendar is
/// placed in the same way.
const COLOR_POPOVER: gpui::Size<f32> = gpui::Size {
    width: 230.,
    height: 286.,
};
const COLOR_PLACEMENT: Placement = Placement::new(Side::Below, 5., 8.).flip(Flip::WhenNoRoom);
/// The size of the icon picker's popover, 260px by 308px. `ICON_PLACEMENT`
/// puts it 4px under its button and keeps it 4px inside the window.
const ICON_POPOVER: gpui::Size<f32> = gpui::Size {
    width: 260.,
    height: 308.,
};
const ICON_PLACEMENT: Placement = Placement::new(Side::Below, 4., 4.);

/// One rule in the editor: the rule, and the text fields and the tag field
/// that hold its values.
struct RuleRow {
    rule: Rule,
    value: Entity<TextInput>,
    value2: Entity<TextInput>,
    tags: Entity<TagTokenInput>,
    /// The colour of a Color rule as `#rrggbb`. The user chooses it in the
    /// colour popover.
    color: String,
    /// The bounds of the button that shows the colour. The colour popover
    /// opens under them.
    color_trigger: Rc<Cell<Bounds<Pixels>>>,
    _subscriptions: Vec<Subscription>,
}

/// The open colour popover of a Color rule: the group and the rule it
/// belongs to, its colour editor, and the bounds of the button it opened
/// under.
struct ColorPopover {
    group: usize,
    rule: usize,
    editor: Entity<ColorEditor>,
    trigger: Bounds<Pixels>,
    _subscription: Subscription,
}

struct GroupRows {
    any: bool,
    negate: bool,
    rules: Vec<RuleRow>,
}

pub struct SmartFolderEditor {
    session: Entity<Session>,
    target: Target,
    part: Part,
    name: Entity<TextInput>,
    notes: Entity<TextInput>,
    icon: Option<String>,
    color: Option<String>,
    sort: ItemSort,
    groups: Vec<GroupRows>,
    /// Whether the rules have been read from the smart folder's filter into
    /// the editor.
    ready: bool,
    /// The library's tags. They are used to find the id of each tag that a
    /// rule names.
    tags: Rc<Vec<TagRecord>>,
    presence: Presence,
    focus: FocusHandle,
    /// The icon picker while its popover is open. `icon_trigger` holds the
    /// bounds of the button that the popover opens under.
    icon_picker: Option<Entity<ui::icon_picker::IconPicker>>,
    icon_trigger: Rc<Cell<Bounds<Pixels>>>,
    color_popover: Option<ColorPopover>,
    preview: Option<Task<()>>,
    /// Whether the dialog is waiting to close until the definition it opened
    /// with has been written back to the library.
    closing: bool,
    _subscriptions: Vec<Subscription>,
}

impl SmartFolderEditor {
    pub fn new(
        session: Entity<Session>,
        target: Target,
        part: Part,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (name, icon, color, notes, view) = match &target {
            Target::Create { .. } => (
                // i18n: a new smart folder's name, English in every language
                "New Smart Folder".to_owned(),
                None,
                None,
                String::new(),
                ViewQuerySpec::default(),
            ),
            Target::Edit { original, .. } => (
                original.name.clone(),
                original.icon.clone(),
                original.color.clone(),
                original.notes.clone().unwrap_or_default(),
                original.view.clone(),
            ),
        };
        let name = cx.new(|cx| {
            let mut input = TextInput::new(Kind::Form, cx)
                .placeholder("Smart Folder Name")
                .submits();
            input.set_text(name, cx);
            input.caret_at_end(cx);
            input
        });
        let notes = cx.new(|cx| {
            let mut input = TextInput::new(Kind::Area, cx).placeholder("Notes...");
            input.set_text(notes, cx);
            input
        });
        let subscriptions = vec![
            cx.subscribe_in(&name, window, |this, _, _: &Submitted, window, cx| {
                this.save(window, cx)
            }),
            cx.observe(&name, |_, _, cx| cx.notify()),
        ];
        let focus = cx.focus_handle();
        if part == Part::Rules {
            window.focus(&focus, cx);
        } else {
            window.focus(&name.focus_handle(cx), cx);
        }
        // The library's tags are read first, because the rules are read from
        // the filter with the names of the tags.
        let answer = session.read(cx).run(|library| library.tags());
        cx.spawn_in(window, async move |this, cx| {
            let tags = answer.await.ok().and_then(Result::ok).unwrap_or_default();
            let _ = this.update_in(cx, move |this, window, cx| {
                let names = tags
                    .iter()
                    .map(|tag| (tag.tag_id, crate::tags::record_name(tag)))
                    .collect::<Vec<_>>();
                this.tags = Rc::new(tags);
                let groups = rules::from_filter(&view.filter, &|id: TagId| {
                    names
                        .iter()
                        .find(|(tag, _)| *tag == id)
                        .map(|(_, name)| name.clone())
                })
                .unwrap_or_else(|_| vec![rules::default_group()]);
                this.groups = groups
                    .into_iter()
                    .map(|group| this.group_rows(group, window, cx))
                    .collect();
                this.ready = true;
                cx.notify();
            });
        })
        .detach();
        Self {
            session,
            part,
            name,
            notes,
            icon,
            color,
            sort: match &target {
                Target::Edit { original, .. } => original.view.sort.clone(),
                Target::Create { .. } => ItemSort::default(),
            },
            target,
            groups: Vec::new(),
            ready: false,
            tags: Rc::new(Vec::new()),
            presence: Presence::new(),
            focus,
            icon_picker: None,
            icon_trigger: Rc::new(Cell::new(Bounds::default())),
            color_popover: None,
            preview: None,
            closing: false,
            _subscriptions: subscriptions,
        }
    }

    /// Returns whether the dialog has finished fading out. After that it can
    /// be dropped.
    pub fn finished(&self) -> bool {
        self.presence.finished()
    }

    fn shows_details(&self) -> bool {
        self.part != Part::Rules
    }

    fn shows_rules(&self) -> bool {
        self.part != Part::Details
    }

    fn title(&self) -> &'static str {
        match (&self.target, self.part) {
            (Target::Create { .. }, _) => t("New Smart Folder"),
            (_, Part::Rules) => crate::i18n::en("Edit Rules"),
            _ => crate::i18n::en("Edit Smart Folder"),
        }
    }

    fn rule_row(&mut self, rule: Rule, window: &mut Window, cx: &mut Context<Self>) -> RuleRow {
        let field = |text: String, placeholder: &'static str, cx: &mut Context<Self>| {
            let input = cx.new(|cx| {
                let mut input = TextInput::new(Kind::Form, cx)
                    .placeholder(placeholder)
                    .submits();
                input.set_text(text, cx);
                input
            });
            input
        };
        let value = field(
            rule.value.clone(),
            match rules::field(rule.field).kind {
                ValueType::FileSize => t("Size"),
                _ => t("Value"),
            },
            cx,
        );
        let value2 = field(rule.value2.clone().unwrap_or_default(), t("Max"), cx);
        let color = rule
            .values
            .first()
            .cloned()
            .filter(|_| rule.field == "color")
            .unwrap_or("#808080".into());
        let search = crate::tags::token_search(self.session.clone());
        let values = if rule.field == "tags" {
            rule.values.clone()
        } else {
            Vec::new()
        };
        let tags = cx.new(|cx| TagTokenInput::new(values, search, cx).compact());
        let mut subscriptions = vec![
            cx.subscribe(&tags, |this, _, _: &tag_token_input::Changed, cx| {
                this.changed(cx)
            }),
        ];
        for input in [&value, &value2] {
            subscriptions.extend(watch_input(input, window, cx));
        }
        RuleRow {
            rule,
            value,
            value2,
            tags,
            color,
            color_trigger: Rc::new(Cell::new(Bounds::default())),
            _subscriptions: subscriptions,
        }
    }

    fn group_rows(
        &mut self,
        group: Group,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> GroupRows {
        GroupRows {
            any: group.any,
            negate: group.negate,
            rules: group
                .rules
                .into_iter()
                .map(|rule| self.rule_row(rule, window, cx))
                .collect(),
        }
    }
}

/// Subscribes to a text field that holds a value of a rule. A change of the
/// field's text calls `changed`, which previews the rules, and Enter in the
/// field saves the dialog.
fn watch_input(
    input: &Entity<TextInput>,
    window: &mut Window,
    cx: &mut Context<SmartFolderEditor>,
) -> [Subscription; 2] {
    let mut last = input.read(cx).value();
    [
        cx.observe(input, move |this, input, cx| {
            let value = input.read(cx).value();
            if value != last {
                last = value;
                this.changed(cx);
            }
        }),
        cx.subscribe_in(input, window, |this, _, _: &Submitted, window, cx| {
            this.save(window, cx)
        }),
    ]
}

/// Which value of a rule a select sets: the first value, the second value,
/// or the unit.
#[derive(Clone, Copy)]
enum Slot {
    Value,
    Value2,
    Unit,
}

fn choices(options: &'static [(&'static str, &'static str)]) -> Vec<ui::select::Choice> {
    options
        .iter()
        .map(|(value, label)| ui::choice(value, *label))
        .collect()
}

/// Draws a short label that stands between the controls of a line, such as
/// "and" or a unit. It is drawn like help text.
fn inline_label(text: &'static str, cx: &App) -> Div {
    ui::form::help_text(theme::theme(cx), text).flex_none()
}

/// Draws the label above a field of the form, with 8px between the label
/// and the field.
fn label(text: &'static str, cx: &App) -> Div {
    div()
        .mb(px(8.))
        .text(theme::TEXT_MD)
        .line_height(px(20.))
        .text_color(theme::theme(cx).text())
        .child(text)
}

/// Builds a button that removes or adds a rule or a group. It shows a minus
/// sign, or a plus sign when `plus` is true. `tip` is the text of its
/// tooltip, and the button is disabled when `enabled` is false.
fn condition_button(
    id: impl Into<ElementId>,
    plus: bool,
    tip: &'static str,
    enabled: bool,
    cx: &mut App,
) -> ui::icon_button::IconButton {
    let theme = theme::theme(cx);
    let line = || div().absolute().rounded(px(1.)).bg(theme.text());
    let mut glyph = div()
        .relative()
        .w(px(9.))
        .h(px(9.))
        .child(line().left_0().top(px(4.)).w(px(9.)).h(px(1.)));
    if plus {
        glyph = glyph.child(line().left(px(4.)).top_0().w(px(1.)).h(px(9.)));
    }
    ui::icon_button(id, "")
        .look(ui::icon_button::Look::Condition)
        .glyph_element(glyph)
        .disabled(!enabled)
        .tip(ui::tip(tip))
}

impl SmartFolderEditor {
    fn rule_element(
        &self,
        group: usize,
        index: usize,
        row: &RuleRow,
        can_add: bool,
        can_remove: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rule = &row.rule;
        let def = rules::field(rule.field);
        let between = rule.op == "between";
        let entity = cx.entity().downgrade();
        let key = |slot: &str| SharedString::from(format!("rule-{group}-{index}-{slot}"));
        let field_select = {
            let entity = entity.clone();
            ui::select(
                key("field"),
                rule.field,
                rules::FIELDS
                    .iter()
                    .map(|field| ui::choice(field.key, field.label))
                    .collect(),
            )
            .width(140.)
            .on_change(move |value, window, cx| {
                let _ = entity.update(cx, |this, cx| {
                    this.set_field(group, index, value, window, cx)
                });
            })
        };
        let op_select = {
            let entity = entity.clone();
            ui::select(key("op"), rule.op, choices(def.operators))
                .width(128.)
                .on_change(move |value, _, cx| {
                    let _ = entity.update(cx, |this, cx| this.set_op(group, index, value, cx));
                })
        };
        let choice = |slot: Slot,
                      current: String,
                      options: &'static [(&'static str, &'static str)],
                      width: f32,
                      name: &str| {
            let entity = entity.clone();
            ui::select(key(name), current, choices(options))
                .width(width)
                .on_change(move |value, _, cx| {
                    let _ = entity.update(cx, |this, cx| {
                        this.set_choice(group, index, slot, value, cx)
                    });
                })
        };
        let text_field = |input: &Entity<TextInput>, name: &str| {
            div()
                .flex_1()
                .min_w_0()
                .child(ui::field(key(name), input, Look::ModalSmall))
        };
        let and = || inline_label(t("and"), cx);
        let value = row.value.read(cx).value();
        let value_element: AnyElement = match def.kind {
            ValueType::Tags => row.tags.clone().into_any_element(),
            ValueType::Color => self.color_trigger(group, index, row, cx).into_any_element(),
            ValueType::Select => choice(
                Slot::Value,
                if value.is_empty() {
                    rules::FILE_TYPES[0].0.into()
                } else {
                    value
                },
                rules::FILE_TYPES,
                180.,
                "type",
            )
            .into_any_element(),
            ValueType::FileSize => div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(text_field(&row.value, "size"))
                .when(between, |row_element| {
                    row_element
                        .child(and())
                        .child(text_field(&row.value2, "size2"))
                })
                .child(choice(
                    Slot::Unit,
                    // i18n: a rule's value, not shown
                    rule.unit.unwrap_or("MB").into(),
                    rules::FILE_SIZE_UNITS,
                    116.,
                    "unit",
                ))
                .into_any_element(),
            ValueType::Number if rule.field == "rating" => div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(choice(
                    Slot::Value,
                    value,
                    rules::RATING_OPTIONS,
                    112.,
                    "rating",
                ))
                .when(between, |row_element| {
                    let value2 = row.value2.read(cx).value();
                    row_element.child(and()).child(choice(
                        Slot::Value2,
                        value2,
                        rules::RATING_OPTIONS,
                        112.,
                        "rating2",
                    ))
                })
                .into_any_element(),
            ValueType::Number => div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(text_field(&row.value, "number"))
                .when(between, |row_element| {
                    row_element
                        .child(and())
                        .child(text_field(&row.value2, "number2"))
                })
                .children(def.unit.map(|unit| inline_label(unit, cx)))
                .into_any_element(),
            ValueType::Date => {
                let date = |second: bool, input: &Entity<TextInput>, name: &str| {
                    let entity = entity.clone();
                    div().flex_1().min_w_0().child(
                        ui::date_picker::date_picker(key(name), input.read(cx).value()).on_change(
                            move |day, _, cx| {
                                let _ = entity.update(cx, |this, cx| {
                                    this.set_date(group, index, second, day, cx)
                                });
                            },
                        ),
                    )
                };
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(date(false, &row.value, "date"))
                    .when(between, |row_element| {
                        row_element
                            .child(and())
                            .child(date(true, &row.value2, "date2"))
                    })
                    .into_any_element()
            }
            ValueType::Text => text_field(&row.value, "text").into_any_element(),
        };
        let remove = condition_button(key("remove"), false, t("Remove rule"), can_remove, cx).when(
            can_remove,
            |button| {
                button
                    .on_click(cx.listener(move |this, _, _, cx| this.remove_rule(group, index, cx)))
            },
        );
        let add = condition_button(
            key("add"),
            true,
            if can_add {
                t("Add rule")
            } else {
                t("Maximum 10 rules")
            },
            can_add,
            cx,
        )
        .when(can_add, |button| {
            button.on_click(
                cx.listener(move |this, _, window, cx| this.add_rule(group, index, window, cx)),
            )
        });
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .min_h(px(26.))
            .child(field_select)
            .child(op_select)
            .when(rule.shows_value(), |line| {
                line.child(div().flex_1().min_w(px(100.)).child(value_element))
            })
            .child(remove)
            .child(add)
            .into_any_element()
    }

    fn group_element(&self, index: usize, group: &GroupRows, cx: &mut Context<Self>) -> AnyElement {
        let theme = theme::theme(cx);
        let can_add = self.rule_count() < rules::MAX_RULES;
        let can_remove_group = self.groups.len() > 1;
        let entity = cx.entity().downgrade();
        let mut list = div().flex().flex_col().gap(px(6.));
        for (position, row) in group.rules.iter().enumerate() {
            list = list.child(self.rule_element(
                index,
                position,
                row,
                can_add,
                group.rules.len() > 1,
                cx,
            ));
        }
        let match_select = {
            let entity = entity.clone();
            ui::select(
                SharedString::from(format!("group-{index}-match")),
                // i18n: the choice's value
                if group.any { "any" } else { "all" },
                // i18n: the choices' values
                vec![ui::choice("all", "all"), ui::choice("any", "any")],
            )
            .width(72.)
            .on_change(move |value, _, cx| {
                let _ = entity.update(cx, |this, cx| {
                    if let Some(group) = this.groups.get_mut(index) {
                        group.any = value == "any";
                    }
                    this.changed(cx);
                });
            })
        };
        let negate_select = ui::select(
            SharedString::from(format!("group-{index}-negate")),
            if group.negate { "exclude" } else { "include" },
            vec![
                ui::choice("include", "included"),
                ui::choice("exclude", "excluded"),
            ],
        )
        .width(112.)
        .on_change(move |value, _, cx| {
            let _ = entity.update(cx, |this, cx| {
                if let Some(group) = this.groups.get_mut(index) {
                    group.negate = value == "exclude";
                }
                this.changed(cx);
            });
        });
        let remove = condition_button(
            SharedString::from(format!("group-{index}-remove")),
            false,
            t("Remove group"),
            can_remove_group,
            cx,
        )
        .when(can_remove_group, |button| {
            button.on_click(cx.listener(move |this, _, _, cx| this.remove_group(index, cx)))
        });
        let add = condition_button(
            SharedString::from(format!("group-{index}-add")),
            true,
            if can_add {
                t("Add group")
            } else {
                t("Maximum 10 rules")
            },
            can_add,
            cx,
        )
        .when(can_add, |button| {
            button.on_click(cx.listener(|this, _, window, cx| this.add_group(window, cx)))
        });
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .min_h(px(26.))
                    .px(px(2.))
                    .child(inline_label(t("Match"), cx))
                    .child(match_select)
                    .child(inline_label(t("of the following"), cx))
                    .child(negate_select),
            )
            .child(
                div()
                    .p(px(8.))
                    .rounded(px(theme::RADIUS_MD))
                    .border_1()
                    .border_color(theme.border())
                    .bg(theme.text_at(2.))
                    .child(list)
                    .child(
                        div()
                            .mt(px(8.))
                            .pt(px(8.))
                            .border_t_1()
                            .border_color(theme.border_secondary())
                            .flex()
                            .justify_end()
                            .gap(px(4.))
                            .child(remove)
                            .child(add),
                    ),
            )
            .into_any_element()
    }

    /// Draws the button that opens the icon picker. It shows the smart
    /// folder's icon, the icon's name or "Default", and a chevron.
    fn icon_trigger(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let bounds = self.icon_trigger.clone();
        let glyph = self
            .icon
            .clone()
            .filter(|icon| crate::assets::has_icon(icon))
            .unwrap_or("IconFolder".into());
        let trigger = div()
            .id("icon-trigger")
            .role(Role::Button)
            .aria_label(t("Change icon"))
            .relative()
            .w_full()
            .h(px(32.))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(theme::RADIUS_SM))
            .border_1()
            .border_color(theme.border())
            .bg(theme.control_surface())
            .text_color(theme.text())
            .cursor_pointer()
            .hover(move |style| style.bg(theme.surface_hover()))
            .child(ui::icon(glyph, 16., theme.text()).flex_none())
            .child(
                div().flex_1().min_w_0().truncate().child(
                    self.icon
                        .clone()
                        .unwrap_or(crate::i18n::en("Default").into()),
                ),
            )
            .child(ui::icon("chevron-down", 14., theme.text_tertiary()).flex_none())
            .child(
                canvas(move |area, _, _| bounds.set(area), |_, _, _, _| {})
                    .absolute()
                    .inset_0(),
            )
            .on_click(cx.listener(|this, _, _, cx| this.toggle_icon_picker(cx)));
        // The button is a tab stop, so it can be reached and pressed with
        // the keyboard.
        ui::focus::stop(
            "icon-trigger",
            trigger,
            cx.listener(|this, _, _, cx| this.toggle_icon_picker(cx)),
        )
    }

    fn toggle_icon_picker(&mut self, cx: &mut Context<Self>) {
        if self.icon_picker.take().is_none() {
            let editor = cx.entity().downgrade();
            let current = self.icon.clone();
            self.icon_picker = Some(cx.new(move |_| {
                ui::icon_picker::IconPicker::new(current, move |icon, _, cx| {
                    let _ = editor.update(cx, |this, cx| {
                        this.icon = icon;
                        this.icon_picker = None;
                        cx.notify();
                    });
                })
            }));
        }
        cx.notify();
    }

    /// Draws the button of a Color rule. It shows a swatch of the rule's
    /// colour and the colour's hex code in capitals. A click opens the
    /// colour popover.
    fn color_trigger(
        &self,
        group: usize,
        index: usize,
        row: &RuleRow,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = theme::theme(cx);
        let bounds = row.color_trigger.clone();
        let swatch = theme::parse_hex_code(&row.color)
            .map(rgb)
            .unwrap_or(theme.text_tertiary());
        let trigger = div()
            .id(SharedString::from(format!("rule-{group}-{index}-color")))
            .role(Role::Button)
            .aria_label(t("Choose color"))
            .aria_value(row.color.to_uppercase())
            .relative()
            .w_full()
            .h(px(theme::CONTROL_SM))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(7.))
            .rounded(px(theme::RADIUS_MD))
            .border_1()
            .border_color(theme.text_at(14.))
            .bg(theme.control_surface())
            .text_color(theme.text())
            .cursor_pointer()
            .hover(move |style| {
                style
                    .border_color(theme.text_at(22.))
                    .bg(theme.surface_hover())
            })
            .child(
                // The swatch of the rule's colour, 14px square.
                div()
                    .size(px(14.))
                    .flex_none()
                    .rounded(px(4.))
                    .border_1()
                    .border_color(rgba(theme::COLOR_SWATCH_RING))
                    .bg(swatch)
                    .shadow(vec![
                        BoxShadow::new(px(0.), px(1.), rgba(theme::COLOR_SWATCH_HIGHLIGHT).into())
                            .inset(),
                    ]),
            )
            .child(
                // The colour's hex code in capitals, in the monospaced font.
                div()
                    .min_w_0()
                    .truncate()
                    .font_family(theme::FONT_MONO)
                    .text(theme::TEXT_SM)
                    .child(row.color.to_uppercase()),
            )
            .child(
                canvas(move |area, _, _| bounds.set(area), |_, _, _, _| {})
                    .absolute()
                    .inset_0(),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_color(group, index, cx)));
        // The button is a tab stop, so it can be reached and pressed with
        // the keyboard.
        ui::focus::stop(
            SharedString::from(format!("rule-{group}-{index}-color")),
            trigger,
            cx.listener(move |this, _, _, cx| this.toggle_color(group, index, cx)),
        )
    }

    /// Draws the colour popover when it is open: the colour editor in a
    /// panel 230px wide, under the rule's button, or above the button when
    /// there is no room below. A click outside the popover closes it.
    fn color_popover(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.color_popover.as_ref()?;
        let corner = place(
            open.trigger,
            COLOR_POPOVER,
            window.viewport_size(),
            COLOR_PLACEMENT,
        );
        Some(
            floating("color-popover", ui::layer::POPOVER)
                .at(corner)
                .dismiss(Dismissal::Backdrop { right: false }, {
                    let this = cx.entity().downgrade();
                    move |_, cx| {
                        let _ = this.update(cx, |this, cx| {
                            this.color_popover = None;
                            cx.notify();
                        });
                    }
                })
                .child(
                    ui::glass_panel("color-popover", theme::RADIUS_LG, cx)
                        .w(px(COLOR_POPOVER.width))
                        .min_h(px(COLOR_POPOVER.height))
                        // The panel's 1px edge is drawn over the padding, so
                        // 8px of padding leave 7px between the edge and the
                        // colour editor.
                        .p(px(8.))
                        .child(open.editor.clone()),
                )
                .into_any_element(),
        )
    }

    /// Draws the icon picker's popover when it is open: a panel 260px by
    /// 308px under the icon button, kept inside the window. A click outside
    /// the popover closes it.
    fn icon_popover(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let picker = self.icon_picker.clone()?;
        let corner = place(
            self.icon_trigger.get(),
            ICON_POPOVER,
            window.viewport_size(),
            ICON_PLACEMENT,
        );
        Some(
            floating("icon-picker-popover", ui::layer::POPOVER)
                .at(corner)
                .dismiss(Dismissal::Backdrop { right: false }, {
                    let this = cx.entity().downgrade();
                    move |_, cx| {
                        let _ = this.update(cx, |this, cx| {
                            this.icon_picker = None;
                            cx.notify();
                        });
                    }
                })
                .child(
                    ui::glass_panel("icon-picker-popover", theme::RADIUS_LG, cx)
                        .w(px(ICON_POPOVER.width))
                        .h(px(ICON_POPOVER.height))
                        // The panel's 1px edge is drawn over the padding, so
                        // 5px of padding leave 4px between the edge and the
                        // icon picker.
                        .p(px(5.))
                        .overflow_hidden()
                        .child(picker),
                )
                .into_any_element(),
        )
    }
}

impl Render for SmartFolderEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let section = |first: bool| {
            let section = div().flex().flex_col().pb(px(16.));
            if first {
                section
            } else {
                section
                    .pt(px(16.))
                    .border_t_1()
                    .border_color(theme.border_secondary())
            }
        };
        let mut form = div()
            .flex()
            .flex_col()
            .text_color(theme.text_secondary())
            .text(theme::TEXT_MD);
        if self.shows_details() {
            let editor = cx.entity().downgrade();
            form = form
                .child(section(true).child(label(t("Name"), cx)).child(ui::field(
                    "smart-folder-name",
                    &self.name,
                    Look::Modal,
                )))
                .child(
                    section(false).child(
                        div()
                            .flex()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(label(t("Icon"), cx))
                                    .child(self.icon_trigger(cx)),
                            )
                            .child(
                                div().flex_1().min_w_0().child(label(t("Color"), cx)).child(
                                    ui::color_picker::color_picker(
                                        "smart-folder-color",
                                        self.color.clone(),
                                    )
                                    .on_pick(
                                        move |hex, _, cx| {
                                            let _ = editor.update(cx, |this, cx| {
                                                this.color = hex.map(str::to_owned);
                                                cx.notify();
                                            });
                                        },
                                    ),
                                ),
                            ),
                    ),
                )
                .child(section(false).child(label(t("Notes"), cx)).child(
                    // The notes field has 5px of space below it, in addition
                    // to the 16px of padding at the bottom of the section.
                    div().pb(px(5.)).child(ui::field(
                        "smart-folder-notes",
                        &self.notes,
                        Look::TextArea,
                    )),
                ));
        }
        if self.shows_rules() {
            let mut conditions = div().flex().flex_col().gap(px(12.));
            for (index, group) in self.groups.iter().enumerate() {
                conditions = conditions.child(self.group_element(index, group, cx));
            }
            form = form.child(section(!self.shows_details()).child(conditions));
        }
        let can_save = self.can_save(cx);
        let create = matches!(self.target, Target::Create { .. });
        let mut dialog = ui::modal("smart-folder-dialog", self.title(), self.presence)
            .size(if self.shows_rules() {
                Size::Large
            } else {
                Size::Medium
            })
            .focus(&self.focus)
            .on_close({
                let editor = cx.entity().downgrade();
                move |window, cx| {
                    let _ = editor.update(cx, |this, cx| this.cancel(window, cx));
                }
            })
            .child(form)
            .footer(
                ui::action_button("smart-folder-save", ButtonLook::Primary)
                    .label(if create {
                        t("Create")
                    } else {
                        t("Save Changes")
                    })
                    .disabled(!can_save)
                    .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
            )
            .footer(
                ui::action_button("smart-folder-cancel", ButtonLook::Standard)
                    .label(t("Cancel"))
                    .on_click(cx.listener(|this, _, window, cx| this.cancel(window, cx))),
            );
        if self.part == Part::Rules {
            let available = f32::from(window.viewport_size().width) - 48.;
            dialog = dialog.width(RULES_WIDTH.min(available));
        }
        div()
            .child(dialog)
            .children(self.icon_popover(window, cx))
            .children(self.color_popover(window, cx))
    }
}
