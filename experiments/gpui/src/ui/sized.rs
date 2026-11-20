//! An element whose contents depend on its own size, built in the frame
//! that gives it that size: what CSS does with a container query, or with a
//! picture placed by its pane's width and height.
//!
//! The element takes the place its style gives it in its parent's layout
//! and has no children there. Once its bounds are known, in the same frame,
//! it builds its contents from them, lays them out in exactly those bounds
//! and paints them. Nothing is measured in one frame and applied in the
//! next, so the contents are right while the window is being resized.
use gpui::*;

type Build = Box<dyn FnOnce(Bounds<Pixels>, &mut Window, &mut App) -> AnyElement>;

pub struct Sized {
    style: StyleRefinement,
    build: Option<Build>,
}

/// The contents `build` makes for the bounds the element is given. Style it
/// as the box the contents fill (`size_full`, `flex_1`, `absolute` …).
pub fn sized(
    build: impl FnOnce(Bounds<Pixels>, &mut Window, &mut App) -> AnyElement + 'static,
) -> Sized {
    Sized {
        style: StyleRefinement::default(),
        build: Some(Box::new(build)),
    }
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
