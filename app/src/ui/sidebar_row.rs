use super::text_input::TextInput;
use crate::i18n::format;
use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::prelude::FluentBuilder as _;
use gpui::*;

pub type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
type PressHandler = Box<dyn Fn(&MouseDownEvent, &mut Window, &mut App) + 'static>;

/// Says where a dragged folder would land relative to this row: before it,
/// inside it or after it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropPosition {
    Before,
    Inside,
    After,
}

/// Holds what a row keeps between frames.
struct State {
    /// The fade of the row's highlight when the pointer moves onto or off
    /// the row. It takes 50 ms.
    hover: motion::Fade,
    /// The fade of the disclosure triangle, which is brighter while the
    /// pointer is over it. It takes 200 ms.
    arrow: motion::Fade,
    /// The quarter turn of the disclosure triangle. It is on while the row's
    /// children are hidden, and takes 200 ms.
    folded: motion::Fade,
}

/// The activity button of the Subscriptions row. The row shows it in place
/// of a count while subscriptions run or are paused. While subscriptions
/// run, it is a pulsing dot, and a pause glyph while the pointer is over it.
/// While all subscriptions are paused, it is a pause glyph, and a play glyph
/// while the pointer is over it.
pub struct Activity {
    pub paused: bool,
    /// Whether a pause or a resume has been asked for and has not finished.
    /// The button is then dimmed and a click does nothing.
    pub busy: bool,
    /// The text that says what a click does. It is the button's tooltip and
    /// its name for assistive technology.
    pub label: SharedString,
    pub on_click: ClickHandler,
}

/// Returns how the pulsing dot of the activity button is drawn when the
/// animation clock reads `elapsed`: its opacity, the blur of its glow in
/// pixels, and the opacity of the glow in percent. One pulse takes 2.4 s. It
/// goes from 65% opacity with a 3px glow at 30% to full opacity with a 9px
/// glow at 60% and back, each half on `motion::EASE_IN_OUT`.
fn activity_pulse(elapsed: std::time::Duration) -> (f32, f32, f32) {
    let along = motion::pulse(elapsed);
    let between = |from: f32, to: f32| from + (to - from) * along;
    (between(0.65, 1.), between(3., 9.), between(30., 60.))
}

#[derive(Clone, Default)]
pub struct Branch {
    /// For each depth above this row, whether the ancestor at that depth has
    /// siblings below it. If it has, its vertical guide line continues
    /// through this row.
    pub continues: Vec<bool>,
    pub last_child: bool,
}

/// One row of the sidebar's list. It draws the tree's guide lines, the
/// disclosure triangle, the glyph, the label and the count over a highlight.
/// The highlight is at full opacity when the row is active. Otherwise it
/// fades in over 50 ms when the pointer moves onto the row and fades out
/// when the pointer leaves.
#[derive(IntoElement)]
pub struct SidebarRow {
    id: ElementId,
    depth: usize,
    icon: SharedString,
    /// The colour the user gave the row's folder, for its glyph.
    tint: Option<Rgba>,
    label: SharedString,
    count: Option<u64>,
    activity: Option<Activity>,
    active: bool,
    /// Whether the row is one of several rows chosen with Cmd or Shift.
    selected: bool,
    /// Whether the row's context menu is open.
    context: bool,
    /// Whether the row is the one the sidebar's keys are on.
    focused: bool,
    /// Whether items dragged from the grid are over the row. The row then
    /// has the highlight it has under the pointer.
    drop_highlight: bool,
    /// Whether something is being dragged. The pointer then highlights no
    /// row.
    dragging: bool,
    drop: Option<DropPosition>,
    /// The field that replaces the label while the row is renamed.
    rename: Option<Entity<TextInput>>,
    /// `Some(expanded)` when the row has children.
    disclosure: Option<bool>,
    branch: Branch,
    /// Whether nested rows draw their guide lines.
    guides: bool,
    on_click: Option<ClickHandler>,
    on_toggle: Option<ClickHandler>,
    on_secondary: Option<PressHandler>,
    on_press: Option<PressHandler>,
}

/// The width of the rename field, with its border, when the row shows no
/// count.
const RENAME_WIDTH: f32 = 137.5;

pub fn sidebar_row(
    id: impl Into<ElementId>,
    icon: impl Into<SharedString>,
    label: impl Into<SharedString>,
) -> SidebarRow {
    SidebarRow {
        id: id.into(),
        depth: 0,
        icon: icon.into(),
        tint: None,
        label: label.into(),
        count: None,
        activity: None,
        active: false,
        selected: false,
        context: false,
        focused: false,
        drop_highlight: false,
        dragging: false,
        drop: None,
        rename: None,
        disclosure: None,
        branch: Branch::default(),
        guides: true,
        on_click: None,
        on_toggle: None,
        on_secondary: None,
        on_press: None,
    }
}

