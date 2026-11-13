use crate::grid::format;
use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

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
    active: bool,
    /// `Some(expanded)` when the row has children.
    disclosure: Option<bool>,
    branch: Branch,
    on_click: Option<ClickHandler>,
    on_toggle: Option<ClickHandler>,
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
        active: false,
        disclosure: None,
        branch: Branch::default(),
        on_click: None,
        on_toggle: None,
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
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
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
            .child(
                div()
                    .absolute()
                    .left(px(theme::SIDEBAR_INSET + indent))
                    .right(px(2.))
                    .top_0()
                    .bottom(px(1.))
                    .rounded(px(theme::RADIUS_MD))
                    .bg(if self.active {
                        theme.surface_active()
                    } else {
                        theme.surface_hover()
                    })
                    .opacity(if self.active { 1. } else { hover }),
            );
        if interactive {
            row = row.cursor_pointer();
        }
        if self.depth > 0 {
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
        row = row
            .child(
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
                        .opacity(if self.active || hovered {
                            1.
                        } else {
                            0.8
                        }),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(self.label),
            );
        if let Some(count) = self.count {
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
                    .text_color(if self.active {
                        theme.text()
                    } else {
                        theme.text_tertiary()
                    })
                    .child(format::grouped(count)),
            );
        }
        if let Some(handler) = self.on_click {
            row = row.on_click(handler);
        }
        row
    }
}
