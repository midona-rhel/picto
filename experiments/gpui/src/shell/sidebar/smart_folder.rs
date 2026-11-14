//! `SmartFolderModal.tsx`: creates a smart folder or edits one, its name,
//! icon, colour and notes ("Edit Smart Folder..."), its rules ("Edit
//! Rules..."), or all of them for a new one. Rules are groups of rules
//! (`RuleGroupEditor`, `RuleEditor`); while rules are edited the folder is
//! saved after each change, 100 ms later, so the grid shows what they match,
//! and Cancel writes the rules it opened with back.
use super::rules::{self, Group, Rule, ValueType};
use crate::session::{Report, Reread, Session};
use crate::theme;
use crate::theme::Text as _;
use crate::ui::color_editor::{ColorCommitted, ColorEditor};
use crate::ui::field::Look;
use crate::ui::tag_token_input::{self, TagTokenInput};
use crate::ui::text_input::{Submitted, TextInput};
use crate::ui::{self, action_button::Look as ButtonLook, modal::Presence, modal::Size};
use gpui::{prelude::FluentBuilder as _, *};
use picto_library::predicate::{ItemSort, ViewQuerySpec};
use picto_library::{SmartFolderId, SmartFolderInput, TagId, TagRecord};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

/// Which parts the dialog edits (`editor`).
#[derive(Clone, Copy, PartialEq)]
pub enum Part {
    All,
    Details,
    Rules,
}

/// A new smart folder, or the one being edited with what it held when the
/// dialog opened.
pub enum Target {
    Create {
        parent: Option<SmartFolderId>,
    },
    Edit {
        id: SmartFolderId,
        original: SmartFolderInput,
    },
}

/// The dialog is done: the sidebar reveals a created folder.
pub enum Event {
    Created(SmartFolderId),
}

impl EventEmitter<Event> for SmartFolderEditor {}

/// The live preview waits this long after the last change.
const PREVIEW_DELAY: Duration = Duration::from_millis(100);
/// `.rulesModal`: `min(760px, 100vw - 48px)`.
const RULES_WIDTH: f32 = 760.;
/// The pop-ups the dialog opens draw above it.
const POPOVER_LAYER: usize = 20;
/// `.colorPickerPopover`: its width and the height it is placed by.
const COLOR_POPOVER_WIDTH: f32 = 230.;
const COLOR_POPOVER_HEIGHT: f32 = 286.;

/// One rule's editor: the rule and the fields its values are typed in.
struct RuleRow {
    rule: Rule,
    value: Entity<TextInput>,
    value2: Entity<TextInput>,
    tags: Entity<TagTokenInput>,
    /// A colour rule's `#rrggbb`, chosen in its popover.
    color: String,
    /// Where the colour's trigger is, to open its popover under it.
    color_trigger: Rc<Cell<Bounds<Pixels>>>,
    _subscriptions: Vec<Subscription>,
}

