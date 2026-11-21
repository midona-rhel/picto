//! The rails' resize edges (`.sidebarResizeHandle`,
//! `.inspectorResizeHandle` in `AppShell.module.css`) and the drag of one.
//! Both edges are one element: a 5px strip over the rail's inner side whose
//! 1px border is the rail's divider. Under the pointer or while dragging the
//! border grows to 2px in the text colour over 200 ms (production's
//! `--color-primary-70` is undefined there, so the border takes
//! `currentColor`), and goes back 100 ms after the pointer leaves.
use crate::{motion, state, theme};
use gpui::prelude::FluentBuilder as _;
use gpui::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rail {
    Sidebar,
    Inspector,
}

/// A drag of a rail's edge: which rail, where the pointer went down, and the
/// rail's width then.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Resize {
    pub rail: Rail,
    pub from: f32,
    pub width: f32,
}

impl Resize {
    /// The rail's width with the pointer at `x`, rounded and kept within the
    /// rail's limits (`setSidebarWidthAtom`, `setInspectorWidthAtom`). The
    /// sidebar's edge is its right side, the inspector's its left.
    pub fn width_at(&self, x: f32) -> f32 {
        match self.rail {
            Rail::Sidebar => state::sidebar_width(self.width + (x - self.from)),
            Rail::Inspector => state::inspector_width(self.width + (self.from - x)),
        }
    }
}

/// The edge of `rail`, without its press handler. `dragging` keeps it lit.
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