impl SidebarRow {
    pub fn depth(mut self, depth: usize) -> Self {
        self.depth = depth;
        self
    }
    pub fn tint(mut self, tint: Option<Rgba>) -> Self {
        self.tint = tint;
        self
    }
    /// Sets the count that the row shows at its right end. A count of zero
    /// is not shown.
    pub fn count(mut self, count: Option<u64>) -> Self {
        self.count = count.filter(|count| *count > 0);
        self
    }
    /// Sets the activity button, which is shown in place of the count.
    pub fn activity(mut self, activity: Option<Activity>) -> Self {
        self.activity = activity;
        self
    }
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
    /// Marks the row as one of several chosen rows. This changes nothing on
    /// the active row, which is already highlighted.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    /// Marks the row as the one whose context menu is open. This changes
    /// nothing on the active row.
    pub fn context_highlight(mut self, context: bool) -> Self {
        self.context = context;
        self
    }
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }
    pub fn drop_highlight(mut self, highlighted: bool) -> Self {
        self.drop_highlight = highlighted;
        self
    }
    pub fn dragging(mut self, dragging: bool) -> Self {
        self.dragging = dragging;
        self
    }
    pub fn drop(mut self, drop: Option<DropPosition>) -> Self {
        self.drop = drop;
        self
    }
    /// Shows this field in place of the label, for renaming the row.
    pub fn rename(mut self, input: Option<Entity<TextInput>>) -> Self {
        self.rename = input;
        self
    }
    pub fn disclosure(mut self, expanded: Option<bool>) -> Self {
        self.disclosure = expanded;
        self
    }
    pub fn branch(mut self, branch: Branch) -> Self {
        self.branch = branch;
        self
    }
    pub fn guides(mut self, guides: bool) -> Self {
        self.guides = guides;
        self
    }
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
    pub fn on_toggle(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_toggle = Some(Box::new(handler));
        self
    }
    /// Sets the handler of a press with the left button, which may start a
    /// drag.
    pub fn on_press(
        mut self,
        handler: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_press = Some(Box::new(handler));
        self
    }
    /// Sets the handler of a press with the right button, which opens the
    /// row's context menu.
    pub fn on_secondary(
        mut self,
        handler: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_secondary = Some(Box::new(handler));
        self
    }
}

/// Returns the highlight behind a row: its fill, the 1px border inside the
/// fill if there is one, and its opacity. A row that a folder would be
/// dropped inside has the primary colour at 10% with a border at 30%.
/// Otherwise the active row has the active surface colour. Both are at full
/// opacity. Every other row has the hover colour. It is at full opacity
/// while the row is selected, focused, showing its context menu or under
/// items dragged from the grid. Otherwise its opacity is `hover`, which
/// follows the pointer, and 0 while something is dragged.
fn highlight(theme: theme::Theme, row: &SidebarRow, hover: f32) -> (Rgba, Option<Rgba>, f32) {
    if row.drop == Some(DropPosition::Inside) {
        (theme.primary_at(10.), Some(theme.primary_at(30.)), 1.)
    } else if row.active {
        (theme.surface_active(), None, 1.)
    } else {
        let held = row.selected || row.context || row.focused || row.drop_highlight;
        let hover = if row.dragging { 0. } else { hover };
        (
            theme.surface_hover(),
            row.context.then(|| theme.border()),
            if held { 1. } else { hover },
        )
    }
}

