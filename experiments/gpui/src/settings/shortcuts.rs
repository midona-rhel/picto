//! The Shortcuts section (`ShortcutsPanel.tsx`): the keyboard preset, a
//! search, and one table per group with two binding fields per shortcut
//! that capture the next key pressed.
use super::registry::{self, KeyboardPreset, Slot};
use super::window::SettingsWindow;
use crate::i18n::t;
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{self, choice, field::Look};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use std::time::Duration;

/// A conflicting first binding stays marked for this long.
const CONFLICT_SHOWN: Duration = Duration::from_millis(1500);
/// A captured binding shows in its field this long before the field lets go.
const CAPTURE_HOLD: Duration = Duration::from_millis(150);

/// Sets a flex item's grow factor with a zero basis (`flex: <grow>`).
fn grow(mut element: Div, factor: f32) -> Div {
    let style = element.style();
    style.flex_grow = Some(factor);
    style.flex_shrink = Some(1.);
    style.flex_basis = Some(relative(0.).into());
    element
}

impl SettingsWindow {
    /// Binds a shortcut slot, unless another shortcut already has the keys
    /// (`handleChange`): then the row is marked instead, the first slot's
    /// mark fading after 1.5 s.
    pub(super) fn bind(
        &mut self,
        id: &'static str,
        slot: Slot,
        keys: String,
        cx: &mut Context<Self>,
    ) {
        let (preset, overrides) = {
            let draft = &self.model.read(cx).draft;
            (draft.keyboard, draft.shortcuts.clone())
        };
        if let Some(label) = registry::conflict(id, &keys, preset, &overrides) {
            self.conflict_generation += 1;
            let generation = self.conflict_generation;
            self.conflicts.insert(id, (label, generation));
            if slot == Slot::Keys {
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(CONFLICT_SHOWN).await;
                    let _ = this.update(cx, |this, cx| {
                        if this
                            .conflicts
                            .get(id)
                            .is_some_and(|(_, at)| *at == generation)
                        {
                            this.conflicts.remove(id);
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
            cx.notify();
            return;
        }
        self.model.update(cx, |model, cx| {
            model.edit(
                |draft| {
                    let entry = draft.shortcuts.entry(id).or_default();
                    match slot {
                        Slot::Keys => entry.keys = Some(keys),
                        Slot::Keys2 => entry.keys2 = Some(keys),
                    }
                },
                cx,
            )
        });
    }

    /// A key pressed while a binding field listens.
    fn captured(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, slot, _)) = self.capturing.clone() else {
            return;
        };
        let modifiers = event.keystroke.modifiers;
        let Some(keys) = registry::from_keystroke(
            &event.keystroke.key,
            modifiers.platform,
            modifiers.control,
            modifiers.alt,
            modifiers.shift,
        ) else {
            return;
        };
        cx.stop_propagation();
        self.capturing = Some((id, slot, Some(registry::display(&keys))));
        self.bind(id, slot, keys, cx);
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(CAPTURE_HOLD).await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this
                    .capturing
                    .as_ref()
                    .is_some_and(|(at, on, _)| *at == id && *on == slot)
                {
                    this.capturing = None;
                    window.blur(cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// One binding field (`ShortcutInput`).
    fn binding(
        &self,
        id: &'static str,
        slot: Slot,
        keys: &str,
        conflict: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = theme::theme(cx);
        let listening = self.capturing.as_ref().filter(|(at, on, _)| {
            *at == id && *on == slot && self.capture_focus.is_focused(window)
        });
        let (text, empty): (SharedString, bool) = match listening {
            Some((_, _, captured)) => (
                captured
                    .clone()
                    .unwrap_or_else(|| t("Press keys").into())
                    .into(),
                false,
            ),
            None if keys.is_empty() => (t("Click to bind").into(), true),
            None => (registry::display(keys).into(), false),
        };
        let border = if conflict {
            theme.shortcut_conflict()
        } else if listening.is_some() {
            theme.primary_at(60.)
        } else {
            gpui::transparent_black().into()
        };
        let slot_index = match slot {
            Slot::Keys => 0,
            Slot::Keys2 => 1,
        };
        let mut input = div()
            .id(ElementId::Name(format!("binding-{id}-{slot_index}").into()))
            .w_full()
            .h(px(28.))
            .pl(px(8.))
            .pr(px(32.))
            .flex()
            .items_center()
            .rounded(px(theme::RADIUS_MD))
            .border_1()
            .border_color(border)
            .text(theme::TEXT_MD)
            .whitespace_nowrap()
            .overflow_hidden()
            .cursor(CursorStyle::IBeam)
            .child(text)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    this.capturing = Some((id, slot, None));
                    window.focus(&this.capture_focus, cx);
                    // The window's root takes the focus on a press unless
                    // told the press was handled.
                    window.prevent_default();
                    cx.notify();
                }),
            );
        input = if empty {
            input.text_color(theme.text_disabled()).italic()
        } else {
            input.text_color(theme.text())
        };
        // The field chosen to listen holds the keyboard focus from the frame
        // after the press; a focus handle no element tracks is let go.
        let chosen = self
            .capturing
            .as_ref()
            .is_some_and(|(at, on, _)| *at == id && *on == slot);
        if chosen {
            input = input
                .track_focus(&self.capture_focus)
                .on_key_down(cx.listener(Self::captured));
        }
        if listening.is_some() {
            input = input.bg(theme.control_surface());
        }
        let mut wrap = div().relative().w_full().child(input);
        if !keys.is_empty() {
            let clear = window.use_keyed_state(
                ElementId::Name(format!("binding-clear-{id}-{slot_index}").into()),
                cx,
                |_, _| false,
            );
            let hovered = *clear.read(cx);
            wrap = wrap.child(
                div()
                    .id(ElementId::Name(
                        format!("binding-clear-button-{id}-{slot_index}").into(),
                    ))
                    .absolute()
                    .top(px(3.))
                    .right(px(3.))
                    .size(px(22.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(theme::RADIUS_CONTROL))
                    .when(hovered, |button| button.bg(theme.contrast(10.)))
                    .cursor_pointer()
                    .child(ui::icon(
                        "x",
                        13.,
                        if hovered {
                            theme.text()
                        } else {
                            theme.text_tertiary()
                        },
                    ))
                    .on_hover(move |hovered, _, cx| {
                        clear.update(cx, |state, cx| {
                            *state = *hovered;
                            cx.notify();
                        })
                    })
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.bind(id, slot, String::new(), cx);
                        if this.capturing.is_some() {
                            this.capturing = None;
                            window.blur(cx);
                        }
                        cx.notify();
                    })),
            );
        }
        grow(div().p(px(4.)), 3.).child(wrap)
    }

