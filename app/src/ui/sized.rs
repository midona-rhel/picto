//! An element that builds its contents from its own bounds, in the same
//! frame in which it gets those bounds.
//!
//! Some contents can only be built when their size is known. A picture is
//! placed according to the width and height of its pane. A row of buttons
//! shows labels only when the row is wide enough.
//!
//! A view could measure the size while one frame is drawn and build the
//! contents from that size in the next frame. The contents would then be
//! built for the previous size in every frame in which the size changes, for
//! example while the window is being resized. This element avoids that. It
//! takes part in its parent's layout with the style it is given and has no
//! children there. When GPUI has computed its bounds, it calls the builder
//! with those bounds, lays the result out in exactly those bounds and paints
//! it, all in that same frame.
use gpui::*;

type Build = Box<dyn FnOnce(Bounds<Pixels>, &mut Window, &mut App) -> AnyElement>;

pub struct Sized {
    style: StyleRefinement,
    build: Option<Build>,
}

/// Makes an element whose contents `build` returns for the element's
/// bounds. Give the element the style of the box that the contents should
/// fill, for example `size_full`, `flex_1` or `absolute` with `inset_0`.
pub fn sized(
    build: impl FnOnce(Bounds<Pixels>, &mut Window, &mut App) -> AnyElement + 'static,
) -> Sized {
    Sized {
        style: StyleRefinement::default(),
        build: Some(Box::new(build)),
    }
}

/// Makes an element whose contents `view` builds for the element's bounds.
/// A view whose whole content depends on its size returns this element from
/// `render` and builds the content in `build`, which runs on the view in the
/// frame that lays the element out. The element draws nothing when the view
/// no longer exists.
pub fn sized_view<V: 'static>(
    view: WeakEntity<V>,
    build: impl FnOnce(&mut V, Bounds<Pixels>, &mut Window, &mut Context<V>) -> AnyElement + 'static,
) -> Sized {
    sized(move |bounds, window, cx| {
        view.update(cx, |view, cx| build(view, bounds, window, cx))
            .unwrap_or_else(|_| Empty.into_any_element())
    })
}

impl Styled for Sized {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl IntoElement for Sized {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Sized {
    type RequestLayoutState = ();
    type PrepaintState = Option<AnyElement>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        // The element has no children during its parent's layout. Its size
        // comes only from its style.
        let mut style = Style::default();
        style.refine(&self.style);
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        // The bounds are known here. The contents are built for them and
        // laid out as a tree of their own that is exactly as large as the
        // bounds.
        let mut contents = (self.build.take()?)(bounds, window, cx);
        contents.prepaint_as_root(bounds.origin, bounds.size.into(), window, cx);
        Some(contents)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        contents: &mut Option<AnyElement>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(contents) = contents {
            contents.paint(window, cx);
        }
    }
}