/// Draws the guide lines of one row. The vertical line of a depth is 3px to
/// the right of the middle of that depth's indent (`theme::SIDEBAR_INDENT`).
/// The heights of the lines are given in units of 1/26 of the row's height.
fn guides(depth: usize, branch: Branch, has_children: bool, color: Rgba) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let unit = bounds.size.height / 26.;
            let column = |level: usize| {
                bounds.left()
                    + px(theme::SIDEBAR_INSET
                        + (level as f32 - 1.) * theme::SIDEBAR_INDENT
                        + theme::SIDEBAR_INDENT / 2.)
            };
            let vertical = |window: &mut Window, x: Pixels, from: f32, to: f32| {
                window.paint_quad(fill(
                    Bounds::from_corners(
                        point(x + px(3.), bounds.top() + unit * from),
                        point(x + px(4.), bounds.top() + unit * to),
                    ),
                    color,
                ));
            };
            let arm = |window: &mut Window, x: Pixels, from: f32, to: f32| {
                let y = (bounds.top() + unit * 13.).floor();
                window.paint_quad(fill(
                    Bounds::from_corners(point(x + px(from), y), point(x + px(to), y + px(1.))),
                    color,
                ));
            };
            for (level, continues) in branch.continues.iter().enumerate() {
                if *continues && level > 0 {
                    vertical(window, column(level), 0., 26.);
                }
            }
            let x = column(depth);
            match (has_children, branch.last_child) {
                // A row with children has a gap in its vertical line where the
                // disclosure triangle is.
                (true, last) => {
                    vertical(window, x, 0., 9.);
                    if !last {
                        vertical(window, x, 17., 26.);
                    }
                    arm(window, x, 8., 12.);
                }
                (false, false) => {
                    vertical(window, x, 0., 26.);
                    arm(window, x, 4., 12.);
                }
                // The vertical line of a last child ends in a rounded corner
                // that joins its horizontal line.
                (false, true) => {
                    let top = bounds.top();
                    let mut path = PathBuilder::stroke(px(1.));
                    path.move_to(point(x + px(3.5), top));
                    path.line_to(point(x + px(3.5), top + unit * 9.5));
                    path.arc_to(
                        point(px(3.5), unit * 3.5),
                        px(0.),
                        false,
                        false,
                        point(x + px(7.), top + unit * 13.),
                    );
                    path.line_to(point(x + px(12.), top + unit * 13.));
                    if let Ok(path) = path.build() {
                        window.paint_path(path, color);
                    }
                }
            }
        },
    )
    .absolute()
    .inset_0()
}

impl RenderOnce for SidebarRow {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let folded = self.disclosure == Some(false);
        let state = window.use_keyed_state(self.id.clone(), cx, |_, _| State {
            hover: motion::Fade::default(),
            arrow: motion::Fade::default(),
            folded: motion::Fade::at(folded),
        });
        if state.read(cx).folded.on() != folded {
            state.update(cx, |state, _| state.folded.set(folded, motion::NORMAL));
        }
        let (hover, hovered, arrow, turn, animating) = {
            let state = state.read(cx);
            (
                state.hover.value(),
                state.hover.on(),
                state.arrow.value(),
                state.folded.value(),
                state.hover.animating() || state.arrow.animating() || state.folded.animating(),
            )
        };
        if animating {
            window.request_animation_frame();
        }
        let indent = self.depth as f32 * theme::SIDEBAR_INDENT;
        let interactive = self.on_click.is_some();
        let (fill, edge, shown) = highlight(theme, &self, hover);
        // The icon is at full opacity on the active row, under the pointer and
        // while items are dragged over the row. Otherwise it is at 80%.
        let lit = self.active || (hovered && !self.dragging) || self.drop_highlight;
        let count_color = if self.active || self.selected {
            theme.text()
        } else {
            theme.text_tertiary()
        };