/// `ColorRuleInput`'s open popover: the rule it edits and its editor.
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
    /// The rules have been read into the editor.
    ready: bool,
    /// The library's tags, to resolve the names the rules hold.
    tags: Rc<Vec<TagRecord>>,
    presence: Presence,
    focus: FocusHandle,
    /// The icon picker's popover, and the trigger it opens under.
    icon_picker: Option<Entity<ui::icon_picker::IconPicker>>,
    icon_trigger: Rc<Cell<Bounds<Pixels>>>,
    color_popover: Option<ColorPopover>,
    preview: Option<Task<()>>,
    /// Closing waits for the rules it opened with to be written back.
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
            let mut input = TextInput::dialog("Smart Folder Name", cx);
            input.set_text(name, cx);
            input.caret_at_end(cx);
            input
        });
        let notes = cx.new(|cx| {
            let mut input = TextInput::text_area("Notes...", cx);
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
        // The library's tags, then the rules read from the filter.
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

    /// Whether the dialog has faded out and can be dropped.
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
            (Target::Create { .. }, _) => "New Smart Folder",
            (_, Part::Rules) => "Edit Rules",
            _ => "Edit Smart Folder",
        }
    }

    fn rule_row(&mut self, rule: Rule, window: &mut Window, cx: &mut Context<Self>) -> RuleRow {
        let field = |text: String, placeholder: &'static str, cx: &mut Context<Self>| {
            let input = cx.new(|cx| {
                let mut input = TextInput::dialog(placeholder, cx);
                input.set_text(text, cx);
                input
            });
            input
        };
        let value = field(
            rule.value.clone(),
            match rules::field(rule.field).kind {
                ValueType::FileSize => "Size",
                _ => "Value",
            },
            cx,
        );
        let value2 = field(rule.value2.clone().unwrap_or_default(), "Max", cx);
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

    /// The rules as typed so far.
    fn current_groups(&self, cx: &App) -> Vec<Group> {
        self.groups
            .iter()
            .map(|group| Group {
                any: group.any,
                negate: group.negate,
                rules: group
                    .rules
                    .iter()
                    .map(|row| {
                        let mut rule = row.rule.clone();
                        rule.value = row.value.read(cx).value();
                        rule.value2 = rule.value2.as_ref().map(|_| row.value2.read(cx).value());
                        rule.values = match rule.field {
                            "tags" => row.tags.read(cx).values().to_vec(),
                            "color" => vec![row.color.clone()],
                            _ => Vec::new(),
                        };
                        rule
                    })
                    .collect(),
            })
            .collect()
    }

    fn rule_count(&self) -> usize {
        self.groups.iter().map(|group| group.rules.len()).sum()
    }

    /// The definition the dialog holds, with its rules compiled.
    fn input(&self, cx: &App) -> Result<SmartFolderInput, String> {
        let tags = self.tags.clone();
        let resolve = move |name: &str| {
            let (namespace, subname) = crate::tags::split(name);
            let namespace = namespace.unwrap_or_default();
            tags.iter()
                .find(|tag| {
                    tag.subname == subname
                        && (tag.namespace == namespace
                            || (namespace.is_empty() && tag.namespace == "general"))
                })
                .map(|tag| tag.tag_id)
        };
        let (parent, original) = match &self.target {
            Target::Create { parent } => (*parent, None),
            Target::Edit { original, .. } => (original.parent_id, Some(original)),
        };
        let filter = if self.shows_rules() {
            rules::compile(&self.current_groups(cx), &resolve)?
        } else {
            original
                .map(|original| original.view.filter.clone())
                .unwrap_or_default()
        };
        let name = self.name.read(cx).value().trim().to_owned();
        let notes = self.notes.read(cx).value().trim().to_owned();
        Ok(SmartFolderInput {
            name: if name.is_empty() {
                original
                    .map(|original| original.name.clone())
                    .unwrap_or_default()
            } else {
                name
            },
            parent_id: parent,
            icon: self.icon.clone(),
            color: self.color.clone(),
            notes: (!notes.is_empty()).then_some(notes),
            view: ViewQuerySpec {
                filter,
                sort: self.sort.clone(),
            },
        })
    }

    /// Something in the rules changed: an edited folder shows the new rules
    /// after a pause.
    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.notify();
        let Target::Edit { id, .. } = self.target else {
            return;
        };
        if !self.shows_rules() || !self.ready || self.closing {
            return;
        }
        let session = self.session.clone();
        self.preview = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PREVIEW_DELAY).await;
            let Ok(Ok(input)) = this.update(cx, |this, cx| this.input(cx)) else {
                return;
            };
            let _ = session.update(cx, |session, cx| {
                session.edit_reporting(
                    Report::failure("Could not preview smart folder rules"),
                    move |library| library.update_smart_folder(id, input),
                    Reread::RESULTS_ONLY,
                    cx,
                )
            });
        }));
    }

    /// Cancel, the close button, Escape or the backdrop: an edited folder
    /// gets the rules it opened with back, then the dialog closes.
    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing || self.presence.closing() {
            return;
        }
        self.preview = None;
        let restore = match &self.target {
            Target::Edit { id, original } if self.shows_rules() => Some((*id, original.clone())),
            _ => None,
        };
        let Some((id, original)) = restore else {
            self.presence.close();
            cx.notify();
            return;
        };
        self.closing = true;
        let answer = self.session.update(cx, |session, cx| {
            session.edit(
                move |library| library.update_smart_folder(id, original),
                Reread::RESULTS_ONLY,
                cx,
            )
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| {
                this.closing = false;
                match result {
                    Ok(Ok(_)) => this.presence.close(),
                    // The previewed rules are still saved, so the dialog stays.
                    Ok(Err(error)) => ui::notify::error(
                        "Could not restore smart folder rules",
                        error.to_string(),
                        cx,
                    ),
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Create or Save Changes.
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_save(cx) || self.presence.closing() {
            return;
        }
        self.preview = None;
        let input = match self.input(cx) {
            Ok(input) => input,
            Err(error) => {
                ui::notify::error("Could not save smart folder", error, cx);
                return;
            }
        };
        let target = match &self.target {
            Target::Create { .. } => None,
            Target::Edit { id, .. } => Some(*id),
        };
        // A created folder is revealed in the sidebar, which needs it among
        // the destinations at once.
        let answer = self.session.update(cx, |session, cx| {
            session.edit(
                move |library| match target {
                    Some(id) => library.update_smart_folder(id, input).map(|_| id),
                    None => library.create_smart_folder(input).map(|(id, _)| id),
                },
                Reread {
                    navigation: true,
                    ..Reread::RESULTS_ONLY
                },
                cx,
            )
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(result) = answer.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(id) => {
                        if target.is_none() {
                            cx.emit(Event::Created(id));
                        }
                        this.presence.close();
                    }
                    Err(error) => {
                        ui::notify::error("Could not save smart folder", error.to_string(), cx)
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The primary button's state: a name, and rules that are read and
    /// within the limit.
    fn can_save(&self, cx: &App) -> bool {
        !self.name.read(cx).value().trim().is_empty()
            && (!self.shows_rules() || (self.ready && self.rule_count() <= rules::MAX_RULES))
    }

    // ── Rule edits ──────────────────────────────────────────────────

    fn set_field(
        &mut self,
        group: usize,
        rule: usize,
        key: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next = Rule::for_field(key, &rules::today());
        let row = self.rule_row(next, window, cx);
        if let Some(slot) = self
            .groups
            .get_mut(group)
            .and_then(|group| group.rules.get_mut(rule))
        {
            *slot = row;
        }
        self.changed(cx);
    }

    fn set_op(&mut self, group: usize, rule: usize, op: &'static str, cx: &mut Context<Self>) {
        let Some(row) = self
            .groups
            .get_mut(group)
            .and_then(|group| group.rules.get_mut(rule))
        else {
            return;
        };
        let value = row.value.read(cx).value();
        let mut current = row.rule.clone();
        current.value = value;
        row.rule = current.with_op(op);
        if let Some(value2) = row.rule.value2.clone() {
            row.value2
                .update(cx, |input, cx| input.set_text(value2, cx));
        }
        self.changed(cx);
    }

    /// A select value of a rule: the file type, a rating, or a size unit.
    fn set_choice(
        &mut self,
        group: usize,
        rule: usize,
        slot: Slot,
        choice: &'static str,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self
            .groups
            .get_mut(group)
            .and_then(|group| group.rules.get_mut(rule))
        else {
            return;
        };
        match slot {
            Slot::Value => row
                .value
                .update(cx, |input, cx| input.set_text(choice.into(), cx)),
            Slot::Value2 => {
                row.rule.value2 = Some(choice.into());
                row.value2
                    .update(cx, |input, cx| input.set_text(choice.into(), cx))
            }
            Slot::Unit => row.rule.unit = Some(choice),
        }
        self.changed(cx);
    }

    /// A date rule's day, from its picker.
    fn set_date(&mut self, group: usize, rule: usize, second: bool, day: String, cx: &mut App) {
        let Some(row) = self
            .groups
            .get(group)
            .and_then(|group| group.rules.get(rule))
        else {
            return;
        };
        let input = if second { &row.value2 } else { &row.value };
        // The field's observer previews the change.
        input.update(cx, |input, cx| input.set_text(day, cx));
    }

    /// Opens the colour popover under a colour rule's trigger, or closes it
    /// when it is open there.
    fn toggle_color(&mut self, group: usize, rule: usize, cx: &mut Context<Self>) {
        if self
            .color_popover
            .take()
            .is_some_and(|open| (open.group, open.rule) == (group, rule))
        {
            cx.notify();
            return;
        }
        let Some(row) = self
            .groups
            .get(group)
            .and_then(|group| group.rules.get(rule))
        else {
            return;
        };
        let (color, trigger) = (row.color.clone(), row.color_trigger.get());
        let editor = cx.new(|cx| ColorEditor::new(Some(color), 0., false, cx).without_clear());
        let subscription = cx.subscribe(&editor, move |this, _, event: &ColorCommitted, cx| {
            let ColorCommitted(Some(color), _) = event else {
                return;
            };
            if let Some(row) = this
                .groups
                .get_mut(group)
                .and_then(|group| group.rules.get_mut(rule))
            {
                row.color = color.clone();
                this.changed(cx);
            }
        });
        self.color_popover = Some(ColorPopover {
            group,
            rule,
            editor,
            trigger,
            _subscription: subscription,
        });
        cx.notify();
    }

    fn add_rule(
        &mut self,
        group: usize,
        after: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.rule_count() >= rules::MAX_RULES {
            return;
        }
        let row = self.rule_row(rules::default_rule(), window, cx);
        if let Some(group) = self.groups.get_mut(group) {
            group.rules.insert((after + 1).min(group.rules.len()), row);
        }
        self.changed(cx);
    }

    fn remove_rule(&mut self, group: usize, rule: usize, cx: &mut Context<Self>) {
        self.color_popover = None;
        if let Some(group) = self.groups.get_mut(group)
            && group.rules.len() > 1
            && rule < group.rules.len()
        {
            group.rules.remove(rule);
        }
        self.changed(cx);
    }

    fn add_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.rule_count() >= rules::MAX_RULES {
            return;
        }
        let group = self.group_rows(rules::default_group(), window, cx);
        self.groups.push(group);
        self.changed(cx);
    }

    fn remove_group(&mut self, group: usize, cx: &mut Context<Self>) {
        self.color_popover = None;
        if self.groups.len() > 1 && group < self.groups.len() {
            self.groups.remove(group);
        }
        self.changed(cx);
    }
}

/// A value field previews the rules as it changes, and Enter saves.
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

/// Which of a rule's select values changed.
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

/// `modalStyles.inlineLabel`.
fn inline_label(text: &'static str, cx: &App) -> Div {
    ui::modal::help_text(theme::theme(cx), text).flex_none()
}

/// `.label`.
fn label(text: &'static str, cx: &App) -> Div {
    div()
        .mb(px(8.))
        .text(theme::TEXT_MD)
        .line_height(px(20.))
        .text_color(theme::theme(cx).text())
        .child(text)
}

/// `.conditionButton`: a 24px square with a minus, or a plus.
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
        let and = || inline_label("and", cx);
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
                        ui::date_picker::date_picker(key(name), input.read(cx).value())
                            .layer(POPOVER_LAYER)
                            .on_change(move |day, _, cx| {
                                let _ = entity.update(cx, |this, cx| {
                                    this.set_date(group, index, second, day, cx)
                                });
                            }),
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
        let remove = condition_button(key("remove"), false, "Remove rule", can_remove, cx).when(
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
                "Add rule"
            } else {
                "Maximum 10 rules"
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
                if group.any { "any" } else { "all" },
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
            "Remove group",
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
                "Add group"
            } else {
                "Maximum 10 rules"
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
                    .child(inline_label("Match", cx))
                    .child(match_select)
                    .child(inline_label("of the following", cx))
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

    /// `IconPickerPopover`'s trigger: the icon, its name or "Default", and a
    /// chevron.
    fn icon_trigger(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let bounds = self.icon_trigger.clone();
        let glyph = self
            .icon
            .clone()
            .filter(|icon| crate::assets::has_icon(icon))
            .unwrap_or("folder".into());
        let trigger = div()
            .id("icon-trigger")
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
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(self.icon.clone().unwrap_or("Default".into())),
            )
            .child(ui::icon("chevron-down", 14., theme.text_tertiary()).flex_none())
            .child(
                canvas(move |area, _, _| bounds.set(area), |_, _, _, _| {})
                    .absolute()
                    .inset_0(),
            )
            .on_click(cx.listener(|this, _, _, cx| this.toggle_icon_picker(cx)));
        // `.iconTrigger` is a `<button>`.
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

    /// `ColorRuleInput`'s trigger (`.colorTrigger`): the swatch and the code
    /// in capitals, which opens the colour popover.
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
                // `.colorSwatch`
                div()
                    .size(px(14.))
                    .flex_none()
                    .rounded(px(4.))
                    .border_1()
                    .border_color(rgba(theme::COLOR_SWATCH_RING))
                    .bg(swatch)
                    .shadow(vec![
                        BoxShadow::new(px(0.), px(1.), rgba(0xffffff2e).into()).inset(),
                    ]),
            )
            .child(
                // `.colorValue`
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
        // `.colorTrigger` is a `<button>`.
        ui::focus::stop(
            SharedString::from(format!("rule-{group}-{index}-color")),
            trigger,
            cx.listener(move |this, _, _, cx| this.toggle_color(group, index, cx)),
        )
    }

    /// `.colorPickerPopover`: the colour editor, 230px wide below the
    /// trigger (above it without room), over a backdrop that closes it.
    fn color_popover(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.color_popover.as_ref()?;
        let theme = theme::theme(cx);
        let viewport = window.viewport_size();
        let (left, top) = ui::date_picker::place_sized(
            open.trigger,
            viewport,
            COLOR_POPOVER_WIDTH,
            COLOR_POPOVER_HEIGHT,
        );
        Some(
            deferred(
                anchored().position(point(px(0.), px(0.))).child(
                    div()
                        .id("color-popover-backdrop")
                        .occlude()
                        .w(viewport.width)
                        .h(viewport.height)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.color_popover = None;
                                cx.notify();
                            }),
                        )
                        .child(
                            ui::GlassPanel::new(
                                div()
                                    .id("color-popover")
                                    .absolute()
                                    .left(px(left))
                                    .top(px(top))
                                    .w(px(COLOR_POPOVER_WIDTH))
                                    .min_h(px(COLOR_POPOVER_HEIGHT))
                                    .p(px(7.))
                                    .bg(theme.glass())
                                    .border_t_1()
                                    .border_color(theme.glass_border_top()),
                                theme::RADIUS_LG,
                            )
                            .backdrop_blur(theme.glass_blur())
                            .drop_shadows(theme.panel_shadow())
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(open.editor.clone()),
                        ),
                ),
            )
            .with_priority(POPOVER_LAYER)
            .into_any_element(),
        )
    }

    /// The icon picker's popover: 260×308 below the trigger, kept on screen,
    /// over a backdrop that closes it.
    fn icon_popover(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let picker = self.icon_picker.clone()?;
        let trigger = self.icon_trigger.get();
        let viewport = window.viewport_size();
        let top = (trigger.bottom() + px(4.))
            .min(viewport.height - px(312.))
            .max(px(4.));
        let left = trigger.left().min(viewport.width - px(264.)).max(px(4.));
        Some(
            deferred(
                anchored().position(point(px(0.), px(0.))).child(
                    div()
                        .id("icon-picker-backdrop")
                        .occlude()
                        .w(viewport.width)
                        .h(viewport.height)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.icon_picker = None;
                                cx.notify();
                            }),
                        )
                        .child(
                            ui::glass_panel("icon-picker-popover", theme::RADIUS_LG, cx)
                                .absolute()
                                .left(left)
                                .top(top)
                                .w(px(260.))
                                .h(px(308.))
                                // `.pickerPopover`'s 4px padding inside its
                                // 1px glass border.
                                .p(px(5.))
                                .overflow_hidden()
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .child(picker),
                        ),
                ),
            )
            .with_priority(POPOVER_LAYER)
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
                .child(section(true).child(label("Name", cx)).child(ui::field(
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
                                    .child(label("Icon", cx))
                                    .child(self.icon_trigger(cx)),
                            )
                            .child(
                                div().flex_1().min_w_0().child(label("Color", cx)).child(
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
                .child(section(false).child(label("Notes", cx)).child(
                    // A `<textarea>` sits on its line's baseline, so the
                    // line's descent (5px at 13px) shows below it.
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
                    .label(if create { "Create" } else { "Save Changes" })
                    .disabled(!can_save)
                    .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
            )
            .footer(
                ui::action_button("smart-folder-cancel", ButtonLook::Standard)
                    .label("Cancel")
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
