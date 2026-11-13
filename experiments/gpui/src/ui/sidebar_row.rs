use super::text_input::TextInput;
use crate::grid::format;
use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::prelude::FluentBuilder as _;
use gpui::*;

pub type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
type PressHandler = Box<dyn Fn(&MouseDownEvent, &mut Window, &mut App) + 'static>;

/// Where a dragged folder would land relative to this row
/// (`.dropBefore`, `.dropInside`, `.dropAfter`).
// Nothing drags rows yet; the looks are here for when something does.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropPosition {
    Before,
    Inside,
    After,
}

/// What a row keeps between frames.
struct State {
    /// The row highlight, 50 ms.
    hover: motion::Fade,
    /// The disclosure triangle brightening under the pointer, 200 ms.
    arrow: motion::Fade,
    /// On while the row's children are folded away: the triangle's quarter
    /// turn, 200 ms.
    folded: motion::Fade,
}

/// The Subscriptions row's activity button (`.activityButton`), shown in
/// place of a count while subscriptions run or are held: a pulsing dot that
/// becomes a pause glyph under the pointer, or, while everything is held, a
/// pause glyph that becomes a play glyph.
pub struct Activity {
    pub paused: bool,
    /// The pause or resume is on its way.
    pub busy: bool,
    /// What a click does, also its tooltip.
    pub label: SharedString,
    pub on_click: ClickHandler,
}

/// `subscriptionActivityPulse`, 2.4 s: from 65% opacity with a 3px glow at
/// 30% to full opacity with a 9px glow at 60% and back, each half on
/// `ease-in-out`. Returns the opacity, the glow's blur and its strength.
pub fn activity_pulse(elapsed: std::time::Duration) -> (f32, f32, f32) {
    let period = motion::STATUS_PULSE.as_secs_f32();
    let phase = (elapsed.as_secs_f32() % period) / (period / 2.);
    let along = motion::EASE_IN_OUT.ease(if phase < 1. { phase } else { 2. - phase });
    let between = |from: f32, to: f32| from + (to - from) * along;
    (between(0.65, 1.), between(3., 9.), between(30., 60.))
}

/// One clock for the pulse, so it does not restart when the row repaints.
fn pulse_clock() -> std::time::Instant {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    *START.get_or_init(std::time::Instant::now)
}

/// Where a nested row sits in its tree, for the guide lines.
#[derive(Clone, Default)]
pub struct Branch {
    /// For each ancestor depth, whether that ancestor has siblings below it,
    /// so its vertical line continues through this row.
    pub continues: Vec<bool>,
    pub last_child: bool,
}

/// One sidebar list row (`SidebarRow.tsx`): tree guides, disclosure, glyph,
/// label and count over a highlight that is solid when the row is active and
/// otherwise fades with the pointer over 50 ms.
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
    /// One of several rows chosen with Cmd or Shift (`.selected`).
    selected: bool,
    /// The row whose context menu is open (`.contextHighlight`).
    context: bool,
    /// The row holds keyboard focus (`.rowInteractive:focus-visible`).
    focused: bool,
    /// The whole row takes a drop (`.dropTarget`).
    drop_target: bool,
    drop: Option<DropPosition>,
    /// The field that replaces the label while the row is renamed.
    rename: Option<Entity<TextInput>>,
    /// `Some(expanded)` when the row has children.
    disclosure: Option<bool>,
    branch: Branch,
    /// `showTreeGuides`: whether nested rows draw their guide lines.
    guides: bool,
    on_click: Option<ClickHandler>,
    on_toggle: Option<ClickHandler>,
    on_secondary: Option<PressHandler>,
}

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
        drop_target: false,
        drop: None,
        rename: None,
        disclosure: None,
        branch: Branch::default(),
        guides: true,
        on_click: None,
        on_toggle: None,
        on_secondary: None,
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
    /// A count of zero is not shown.
    pub fn count(mut self, count: Option<u64>) -> Self {
        self.count = count.filter(|count| *count > 0);
        self
    }
    /// The activity button, which takes the count's place.
    pub fn activity(mut self, activity: Option<Activity>) -> Self {
        self.activity = activity;
        self
    }
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
    /// Not drawn on the active row, which is already highlighted.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    /// Not drawn on the active row.
    pub fn context_highlight(mut self, context: bool) -> Self {
        self.context = context;
        self
    }
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }
    pub fn drop_target(mut self, drop_target: bool) -> Self {
        self.drop_target = drop_target;
        self
    }
    pub fn drop(mut self, drop: Option<DropPosition>) -> Self {
        self.drop = drop;
        self
    }
    /// Shows this field in place of the label.
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
    /// A right-button press: the row's context menu.
    pub fn on_secondary(
        mut self,
        handler: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_secondary = Some(Box::new(handler));
        self
    }
}