        let row_state = state.clone();
        // Assistive technology gets the row as a tree item at its level. Its
        // name is its label followed by its count. It is selected while it
        // is the active row or one of the chosen rows, and expanded or
        // collapsed when it has children. The name is built only while the
        // window builds its accessibility tree, because the sidebar draws
        // every row in each frame.
        let name: SharedString = if super::a11y::active(window) {
            let shown_label = crate::i18n::t_shared(self.label.clone());
            match self.count {
                Some(count) => format!("{shown_label} {}", format::grouped(count)).into(),
                None => shown_label,
            }
        } else {
            SharedString::default()
        };
        let mut row = div()
            .id(self.id)
            .role(Role::TreeItem)
            .aria_label(name.clone())
            .aria_level(self.depth + 1)
            .aria_selected(self.active || self.selected)
            .when_some(self.disclosure, |row, expanded| row.aria_expanded(expanded))
            // The row that the sidebar's keys are on is reported as the
            // active descendant, so assistive technology reads it as focused.
            .when(self.focused, |row| row.aria_active_descendant())
            .relative()
            .flex()
            .items_center()
            .h(px(theme::SIDEBAR_ROW))
            .pl(px(theme::SIDEBAR_INSET + indent))
            .pr(px(2.))
            .on_hover(move |hovered, _, cx| {
                row_state.update(cx, |state, cx| {
                    state.hover.set(*hovered, motion::FAST);
                    cx.notify();
                })
            })
            .child({
                let mut layer = div()
                    .absolute()
                    .left(px(theme::SIDEBAR_INSET + indent))
                    .right(px(2.))
                    .top_0()
                    .bottom(px(1.))
                    .rounded(px(theme::RADIUS_MD))
                    .bg(fill)
                    .opacity(shown);
                if let Some(edge) = edge {
                    layer = layer.border_1().border_color(edge);
                }
                layer
            });
        // A folder that would be dropped before or after the row is shown by
        // a 2px bar across the row's top or bottom edge, from the start of
        // the row's content to the right edge of the list.
        if let Some(position @ (DropPosition::Before | DropPosition::After)) = self.drop {
            let bar = div()
                .absolute()
                .left(px(theme::SIDEBAR_INSET + indent))
                .right_0()
                .h(px(2.))
                .rounded(px(1.))
                .bg(theme.primary());
            row = row.child(if position == DropPosition::Before {
                bar.top(px(-1.))
            } else {
                bar.bottom(px(-1.))
            });
        }
        if interactive {
            row = row.cursor_pointer();
        }
        if self.depth > 0 && self.guides {
            row = row.child(guides(
                self.depth,
                self.branch,
                self.disclosure.is_some(),
                theme.tree_guide(),
            ));
        }
        if self.disclosure.is_some() {
            let arrow_state = state.clone();
            let expanded = self.disclosure == Some(true);
            let mut disclosure = div()
                .id("disclosure")
                .role(Role::Button)
                .aria_label(if expanded {
                    crate::tf!("Collapse {value0}", name)
                } else {
                    crate::tf!("Expand {value0}", name)
                })
                .aria_expanded(expanded)
                .absolute()
                .left(px(theme::SIDEBAR_INSET + indent - theme::SIDEBAR_DISCLOSURE))
                .w(px(theme::SIDEBAR_DISCLOSURE))
                .h(px(theme::SIDEBAR_ROW))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .on_hover(move |hovered, _, cx| {
                    arrow_state.update(cx, |state, cx| {
                        state.arrow.set(*hovered, motion::NORMAL);
                        cx.notify();
                    })
                })
                .child(
                    svg()
                        .path("triangle-down")
                        .w(px(7.))
                        .h(px(5.))
                        .text_color(theme.text_tertiary())
                        .opacity(0.7 + 0.3 * arrow)
                        .with_transformation(Transformation::rotate(radians(
                            -std::f32::consts::FRAC_PI_2 * turn,
                        ))),
                );
            if let Some(handler) = self.on_toggle {
                disclosure = disclosure
                    // A press on the triangle is not a press on the row.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |event, window, cx| {
                        cx.stop_propagation();
                        handler(event, window, cx);
                    });
            }
            row = row.child(disclosure);
        }
        row = row.child(
            div()
                .relative()
                .w(px(theme::SIDEBAR_GLYPH_COLUMN))
                .h(px(theme::SIDEBAR_ROW))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    super::icon(
                        self.icon,
                        theme::SIDEBAR_ICON,
                        self.tint.unwrap_or(theme.text()),
                    )
                    .opacity(if lit { 1. } else { 0.8 }),
                ),
        );
        row = match self.rename {
            // The rename field is 20px tall and has a 1px border in the primary
            // colour on the app's background colour. It starts 1px to the left
            // of the label's place, so that its text starts where the label's
            // text did.
            Some(input) => {
                let commit = input.clone();
                // With a count or an activity button, the field fills the row
                // up to it. Without one, the field has the width
                // `RENAME_WIDTH`.
                let fills = self.count.is_some() || self.activity.is_some();
                row.child(
                    div()
                        .id("rename")
                        .relative()
                        .map(|field| {
                            if fills {
                                field.flex_1()
                            } else {
                                field.flex_shrink(1.).w(px(RENAME_WIDTH))
                            }
                        })
                        .min_w_0()
                        .h(px(theme::SIDEBAR_RENAME_HEIGHT))
                        .ml(px(-1.))
                        .border_1()
                        .border_color(theme.primary())
                        .rounded(px(theme::SIDEBAR_RENAME_RADIUS))
                        .bg(theme.background())
                        .overflow_hidden()
                        // A press in the field belongs to the editing of the
                        // name. It is not passed on to the row.
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        // A click with the right button commits the name.
                        .on_mouse_down(MouseButton::Right, move |_, _, cx| {
                            cx.stop_propagation();
                            commit.update(cx, |input, cx| input.finish_rename(true, cx));
                        })
                        .child(input),
                )
            }
            None => row.child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    // The row's name is translated into the interface's
                    // language here.
                    .child(crate::i18n::t_shared(self.label)),
            ),
        };
        if let Some(activity) = self.activity {
            row = row.child(activity_button(activity, window, cx));
        } else if let Some(count) = self.count {
            row = row.child(
                div()
                    .relative()
                    .flex_none()
                    .px(px(5.))
                    .min_w(px(36.))
                    .text_right()
                    .text(theme::TEXT_CAPTION)
                    .font_family(theme::FONT_INSPECTOR_MONO)
                    .font_features(theme::tabular_figures())
                    .text_color(count_color)
                    .child(format::grouped(count)),
            );
        }
        if let Some(handler) = self.on_click {
            row = row.on_click(handler);
        }
        if let Some(handler) = self.on_secondary {
            row = row.on_mouse_down(MouseButton::Right, handler);
        }
        if let Some(handler) = self.on_press {
            row = row.on_mouse_down(MouseButton::Left, handler);
        }
        row
    }
}

