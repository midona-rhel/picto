//! The glass surface of every floating panel (`surfaces.css`
//! `.glassSurface` and `.floatingGlassSurface::after`): the glass fill, the
//! panel shadow, and a 1px edge brighter on top (`--glass-border-top`) than
//! at the sides (`--glass-border-side`) and the bottom
//! (`--glass-border-bottom`).
use crate::theme;
use gpui::*;

/// The alpha an edge laid over the bottom edge's colour needs so that the
/// two together show `wanted`: `1 − (1 − bottom)(1 − a) = wanted`.
fn laid_over(wanted: f32, bottom: f32) -> f32 {
    1. - (1. - wanted) / (1. - bottom)
}

/// The three glass edges, inside a box of `radius` that fills its parent.
/// One border has one colour, so the bottom's goes all the way round and the
/// sides and the top are laid over it, white at the strengths that add up
/// to the tokens'.
pub fn glass_edges(radius: f32, cx: &App) -> Div {
    let theme = theme::theme(cx);
    let bottom = theme.glass_border_bottom();
    let over = |token: Rgba| Rgba {
        a: laid_over(token.a, bottom.a),
        ..token
    };
    let edge = || div().absolute().inset_0().rounded(px(radius));
    div()
        .absolute()
        .inset_0()
        .child(edge().border_1().border_color(bottom))
        .child(
            edge()
                .border_l_1()
                .border_r_1()
                .border_color(over(theme.glass_border())),
        )
        .child(
            edge()
                .border_t_1()
                .border_color(over(theme.glass_border_top())),
        )
}

/// A glass panel with corners of `radius`: a div to lay out and fill as
/// usual, whose glass edges are drawn over its contents, as a border would
/// be, once it renders. The edges take no room.
pub fn glass_panel(id: impl Into<ElementId>, radius: f32, cx: &App) -> GlassPanel {
    let theme = theme::theme(cx);
    GlassPanel {
        panel: div()
            .id(id)
            .rounded(px(radius))
            .bg(theme.glass())
            .shadow(theme.panel_shadow()),
        edges: glass_edges(radius, cx),
    }
}

#[derive(IntoElement)]
pub struct GlassPanel {
    panel: Stateful<Div>,
    edges: Div,
}

impl GlassPanel {
    /// The panel as a plain element, its edges over what it holds so far,
    /// for a caller that must return a `Stateful<Div>`.
    pub fn into_div(self) -> Stateful<Div> {
        self.panel.child(self.edges)
    }
}

impl Styled for GlassPanel {
    fn style(&mut self) -> &mut StyleRefinement {
        self.panel.style()
    }
}

impl InteractiveElement for GlassPanel {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.panel.interactivity()
    }
}

impl StatefulInteractiveElement for GlassPanel {}

impl ParentElement for GlassPanel {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.panel.extend(elements);
    }
}

impl RenderOnce for GlassPanel {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.into_div()
    }
}

#[cfg(test)]
mod tests {
    use super::laid_over;

    #[test]
    fn edges_laid_over_the_bottom_show_the_tokens_strengths() {
        let bottom = 0.1;
        for wanted in [0.15, 0.25] {
            let alpha = laid_over(wanted, bottom);
            // White over white at `bottom`, then at `alpha`.
            let shown = 1. - (1. - bottom) * (1. - alpha);
            assert!((shown - wanted).abs() < 1e-6, "{wanted} {shown}");
        }
        assert!((laid_over(0.15, 0.1) - 0.0556).abs() < 1e-4);
        assert!((laid_over(0.25, 0.1) - 0.1667).abs() < 1e-4);
    }
}
