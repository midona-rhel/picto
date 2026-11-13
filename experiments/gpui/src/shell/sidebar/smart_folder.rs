//! `SmartFolderModal.tsx`: creates a smart folder or edits one, its name,
//! icon, colour and notes ("Edit Smart Folder..."), its rules ("Edit
//! Rules..."), or all of them for a new one. Rules are groups of rules
//! (`RuleGroupEditor`, `RuleEditor`); while rules are edited the folder is
//! saved after each change, 100 ms later, so the grid shows what they match,
//! and Cancel writes the rules it opened with back.
use super::rules::{self, Group, Rule, ValueType};
use crate::session::Session;
use crate::theme;
use crate::theme::Text as _;
use crate::ui::field::Look;
use crate::ui::tag_token_input::{self, Found, TagTokenInput};
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

/// One rule's editor: the rule and the fields its values are typed in.
struct RuleRow {
    rule: Rule,
    value: Entity<TextInput>,
    value2: Entity<TextInput>,
    tags: Entity<TagTokenInput>,
    color: Entity<TextInput>,
    _subscriptions: Vec<Subscription>,
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
    error: Option<String>,
    presence: Presence,
    focus: FocusHandle,
    /// The icon picker's popover, and the trigger it opens under.
    icon_picker: Option<Entity<ui::icon_picker::IconPicker>>,
    icon_trigger: Rc<Cell<Bounds<Pixels>>>,
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
            input
        });
        let notes = cx.new(|cx| {
            let mut input = TextInput::dialog("Notes...", cx);
            input.set_text(notes, cx);
            input
        });
        let subscriptions = vec![
            cx.subscribe_in(&name, window, |this, _, _: &Submitted, window, cx| {
                this.save(window, cx)
            }),
            cx.subscribe_in(&notes, window, |this, _, _: &Submitted, window, cx| {
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
                    .map(|tag| (tag.tag_id, found(tag).name()))
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
            error: None,
            presence: Presence::new(),
            focus,
            icon_picker: None,
            icon_trigger: Rc::new(Cell::new(Bounds::default())),
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
                ValueType::Date => "YYYY-MM-DD",
                _ => "Value",
            },
            cx,
        );
        let value2 = field(
            rule.value2.clone().unwrap_or_default(),
            match rules::field(rule.field).kind {
                ValueType::Date => "YYYY-MM-DD",
                _ => "Max",
            },
            cx,
        );
        let color = field(
            rule.values
                .first()
                .cloned()
                .filter(|_| rule.field == "color")
                .unwrap_or("#808080".into()),
            "#808080",
            cx,
        );
        let session = self.session.clone();
        let search: tag_token_input::Search = Rc::new(move |query, cx| {
            session.read(cx).run(move |library| {
                library
                    .tag_page(None, Some(&query), 0, 12)
                    .map(|(tags, _, _)| tags.iter().map(found).collect())
                    .unwrap_or_default()
            })
        });
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
        for input in [&value, &value2, &color] {
            subscriptions.extend(watch_input(input, window, cx));
        }
        RuleRow {
            rule,
            value,
            value2,
            tags,
            color,
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
                            "color" => vec![row.color.read(cx).value()],
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
            let (namespace, subname) = match name.split_once(':') {
                Some((namespace, subname)) => (namespace, subname),
                None => ("", name),
            };
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
            let answer = session.update(cx, |session, _| {
                session.run(move |library| library.update_smart_folder(id, input).map(|_| ()))
            });
            if let Ok(Err(error)) = answer.await {
                eprintln!("smart folder preview: {error}");
            }
            let _ = session.update(cx, |session, cx| session.reload(cx));
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
        let answer = self
            .session
            .read(cx)
            .run(move |library| library.update_smart_folder(id, original).map(|_| ()));
        cx.spawn_in(window, async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| {
                this.closing = false;
                match result {
                    Ok(Ok(())) => {
                        this.presence.close();
                        this.session.update(cx, |session, cx| session.reload(cx));
                    }
                    // The previewed rules are still saved, so the dialog stays.
                    Ok(Err(error)) => {
                        this.error = Some(format!("Could not restore smart folder rules: {error}"))
                    }
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
                self.error = Some(format!("Could not save smart folder: {error}"));
                cx.notify();
                return;
            }
        };
        let target = match &self.target {
            Target::Create { .. } => None,
            Target::Edit { id, .. } => Some(*id),
        };
        let answer = self.session.read(cx).run(move |library| {
            let result = match target {
                Some(id) => library.update_smart_folder(id, input).map(|_| id),
                None => library.create_smart_folder(input).map(|(id, _)| id),
            };
            (result, crate::backend::navigation(library))
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok((result, navigation)) = answer.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if let Ok((nav, tags)) = navigation {
                    this.session.update(cx, |session, cx| {
                        session.replace_navigation(nav, tags, cx);
                        session.reload(cx);
                    });
                }
                match result {
                    Ok(id) => {
                        if target.is_none() {
                            cx.emit(Event::Created(id));
                        }
                        this.presence.close();
                    }
                    Err(error) => {
                        this.error = Some(format!("Could not save smart folder: {error}"))
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

fn found(tag: &TagRecord) -> Found {
    Found {
        namespace: tag.namespace.clone(),
        subname: tag.subname.clone(),
    }
}

fn choices(options: &'static [(&'static str, &'static str)]) -> Vec<ui::select::Choice> {
    options
        .iter()
        .map(|(value, label)| ui::choice(value, *label))
        .collect()
}

/// `modalStyles.inlineLabel`.
fn inline_label(text: &'static str, cx: &App) -> Div {
    div()
        .flex_none()
        .text(theme::TEXT_SM)
        .text_color(theme::theme(cx).text_secondary())
        .child(text)
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
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
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
    let id = id.into();
    let button = div()
        .id(id.clone())
        .size(px(24.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(3.))
        .border_1()
        .border_color(theme.text_at(15.))
        .bg(theme.text_at(5.))
        .child(glyph);
    if !enabled {
        return button.opacity(0.3);
    }
    ui::tooltip::attach(
        button
            .cursor_pointer()
            .hover(move |style| {
                style
                    .bg(theme.text_at(10.))
                    .border_color(theme.text_at(20.))
            })
            .active(move |style| style.bg(theme.text_at(15.))),
        ElementId::NamedChild(std::sync::Arc::new(id), "tip".into()),
        ui::tip(tip),
        24.,
        window,
        cx,
    )
}

impl SmartFolderEditor {
    fn rule_element(
        &self,
        group: usize,
        index: usize,
        row: &RuleRow,
        can_add: bool,
        can_remove: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = theme::theme(cx);
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
            ValueType::Color => {
                let hex = row.color.read(cx).value();
                // TODO: open the colour editor popover (`ColorFilterEditor`)
                // from the swatch; the hex is typed here instead.
                div()
                    .h(px(theme::CONTROL_SM))
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .child(
                        div()
                            .size(px(14.))
                            .flex_none()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(rgba(0xffffff38))
                            .bg(ui::color_picker::parse(&hex).unwrap_or(theme.text_tertiary())),
                    )
                    .child(text_field(&row.color, "color"))
                    .into_any_element()
            }
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
            // TODO: the date picker button (`DatePickerButton`); the date is
            // typed as YYYY-MM-DD here.
            ValueType::Date => div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(text_field(&row.value, "date"))
                .when(between, |row_element| {
                    row_element
                        .child(and())
                        .child(text_field(&row.value2, "date2"))
                })
                .into_any_element(),
            ValueType::Text => text_field(&row.value, "text").into_any_element(),
        };
        let remove = condition_button(key("remove"), false, "Remove rule", can_remove, window, cx)
            .when(can_remove, |button| {
                button
                    .on_click(cx.listener(move |this, _, _, cx| this.remove_rule(group, index, cx)))
            });
        let add = condition_button(
            key("add"),
            true,
            if can_add {
                "Add rule"
            } else {
                "Maximum 10 rules"
            },
            can_add,
            window,
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

    fn group_element(
        &self,
        index: usize,
        group: &GroupRows,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
                window,
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
            window,
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
            window,
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
        div()
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
            .on_click(cx.listener(|this, _, _, cx| {
                if this.icon_picker.take().is_none() {
                    let editor = cx.entity().downgrade();
                    let current = this.icon.clone();
                    this.icon_picker = Some(cx.new(move |_| {
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
            }))
    }

    /// The icon picker's popover: 260×308 below the trigger, kept on screen,
    /// over a backdrop that closes it.
    fn icon_popover(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let picker = self.icon_picker.clone()?;
        let theme = theme::theme(cx);
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
                            div()
                                .id("icon-picker-popover")
                                .absolute()
                                .left(left)
                                .top(top)
                                .w(px(260.))
                                .h(px(308.))
                                .p(px(4.))
                                .overflow_hidden()
                                .rounded(px(theme::RADIUS_LG))
                                .bg(theme.glass())
                                .border_t_1()
                                .border_color(theme.glass_border_top())
                                .shadow(theme::panel_shadow())
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .child(picker),
                        ),
                ),
            )
            .with_priority(20)
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
        if let Some(error) = &self.error {
            form = form.child(
                div()
                    .mb(px(12.))
                    .px(px(12.))
                    .py(px(9.))
                    .rounded(px(theme::RADIUS_SM))
                    .border_1()
                    .border_color(theme.negative_at(35.))
                    .bg(theme.negative_at(8.))
                    .text_color(rgb(theme::ERROR_TEXT))
                    .text(theme::TEXT_SM)
                    .child(error.clone()),
            );
        }
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
                // TODO: notes are a three-row text area (`GlassTextarea`);
                // the prototype's text field is one line.
                .child(section(false).child(label("Notes", cx)).child(ui::field(
                    "smart-folder-notes",
                    &self.notes,
                    Look::Modal,
                )));
        }
        if self.shows_rules() {
            let mut conditions = div().flex().flex_col().gap(px(12.));
            for (index, group) in self.groups.iter().enumerate() {
                conditions = conditions.child(self.group_element(index, group, window, cx));
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
        div().child(dialog).children(self.icon_popover(window, cx))
    }
}