    /// The section's content: the preset block over the shortcut tables,
    /// which scroll on their own.
    pub(super) fn shortcuts_panel(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = theme::theme(cx);
        let (preset, overrides) = {
            let draft = &self.model.read(cx).draft;
            (draft.keyboard, draft.shortcuts.clone())
        };
        let query = self.shortcut_search.read(cx).value();
        let groups = registry::filter(registry::groups(preset, &overrides), &query);
        let model = self.model.clone();
        let preset_block = div().w_full().mb(px(12.)).child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .p(px(12.))
                .rounded(px(theme::RADIUS_LG))
                .border_1()
                .border_color(theme.border_secondary())
                .bg(theme.contrast(3.))
                .child(
                    div()
                        .text(theme::TEXT_MD)
                        .line_height(px(26.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(t("Keyboard")),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .min_h(px(26.))
                        .child(
                            div()
                                .min_w(px(100.))
                                .pt(px(2.))
                                .text(theme::TEXT_MD)
                                .line_height(px(theme::TEXT_MD * 1.3))
                                .text_color(theme.text_secondary())
                                .child(t("Keyboard layout")),
                        )
                        .child(
                            div().ml_auto().flex().items_center().child(
                                ui::select(
                                    "keyboard-layout",
                                    preset.value(),
                                    vec![
                                        choice("us", "US (QWERTY)"),
                                        choice("eu", "EU (QWERTZ / AZERTY / Nordic)"),
                                    ],
                                )
                                .width(260.)
                                .on_change(move |value, _, cx| {
                                    model.update(cx, |model, cx| {
                                        model.edit(
                                            |draft| draft.keyboard = KeyboardPreset::from_value(value),
                                            cx,
                                        )
                                    })
                                }),
                            ),
                        ),
                )
                .child(
                    div()
                        .mt(px(8.))
                        .text(theme::TEXT_SM)
                        .line_height(px(20.))
                        .text_color(theme.text_secondary())
                        .child(t("EU mode adds alternatives for shortcuts that use backtick, backslash, and brackets.")),
                ),
        );

        let mut list = div().w_full().flex().flex_col();
        if groups.is_empty() {
            list = list.child(
                div()
                    .p(px(24.))
                    .flex()
                    .justify_center()
                    .text(theme::TEXT_MD)
                    .text_color(theme.text_tertiary())
                    .child(t("No shortcuts match your search.")),
            );
        }
        for group in &groups {
            list = list.child(
                div()
                    .mt(px(16.))
                    .mb(px(8.))
                    .flex()
                    .text(theme::TEXT_SM)
                    // Mantine's body `line-height: 1.55`.
                    .line_height(px(theme::TEXT_SM * 1.55))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_secondary())
                    .child(format!("{} ", t(group.name)))
                    .child(
                        div()
                            .ml(px(4.))
                            .font_weight(FontWeight::NORMAL)
                            .text_color(theme.text_tertiary())
                            .child(format!("({})", group.items.len())),
                    ),
            );
            let last = group.items.len().saturating_sub(1);
            let mut table = div()
                .flex()
                .flex_col()
                .rounded(px(theme::RADIUS_LG))
                .border_1()
                .border_color(theme.border_secondary())
                .overflow_hidden();
            for (index, item) in group.items.iter().enumerate() {
                let conflict = self.conflicts.contains_key(item.id);
                let mut label = grow(div(), 4.)
                    .min_w_0()
                    .py(px(4.))
                    .px(px(12.))
                    .overflow_hidden()
                    .child(
                        // Mantine's body `line-height: 1.55` for both lines,
                        // which overflow the 36px row evenly when there is a
                        // description.
                        div()
                            .truncate()
                            .text(theme::TEXT_MD)
                            .line_height(px(theme::TEXT_MD * 1.55))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text())
                            .child(t(item.label)),
                    );
                if let Some(description) = item.description {
                    label = label.child(
                        div()
                            .truncate()
                            .text(theme::TEXT_CAPTION)
                            .line_height(px(theme::TEXT_CAPTION * 1.55))
                            .text_color(theme.text_secondary())
                            .opacity(0.8)
                            .child(t(description)),
                    );
                }
                let mut row = div()
                    .h(px(36.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .child(label)
                    .child(self.binding(item.id, Slot::Keys, &item.keys, conflict, window, cx))
                    .child(self.binding(item.id, Slot::Keys2, &item.keys2, conflict, window, cx));
                if index < last {
                    row = row.border_b_1().border_color(theme.border_secondary());
                }
                // Even rows are shaded; the shading keeps inside the
                // table's rounded corners.
                if index % 2 == 1 {
                    row = row.bg(theme.contrast(3.));
                    if index == last {
                        row = row.rounded_b(px(theme::RADIUS_LG - 1.));
                    }
                }
                table = table.child(row);
            }
            list = list.child(table);
        }

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(preset_block)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(div().mb(px(12.)).child(ui::field(
                        "shortcut-search",
                        &self.shortcut_search,
                        Look::Form,
                    )))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .child(
                                div()
                                    .id("shortcut-list")
                                    .size_full()
                                    .overflow_y_scroll()
                                    .track_scroll(&self.shortcuts_scroll)
                                    .pr(px(ui::scrollbar::GUTTER))
                                    .child(list),
                            )
                            .child(ui::scrollbar(
                                "shortcut-scrollbar",
                                &self.shortcuts_scroll,
                                window,
                                cx,
                            )),
                    ),
            )
    }
}