/// Draws the activity button at the right end of the row. It is 22 × 22 with
/// a corner radius of 5px. Its glyphs are in the primary colour, and its
/// fill is the primary colour at 30% while the pointer is over it.
fn activity_button(activity: Activity, window: &mut Window, cx: &mut App) -> impl IntoElement {
    let theme = theme::theme(cx);
    let hovered_state = window.use_keyed_state("subscriptions-activity", cx, |_, _| false);
    let hint = window.use_keyed_state("subscriptions-activity-tip", cx, |_, _| {
        super::tooltip::Hover::default()
    });
    if hint.read(cx).animating() {
        window.request_animation_frame();
    }
    let hovered = *hovered_state.read(cx) && !activity.busy;
    // While subscriptions run, the button shows the dot, and the pause glyph
    // while the pointer is over it. While they are paused, it shows the
    // pause glyph, and the play glyph while the pointer is over it.
    let glyph = match (activity.paused, hovered) {
        (false, false) => None,
        (false, true) | (true, false) => Some("activity-pause"),
        (true, true) => Some("activity-play"),
    };
    let inside = match glyph {
        Some(name) => super::icon(name, 14., theme.primary()).into_any_element(),
        None => {
            motion::loop_frame(window, cx);
            let (opacity, blur, strength) = activity_pulse(motion::loop_clock());
            div()
                .size(px(8.))
                .rounded_full()
                .bg(theme.primary())
                .opacity(opacity)
                .shadow(vec![
                    BoxShadow::new(px(0.), px(0.), theme.primary_at(strength).into())
                        .blur_radius(px(blur)),
                ])
                .into_any_element()
        }
    };
    let tip = hint
        .read(cx)
        .layer(&super::tip(activity.label.clone()), 22., cx);
    let on_click = activity.on_click;
    let busy = activity.busy;
    div()
        .relative()
        .flex_none()
        .min_w(px(22.))
        .h(px(theme::SIDEBAR_ROW))
        .ml_auto()
        .flex()
        .items_center()
        .justify_end()
        .child(
            div()
                .id("subscriptions-activity")
                .role(Role::Button)
                .aria_label(activity.label.clone())
                .aria_busy(busy)
                .relative()
                .size(px(22.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.))
                .when(hovered, |button| button.bg(theme.primary_at(30.)))
                .when(busy, |button| button.opacity(0.55))
                .when(!busy, |button| button.cursor_pointer())
                .on_hover(move |hovered, _, cx| {
                    hovered_state.update(cx, |state, cx| {
                        *state = *hovered;
                        cx.notify();
                    });
                    hint.update(cx, |hint, cx| hint.set(*hovered, cx));
                })
                // The press and the click on the button are not passed on to
                // the row.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |event, window, cx| {
                    cx.stop_propagation();
                    if !busy {
                        on_click(event, window, cx);
                    }
                })
                .child(inside)
                .children(tip),
        )
}

#[cfg(test)]
mod activity_tests {
    use super::activity_pulse;
    use std::time::Duration;

    #[test]
    fn the_activity_dot_breathes_between_dim_and_glowing() {
        let (opacity, blur, strength) = activity_pulse(Duration::ZERO);
        assert!((opacity - 0.65).abs() < 1e-4 && (blur - 3.).abs() < 1e-4);
        assert!((strength - 30.).abs() < 1e-3);
        let (opacity, blur, strength) = activity_pulse(Duration::from_millis(1200));
        assert!((opacity - 1.).abs() < 1e-4 && (blur - 9.).abs() < 1e-4);
        assert!((strength - 60.).abs() < 1e-3);
        let rising = activity_pulse(Duration::from_millis(600));
        let falling = activity_pulse(Duration::from_millis(1800));
        assert!((rising.0 - falling.0).abs() < 1e-3);
    }
}