/// The highlight behind a row (`.row::before`) in the order the stylesheet
/// resolves it: a drop inside, a drop target, then the active row, each
/// forced; otherwise the hover colour, held on while the row is selected,
/// focused or showing its menu, and fading with the pointer otherwise.
/// Returns the fill, the 1px border inside it, and its opacity.
fn highlight(theme: theme::Theme, row: &SidebarRow, hover: f32) -> (Rgba, Option<Rgba>, f32) {
    if row.drop == Some(DropPosition::Inside) {
        (theme.primary_at(10.), Some(theme.primary_at(30.)), 1.)
    } else if row.drop_target {
        (theme.surface_drop(), Some(theme.primary()), 1.)
    } else if row.active {
        (theme.surface_active(), None, 1.)
    } else {
        let held = row.selected || row.context || row.focused;
        (
            theme.surface_hover(),
            row.context.then(|| theme.border()),
            if held { 1. } else { hover },
        )
    }
}

/// The guide lines of one row, as the two SVGs in `SidebarRow.tsx` draw them:
/// 10px columns whose 26-unit height is stretched over the row.
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
                // The line breaks around the disclosure triangle.
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
                // The last child's line turns the corner into its arm.
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
        // The icon brightens with the pointer and on the active row.
        let lit = self.active || hovered;
        let count_color = if self.active || self.selected {
            theme.text()
        } else {
            theme.text_tertiary()
        };

        let row_state = state.clone();
        let mut row = div()
            .id(self.id)
            .relative()
            .flex()
            .items_center()
            .h(px(theme::SIDEBAR_ROW))
            .pl(px(theme::SIDEBAR_INSET + indent))
            .pr(px(2.))
            .on_hover(move |hovered, _, cx| {
                row_state.update(cx, |state, cx| {
                    state.hover.set(*hovered, motion::SIDEBAR_HOVER);
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
        // `.dropBefore::after` / `.dropAfter::after`: a 2px bar across the
        // row's top or bottom edge, from the label column to the list edge.
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
            let mut disclosure = div()
                .id("disclosure")
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
            // `.renameInput`: a 20px field with a 1px primary border on the
            // app background, pulled 1px left so the text starts where the
            // label did.
            Some(input) => {
                let commit = input.clone();
                row.child(
                    div()
                        .id("rename")
                        .relative()
                        .flex_1()
                        .min_w_0()
                        .h(px(theme::SIDEBAR_RENAME_HEIGHT))
                        .ml(px(-1.))
                        .border_1()
                        .border_color(theme.primary())
                        .rounded(px(theme::SIDEBAR_RENAME_RADIUS))
                        .bg(theme.background())
                        .overflow_hidden()
                        // A press in the field edits the name; it is not a
                        // click on the row.
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        // A right-click commits the name, as the Electron
                        // field's context-menu handler does.
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
                    .child(self.label),
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
        row
    }
}

/// `.activitySlot` holding `.activityButton`: 22 × 22, radius 5, primary
/// glyphs, a 30% primary fill under the pointer.
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
    // What shows: the dot while running and not hovered; the pause glyph
    // under the pointer, or while held; the play glyph on a held one hovered.
    let glyph = match (activity.paused, hovered) {
        (false, false) => None,
        (false, true) | (true, false) => Some("activity-pause"),
        (true, true) => Some("activity-play"),
    };
    let inside = match glyph {
        Some(name) => super::icon(name, 14., theme.primary()).into_any_element(),
        None => {
            window.request_animation_frame();
            let (opacity, blur, strength) = activity_pulse(pulse_clock().elapsed());
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
                // The press and the click are the button's, not the row's.
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
