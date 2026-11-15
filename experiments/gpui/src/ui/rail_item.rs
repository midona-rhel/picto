//! A row of a rail of views: a mark or a group's dot, a name cut short
//! with an ellipsis, a count at the end, and the chosen row's fill. A button
//! read as its name and count, pressed while its view is shown. Each
//! production stylesheet that draws one is a [`Look`], as glyph buttons
//! are, since their sizes, paddings and strengths differ.
use crate::theme;
use crate::theme::Text as _;
use gpui::prelude::FluentBuilder as _;
use gpui::*;

/// Which production class the row ports.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    /// `SelectionModalShell.module.css` `.sidebarItem` (the Add Tags and
    /// Folders dialogs): 28px, 10px either side, the chosen row filled and
    /// ringed, the subtle overlay under the pointer; the dot at 60% and the
    /// count at half strength until the row is chosen or under the pointer.
    SelectionModal,
    /// `TagPicker.module.css` `.sidebarItem`: 24px on a 24px line, the mark
    /// bringing its own margins, 8px on the right; the hover surface under
    /// the pointer unless chosen; the count at 70% until then.
    Picker,
    /// `AiTaggerPanel.module.css` `.sidebarItem`: 24px, 8px either side,
    /// the name at most 122px; the hover surface under the pointer, chosen
    /// or not; disabled at 45%.
    AiTagger,
    /// `TagsScreen.module.css` `.namespaceItem`: the small control's height
    /// and line, 8px in from the rail and 6px inside, the 16px icon and the
    /// name together on the left and the count on the right; the hover
    /// surface under the pointer outranks the chosen fill.
    Namespace,
}

pub struct RailItem {
    id: ElementId,
    look: Look,
    group: SharedString,
    label: SharedString,
    count: SharedString,
    name: Option<SharedString>,
    mark: Option<AnyElement>,
    dot: Option<u32>,
    active: bool,
    disabled: bool,
    name_width: Option<f32>,
}

/// A row named `label` showing `count`.
pub fn rail_item(
    id: impl Into<ElementId>,
    look: Look,
    label: impl Into<SharedString>,
    count: impl Into<SharedString>,
) -> RailItem {
    let id = id.into();
    RailItem {
        group: SharedString::from(format!("rail-item-{id}")),
        id,
        look,
        label: label.into(),
        count: count.into(),
        name: None,
        mark: None,
        dot: None,
        active: false,
        disabled: false,
        name_width: None,
    }
}

impl RailItem {
    /// The hover group of the row, for a mark that brightens with it.
    pub fn group(&self) -> SharedString {
        self.group.clone()
    }
    /// The row's view is the one shown.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
    /// The row cannot be chosen.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    /// An icon, a check box or a dot of the caller's before the name.
    pub fn mark(mut self, mark: impl IntoElement) -> Self {
        self.mark = Some(mark.into_any_element());
        self
    }
    /// A group's 10px dot in `color` before the name (`.sidebarDot`).
    pub fn dot(mut self, color: Option<u32>) -> Self {
        self.dot = color;
        self
    }
    /// The name's widest.
    pub fn name_width(mut self, width: f32) -> Self {
        self.name_width = Some(width);
        self
    }
    /// What assistive technology reads, when it is not the name and the
    /// count shown.
    pub fn accessible_name(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// The row, for the caller to give its click.
    pub fn build(self, cx: &App) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let (look, active, group) = (self.look, self.active, self.group.clone());
        let name = self
            .name
            .clone()
            .unwrap_or_else(|| format!("{} {}", self.label, self.count).into());
        let row = div()
            .id(self.id.clone())
            .role(Role::Button)
            .aria_label(name)
            .aria_toggled(super::a11y::toggled(active))
            .when(look == Look::AiTagger, |row| row.aria_disabled(self.disabled))
            .group(group.clone())
            .rounded(px(theme::RADIUS_MD))
            .text(theme::TEXT_MD);
        match look {
            Look::SelectionModal => {
                let mut label = div().flex_1().min_w_0().truncate().child(self.label);
                if let Some(width) = self.name_width {
                    label = label.max_w(px(width));
                }
                let mut row = row
                    .relative()
                    .h(px(super::selection_modal::ROW))
                    .flex_none()
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .cursor_pointer()
                    .text_color(theme.text())
                    .whitespace_nowrap();
                if active {
                    row = row.bg(theme.surface_active()).child(
                        // `outline: 1px solid var(--color-contrast-25)`, inside.
                        div()
                            .absolute()
                            .inset_0()
                            .rounded(px(theme::RADIUS_MD))
                            .border_1()
                            .border_color(theme.contrast(25.)),
                    );
                } else {
                    row = row.hover(move |style| style.bg(theme.subtle_overlay()));
                }
                if let Some(color) = self.dot {
                    row = row.child(
                        div()
                            .size(px(10.))
                            .flex_none()
                            .rounded_full()
                            .bg(rgb(color))
                            .opacity(if active { 1. } else { 0.6 })
                            .group_hover(group.clone(), |style| style.opacity(1.)),
                    );
                }
                row.children(self.mark).child(label).child(
                    super::selection_modal::badge(group, active, cx)
                        .ml_auto()
                        .child(self.count),
                )
            }
            Look::Picker => row
                .h(px(24.))
                .flex_none()
                .pr(px(8.))
                .flex()
                .items_center()
                .line_height(px(24.))
                .text_color(theme.text())
                .whitespace_nowrap()
                .bg(if active {
                    theme.surface_active()
                } else {
                    gpui::transparent_black().into()
                })
                .when(!active, move |row| {
                    row.hover(move |style| style.bg(theme.surface_hover()))
                })
                .children(self.mark)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h(px(18.))
                        .line_height(px(18.))
                        .truncate()
                        .child(self.label),
                )
                .child(
                    div()
                        .flex_none()
                        .ml_auto()
                        .text(theme::TEXT_CAPTION)
                        .text_color(theme.text_tertiary())
                        .opacity(if active { 1. } else { 0.7 })
                        .group_hover(group, |style| style.opacity(1.))
                        .child(self.count),
                ),
            Look::AiTagger => {
                let row = row
                    .w_full()
                    .h(px(24.))
                    .flex_none()
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_color(theme.text())
                    .whitespace_nowrap()
                    .children(self.mark)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .max_w(px(122.))
                            .truncate()
                            .child(self.label),
                    )
                    .child(
                        div()
                            .ml_auto()
                            .flex_none()
                            .text(theme::TEXT_CAPTION)
                            .text_color(theme.text_tertiary())
                            .child(self.count),
                    )
                    .when(active, |row| row.bg(theme.surface_active()));
                if self.disabled {
                    row.opacity(0.45)
                } else {
                    row.cursor_pointer()
                        .hover(move |style| style.bg(theme.surface_hover()))
                }
            }
            Look::Namespace => row
                .h(px(theme::CONTROL_SM))
                .mx(px(8.))
                .mb(px(1.))
                .px(px(6.))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .when(active, |row| row.bg(theme.surface_active()))
                .hover(move |style| style.bg(theme.surface_hover()))
                .cursor_pointer()
                .line_height(px(theme::CONTROL_SM))
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .children(self.mark)
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_color(theme.text())
                                .child(self.label),
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .ml_auto()
                        .text_color(theme.text_tertiary())
                        .text(theme::TEXT_CAPTION)
                        .child(self.count),
                ),
        }
    }
}
