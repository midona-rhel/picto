//! The resize edges of the sidebar and the inspector, and the drag of one.
//! One function builds both edges. An edge is a 5px wide strip over the
//! rail's inner side, and its 1px border is the line between the rail and
//! the centre of the window. While the pointer is over the edge, or the edge
//! is being dragged, the border grows to 2px and takes the text colour, over
//! 200 ms. It starts to change back 100 ms after the pointer has left.
use crate::{motion, state, theme};
use gpui::prelude::FluentBuilder as _;
use gpui::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rail {
    Sidebar,
    Inspector,
}

/// A drag of a rail's edge: which rail, the horizontal position at which the
/// pointer was pressed, and the rail's width at that moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Resize {
    pub rail: Rail,
    pub from: f32,
    pub width: f32,
}

impl Resize {
    /// Returns the rail's width when the pointer is at the horizontal
    /// position `x`, rounded and kept within the rail's limits
    /// (`state::sidebar_width`, `state::inspector_width`). The sidebar's
    /// edge is at its right side, so the sidebar gets wider as the pointer
    /// moves right. The inspector's edge is at its left side, so the
    /// inspector gets wider as the pointer moves left.
    pub fn width_at(&self, x: f32) -> f32 {
        match self.rail {
            Rail::Sidebar => state::sidebar_width(self.width + (x - self.from)),
            Rail::Inspector => state::inspector_width(self.width + (self.from - x)),
        }
    }
}

/// Builds the resize edge of `rail`. The caller adds the listener for the
/// press that starts a drag. While `dragging` is true, the edge is drawn with
/// its 2px border in the text colour.
pub fn edge(rail: Rail, dragging: bool, window: &mut Window, cx: &mut App) -> Stateful<Div> {
    let theme = theme::theme(cx);
    let (id, key) = match rail {
        Rail::Sidebar => ("sidebar-resize", "sidebar-resize-edge"),
        Rail::Inspector => ("inspector-resize", "inspector-resize-edge"),
    };
    let fade = window.use_keyed_state(key, cx, |_, _| motion::Fade::default());
    let lit = if dragging { 1. } else { fade.read(cx).value() };
    if fade.read(cx).animating() {
        window.request_animation_frame();
    }
    let border = px(1. + lit);
    div()
        .id(id)
        .occlude()
        .absolute()
        .top_0()
        .bottom_0()
        .w(px(5.))
        .cursor(CursorStyle::ResizeColumn)
        .map(|edge| match rail {
            Rail::Sidebar => edge.border_r(border),
            Rail::Inspector => edge.border_l(border),
        })
        .border_color(theme::blend(theme.border_secondary(), theme.text(), lit))
        .on_hover(move |hovered, _, cx| {
            fade.update(cx, |fade, cx| {
                let delay = if *hovered {
                    std::time::Duration::ZERO
                } else {
                    motion::RESIZE_EDGE_DELAY
                };
                fade.set_after(*hovered, motion::NORMAL, delay);
                cx.notify();
            })
        })
}

#[cfg(test)]
mod tests {
    use super::{Rail, Resize};

    #[test]
    fn each_edge_follows_the_pointer_within_its_rails_limits() {
        let sidebar = Resize {
            rail: Rail::Sidebar,
            from: 300.,
            width: 300.,
        };
        assert_eq!(sidebar.width_at(360.4), 360.);
        assert_eq!(sidebar.width_at(0.), 220.);
        assert_eq!(sidebar.width_at(900.), 500.);
        let inspector = Resize {
            rail: Rail::Inspector,
            from: 893.,
            width: 320.,
        };
        assert_eq!(inspector.width_at(843.), 370.);
        assert_eq!(inspector.width_at(1200.), 260.);
        assert_eq!(inspector.width_at(0.), 550.);
    }
}
