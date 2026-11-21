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
/// be, once it renders. The edges take no room. What the window drew behind
/// it shows through its fill blurred (`--glass-blur`), and its shadow
/// (`--shadow-panel`) is cut out beneath it.
pub fn glass_panel(id: impl Into<ElementId>, radius: f32, cx: &App) -> GlassPanel {
    let theme = theme::theme(cx);
    let mut panel = GlassPanel::new(div().id(id).bg(theme.glass()), radius)
        .backdrop_blur(theme.glass_blur())
        .drop_shadows(theme.panel_shadow());
    panel.edges = Some(glass_edges(radius, cx));
    panel
}

/// The fill a glass panel in its own transparent window draws over the
/// system's plain blur of what is behind the window
/// ([`super::window_chrome::system_blur_behind`]), so that it shows what
/// `glass` over a blur with `brightness` shows. A brightness below 1 is a
/// black layer of `1 − brightness` over the blur, exactly. Above 1 there
/// is no layer that multiplies, and a white layer of 0.4 comes nearest to
/// `min(1, 3x)` over all of `x` (the two differ by at most 0.4, where the
/// blur is black or a third bright, of which the glass lets a tenth
/// through). The glass and the layer are then one colour.
pub fn fill_over_system_blur(glass: Rgba, brightness: f32) -> Rgba {
    let (layer, strength) = if brightness <= 1. {
        (0., 1. - brightness)
    } else {
        (1., 0.4)
    };
    let alpha = 1. - (1. - glass.a) * (1. - strength);
    let channel = |value: f32| {
        if alpha <= 0. {
            0.
        } else {
            (glass.a * value + (1. - glass.a) * strength * layer) / alpha
        }
    };
    Rgba {
        r: channel(glass.r),
        g: channel(glass.g),
        b: channel(glass.b),
        a: alpha,
    }
}

/// The fill of a glass panel in its own transparent window where the
/// system cannot blur what is behind the window: with nothing to blur it,
/// the glass lets half as much through, so that what shows behind stays
/// as faint as the blurred glass keeps it.
pub fn fill_without_blur(glass: Rgba) -> Rgba {
    Rgba {
        a: 1. - (1. - glass.a) / 2.,
        ..glass
    }
}

/// A surface that shows what the window drew behind it through a backdrop
/// blur (CSS `backdrop-filter`) and casts its drop shadows only outside its
/// rounded shape, as CSS draws a `box-shadow`. Give it its fill as usual;
/// set its shadows here rather than with `.shadow()`, which GPUI would draw
/// beneath the fill as well, where the blur would take them in.
pub struct GlassPanel {
    panel: Stateful<Div>,
    /// Added as the last child when the panel lays out.
    edges: Option<Div>,
    radius: f32,
    backdrop: Option<BackdropFilter>,
    shadows: Vec<BoxShadow>,
}

impl GlassPanel {
    /// `panel`, rounded to `radius`, with no blur or shadow yet.
    pub fn new(panel: Stateful<Div>, radius: f32) -> Self {
        Self {
            panel: panel.rounded(px(radius)),
            edges: None,
            radius,
            backdrop: None,
            shadows: Vec::new(),
        }
    }

    /// Shows what is behind the surface through `filter`.
    pub fn backdrop_blur(mut self, filter: BackdropFilter) -> Self {
        self.backdrop = Some(filter);
        self
    }

    /// Leaves what is behind the panel unblurred: for a panel alone in a
    /// transparent window, where the window draws nothing behind it to blur.
    pub fn without_backdrop_blur(mut self) -> Self {
        self.backdrop = None;
        self
    }

    /// The surface's drop shadows, drawn outside it only.
    pub fn drop_shadows(mut self, shadows: Vec<BoxShadow>) -> Self {
        self.shadows = shadows;
        self
    }

