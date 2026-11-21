//! The data side of the smart-folder editor: reading the rules that the user
//! has entered, building the definition that is written to the library,
//! saving and cancelling, and the changes that the editor's controls make to
//! a field, an operator, a value, a colour, a rule or a group.
use super::*;

impl SmartFolderEditor {
    /// Returns the groups of rules with the values that the editor's fields
    /// hold now.
    pub(super) fn current_groups(&self, cx: &App) -> Vec<Group> {
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

    pub(super) fn rule_count(&self) -> usize {
        self.groups.iter().map(|group| group.rules.len()).sum()
    }

    /// Returns the smart folder's definition as the dialog holds it now,
    /// with the rules compiled to a filter. When the dialog does not show
    /// the rules, the filter is the one that the smart folder had when the
    /// dialog opened. Returns an error when a rule cannot be compiled.
    pub(super) fn input(&self, cx: &App) -> Result<SmartFolderInput, String> {
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

    /// Redraws the editor after a change to the rules. When the rules of an
    /// existing smart folder are being edited, the new rules are also
    /// written to the library after `PREVIEW_DELAY`, so that the grid shows
    /// the items they match. A later change replaces a write that is still
    /// waiting.
    pub(super) fn changed(&mut self, cx: &mut Context<Self>) {
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
                    Report::failure(t("Could not preview smart folder rules")),
                    move |library| library.update_smart_folder(id, input),
                    Reread::RESULTS_ONLY,
                    cx,
                )
            });
        }));
    }

    /// Closes the dialog without saving. Cancel, the close button, Escape
    /// and a click on the backdrop run this. When the rules of an existing
    /// smart folder were shown, the definition that the smart folder had
    /// when the dialog opened is first written back to the library, because
    /// the preview may have saved changed rules.
    pub(super) fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
                    // The definition could not be written back, so the
                    // library still holds the previewed rules, and the
                    // dialog stays open.
                    Ok(Err(error)) => ui::notify::error(
                        t("Could not restore smart folder rules"),
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

    /// Saves the definition to the library and closes the dialog. The
    /// buttons Create and Save Changes run this. A new smart folder is
    /// created, and an existing one is updated.
    pub(super) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_save(cx) || self.presence.closing() {
            return;
        }
        self.preview = None;
        let input = match self.input(cx) {
            Ok(input) => input,
            Err(error) => {
                ui::notify::error(t("Could not save smart folder"), error, cx);
                return;
            }
        };
        let target = match &self.target {
            Target::Create { .. } => None,
            Target::Edit { id, .. } => Some(*id),
        };
        // The edit also reads the sidebar's destinations again
        // (`navigation`). The sidebar expands the branch of a new smart
        // folder as soon as `Event::Created` arrives, and it can do that
        // only when the smart folder is among the destinations.
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
                        ui::notify::error(t("Could not save smart folder"), error.to_string(), cx)
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Returns whether the primary button is enabled. The name must not be
    /// empty. When the dialog shows the rules, they must have been read into
    /// the editor, and there must be at most `rules::MAX_RULES` of them.
    pub(super) fn can_save(&self, cx: &App) -> bool {
        !self.name.read(cx).value().trim().is_empty()
            && (!self.shows_rules() || (self.ready && self.rule_count() <= rules::MAX_RULES))
    }

    // The functions below change the rules.

    pub(super) fn set_field(
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

    pub(super) fn set_op(
        &mut self,
        group: usize,
        rule: usize,
        op: &'static str,
        cx: &mut Context<Self>,
    ) {
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

    /// Sets a value of a rule that the user chose in a select: the file
    /// type, a rating, or the unit of a file size. `slot` says which of the
    /// rule's values the select sets.
    pub(super) fn set_choice(
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

    /// Sets the day of a date rule to the day that the user chose in its
    /// date picker. When `second` is true, it sets the second day of a
    /// "between" rule.
    pub(super) fn set_date(
        &mut self,
        group: usize,
        rule: usize,
        second: bool,
        day: String,
        cx: &mut App,
    ) {
        let Some(row) = self
            .groups
            .get(group)
            .and_then(|group| group.rules.get(rule))
        else {
            return;
        };
        let input = if second { &row.value2 } else { &row.value };
        // The editor observes this text field (`watch_input`), and the
        // observer calls `changed`, which previews the change.
        input.update(cx, |input, cx| input.set_text(day, cx));
    }

    /// Opens the colour popover under a colour rule's trigger, or closes it
    /// when it is open there.
    pub(super) fn toggle_color(&mut self, group: usize, rule: usize, cx: &mut Context<Self>) {
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

    pub(super) fn add_rule(
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

    pub(super) fn remove_rule(&mut self, group: usize, rule: usize, cx: &mut Context<Self>) {
        self.color_popover = None;
        if let Some(group) = self.groups.get_mut(group)
            && group.rules.len() > 1
            && rule < group.rules.len()
        {
            group.rules.remove(rule);
        }
        self.changed(cx);
    }

    pub(super) fn add_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.rule_count() >= rules::MAX_RULES {
            return;
        }
        let group = self.group_rows(rules::default_group(), window, cx);
        self.groups.push(group);
        self.changed(cx);
    }

    pub(super) fn remove_group(&mut self, group: usize, cx: &mut Context<Self>) {
        self.color_popover = None;
        if self.groups.len() > 1 && group < self.groups.len() {
            self.groups.remove(group);
        }
        self.changed(cx);
    }
}
