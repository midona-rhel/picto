//! Holds the open context menu of a view. `Host` keeps the menu while it is
//! open and while it fades out, passes the rows that are chosen to the view,
//! and gives the keyboard focus back when the menu has gone.
use super::{Event, Menu};
use gpui::*;

/// Says where the keyboard focus goes when a menu or dialog has gone. The
/// focus does not go there if the action of the menu or dialog moved it
/// somewhere else, for example into a name field.
#[derive(Clone)]
pub enum Return {
    /// The focus goes to this handle.
    To(FocusHandle),
    /// The focus goes to whatever had it when the menu or dialog opened.
    Opener,
    /// The focus is left where it is.
    Stay,
}

impl Return {
    /// Returns the handle that the focus goes back to. This is called when
    /// the menu or dialog opens, because `Opener` means whatever has the
    /// focus at that moment.
    pub(crate) fn resolve(&self, window: &Window, cx: &App) -> Option<FocusHandle> {
        match self {
            Return::To(handle) => Some(handle.clone()),
            Return::Opener => window.focused(cx),
            Return::Stay => None,
        }
    }
}

/// Gives the keyboard focus to `back`, but only if the focus is still in the
/// menu or dialog that is going (`still_there`) or nothing has the focus.
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
    /// The menu is drawn where the owner puts [`Host::layer`] among its
    /// elements. The window's root view uses this, because its layer already
    /// covers the window.
    Inline,
    /// The menu is drawn over the whole window as a deferred element of this
    /// priority, above the owner's deferred elements of a lower priority.
    Deferred(usize),
}

/// Holds the one context menu that its owner has open. A menu that is opened
/// while another is showing replaces it at once.
pub struct Host {
    menu: Option<Entity<Menu>>,
    placement: Placement,
    back: Return,
    back_to: Option<FocusHandle>,
    _chosen: Option<Subscription>,
}

impl Host {
    /// Returns a host whose menus are drawn over the whole window as a
    /// deferred element of this priority.
    pub fn deferred(priority: usize, back: Return) -> Self {
        Self::with(Placement::Deferred(priority), back)
    }

    /// Returns a host whose menus are drawn where the owner puts
    /// [`Host::layer`] among its elements. This is for the window's root
    /// view.
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

    /// Shows `menu`, gives it the keyboard focus, and calls `on_choose` with
    /// the action of each row that is chosen.
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

    /// Shows `menu` and gives it the keyboard focus, without subscribing to
    /// the rows that are chosen. This is for a menu whose chosen rows
    /// another view handles. The window shows the menus of the sidebar and
    /// of the filter bar this way, above everything else.
    pub fn show<V: 'static>(
        &mut self,
        menu: Entity<Menu>,
        window: &mut Window,
        cx: &mut Context<V>,
    ) {
        self._chosen = None;
        // When this menu replaces another, the focus later goes back to
        // where it would have gone after the first menu.
        if self.menu.is_none() {
            self.back_to = self.back.resolve(window, cx);
        }
        window.focus(&menu.focus_handle(cx), cx);
        self.menu = Some(menu);
        cx.notify();
    }

    /// Sets where the keyboard focus goes when the next menu that opens has
    /// gone.
    pub fn returning(&mut self, back: Return) {
        self.back = back;
    }

    /// Returns whether a menu is showing. A menu that is fading out counts
    /// as showing.
    pub fn is_open(&self) -> bool {
        self.menu.is_some()
    }

    /// Dismisses the open menu, which then fades out. Returns whether there
    /// was a menu that had not finished fading out.
    pub fn close(&self, cx: &mut App) -> bool {
        match &self.menu {
            Some(menu) if !menu.read(cx).finished() => {
                menu.update(cx, |menu, cx| menu.dismiss(cx));
                true
            }
            _ => false,
        }
    }

    /// Returns the element that draws the menu, for the owner to add in its
    /// `render`. A menu that has finished fading out is dropped here, and
    /// the keyboard focus goes back unless the menu's action moved it. The
    /// pop-up windows of a menu can stay open for a moment after the fade
    /// and still have the focus. This counts as the focus not having moved.
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