    /// The panel's own opacity, which its blur and shadows, drawn outside
    /// it, take too.
    fn opacity(&mut self) -> f32 {
        self.panel.style().opacity.unwrap_or(1.)
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

impl IntoElement for GlassPanel {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

/// The panel lays out and paints as its div; before the div paints, the
/// blurred backdrop and the cut-out shadow go beneath it.
impl Element for GlassPanel {
    type RequestLayoutState = <Stateful<Div> as Element>::RequestLayoutState;
    type PrepaintState = <Stateful<Div> as Element>::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        Element::id(&self.panel)
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        Element::source_location(&self.panel)
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        if let Some(edges) = self.edges.take() {
            self.panel.extend([edges.into_any_element()]);
        }
        Element::request_layout(&mut self.panel, id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        Element::prepaint(
            &mut self.panel,
            id,
            inspector_id,
            bounds,
            request_layout,
            window,
            cx,
        )
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let opacity = self.opacity();
        let corners = Corners::all(px(self.radius));
        if let Some(filter) = self.backdrop {
            let filter = BackdropFilter {
                opacity: filter.opacity * opacity,
                ..filter
            };
            window.paint_backdrop_blur(bounds, corners, filter);
        }
        let shadows: Vec<BoxShadow> = self
            .shadows
            .iter()
            .map(|shadow| BoxShadow {
                color: shadow.color.opacity(opacity),
                ..shadow.clone()
            })
            .collect();
        if !shadows.is_empty() {
            window.paint_drop_shadows_outside(bounds, corners, &shadows);
        }
        Element::paint(
            &mut self.panel,
            id,
            inspector_id,
            bounds,
            request_layout,
            prepaint,
            window,
            cx,
        );
    }

    fn a11y_role(&self) -> Option<accesskit::Role> {
        Element::a11y_role(&self.panel)
    }

    fn write_a11y_info(&self, node: &mut accesskit::Node) {
        Element::write_a11y_info(&self.panel, node)
    }

    fn a11y_synthetic_children(
        &mut self,
        prepaint: &mut Self::PrepaintState,
        builder: &mut A11ySubtreeBuilder,
    ) {
        Element::a11y_synthetic_children(&mut self.panel, prepaint, builder)
    }
}

#[cfg(test)]
mod tests {
    use super::{fill_over_system_blur, fill_without_blur, laid_over};

    /// A glass panel's shadow is a drop shadow with the panel cut out of it
    /// (`Shadow::inset == 2`). Each renderer's shader must reach that branch:
    /// tested as "inset is not 0", the branch before it takes mode 2 as an
    /// inset shadow, and no panel has a shadow or an outline outside it.
    #[test]
    fn every_renderer_reaches_the_cut_out_shadow() {
        for (renderer, shader) in [
            (
                "Metal",
                include_str!("../../vendor/gpui-pre-apple/src/shaders.metal"),
            ),
            (
                "wgpu",
                include_str!("../../vendor/gpui-pre-wgpu/src/shaders.wgsl"),
            ),
            (
                "DirectX",
                include_str!("../../vendor/gpui-pre-windows/src/shaders.hlsl"),
            ),
        ] {
            let cut_out = shader
                .find("} else if (shadow.inset == 2u) {")
                .unwrap_or_else(|| panic!("{renderer}: no cut-out branch"));
            let before = &shader[..cut_out];
            let condition = before.rfind("if (shadow.inset").unwrap();
            assert!(
                before[condition..].starts_with("if (shadow.inset == 1u) {"),
                "{renderer}: the branch before the cut-out one takes it too"
            );
        }
    }
    use gpui::{
        BACKDROP_BLUR_MAX_TAPS, BackdropBlur, BackdropBlurKernel, Bounds, ContentMask, Corners,
        DevicePixels, PrimitiveBatch, Quad, Rgba, ScaledPixels, Scene, Size,
        backdrop_blur_downsample, backdrop_blur_reduced_sigma, point, size,
    };

    /// What `fill` over `blurred` shows, one channel.
    fn over(fill: Rgba, value: f32, blurred: f32) -> f32 {
        fill.a * value + (1. - fill.a) * blurred
    }

    #[test]
    fn unblurred_glass_lets_half_as_much_through() {
        let glass = Rgba {
            r: 0.1,
            g: 0.2,
            b: 0.3,
            a: 0.92,
        };
        let fill = fill_without_blur(glass);
        assert!((fill.a - 0.96).abs() < 1e-6);
        assert_eq!((fill.r, fill.g, fill.b), (0.1, 0.2, 0.3));
    }

    #[test]
    fn a_dark_pop_up_shows_what_its_glass_shows_over_the_dimmed_blur() {
        let glass = Rgba {
            r: 0.1,
            g: 0.1,
            b: 0.1,
            a: 0.92,
        };
        let fill = fill_over_system_blur(glass, 0.7);
        for blurred in [0., 0.25, 0.5, 1.] {
            let wanted = 0.92 * 0.1 + 0.08 * 0.7 * blurred;
            assert!((over(fill, fill.r, blurred) - wanted).abs() < 1e-5);
        }
    }

    #[test]
    fn a_light_pop_up_comes_within_its_glass_share_of_the_brightened_blur() {
        let glass = Rgba {
            r: 1.,
            g: 1.,
            b: 1.,
            a: 0.9,
        };
        let fill = fill_over_system_blur(glass, 3.);
        assert!(fill.a > glass.a && fill.a < 1.);
        for step in 0..=20 {
            let blurred = step as f32 / 20.;
            let wanted = 0.9 + 0.1 * (3. * blurred).min(1.);
            assert!((over(fill, fill.r, blurred) - wanted).abs() <= 0.1 * 0.4 + 1e-5);
        }
    }

    fn scaled(x: f32, y: f32, w: f32, h: f32) -> Bounds<ScaledPixels> {
        Bounds {
            origin: point(ScaledPixels(x), ScaledPixels(y)),
            size: size(ScaledPixels(w), ScaledPixels(h)),
        }
    }

    fn blur(bounds: Bounds<ScaledPixels>, clip: Bounds<ScaledPixels>, sigma: f32) -> BackdropBlur {
        BackdropBlur {
            order: 0,
            bounds,
            corner_radii: Corners::all(ScaledPixels(16.)),
            content_mask: ContentMask { bounds: clip },
            blur_radius: ScaledPixels(sigma),
            saturation: 1.,
            brightness: 0.7,
            opacity: 1.,
        }
    }

    fn viewport() -> Size<DevicePixels> {
        size(DevicePixels(2426), DevicePixels(1730))
    }

    #[test]
    fn blurs_shrink_the_backdrop_by_an_even_factor_once_they_are_wide() {
        assert_eq!(backdrop_blur_downsample(0.), 1);
        assert_eq!(backdrop_blur_downsample(5.9), 1);
        assert_eq!(backdrop_blur_downsample(6.), 2);
        // `--glass-blur` at 2x: 13px and 25px.
        assert_eq!(backdrop_blur_downsample(26.), 8);
        assert_eq!(backdrop_blur_downsample(50.), 16);
        for sigma in [7., 13., 26., 32., 50., 100.] {
            assert_eq!(backdrop_blur_downsample(sigma) % 2, 0, "{sigma}");
        }
    }

    #[test]
    fn the_reduced_gaussian_and_the_resampling_add_up_to_the_requested_blur() {
        assert_eq!(backdrop_blur_reduced_sigma(4., 1), 4.);
        for sigma in [6., 10., 24., 26., 32., 50., 100.] {
            let d = backdrop_blur_downsample(sigma);
            let reduced = backdrop_blur_reduced_sigma(sigma, d);
            let variance = (reduced * d as f32).powi(2) + (d * d) as f32 / 4.;
            assert!((variance.sqrt() - sigma).abs() < 1e-3, "{sigma}");
            // About three reduced texels: a short kernel at every size.
            assert!((2.5..6.).contains(&reduced), "{sigma} {reduced}");
        }
    }

    #[test]
    fn kernels_are_normalised_and_fit_their_taps() {
        for sigma in [0., 0.5, 1., 2.9, 3.2, 4.5, 5.99, 8., 40.] {
            let kernel = BackdropBlurKernel::new(sigma);
            let used = kernel.tap_count as usize;
            assert!((1..=BACKDROP_BLUR_MAX_TAPS).contains(&used), "{sigma}");
            let total = kernel.weights[0] + 2. * kernel.weights[1..used].iter().sum::<f32>();
            assert!((total - 1.).abs() < 1e-5, "{sigma} {total}");
            assert_eq!(kernel.offsets[0], 0.);
            for pair in kernel.offsets[..used].windows(2) {
                assert!(pair[1] > pair[0], "{sigma} {:?}", kernel.offsets);
            }
        }
        // Each pair sits between its two texels, nearer the heavier one.
        let kernel = BackdropBlurKernel::new(3.);
        assert!((1. ..=2.).contains(&kernel.offsets[1]));
        assert!(kernel.offsets[1] < 1.5);
        // Cut at three deviations.
        assert_eq!(BackdropBlurKernel::new(3.).reach(), 9);
        assert_eq!(BackdropBlurKernel::new(0.).reach(), 0);
    }

    #[test]
    fn a_plan_reads_around_the_clipped_region_and_stays_on_screen() {
        let menu = blur(
            scaled(900., 40., 532., 1660.),
            scaled(0., 0., 2426., 1730.),
            26.,
        );
        let plan = menu.plan(viewport()).unwrap();
        assert_eq!(plan.downsample, 8);
        assert_eq!(
            plan.target.origin,
            point(DevicePixels(900), DevicePixels(40))
        );
        assert_eq!(
            plan.target.size,
            size(DevicePixels(532), DevicePixels(1660))
        );
        let margin = (plan.kernel.reach() + 1) * 8;
        assert_eq!(plan.source_origin.x.0, 900 - margin);
        // Clamped at the top of the frame.
        assert_eq!(plan.source_origin.y.0, 0);
        let source_right = plan.source_origin.x.0 + plan.reduced_size.width.0 * 8;
        assert!(source_right >= 900 + 532 + margin);
        let source_bottom = plan.source_origin.y.0 + plan.reduced_size.height.0 * 8;
        assert!(source_bottom >= 1730 && source_bottom < 1730 + 8);

        // Fractional bounds round out; the clip trims them.
        let clipped = blur(
            scaled(10.5, 20.25, 100., 100.),
            scaled(0., 50., 2426., 1730.),
            4.,
        );
        let plan = clipped.plan(viewport()).unwrap();
        assert_eq!(plan.downsample, 1);
        assert_eq!(
            plan.target.origin,
            point(DevicePixels(10), DevicePixels(50))
        );
        assert_eq!(plan.target.size, size(DevicePixels(101), DevicePixels(71)));

        let off_screen = blur(
            scaled(3000., 0., 50., 50.),
            scaled(0., 0., 4000., 4000.),
            4.,
        );
        assert_eq!(off_screen.plan(viewport()), None);
    }

    #[test]
    fn a_backdrop_blur_ends_the_batch_beneath_it() {
        let quad = |x: f32| Quad {
            bounds: scaled(x, 0., 100., 100.),
            content_mask: ContentMask {
                bounds: scaled(0., 0., 1000., 1000.),
            },
            ..Default::default()
        };
        let mut scene = Scene::default();
        scene.insert_primitive(quad(0.));
        scene.insert_backdrop_blur(blur(
            scaled(50., 0., 100., 100.),
            scaled(0., 0., 1000., 1000.),
            26.,
        ));
        scene.insert_primitive(quad(60.));
        scene.finish();
        let blur_order = scene.backdrop_blurs[0].order;
        let batches: Vec<_> = scene.batches().collect();
        assert_eq!(batches.len(), 2);
        assert!(matches!(&batches[0], PrimitiveBatch::Quads(range) if range.len() == 1));
        assert!(scene.batch_order(&batches[0]) < blur_order);
        assert!(scene.batch_order(&batches[1]) > blur_order);

        // Without the blur, the two quads are one batch.
        let mut scene = Scene::default();
        scene.insert_primitive(quad(0.));
        scene.insert_primitive(quad(60.));
        scene.finish();
        assert_eq!(scene.batches().count(), 1);
    }

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
