//! A row of a rail of views: a mark or a group's dot, a name that is cut off
//! with an ellipsis, a count at the end, and a fill when the row is chosen.
//! For assistive technology the row is a button whose name is the row's name
//! and count, and it is pressed while its view is shown. The app's rails
//! differ in sizes, paddings and strengths, so each rail is a [`Look`].
use crate::theme;
use crate::theme::Text as _;
use gpui::prelude::FluentBuilder as _;
use gpui::*;

/// Which rail the row belongs to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    /// The rail of the Add Tags dialog and the Folders dialog. The row is 28px
    /// high with 10px of padding on the left and the right. The chosen row has
    /// the active surface colour and a 1px line inside its edge. Another row
    /// has the subtle overlay under the pointer. The dot is at 60% and the
    /// count at half strength, until the row is chosen or under the pointer.
    SelectionModal,
    /// The rail of the tag picker. The row is 24px high with a 24px line
    /// height, no padding on the left (the mark has its own margins) and 8px
    /// on the right. A row that is not chosen has the hover surface colour
    /// under the pointer. The count is at 70% until the row is chosen or under
    /// the pointer.
    Picker,
    /// The rail of the AI tagger panel. The row is 24px high with 8px of
    /// padding on the left and the right, and the name is at most 122px wide.
    /// The row has the hover surface colour under the pointer, whether it is
    /// chosen or not. A disabled row is drawn at 45%.
    AiTagger,
    /// The list of namespaces on the Tags screen. The row is as high as a
    /// small control (`theme::CONTROL_SM`) and has that line height. It is 8px
    /// in from the rail's edges and has 6px of padding. The 16px icon and the
    /// name are together on the left, and the count is on the right. Under the
    /// pointer the row has the hover surface colour, also when it is chosen.
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

/// Returns a row with the name `label` and the count `count`.
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
    /// Returns the name of the row's hover group. A mark that changes while
    /// the pointer is over the row uses it.
    pub fn group(&self) -> SharedString {
        self.group.clone()
    }
    /// Sets whether the row's view is the one that is shown.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
    /// Sets whether the row cannot be chosen.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    /// Sets what is drawn before the name: an icon, a check box or a dot that
    /// the caller made.
    pub fn mark(mut self, mark: impl IntoElement) -> Self {
        self.mark = Some(mark.into_any_element());
        self
    }
    /// Draws a 10px dot in `color` before the name. The dot is the colour of
    /// the group that the row stands for.
    pub fn dot(mut self, color: Option<u32>) -> Self {
        self.dot = color;
        self
    }
    /// Sets the largest width of the name.
    pub fn name_width(mut self, width: f32) -> Self {
        self.name_width = Some(width);
        self
    }
    /// Sets what assistive technology reads for the row, when that is not the
    /// name and the count that the row shows.
    pub fn accessible_name(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Returns the row's element. The caller adds the click handler.
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
            .when(look == Look::AiTagger, |row| {
                row.aria_disabled(self.disabled)
            })
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
                        // A 1px line in `Theme::contrast(25.)` just inside the
                        // row's edge.
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
