//! Holding an open context menu for a view: the menu while it is open and
//! while it fades out, what its rows do, and where the keyboard goes back
//! to once it has gone.
use super::{Event, Menu};
use gpui::*;

/// Where the keyboard goes once a menu or dialog has gone, unless what it
/// did moved the keyboard somewhere else (into a name field, say).
#[derive(Clone)]
pub enum Return {
    /// To this handle.
    To(FocusHandle),
    /// To whatever had the keyboard when it opened.
    Opener,
    /// Nowhere: the keyboard is left where it is.
    Stay,
}

impl Return {
    /// The handle to go back to, worked out as the menu or dialog opens.
    pub(crate) fn resolve(&self, window: &Window, cx: &App) -> Option<FocusHandle> {
        match self {
            Return::To(handle) => Some(handle.clone()),
            Return::Opener => window.focused(cx),
            Return::Stay => None,
        }
    }
}

/// Gives the keyboard back to `back` if it is still in what is going
/// (`still_there`) or nothing has it.
pub(crate) fn give_back(
    back: Option<FocusHandle>,
    still_there: bool,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(back) = back
        && (still_there || window.focused(cx).is_none())
    {
        window.focus(&back, cx);
    }
}

#[derive(Clone, Copy)]
enum Placement {
    /// Where [`Host::layer`] is placed in the owner's tree, for the window's
    /// root view, whose layer already covers the window.
    Inline,
    /// Over the whole window, above the owner's other deferred layers drawn
    /// at a lower priority.
    Deferred(usize),
}

/// One context menu at a time for its owner. A menu opened while another
/// is showing replaces it at once.
pub struct Host {
    menu: Option<Entity<Menu>>,
    placement: Placement,
    back: Return,
    back_to: Option<FocusHandle>,
    _chosen: Option<Subscription>,
}

impl Host {
    /// Menus drawn over the whole window at this `deferred` priority.
    pub fn deferred(priority: usize, back: Return) -> Self {
        Self::with(Placement::Deferred(priority), back)
    }

    /// Menus drawn where the owner places [`Host::layer`]: for the window's
    /// root view.
    pub fn inline(back: Return) -> Self {
        Self::with(Placement::Inline, back)
    }

    fn with(placement: Placement, back: Return) -> Self {
        Self {
            menu: None,
            placement,
            back,
            back_to: None,
            _chosen: None,
        }
    }

    /// Shows `menu` with the keyboard, and tells `on_choose` the action of
    /// each row chosen.
    pub fn open<V: 'static>(
        &mut self,
        menu: Entity<Menu>,
        window: &mut Window,
        cx: &mut Context<V>,
        on_choose: impl Fn(&mut V, &Entity<Menu>, &'static str, &mut Window, &mut Context<V>) + 'static,
    ) {
        let chosen = cx.subscribe_in(
            &menu,
            window,
            move |this, menu, event: &Event, window, cx| {
                let Event::Chosen(action) = event;
                on_choose(this, menu, action, window, cx);
            },
        );
        self.show(menu, window, cx);
        self._chosen = Some(chosen);
    }

    /// Shows a menu whose rows someone else acts on (the sidebar's and the
    /// filter bar's, which the window shows above everything).
    pub fn show<V: 'static>(
        &mut self,
        menu: Entity<Menu>,
        window: &mut Window,
        cx: &mut Context<V>,
    ) {
        self._chosen = None;
        // A menu replacing another goes back where the first would have.
        if self.menu.is_none() {
            self.back_to = self.back.resolve(window, cx);
        }
        window.focus(&menu.focus_handle(cx), cx);
        self.menu = Some(menu);
        cx.notify();
    }

    /// Whether a menu is showing, fading out included.
    pub fn is_open(&self) -> bool {
        self.menu.is_some()
    }

    /// Starts the open menu's fade. Returns whether there was one that had
    /// not already gone.
    pub fn close(&self, cx: &mut App) -> bool {
        match &self.menu {
            Some(menu) if !menu.read(cx).finished() => {
                menu.update(cx, |menu, cx| menu.dismiss(cx));
                true
            }
            _ => false,
        }
    }

    /// The menu's layer, to draw from the owner's `render`. A menu that has
    /// faded out is dropped here, and the keyboard goes back unless the
    /// menu's action moved it. A menu in pop-up windows can outlive its
    /// fade by a moment, still holding the keyboard, which counts as not
    /// moved.
    pub fn layer(&mut self, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
        if let Some(menu) = self.menu.take_if(|menu| menu.read(cx).finished()) {
            self._chosen = None;
            let still_there = menu.read(cx).holds_focus(window, cx);
            give_back(self.back_to.take(), still_there, window, cx);
        }
        let menu = self.menu.clone()?;
        Some(match self.placement {
            Placement::Inline => menu.into_any_element(),
            Placement::Deferred(priority) => {
                let view = window.viewport_size();
                deferred(
                    anchored()
                        .position(point(px(0.), px(0.)))
                        .child(div().w(view.width).h(view.height).child(menu)),
                )
                .with_priority(priority)
                .into_any_element()
            }
        })
    }
}
