# Patched GPUI crates

The vendored `gpui-pre` crate leaves out the published package's `examples/`
folder (4.9 MB of sample programs and images) and the `[[example]]` entries
of its manifest that name them; nothing else differs from 0.3.12 apart from
the changes described here.

These are the crates.io sources of `gpui-pre`, `gpui-pre-apple`,
`gpui-pre-wgpu` and `gpui-pre-windows` version 0.3.12 (snapshot of
zed-industries/zed at d2a4fb6), Apache-2.0 (each keeps its `LICENSE-APACHE`),
with a backdrop-blur primitive added. `../Cargo.toml` substitutes them for the
published crates with `[patch.crates-io]`; every other GPUI crate still comes
from crates.io. Everything else in them is byte-for-byte the published
package, so the diff at the end of this file is the whole change.

The design follows dux-web's port of backdrop materials to gpui-pre 0.3.4
(github.com/dux-web/gpui, commit b9b7df7, from crabtalk/zed's `bezel-gpui`;
proposed upstream in longbridge/gpui-component#3598): a blur list outside the
primitive batches, ordered by draw order, with the batch iterator ending a
batch at each blur so a renderer can stop, read what it has drawn and go on;
and a drop shadow that cuts its element out. The blur itself is written anew
(dux-web uses Metal Performance Shaders on Metal and leaves out what this app
does not need: lens, dispersion, tint and rim light).

## What changed, and why

`gpui-pre`
- `src/scene.rs`: `BackdropBlur` (rounded rectangle, clip, gaussian standard
  deviation, CSS `saturate()` and `brightness()`, opacity) and
  `Scene::backdrop_blurs`, kept in draw order and replayed with the cached
  paint operations (`PaintOperation::BackdropBlur`). `Scene::insert_backdrop_blur`
  gives a blur the next draw order over its bounds, like a primitive.
  `BatchIterator` ends a batch before the next blur; `Scene::batch_order`
  tells a renderer a batch's order. `BackdropBlur::plan` and
  `BackdropBlurKernel` are the arithmetic both renderers share: the pixels
  the blur covers and reads, the downsample factor (1, or even so that about
  three reduced texels of gaussian remain), and a separable gaussian as
  linear-sampled tap pairs. `Shadow::inset == 2` is documented as a drop
  shadow with its element cut out.
- `src/window.rs`: `BackdropFilter` and `Window::paint_backdrop_blur`, which
  inserts a blur at the element opacity; `Window::paint_drop_shadows_outside`,
  which paints drop shadows with `inset: 2`, so that a translucent panel does
  not show (or blur in) its own shadow, as CSS never draws a `box-shadow`
  under its element.

`gpui-pre-apple` (Metal)
- `src/shaders.metal`: the shadow shader takes `inset == 2` (geometry of a
  drop shadow, coverage times the outside of the element). New blur shaders:
  downsample (box average with bilinear taps), gaussian (one direction,
  clamped to the reduced region), composite (rounded-rectangle coverage,
  clip, saturate, brightness, premultiplied output).
- `src/metal_renderer.rs`: three pipelines, two scratch textures (made on the
  first blur, grown in steps of 128 texels), and the frame loop: before each
  batch, every blur ordered at or before it ends the render pass, runs
  downsample, across, down and composite passes, and starts a load pass. A
  drawable from a framebuffer-only layer cannot be read, so the window's
  layer is made readable the first time a frame has a blur (and stays so);
  a frame whose drawable is still framebuffer-only skips its blurs.

`gpui-pre-wgpu` (Linux; also the web)
- `src/backdrop_blur.rs`, `src/backdrop_blur.wgsl`: the same passes. A
  surface texture can only be drawn to, so a frame with blurs is drawn into a
  texture of this module's and copied to the surface at the end; a frame
  without blurs is drawn as before.
- `src/wgpu_renderer.rs`: holds the blur renderer with the other device
  resources (dropped on device loss) and interleaves blurs in `record_frame`
  as the Metal renderer does. `src/gpui_wgpu.rs` declares the module.
- `src/shaders.wgsl`: the shadow shader takes `inset == 2`.

`gpui-pre-windows` (DirectX)
- `src/shaders.hlsl`: the shadow shader takes `inset == 2`. The DirectX
  renderer ignores `Scene::backdrop_blurs`, so on Windows a glass panel shows
  its translucent fill over an unblurred backdrop.

## Re-applying to a newer gpui-pre

Copy the four new crates from `~/.cargo/registry/src/*/` over these folders,
apply the diff below (`patch -p1` from this folder), and fix what moved. The
Metal header is generated from the `vendor/gpui` copy inside
`gpui-pre-apple`; the blur parameters are a struct of floats declared on both
sides by hand, so no generated type changes. Check: `cargo test` in
`experiments/gpui` (the blur arithmetic and batching tests are in
`src/ui/glass.rs`) and an off-screen picture of a menu over the grid.

## The diff against 0.3.12

```diff
diff --color -ruN gpui-pre-0.3.12/src/scene.rs b/gpui-pre/src/scene.rs
--- a/gpui-pre/src/scene.rs	2006-07-24 03:21:28
+++ b/gpui-pre/src/scene.rs	2026-11-13 20:28:30
@@ -5,8 +5,8 @@
 use serde::{Deserialize, Serialize};
 
 use crate::{
-    AtlasTextureId, AtlasTile, Background, Bounds, ContentMask, Corners, Edges, Hsla, Pixels,
-    Point, Radians, ScaledPixels, Size, bounds_tree::BoundsTree, point,
+    AtlasTextureId, AtlasTile, Background, Bounds, ContentMask, Corners, DevicePixels, Edges,
+    Hsla, Pixels, Point, Radians, ScaledPixels, Size, bounds_tree::BoundsTree, point,
 };
 use std::{
     fmt::Debug,
@@ -50,6 +50,11 @@
     pub subpixel_sprites: Vec<SubpixelSprite>,
     pub polychrome_sprites: Vec<PolychromeSprite>,
     pub surfaces: Vec<PaintSurface>,
+    /// Backdrop blurs, in draw order. They are not primitives of a batch: a
+    /// renderer that supports them ends its pass before the first batch at or
+    /// after each one's order, blurs what it has drawn, and resumes. A
+    /// renderer that does not ignores them.
+    pub backdrop_blurs: Vec<BackdropBlur>,
 }
 
 #[expect(missing_docs)]
@@ -66,6 +71,7 @@
         self.subpixel_sprites.clear();
         self.polychrome_sprites.clear();
         self.surfaces.clear();
+        self.backdrop_blurs.clear();
     }
 
     pub fn len(&self) -> usize {
@@ -138,10 +144,50 @@
             .push(PaintOperation::Primitive(primitive));
     }
 
+    /// Add a backdrop blur at the next draw order over its bounds, so that it
+    /// blurs what was painted beneath it and what is painted over it later
+    /// lands on top.
+    pub fn insert_backdrop_blur(&mut self, mut blur: BackdropBlur) {
+        let clipped_bounds = blur.bounds.intersect(&blur.content_mask.bounds);
+        if clipped_bounds.is_empty() {
+            return;
+        }
+        blur.order = self
+            .layer_stack
+            .last()
+            .copied()
+            .unwrap_or_else(|| self.primitive_bounds.insert(clipped_bounds));
+        self.backdrop_blurs.push(blur);
+        self.paint_operations
+            .push(PaintOperation::BackdropBlur(blur));
+    }
+
+    /// The draw order of a batch's first primitive, which tells a renderer
+    /// which backdrop blurs come before the batch.
+    pub fn batch_order(&self, batch: &PrimitiveBatch) -> DrawOrder {
+        match batch {
+            PrimitiveBatch::Shadows(range) => self.shadows[range.start].order,
+            PrimitiveBatch::Quads(range) => self.quads[range.start].order,
+            PrimitiveBatch::Paths(range) => self.paths[range.start].order,
+            PrimitiveBatch::Underlines(range) => self.underlines[range.start].order,
+            PrimitiveBatch::MonochromeSprites { range, .. } => {
+                self.monochrome_sprites[range.start].order
+            }
+            PrimitiveBatch::SubpixelSprites { range, .. } => {
+                self.subpixel_sprites[range.start].order
+            }
+            PrimitiveBatch::PolychromeSprites { range, .. } => {
+                self.polychrome_sprites[range.start].order
+            }
+            PrimitiveBatch::Surfaces(range) => self.surfaces[range.start].order,
+        }
+    }
+
     pub fn replay(&mut self, range: Range<usize>, prev_scene: &Scene) {
         for operation in &prev_scene.paint_operations[range] {
             match operation {
                 PaintOperation::Primitive(primitive) => self.insert_primitive(primitive.clone()),
+                PaintOperation::BackdropBlur(blur) => self.insert_backdrop_blur(*blur),
                 PaintOperation::StartLayer(bounds) => self.push_layer(*bounds),
                 PaintOperation::EndLayer => self.pop_layer(),
             }
@@ -160,6 +206,7 @@
         self.polychrome_sprites
             .sort_by_key(|sprite| (sprite.order, sprite.tile.texture_id.index));
         self.surfaces.sort_by_key(|surface| surface.order);
+        self.backdrop_blurs.sort_by_key(|blur| blur.order);
     }
 
     #[cfg_attr(
@@ -171,6 +218,7 @@
     )]
     pub fn batches(&self) -> impl Iterator<Item = PrimitiveBatch> + '_ {
         BatchIterator {
+            backdrop_blurs_iter: self.backdrop_blurs.iter().peekable(),
             shadows_start: 0,
             shadows_iter: self.shadows.iter().peekable(),
             quads_start: 0,
@@ -213,6 +261,7 @@
 
 pub(crate) enum PaintOperation {
     Primitive(Primitive),
+    BackdropBlur(BackdropBlur),
     StartLayer(Bounds<ScaledPixels>),
     EndLayer,
 }
@@ -267,6 +316,7 @@
     allow(dead_code)
 )]
 struct BatchIterator<'a> {
+    backdrop_blurs_iter: Peekable<slice::Iter<'a, BackdropBlur>>,
     shadows_start: usize,
     shadows_iter: Peekable<slice::Iter<'a, Shadow>>,
     quads_start: usize,
@@ -321,12 +371,23 @@
 
         let first = orders_and_kinds[0];
         let second = orders_and_kinds[1];
-        let (batch_kind, max_order_and_kind) = if first.0.is_some() {
+        let (batch_kind, mut max_order_and_kind) = if first.0.is_some() {
             (first.1, (second.0.unwrap_or(u32::MAX), second.1))
         } else {
             return None;
         };
 
+        // A batch ends before the next backdrop blur, which is drawn between
+        // it and the batch after.
+        while self
+            .backdrop_blurs_iter
+            .next_if(|blur| Some(blur.order) <= first.0)
+            .is_some()
+        {}
+        if let Some(blur) = self.backdrop_blurs_iter.peek() {
+            max_order_and_kind = max_order_and_kind.min((blur.order, PrimitiveKind::Shadow));
+        }
+
         match batch_kind {
             PrimitiveKind::Shadow => {
                 let shadows_start = self.shadows_start;
@@ -580,7 +641,8 @@
     pub color: Hsla,
     pub element_bounds: Bounds<ScaledPixels>,
     pub element_corner_radii: Corners<ScaledPixels>,
-    /// 0 = drop shadow (rendered outside the element), 1 = inset shadow (rendered inside).
+    /// 0 = drop shadow, 1 = inset shadow (rendered inside the element), 2 = drop
+    /// shadow with the element's own shape cut out of it, as CSS draws one.
     pub inset: u32,
     pub pad: u32, // align to 8 bytes
 }
@@ -589,6 +651,173 @@
     fn from(shadow: Shadow) -> Self {
         Primitive::Shadow(shadow)
     }
+}
+
+/// A region whose backdrop, everything drawn beneath it so far, is shown
+/// blurred, as CSS `backdrop-filter: blur() saturate() brightness()` does.
+/// See [`crate::Window::paint_backdrop_blur`].
+#[derive(Debug, Copy, Clone, PartialEq)]
+pub struct BackdropBlur {
+    /// The draw order the region is drawn at.
+    pub order: DrawOrder,
+    /// The rounded rectangle that shows the blurred backdrop.
+    pub bounds: Bounds<ScaledPixels>,
+    /// The corner radii of `bounds`.
+    pub corner_radii: Corners<ScaledPixels>,
+    /// The clip the region is drawn under.
+    pub content_mask: ContentMask<ScaledPixels>,
+    /// The standard deviation of the gaussian, in device pixels.
+    pub blur_radius: ScaledPixels,
+    /// CSS `saturate()`: 1 leaves the colours as they are.
+    pub saturation: f32,
+    /// CSS `brightness()`: 1 leaves the colours as they are.
+    pub brightness: f32,
+    /// How much of the blurred backdrop replaces the sharp one, for a panel
+    /// that fades.
+    pub opacity: f32,
+}
+
+impl BackdropBlur {
+    /// How a renderer draws this blur into a frame of `viewport` device
+    /// pixels, or `None` when none of it is on screen.
+    pub fn plan(&self, viewport: Size<DevicePixels>) -> Option<BackdropBlurPlan> {
+        let clipped = self.bounds.intersect(&self.content_mask.bounds);
+        let (width, height) = (viewport.width.0.max(0), viewport.height.0.max(0));
+        let x0 = (clipped.origin.x.0.floor() as i32).clamp(0, width);
+        let y0 = (clipped.origin.y.0.floor() as i32).clamp(0, height);
+        let x1 = ((clipped.origin.x.0 + clipped.size.width.0).ceil() as i32).clamp(0, width);
+        let y1 = ((clipped.origin.y.0 + clipped.size.height.0).ceil() as i32).clamp(0, height);
+        if x1 <= x0 || y1 <= y0 {
+            return None;
+        }
+
+        let sigma = self.blur_radius.0.max(0.);
+        let downsample = backdrop_blur_downsample(sigma);
+        let kernel = BackdropBlurKernel::new(backdrop_blur_reduced_sigma(sigma, downsample));
+        // What the blur reads around the region: the kernel's reach, in full
+        // pixels, plus the box each reduced texel averages.
+        let margin = (kernel.reach() + 1) * downsample as i32;
+        let source_x0 = (x0 - margin).max(0);
+        let source_y0 = (y0 - margin).max(0);
+        let source_x1 = (x1 + margin).min(width);
+        let source_y1 = (y1 + margin).min(height);
+        let d = downsample as i32;
+        Some(BackdropBlurPlan {
+            target: Bounds {
+                origin: point(DevicePixels(x0), DevicePixels(y0)),
+                size: Size {
+                    width: DevicePixels(x1 - x0),
+                    height: DevicePixels(y1 - y0),
+                },
+            },
+            source_origin: point(DevicePixels(source_x0), DevicePixels(source_y0)),
+            downsample,
+            reduced_size: Size {
+                width: DevicePixels((source_x1 - source_x0 + d - 1) / d),
+                height: DevicePixels((source_y1 - source_y0 + d - 1) / d),
+            },
+            kernel,
+        })
+    }
+}
+
+/// The most taps [`BackdropBlurKernel`] has.
+pub const BACKDROP_BLUR_MAX_TAPS: usize = 12;
+
+/// How much a backdrop blur of standard deviation `sigma` (device pixels)
+/// shrinks the backdrop before blurring it: 1, or an even factor that leaves
+/// a gaussian of about three reduced texels.
+pub fn backdrop_blur_downsample(sigma: f32) -> u32 {
+    if sigma < 6. {
+        1
+    } else {
+        2 * (sigma / 6.).floor() as u32
+    }
+}
+
+/// The standard deviation, in reduced texels, of the gaussian that leaves a
+/// blur of `sigma` device pixels once the box that shrinks the backdrop by
+/// `downsample` and the bilinear filter that enlarges it again have added
+/// theirs (variances of about d²/12 and d²/6).
+pub fn backdrop_blur_reduced_sigma(sigma: f32, downsample: u32) -> f32 {
+    if downsample <= 1 {
+        return sigma;
+    }
+    let d = downsample as f32;
+    (sigma * sigma - d * d / 4.).max(0.).sqrt() / d
+}
+
+/// A separable gaussian as a centre tap and pairs of taps either side of
+/// it, each pair placed between two texels so that one bilinear sample
+/// reads both at their weights.
+#[derive(Debug, Clone, Copy, PartialEq)]
+pub struct BackdropBlurKernel {
+    /// How many of the taps are used; the first is the centre.
+    pub tap_count: u32,
+    /// Each tap's distance from the centre, in texels; the others are read
+    /// on both sides.
+    pub offsets: [f32; BACKDROP_BLUR_MAX_TAPS],
+    /// Each tap's weight, given to each side.
+    pub weights: [f32; BACKDROP_BLUR_MAX_TAPS],
+}
+
+impl BackdropBlurKernel {
+    /// The kernel for a gaussian of standard deviation `sigma` texels, cut
+    /// at three deviations.
+    pub fn new(sigma: f32) -> Self {
+        let mut kernel = Self {
+            tap_count: 1,
+            offsets: [0.; BACKDROP_BLUR_MAX_TAPS],
+            weights: [0.; BACKDROP_BLUR_MAX_TAPS],
+        };
+        kernel.weights[0] = 1.;
+        if !(sigma > 0.01) {
+            return kernel;
+        }
+        let max_radius = 2 * (BACKDROP_BLUR_MAX_TAPS - 1);
+        let radius = ((3. * sigma).ceil() as usize).clamp(1, max_radius);
+        let texel = |i: usize| {
+            if i > radius {
+                0.
+            } else {
+                (-((i * i) as f32) / (2. * sigma * sigma)).exp()
+            }
+        };
+        let total = texel(0) + 2. * (1..=radius).map(texel).sum::<f32>();
+        kernel.weights[0] = texel(0) / total;
+        let mut i = 1;
+        while i <= radius {
+            let (near, far) = (texel(i) / total, texel(i + 1) / total);
+            let index = kernel.tap_count as usize;
+            kernel.weights[index] = near + far;
+            kernel.offsets[index] = (i as f32 * near + (i + 1) as f32 * far) / (near + far);
+            kernel.tap_count += 1;
+            i += 2;
+        }
+        kernel
+    }
+
+    /// How many texels from the centre the furthest tap reads.
+    pub fn reach(&self) -> i32 {
+        self.offsets[self.tap_count as usize - 1].ceil() as i32
+    }
+}
+
+/// Where a renderer reads and writes for one [`BackdropBlur`], in device
+/// pixels of the frame.
+#[derive(Debug, Clone, Copy, PartialEq)]
+pub struct BackdropBlurPlan {
+    /// The pixels the blurred backdrop is drawn over: the region, clipped,
+    /// rounded out to whole pixels.
+    pub target: Bounds<DevicePixels>,
+    /// The frame pixel at the corner of the first reduced texel.
+    pub source_origin: Point<DevicePixels>,
+    /// How many frame pixels each reduced texel spans in each direction.
+    pub downsample: u32,
+    /// The reduced backdrop's size in texels.
+    pub reduced_size: Size<DevicePixels>,
+    /// The gaussian run over the reduced backdrop, once across and once down.
+    pub kernel: BackdropBlurKernel,
 }
 
 /// The style of a border.
diff --color -ruN gpui-pre-0.3.12/src/window.rs b/gpui-pre/src/window.rs
--- a/gpui-pre/src/window.rs	2006-07-24 03:21:28
+++ b/gpui-pre/src/window.rs	2026-11-13 20:28:46
@@ -8,7 +8,7 @@
 use crate::taffy::TaffyLayoutEngine;
 use crate::{
     Action, AnyDrag, AnyElement, AnyImageCache, AnyTooltip, AnyView, App, AppContext, Arena, Asset,
-    AsyncWindowContext, AtlasTile, AvailableSpace, Background, BorderStyle, Bounds, BoxShadow,
+    AsyncWindowContext, AtlasTile, AvailableSpace, BackdropBlur, Background, BorderStyle, Bounds, BoxShadow,
     Capslock, Context, Corners, CursorHideMode, CursorStyle, Decorations, DevicePixels,
     DispatchActionListener, DispatchNodeId, DispatchTree, DisplayId, Edges, Effect, Entity,
     EntityId, EventEmitter, FileDropEvent, FontId, Global, GlobalElementId, GlyphId, GpuSpecs,
@@ -1634,6 +1634,33 @@
     window_bounds_ctor(Bounds::new(final_origin, base_size))
 }
 
+/// What [`Window::paint_backdrop_blur`] does to the backdrop: CSS
+/// `backdrop-filter: blur(blur_radius) saturate(saturation)
+/// brightness(brightness)`.
+#[derive(Debug, Clone, Copy, PartialEq)]
+pub struct BackdropFilter {
+    /// The standard deviation of the gaussian, as CSS `blur()` takes it.
+    pub blur_radius: Pixels,
+    /// CSS `saturate()`: 1 leaves the colours as they are.
+    pub saturation: f32,
+    /// CSS `brightness()`: 1 leaves the colours as they are.
+    pub brightness: f32,
+    /// How much of the filtered backdrop shows, on top of the element opacity.
+    pub opacity: f32,
+}
+
+impl BackdropFilter {
+    /// A gaussian blur of `radius` and nothing else.
+    pub fn blur(radius: Pixels) -> Self {
+        Self {
+            blur_radius: radius,
+            saturation: 1.,
+            brightness: 1.,
+            opacity: 1.,
+        }
+    }
+}
+
 impl Window {
     pub(crate) fn new(
         handle: AnyWindowHandle,
@@ -4811,6 +4838,31 @@
         corner_radii: Corners<Pixels>,
         shadows: &[BoxShadow],
     ) {
+        self.paint_drop_shadows_cut(bounds, corner_radii, shadows, false);
+    }
+
+    /// Paint the drop shadows from `shadows` with the element's rounded shape
+    /// cut out of them, as CSS draws a `box-shadow`: nothing of them shows
+    /// through a translucent element. Paint them after a backdrop blur, which
+    /// would otherwise blur them in.
+    ///
+    /// This method should only be called as part of the paint phase of element drawing.
+    pub fn paint_drop_shadows_outside(
+        &mut self,
+        bounds: Bounds<Pixels>,
+        corner_radii: Corners<Pixels>,
+        shadows: &[BoxShadow],
+    ) {
+        self.paint_drop_shadows_cut(bounds, corner_radii, shadows, true);
+    }
+
+    fn paint_drop_shadows_cut(
+        &mut self,
+        bounds: Bounds<Pixels>,
+        corner_radii: Corners<Pixels>,
+        shadows: &[BoxShadow],
+        outside: bool,
+    ) {
         self.invalidator.debug_assert_paint();
 
         let scale_factor = self.scale_factor();
@@ -4832,10 +4884,50 @@
                 color: shadow.color.opacity(opacity),
                 element_bounds,
                 element_corner_radii,
-                inset: 0,
+                inset: if outside { 2 } else { 0 },
                 pad: 0,
             });
         }
+    }
+
+    /// Show what has been painted beneath `bounds` so far blurred inside its
+    /// rounded shape, as CSS `backdrop-filter` does; paint the element's
+    /// fill and contents after it. The blur follows the element opacity, so
+    /// a fading panel fades its blur with it. A renderer that cannot blur
+    /// leaves the backdrop as it is.
+    ///
+    /// This method should only be called as part of the paint phase of element drawing.
+    pub fn paint_backdrop_blur(
+        &mut self,
+        bounds: Bounds<Pixels>,
+        corner_radii: Corners<Pixels>,
+        filter: BackdropFilter,
+    ) {
+        self.invalidator.debug_assert_paint();
+        let opacity = self.element_opacity() * filter.opacity;
+        if opacity <= 0. {
+            return;
+        }
+        let scale_factor = self.scale_factor();
+        let content_mask = self.snapped_content_mask();
+        // A radius past half the shorter side has no shape to round.
+        let limit = bounds.size.width.min(bounds.size.height) / 2.;
+        let corner_radii = Corners {
+            top_left: corner_radii.top_left.min(limit),
+            top_right: corner_radii.top_right.min(limit),
+            bottom_right: corner_radii.bottom_right.min(limit),
+            bottom_left: corner_radii.bottom_left.min(limit),
+        };
+        self.next_frame.scene.insert_backdrop_blur(BackdropBlur {
+            order: 0,
+            bounds: self.cover_bounds(bounds),
+            corner_radii: corner_radii.scale(scale_factor),
+            content_mask,
+            blur_radius: filter.blur_radius.scale(scale_factor),
+            saturation: filter.saturation,
+            brightness: filter.brightness,
+            opacity: opacity.min(1.),
+        });
     }
 
     /// Paint the inset shadows from `shadows` into the scene at the current z-index. Should
diff --color -ruN gpui-pre-apple-0.3.12/src/metal_renderer.rs b/gpui-pre-apple/src/metal_renderer.rs
--- a/gpui-pre-apple/src/metal_renderer.rs	2006-07-24 03:21:28
+++ b/gpui-pre-apple/src/metal_renderer.rs	2026-11-13 20:30:54
@@ -2,8 +2,9 @@
 use anyhow::{Context as _, Result};
 use block2::RcBlock;
 use gpui::{
-    AtlasTextureId, Background, Bounds, ContentMask, DevicePixels, PaintSurface, Path, Point,
-    PrimitiveBatch, ScaledPixels, Scene, Size, point, size,
+    AtlasTextureId, BACKDROP_BLUR_MAX_TAPS, BackdropBlur, BackdropBlurPlan, Background, Bounds,
+    ContentMask, DevicePixels, PaintSurface, Path, Point, PrimitiveBatch, ScaledPixels, Scene,
+    Size, point, size,
 };
 #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
 use image::RgbaImage;
@@ -143,6 +144,12 @@
     monochrome_sprites_pipeline_state: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
     polychrome_sprites_pipeline_state: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
     surfaces_pipeline_state: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
+    backdrop_blur_downsample_pipeline_state: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
+    backdrop_blur_gaussian_pipeline_state: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
+    backdrop_blur_composite_pipeline_state: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
+    /// The two scratch textures a backdrop blur reduces and blurs the frame
+    /// in, made on the first blur and grown when one needs more room.
+    backdrop_blur_textures: Option<[Retained<ProtocolObject<dyn MTLTexture>>; 2]>,
     unit_vertices: Retained<ProtocolObject<dyn MTLBuffer>>,
     instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>,
     sprite_atlas: Arc<MetalAtlas>,
@@ -394,6 +401,30 @@
             MTLPixelFormat::BGRA8Unorm,
         )?;
 
+        let backdrop_blur_downsample_pipeline_state = build_replacing_pipeline_state(
+            &device,
+            &library,
+            "backdrop_blur_downsample",
+            "backdrop_blur_pass_vertex",
+            "backdrop_blur_downsample_fragment",
+        )?;
+        let backdrop_blur_gaussian_pipeline_state = build_replacing_pipeline_state(
+            &device,
+            &library,
+            "backdrop_blur_gaussian",
+            "backdrop_blur_pass_vertex",
+            "backdrop_blur_gaussian_fragment",
+        )?;
+        // Premultiplied, like the path sprites.
+        let backdrop_blur_composite_pipeline_state = build_path_sprite_pipeline_state(
+            &device,
+            &library,
+            "backdrop_blur_composite",
+            "backdrop_blur_composite_vertex",
+            "backdrop_blur_composite_fragment",
+            MTLPixelFormat::BGRA8Unorm,
+        )?;
+
         let command_queue = device.newCommandQueue().context("creating command queue")?;
         let supports_shared_storage = cfg!(target_os = "ios") || is_apple_gpu;
         #[expect(
@@ -431,6 +462,10 @@
             monochrome_sprites_pipeline_state,
             polychrome_sprites_pipeline_state,
             surfaces_pipeline_state,
+            backdrop_blur_downsample_pipeline_state,
+            backdrop_blur_gaussian_pipeline_state,
+            backdrop_blur_composite_pipeline_state,
+            backdrop_blur_textures: None,
             unit_vertices,
             instance_buffer_pool,
             sprite_atlas,
@@ -601,6 +636,15 @@
                 return;
             }
         };
+        // A backdrop blur reads the drawable, which a framebuffer-only layer
+        // does not allow. Windows that never blur keep the faster drawables;
+        // one that has blurred keeps readable ones from then on, so that its
+        // drawables are not remade each time a panel opens. The frame in
+        // flight may still have a framebuffer-only drawable; it draws its
+        // blurs' regions unblurred.
+        if !scene.backdrop_blurs.is_empty() && layer.framebufferOnly() {
+            layer.setFramebufferOnly(false);
+        }
         let drawable = match &mut self.display_link_mode {
             DisplayLinkMode::Attached { drawable } => {
                 let Some(drawable) = drawable.take() else {
@@ -868,7 +912,39 @@
             }),
         )?;
 
-        for batch in scene.batches() {
+        let can_blur = !texture.isFramebufferOnly();
+        let mut backdrop_blurs = scene.backdrop_blurs.iter().peekable();
+        let mut batches = scene.batches().peekable();
+        loop {
+            // The blurs ordered before the next batch (all that are left
+            // after the last) read what has been drawn so far.
+            let next_order = batches.peek().map(|batch| scene.batch_order(batch));
+            while let Some(blur) =
+                backdrop_blurs.next_if(|blur| next_order.is_none_or(|order| blur.order <= order))
+            {
+                if !can_blur {
+                    continue;
+                }
+                let Some(plan) = blur.plan(viewport_size) else {
+                    continue;
+                };
+                command_encoder.endEncoding();
+                let result =
+                    self.draw_backdrop_blur(blur, &plan, &command_buffer, texture, viewport_size);
+                command_encoder = new_command_encoder_for_texture(
+                    &command_buffer,
+                    texture,
+                    viewport_size,
+                    None,
+                )?;
+                if let Err(error) = result {
+                    command_encoder.endEncoding();
+                    return Err(error);
+                }
+            }
+            let Some(batch) = batches.next() else {
+                break;
+            };
             match batch {
                 PrimitiveBatch::Shadows(range) => {
                     self.draw_shadows(range, instance_bindings, viewport_size, &command_encoder)
@@ -947,6 +1023,128 @@
         Ok(command_buffer)
     }
 
+    /// Blur the frame drawn so far around `blur` and draw it back inside the
+    /// region, in passes of their own between the frame's.
+    fn draw_backdrop_blur(
+        &mut self,
+        blur: &BackdropBlur,
+        plan: &BackdropBlurPlan,
+        command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
+        frame: &ProtocolObject<dyn MTLTexture>,
+        viewport_size: Size<DevicePixels>,
+    ) -> Result<()> {
+        let reduced_width = plan.reduced_size.width.0.max(1) as usize;
+        let reduced_height = plan.reduced_size.height.0.max(1) as usize;
+        let [reduced, blurred_across] = self.backdrop_blur_textures(reduced_width, reduced_height)?;
+        let mut params = BackdropBlurParams::new(blur, plan, viewport_size, &reduced);
+
+        let reduced_viewport = MTLViewport {
+            originX: 0.,
+            originY: 0.,
+            width: reduced_width as f64,
+            height: reduced_height as f64,
+            znear: 0.,
+            zfar: 1.,
+        };
+        // Shrink the frame into `reduced`, blur it across into
+        // `blurred_across`, then down back into `reduced`.
+        let passes = [
+            (&self.backdrop_blur_downsample_pipeline_state, frame, &reduced, [0., 0.]),
+            (&self.backdrop_blur_gaussian_pipeline_state, &*reduced, &blurred_across, [1., 0.]),
+            (&self.backdrop_blur_gaussian_pipeline_state, &*blurred_across, &reduced, [0., 1.]),
+        ];
+        for (pipeline_state, source, target, direction) in passes {
+            [params.direction_x, params.direction_y] = direction;
+            let descriptor = MTLRenderPassDescriptor::new();
+            // SAFETY: Index zero is within the render pass's color attachment array.
+            let color_attachment =
+                unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) };
+            color_attachment.setTexture(Some(target));
+            color_attachment.setLoadAction(MTLLoadAction::DontCare);
+            color_attachment.setStoreAction(MTLStoreAction::Store);
+            let encoder = command_buffer
+                .renderCommandEncoderWithDescriptor(&descriptor)
+                .context("creating backdrop blur command encoder")?;
+            encoder.setViewport(reduced_viewport);
+            encoder.setRenderPipelineState(pipeline_state);
+            // SAFETY: The slots match the backdrop blur shaders; the parameters are copied from a
+            // live value of the layout the shaders declare, and Metal retains both textures. Each
+            // pass reads a texture the previous one wrote, which Metal's hazard tracking orders.
+            unsafe {
+                encoder.setVertexBuffer_offset_atIndex(Some(&self.unit_vertices), 0, 0);
+                encoder.setFragmentBytes_length_atIndex(
+                    ptr::NonNull::from(&params).cast(),
+                    mem::size_of_val(&params),
+                    1,
+                );
+                encoder.setFragmentTexture_atIndex(Some(source), 0);
+                encoder.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 6);
+            }
+            encoder.endEncoding();
+        }
+
+        let encoder = new_command_encoder_for_texture(command_buffer, frame, viewport_size, None)?;
+        encoder.setRenderPipelineState(&self.backdrop_blur_composite_pipeline_state);
+        // SAFETY: As above; the composite reads the texture the last pass wrote.
+        unsafe {
+            encoder.setVertexBuffer_offset_atIndex(Some(&self.unit_vertices), 0, 0);
+            encoder.setVertexBytes_length_atIndex(
+                ptr::NonNull::from(&params).cast(),
+                mem::size_of_val(&params),
+                1,
+            );
+            encoder.setFragmentBytes_length_atIndex(
+                ptr::NonNull::from(&params).cast(),
+                mem::size_of_val(&params),
+                1,
+            );
+            encoder.setFragmentTexture_atIndex(Some(&reduced), 0);
+            encoder.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 6);
+        }
+        encoder.endEncoding();
+        Ok(())
+    }
+
+    /// The backdrop blur's scratch textures, at least `width` by `height`.
+    fn backdrop_blur_textures(
+        &mut self,
+        width: usize,
+        height: usize,
+    ) -> Result<[Retained<ProtocolObject<dyn MTLTexture>>; 2]> {
+        if let Some(textures) = &self.backdrop_blur_textures
+            && textures[0].width() >= width
+            && textures[0].height() >= height
+        {
+            return Ok(textures.clone());
+        }
+        // Grow in steps, so a panel that grows does not remake them each frame.
+        let round_up = |length: usize| length.div_ceil(128) * 128;
+        let (width, height) = match &self.backdrop_blur_textures {
+            Some(textures) => (
+                round_up(width.max(textures[0].width())),
+                round_up(height.max(textures[0].height())),
+            ),
+            None => (round_up(width), round_up(height)),
+        };
+        let descriptor = MTLTextureDescriptor::new();
+        // SAFETY: The dimensions are positive and this descriptor creates a 2D texture.
+        unsafe {
+            descriptor.setWidth(width);
+            descriptor.setHeight(height);
+        }
+        descriptor.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
+        descriptor.setStorageMode(MTLStorageMode::Private);
+        descriptor.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
+        let new_texture = || {
+            self.device
+                .newTextureWithDescriptor(&descriptor)
+                .context("allocating backdrop blur texture")
+        };
+        let textures = [new_texture()?, new_texture()?];
+        self.backdrop_blur_textures = Some(textures.clone());
+        Ok(textures)
+    }
+
     fn draw_paths_to_intermediate(
         &self,
         paths: &[Path<ScaledPixels>],
@@ -1583,6 +1781,118 @@
     device
         .newRenderPipelineStateWithDescriptor_error(&descriptor)
         .with_context(|| format!("creating {label} render pipeline"))
+}
+
+/// A pipeline that writes its output as it is, without blending.
+fn build_replacing_pipeline_state(
+    device: &ProtocolObject<dyn MTLDevice>,
+    library: &ProtocolObject<dyn MTLLibrary>,
+    label: &str,
+    vertex_fn_name: &str,
+    fragment_fn_name: &str,
+) -> Result<Retained<ProtocolObject<dyn MTLRenderPipelineState>>> {
+    let vertex_fn = library
+        .newFunctionWithName(&NSString::from_str(vertex_fn_name))
+        .with_context(|| format!("locating vertex function {vertex_fn_name}"))?;
+    let fragment_fn = library
+        .newFunctionWithName(&NSString::from_str(fragment_fn_name))
+        .with_context(|| format!("locating fragment function {fragment_fn_name}"))?;
+
+    let descriptor = MTLRenderPipelineDescriptor::new();
+    descriptor.setLabel(Some(&NSString::from_str(label)));
+    descriptor.setVertexFunction(Some(vertex_fn.as_ref()));
+    descriptor.setFragmentFunction(Some(fragment_fn.as_ref()));
+    // SAFETY: Index zero is within the pipeline's color attachment array.
+    let color_attachment = unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) };
+    color_attachment.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
+    color_attachment.setBlendingEnabled(false);
+
+    device
+        .newRenderPipelineStateWithDescriptor_error(&descriptor)
+        .with_context(|| format!("creating {label} render pipeline"))
+}
+
+/// What the backdrop blur shaders read; mirrors `BackdropBlurParams` in
+/// shaders.metal. Floats only, so both sides lay it out the same.
+#[repr(C)]
+#[derive(Clone, Copy)]
+struct BackdropBlurParams {
+    target: [f32; 4],
+    source: [f32; 2],
+    downsample: f32,
+    reduced_size: [f32; 2],
+    texture_size: [f32; 2],
+    viewport_size: [f32; 2],
+    direction_x: f32,
+    direction_y: f32,
+    tap_count: f32,
+    tap_offsets: [f32; BACKDROP_BLUR_MAX_TAPS],
+    tap_weights: [f32; BACKDROP_BLUR_MAX_TAPS],
+    bounds: [f32; 4],
+    corner_radii: [f32; 4],
+    clip: [f32; 4],
+    saturation: f32,
+    brightness: f32,
+    opacity: f32,
+}
+
+impl BackdropBlurParams {
+    fn new(
+        blur: &BackdropBlur,
+        plan: &BackdropBlurPlan,
+        viewport_size: Size<DevicePixels>,
+        scratch: &ProtocolObject<dyn MTLTexture>,
+    ) -> Self {
+        let clip = blur.content_mask.bounds;
+        Self {
+            target: [
+                plan.target.origin.x.0 as f32,
+                plan.target.origin.y.0 as f32,
+                plan.target.size.width.0 as f32,
+                plan.target.size.height.0 as f32,
+            ],
+            source: [
+                plan.source_origin.x.0 as f32,
+                plan.source_origin.y.0 as f32,
+            ],
+            downsample: plan.downsample as f32,
+            reduced_size: [
+                plan.reduced_size.width.0 as f32,
+                plan.reduced_size.height.0 as f32,
+            ],
+            texture_size: [scratch.width() as f32, scratch.height() as f32],
+            viewport_size: [
+                viewport_size.width.0 as f32,
+                viewport_size.height.0 as f32,
+            ],
+            direction_x: 0.,
+            direction_y: 0.,
+            tap_count: plan.kernel.tap_count as f32,
+            tap_offsets: plan.kernel.offsets,
+            tap_weights: plan.kernel.weights,
+            bounds: [
+                blur.bounds.origin.x.0,
+                blur.bounds.origin.y.0,
+                blur.bounds.size.width.0,
+                blur.bounds.size.height.0,
+            ],
+            corner_radii: [
+                blur.corner_radii.top_left.0,
+                blur.corner_radii.top_right.0,
+                blur.corner_radii.bottom_right.0,
+                blur.corner_radii.bottom_left.0,
+            ],
+            clip: [
+                clip.origin.x.0,
+                clip.origin.y.0,
+                clip.size.width.0,
+                clip.size.height.0,
+            ],
+            saturation: blur.saturation,
+            brightness: blur.brightness,
+            opacity: blur.opacity,
+        }
+    }
 }
 
 fn build_path_sprite_pipeline_state(
diff --color -ruN gpui-pre-apple-0.3.12/src/shaders.metal b/gpui-pre-apple/src/shaders.metal
--- a/gpui-pre-apple/src/shaders.metal	2006-07-24 03:21:28
+++ b/gpui-pre-apple/src/shaders.metal	2026-11-13 20:29:53
@@ -469,7 +469,7 @@
   Shadow shadow = shadows[shadow_id];
 
   Bounds_ScaledPixels bounds;
-  if (shadow.inset != 0u) {
+  if (shadow.inset == 1u) {
     bounds = shadow.element_bounds;
   } else {
     // Leave room for the gaussian tail outside the shadow rect.
@@ -549,6 +549,11 @@
     float element_distance = quad_sdf(input.position.xy, shadow.element_bounds,
                                       shadow.element_corner_radii);
     alpha *= saturate(0.5 - element_distance);
+  } else if (shadow.inset == 2u) {
+    // A drop shadow with the element's shape cut out of it.
+    float element_distance = quad_sdf(input.position.xy, shadow.element_bounds,
+                                      shadow.element_corner_radii);
+    alpha *= saturate(0.5 + element_distance);
   }
 
   return input.color * float4(1., 1., 1., alpha);
@@ -1276,4 +1281,153 @@
   }
 
   return color;
+}
+
+// --- backdrop blur --- //
+//
+// A backdrop blur reads the frame drawn so far: it shrinks the region around
+// it into a scratch texture (averaging boxes of `downsample` pixels), runs a
+// separable gaussian across and down that, then draws the result, saturated
+// and brightened, over the frame inside its rounded rectangle.
+
+#define BACKDROP_BLUR_MAX_TAPS 12
+
+// Mirrors `BackdropBlurParams` in metal_renderer.rs; floats only, so the two
+// layouts agree without padding.
+struct BackdropBlurParams {
+  float target_x, target_y, target_width, target_height;
+  float source_x, source_y;
+  float downsample;
+  float reduced_width, reduced_height;
+  float texture_width, texture_height;
+  float viewport_width, viewport_height;
+  float direction_x, direction_y;
+  float tap_count;
+  float tap_offsets[BACKDROP_BLUR_MAX_TAPS];
+  float tap_weights[BACKDROP_BLUR_MAX_TAPS];
+  float bounds_x, bounds_y, bounds_width, bounds_height;
+  float radius_top_left, radius_top_right, radius_bottom_right, radius_bottom_left;
+  float clip_x, clip_y, clip_width, clip_height;
+  float saturation, brightness, opacity;
+};
+
+struct BackdropBlurVertexOutput {
+  float4 position [[position]];
+};
+
+// Covers the whole render target; the viewport limits it to the reduced region.
+vertex BackdropBlurVertexOutput backdrop_blur_pass_vertex(
+    uint unit_vertex_id [[vertex_id]],
+    constant float2 *unit_vertices [[buffer(0)]]) {
+  float2 unit_vertex = unit_vertices[unit_vertex_id];
+  return BackdropBlurVertexOutput{
+      float4(unit_vertex * float2(2., -2.) + float2(-1., 1.), 0., 1.)};
+}
+
+// One reduced texel: the average of its box of frame pixels, read two by two
+// with bilinear samples between them.
+fragment float4 backdrop_blur_downsample_fragment(
+    BackdropBlurVertexOutput input [[stage_in]],
+    constant BackdropBlurParams *params [[buffer(1)]],
+    texture2d<float> frame [[texture(0)]]) {
+  constexpr sampler frame_sampler(mag_filter::linear, min_filter::linear,
+                                  address::clamp_to_edge);
+  float2 viewport = float2(params->viewport_width, params->viewport_height);
+  float downsample = params->downsample;
+  float2 box_origin = float2(params->source_x, params->source_y) +
+                      floor(input.position.xy) * downsample;
+  if (downsample <= 1.) {
+    return frame.sample(frame_sampler, (box_origin + 0.5) / viewport);
+  }
+  int pairs = int(downsample / 2.);
+  float4 sum = float4(0.);
+  for (int y = 0; y < pairs; y++) {
+    for (int x = 0; x < pairs; x++) {
+      float2 point = box_origin + float2(2 * x + 1, 2 * y + 1);
+      sum += frame.sample(frame_sampler, point / viewport);
+    }
+  }
+  return sum / float(pairs * pairs);
+}
+
+// One direction of the gaussian over the reduced region, which ends at its
+// own edge rather than at the scratch texture's.
+fragment float4 backdrop_blur_gaussian_fragment(
+    BackdropBlurVertexOutput input [[stage_in]],
+    constant BackdropBlurParams *params [[buffer(1)]],
+    texture2d<float> source [[texture(0)]]) {
+  constexpr sampler source_sampler(mag_filter::linear, min_filter::linear,
+                                   address::clamp_to_edge);
+  float2 texture_size = float2(params->texture_width, params->texture_height);
+  float2 low = float2(0.5);
+  float2 high = float2(params->reduced_width, params->reduced_height) - 0.5;
+  float2 direction = float2(params->direction_x, params->direction_y);
+  float2 center = input.position.xy;
+  float4 sum = source.sample(source_sampler, center / texture_size) *
+               params->tap_weights[0];
+  int taps = int(params->tap_count);
+  for (int i = 1; i < taps; i++) {
+    float2 offset = direction * params->tap_offsets[i];
+    float weight = params->tap_weights[i];
+    sum += source.sample(source_sampler,
+                         clamp(center + offset, low, high) / texture_size) *
+           weight;
+    sum += source.sample(source_sampler,
+                         clamp(center - offset, low, high) / texture_size) *
+           weight;
+  }
+  return sum;
+}
+
+vertex BackdropBlurVertexOutput backdrop_blur_composite_vertex(
+    uint unit_vertex_id [[vertex_id]],
+    constant float2 *unit_vertices [[buffer(0)]],
+    constant BackdropBlurParams *params [[buffer(1)]]) {
+  float2 unit_vertex = unit_vertices[unit_vertex_id];
+  float2 position = float2(params->target_x, params->target_y) +
+                    unit_vertex * float2(params->target_width, params->target_height);
+  float2 viewport = float2(params->viewport_width, params->viewport_height);
+  return BackdropBlurVertexOutput{
+      float4(position / viewport * float2(2., -2.) + float2(-1., 1.), 0., 1.)};
+}
+
+// The blurred backdrop inside the rounded rectangle, saturated and
+// brightened as CSS `saturate()` and `brightness()` do, premultiplied, at
+// the region's coverage.
+fragment float4 backdrop_blur_composite_fragment(
+    BackdropBlurVertexOutput input [[stage_in]],
+    constant BackdropBlurParams *params [[buffer(1)]],
+    texture2d<float> blurred [[texture(0)]]) {
+  constexpr sampler blurred_sampler(mag_filter::linear, min_filter::linear,
+                                    address::clamp_to_edge);
+  float2 point = input.position.xy;
+  float2 clip_origin = float2(params->clip_x, params->clip_y);
+  float2 clip_end = clip_origin + float2(params->clip_width, params->clip_height);
+  if (any(point < clip_origin) || any(point >= clip_end)) {
+    discard_fragment();
+  }
+  Bounds_ScaledPixels bounds;
+  bounds.origin.x = params->bounds_x;
+  bounds.origin.y = params->bounds_y;
+  bounds.size.width = params->bounds_width;
+  bounds.size.height = params->bounds_height;
+  Corners_ScaledPixels radii;
+  radii.top_left = params->radius_top_left;
+  radii.top_right = params->radius_top_right;
+  radii.bottom_right = params->radius_bottom_right;
+  radii.bottom_left = params->radius_bottom_left;
+  float coverage = saturate(0.5 - quad_sdf(point, bounds, radii)) * params->opacity;
+  if (coverage <= 0.) {
+    discard_fragment();
+  }
+
+  float2 texel = (point - float2(params->source_x, params->source_y)) / params->downsample;
+  texel = clamp(texel, float2(0.5),
+                float2(params->reduced_width, params->reduced_height) - 0.5);
+  float4 color = blurred.sample(
+      blurred_sampler, texel / float2(params->texture_width, params->texture_height));
+  float luma = dot(color.rgb, float3(0.213, 0.715, 0.072));
+  float3 rgb = mix(float3(luma), color.rgb, params->saturation) * params->brightness;
+  color.rgb = clamp(rgb, float3(0.), float3(color.a));
+  return color * coverage;
 }
diff --color -ruN gpui-pre-wgpu-0.3.12/src/backdrop_blur.rs b/gpui-pre-wgpu/src/backdrop_blur.rs
--- a/gpui-pre-wgpu/src/backdrop_blur.rs	1970-01-01 01:00:00
+++ b/gpui-pre-wgpu/src/backdrop_blur.rs	2026-11-13 20:48:11
@@ -0,0 +1,465 @@
+//! Backdrop blurs ([`gpui::BackdropBlur`]) for the wgpu renderer.
+//!
+//! A surface texture can only be drawn to, so while a blur is on screen the
+//! frame is drawn into a texture of this module's instead, which each blur
+//! reads, and is copied to the surface once it is finished. A frame without
+//! blurs is drawn to the surface directly, as before.
+
+use bytemuck::{Pod, Zeroable};
+use gpui::{BACKDROP_BLUR_MAX_TAPS, BackdropBlur, BackdropBlurPlan, DevicePixels, Size};
+use std::num::NonZeroU64;
+
+pub(crate) const SHADERS: &str = include_str!("backdrop_blur.wgsl");
+
+/// Mirrors `BlurParams` in backdrop_blur.wgsl.
+#[repr(C)]
+#[derive(Clone, Copy, Pod, Zeroable)]
+struct BackdropBlurParams {
+    target_rect: [f32; 4],
+    source_downsample_taps: [f32; 4],
+    reduced_texture: [f32; 4],
+    viewport_direction: [f32; 4],
+    tap_offsets: [f32; BACKDROP_BLUR_MAX_TAPS],
+    tap_weights: [f32; BACKDROP_BLUR_MAX_TAPS],
+    bounds: [f32; 4],
+    corner_radii: [f32; 4],
+    clip: [f32; 4],
+    filter: [f32; 4],
+}
+
+/// The passes one blur draws: shrink, blur across, blur down, composite.
+const PASSES_PER_BLUR: u64 = 4;
+
+pub(crate) struct BackdropBlurRenderer {
+    downsample: wgpu::RenderPipeline,
+    gaussian: wgpu::RenderPipeline,
+    composite: wgpu::RenderPipeline,
+    present: wgpu::RenderPipeline,
+    layout: wgpu::BindGroupLayout,
+    sampler: wgpu::Sampler,
+    format: wgpu::TextureFormat,
+    params_stride: u64,
+    params: Option<(wgpu::Buffer, u64)>,
+    frame: Option<Target>,
+    scratch: Option<[Target; 2]>,
+}
+
+struct Target {
+    texture: wgpu::Texture,
+    view: wgpu::TextureView,
+}
+
+impl BackdropBlurRenderer {
+    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
+        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
+            label: Some("backdrop_blur_shaders"),
+            source: wgpu::ShaderSource::Wgsl(SHADERS.into()),
+        });
+        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
+            label: Some("backdrop_blur_layout"),
+            entries: &[
+                wgpu::BindGroupLayoutEntry {
+                    binding: 0,
+                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
+                    ty: wgpu::BindingType::Buffer {
+                        ty: wgpu::BufferBindingType::Uniform,
+                        has_dynamic_offset: true,
+                        min_binding_size: NonZeroU64::new(
+                            std::mem::size_of::<BackdropBlurParams>() as u64,
+                        ),
+                    },
+                    count: None,
+                },
+                wgpu::BindGroupLayoutEntry {
+                    binding: 1,
+                    visibility: wgpu::ShaderStages::FRAGMENT,
+                    ty: wgpu::BindingType::Texture {
+                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
+                        view_dimension: wgpu::TextureViewDimension::D2,
+                        multisampled: false,
+                    },
+                    count: None,
+                },
+                wgpu::BindGroupLayoutEntry {
+                    binding: 2,
+                    visibility: wgpu::ShaderStages::FRAGMENT,
+                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
+                    count: None,
+                },
+            ],
+        });
+        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
+            label: Some("backdrop_blur_pipeline_layout"),
+            bind_group_layouts: &[Some(&layout)],
+            immediate_size: 0,
+        });
+        let pipeline = |name: &str, vertex: &str, fragment: &str, blend| {
+            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
+                label: Some(name),
+                layout: Some(&pipeline_layout),
+                vertex: wgpu::VertexState {
+                    module: &module,
+                    entry_point: Some(vertex),
+                    buffers: &[],
+                    compilation_options: wgpu::PipelineCompilationOptions::default(),
+                },
+                fragment: Some(wgpu::FragmentState {
+                    module: &module,
+                    entry_point: Some(fragment),
+                    targets: &[Some(wgpu::ColorTargetState {
+                        format,
+                        blend,
+                        write_mask: wgpu::ColorWrites::ALL,
+                    })],
+                    compilation_options: wgpu::PipelineCompilationOptions::default(),
+                }),
+                primitive: wgpu::PrimitiveState {
+                    topology: wgpu::PrimitiveTopology::TriangleStrip,
+                    ..Default::default()
+                },
+                depth_stencil: None,
+                multisample: wgpu::MultisampleState::default(),
+                multiview_mask: None,
+                cache: None,
+            })
+        };
+        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
+            label: Some("backdrop_blur_sampler"),
+            address_mode_u: wgpu::AddressMode::ClampToEdge,
+            address_mode_v: wgpu::AddressMode::ClampToEdge,
+            mag_filter: wgpu::FilterMode::Linear,
+            min_filter: wgpu::FilterMode::Linear,
+            ..Default::default()
+        });
+        let alignment = u64::from(device.limits().min_uniform_buffer_offset_alignment);
+        Self {
+            downsample: pipeline(
+                "backdrop_blur_downsample",
+                "vs_blur_pass",
+                "fs_blur_downsample",
+                None,
+            ),
+            gaussian: pipeline(
+                "backdrop_blur_gaussian",
+                "vs_blur_pass",
+                "fs_blur_gaussian",
+                None,
+            ),
+            composite: pipeline(
+                "backdrop_blur_composite",
+                "vs_blur_composite",
+                "fs_blur_composite",
+                Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
+            ),
+            present: pipeline(
+                "backdrop_blur_present",
+                "vs_blur_pass",
+                "fs_blur_present",
+                None,
+            ),
+            layout,
+            sampler,
+            format,
+            params_stride: (std::mem::size_of::<BackdropBlurParams>() as u64)
+                .next_multiple_of(alignment),
+            params: None,
+            frame: None,
+            scratch: None,
+        }
+    }
+
+    /// The texture a frame of `size` with blurs is drawn into. Its contents
+    /// are left from the last frame; the frame clears it.
+    pub(crate) fn begin_frame(
+        &mut self,
+        device: &wgpu::Device,
+        size: Size<DevicePixels>,
+        blur_count: usize,
+    ) -> wgpu::TextureView {
+        let (width, height) = (size.width.0.max(1) as u32, size.height.0.max(1) as u32);
+        if self
+            .frame
+            .as_ref()
+            .is_none_or(|frame| frame.texture.width() != width || frame.texture.height() != height)
+        {
+            self.frame = Some(self.target(device, "backdrop_blur_frame", width, height));
+        }
+        let needed = self.params_stride * PASSES_PER_BLUR * blur_count.max(1) as u64;
+        if self.params.as_ref().is_none_or(|(_, size)| *size < needed) {
+            let size = needed.next_power_of_two();
+            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
+                label: Some("backdrop_blur_params"),
+                size,
+                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
+                mapped_at_creation: false,
+            });
+            self.params = Some((buffer, size));
+        }
+        self.frame.as_ref().unwrap().view.clone()
+    }
+
+    /// Blurs what has been drawn into the frame texture around the `index`th
+    /// blur of the frame and draws it back inside the blur's region.
+    #[allow(clippy::too_many_arguments)]
+    pub(crate) fn draw(
+        &mut self,
+        device: &wgpu::Device,
+        queue: &wgpu::Queue,
+        encoder: &mut wgpu::CommandEncoder,
+        index: usize,
+        blur: &BackdropBlur,
+        plan: &BackdropBlurPlan,
+        size: Size<DevicePixels>,
+    ) {
+        let reduced_width = plan.reduced_size.width.0.max(1) as u32;
+        let reduced_height = plan.reduced_size.height.0.max(1) as u32;
+        self.ensure_scratch(device, reduced_width, reduced_height);
+        let (Some(frame), Some([reduced, across]), Some((params_buffer, _))) =
+            (&self.frame, &self.scratch, &self.params)
+        else {
+            return;
+        };
+
+        let mut params = BackdropBlurParams::new(blur, plan, size, &reduced.texture);
+        let base = self.params_stride * PASSES_PER_BLUR * index as u64;
+        let directions = [[0., 0.], [1., 0.], [0., 1.], [0., 0.]];
+        for (pass, direction) in directions.iter().enumerate() {
+            params.viewport_direction[2] = direction[0];
+            params.viewport_direction[3] = direction[1];
+            queue.write_buffer(
+                params_buffer,
+                base + self.params_stride * pass as u64,
+                bytemuck::bytes_of(&params),
+            );
+        }
+
+        let bind_group = |source: &wgpu::TextureView| {
+            device.create_bind_group(&wgpu::BindGroupDescriptor {
+                label: Some("backdrop_blur_bind_group"),
+                layout: &self.layout,
+                entries: &[
+                    wgpu::BindGroupEntry {
+                        binding: 0,
+                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
+                            buffer: params_buffer,
+                            offset: 0,
+                            size: NonZeroU64::new(std::mem::size_of::<BackdropBlurParams>() as u64),
+                        }),
+                    },
+                    wgpu::BindGroupEntry {
+                        binding: 1,
+                        resource: wgpu::BindingResource::TextureView(source),
+                    },
+                    wgpu::BindGroupEntry {
+                        binding: 2,
+                        resource: wgpu::BindingResource::Sampler(&self.sampler),
+                    },
+                ],
+            })
+        };
+        // Shrink the frame into `reduced`, blur it across into `across`,
+        // then down back into `reduced`.
+        let passes = [
+            (&self.downsample, bind_group(&frame.view), &reduced.view),
+            (&self.gaussian, bind_group(&reduced.view), &across.view),
+            (&self.gaussian, bind_group(&across.view), &reduced.view),
+        ];
+        for (pass_index, (pipeline, bind_group, target)) in passes.iter().enumerate() {
+            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
+                label: Some("backdrop_blur_pass"),
+                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
+                    view: target,
+                    resolve_target: None,
+                    ops: wgpu::Operations {
+                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
+                        store: wgpu::StoreOp::Store,
+                    },
+                    depth_slice: None,
+                })],
+                depth_stencil_attachment: None,
+                ..Default::default()
+            });
+            pass.set_viewport(0., 0., reduced_width as f32, reduced_height as f32, 0., 1.);
+            pass.set_pipeline(pipeline);
+            let offset = base + self.params_stride * pass_index as u64;
+            pass.set_bind_group(0, bind_group, &[offset as u32]);
+            pass.draw(0..4, 0..1);
+        }
+
+        let composite = bind_group(&reduced.view);
+        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
+            label: Some("backdrop_blur_composite"),
+            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
+                view: &frame.view,
+                resolve_target: None,
+                ops: wgpu::Operations {
+                    load: wgpu::LoadOp::Load,
+                    store: wgpu::StoreOp::Store,
+                },
+                depth_slice: None,
+            })],
+            depth_stencil_attachment: None,
+            ..Default::default()
+        });
+        pass.set_pipeline(&self.composite);
+        let offset = base + self.params_stride * 3;
+        pass.set_bind_group(0, &composite, &[offset as u32]);
+        pass.draw(0..4, 0..1);
+    }
+
+    /// Copies the finished frame texture to `surface`.
+    pub(crate) fn present(
+        &self,
+        device: &wgpu::Device,
+        encoder: &mut wgpu::CommandEncoder,
+        surface: &wgpu::TextureView,
+    ) {
+        let (Some(frame), Some((params_buffer, _))) = (&self.frame, &self.params) else {
+            return;
+        };
+        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
+            label: Some("backdrop_blur_present_bind_group"),
+            layout: &self.layout,
+            entries: &[
+                wgpu::BindGroupEntry {
+                    binding: 0,
+                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
+                        buffer: params_buffer,
+                        offset: 0,
+                        size: NonZeroU64::new(std::mem::size_of::<BackdropBlurParams>() as u64),
+                    }),
+                },
+                wgpu::BindGroupEntry {
+                    binding: 1,
+                    resource: wgpu::BindingResource::TextureView(&frame.view),
+                },
+                wgpu::BindGroupEntry {
+                    binding: 2,
+                    resource: wgpu::BindingResource::Sampler(&self.sampler),
+                },
+            ],
+        });
+        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
+            label: Some("backdrop_blur_present"),
+            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
+                view: surface,
+                resolve_target: None,
+                ops: wgpu::Operations {
+                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
+                    store: wgpu::StoreOp::Store,
+                },
+                depth_slice: None,
+            })],
+            depth_stencil_attachment: None,
+            ..Default::default()
+        });
+        pass.set_pipeline(&self.present);
+        pass.set_bind_group(0, &bind_group, &[0]);
+        pass.draw(0..4, 0..1);
+    }
+
+    fn ensure_scratch(&mut self, device: &wgpu::Device, width: u32, height: u32) {
+        if let Some([reduced, _]) = &self.scratch
+            && reduced.texture.width() >= width
+            && reduced.texture.height() >= height
+        {
+            return;
+        }
+        // Grow in steps, so a panel that grows does not remake them each frame.
+        let round_up = |length: u32| length.div_ceil(128) * 128;
+        let (width, height) = match &self.scratch {
+            Some([reduced, _]) => (
+                round_up(width.max(reduced.texture.width())),
+                round_up(height.max(reduced.texture.height())),
+            ),
+            None => (round_up(width), round_up(height)),
+        };
+        self.scratch = Some([
+            self.target(device, "backdrop_blur_reduced", width, height),
+            self.target(device, "backdrop_blur_across", width, height),
+        ]);
+    }
+
+    fn target(&self, device: &wgpu::Device, label: &str, width: u32, height: u32) -> Target {
+        let texture = device.create_texture(&wgpu::TextureDescriptor {
+            label: Some(label),
+            size: wgpu::Extent3d {
+                width,
+                height,
+                depth_or_array_layers: 1,
+            },
+            mip_level_count: 1,
+            sample_count: 1,
+            dimension: wgpu::TextureDimension::D2,
+            format: self.format,
+            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
+            view_formats: &[],
+        });
+        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
+        Target { texture, view }
+    }
+}
+
+impl BackdropBlurParams {
+    fn new(
+        blur: &BackdropBlur,
+        plan: &BackdropBlurPlan,
+        size: Size<DevicePixels>,
+        scratch: &wgpu::Texture,
+    ) -> Self {
+        let clip = blur.content_mask.bounds;
+        Self {
+            target_rect: [
+                plan.target.origin.x.0 as f32,
+                plan.target.origin.y.0 as f32,
+                plan.target.size.width.0 as f32,
+                plan.target.size.height.0 as f32,
+            ],
+            source_downsample_taps: [
+                plan.source_origin.x.0 as f32,
+                plan.source_origin.y.0 as f32,
+                plan.downsample as f32,
+                plan.kernel.tap_count as f32,
+            ],
+            reduced_texture: [
+                plan.reduced_size.width.0 as f32,
+                plan.reduced_size.height.0 as f32,
+                scratch.width() as f32,
+                scratch.height() as f32,
+            ],
+            viewport_direction: [size.width.0 as f32, size.height.0 as f32, 0., 0.],
+            tap_offsets: plan.kernel.offsets,
+            tap_weights: plan.kernel.weights,
+            bounds: [
+                blur.bounds.origin.x.0,
+                blur.bounds.origin.y.0,
+                blur.bounds.size.width.0,
+                blur.bounds.size.height.0,
+            ],
+            corner_radii: [
+                blur.corner_radii.top_left.0,
+                blur.corner_radii.top_right.0,
+                blur.corner_radii.bottom_right.0,
+                blur.corner_radii.bottom_left.0,
+            ],
+            clip: [
+                clip.origin.x.0,
+                clip.origin.y.0,
+                clip.size.width.0,
+                clip.size.height.0,
+            ],
+            filter: [blur.saturation, blur.brightness, blur.opacity, 0.],
+        }
+    }
+}
+
+#[cfg(test)]
+mod tests {
+    use super::BackdropBlurParams;
+
+    #[test]
+    fn backdrop_blur_params_match_the_shader_struct() {
+        // Fifteen vec4s.
+        assert_eq!(std::mem::size_of::<BackdropBlurParams>(), 15 * 16);
+    }
+}
diff --color -ruN gpui-pre-wgpu-0.3.12/src/backdrop_blur.wgsl b/gpui-pre-wgpu/src/backdrop_blur.wgsl
--- a/gpui-pre-wgpu/src/backdrop_blur.wgsl	1970-01-01 01:00:00
+++ b/gpui-pre-wgpu/src/backdrop_blur.wgsl	2026-11-13 20:50:08
@@ -0,0 +1,149 @@
+// Backdrop blur. The frame is drawn into a texture of its own while a blur is
+// on screen; each blur shrinks the region around it into a scratch texture
+// (averaging boxes of `downsample` pixels), runs a separable gaussian across
+// and down that, then draws the result, saturated and brightened, over the
+// frame inside its rounded rectangle. The finished frame is then copied to
+// the surface.
+
+// Mirrors `BackdropBlurParams` in backdrop_blur.rs.
+struct BlurParams {
+    // x, y, width, height of the pixels the composite covers.
+    target_rect: vec4<f32>,
+    // The frame pixel at the first reduced texel's corner, the downsample
+    // factor, and the number of taps used.
+    source_downsample_taps: vec4<f32>,
+    // The reduced region's size, then the scratch texture's.
+    reduced_texture: vec4<f32>,
+    // The frame's size, then the gaussian's direction.
+    viewport_direction: vec4<f32>,
+    tap_offsets: array<vec4<f32>, 3>,
+    tap_weights: array<vec4<f32>, 3>,
+    bounds: vec4<f32>,
+    // top left, top right, bottom right, bottom left.
+    corner_radii: vec4<f32>,
+    clip: vec4<f32>,
+    // saturation, brightness, opacity.
+    effect: vec4<f32>,
+}
+
+@group(0) @binding(0) var<uniform> params: BlurParams;
+@group(0) @binding(1) var source: texture_2d<f32>;
+@group(0) @binding(2) var source_sampler: sampler;
+
+struct BlurVarying {
+    @builtin(position) position: vec4<f32>,
+}
+
+fn unit_vertex(vertex_id: u32) -> vec2<f32> {
+    return vec2<f32>(f32(vertex_id & 1u), 0.5 * f32(vertex_id & 2u));
+}
+
+// Covers the whole viewport, which each pass sets to the region it writes.
+@vertex
+fn vs_blur_pass(@builtin(vertex_index) vertex_id: u32) -> BlurVarying {
+    var out = BlurVarying();
+    out.position = vec4<f32>(unit_vertex(vertex_id) * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
+    return out;
+}
+
+fn tap_offset(index: i32) -> f32 {
+    return params.tap_offsets[index / 4][index % 4];
+}
+
+fn tap_weight(index: i32) -> f32 {
+    return params.tap_weights[index / 4][index % 4];
+}
+
+// One reduced texel: the average of its box of frame pixels, read two by two
+// with bilinear samples between them.
+@fragment
+fn fs_blur_downsample(input: BlurVarying) -> @location(0) vec4<f32> {
+    let viewport = params.viewport_direction.xy;
+    let downsample = params.source_downsample_taps.z;
+    let box_origin = params.source_downsample_taps.xy + floor(input.position.xy) * downsample;
+    if (downsample <= 1.0) {
+        return textureSampleLevel(source, source_sampler, (box_origin + 0.5) / viewport, 0.0);
+    }
+    let pairs = i32(downsample / 2.0);
+    var sum = vec4<f32>(0.0);
+    for (var y = 0; y < pairs; y += 1) {
+        for (var x = 0; x < pairs; x += 1) {
+            let point = box_origin + vec2<f32>(f32(2 * x + 1), f32(2 * y + 1));
+            sum += textureSampleLevel(source, source_sampler, point / viewport, 0.0);
+        }
+    }
+    return sum / f32(pairs * pairs);
+}
+
+// One direction of the gaussian over the reduced region, which ends at its
+// own edge rather than at the scratch texture's.
+@fragment
+fn fs_blur_gaussian(input: BlurVarying) -> @location(0) vec4<f32> {
+    let texture_size = params.reduced_texture.zw;
+    let low = vec2<f32>(0.5);
+    let high = params.reduced_texture.xy - 0.5;
+    let direction = params.viewport_direction.zw;
+    let center = input.position.xy;
+    var sum = textureSampleLevel(source, source_sampler, center / texture_size, 0.0) * tap_weight(0);
+    let taps = i32(params.source_downsample_taps.w);
+    for (var i = 1; i < taps; i += 1) {
+        let offset = direction * tap_offset(i);
+        let weight = tap_weight(i);
+        sum += textureSampleLevel(source, source_sampler, clamp(center + offset, low, high) / texture_size, 0.0) * weight;
+        sum += textureSampleLevel(source, source_sampler, clamp(center - offset, low, high) / texture_size, 0.0) * weight;
+    }
+    return sum;
+}
+
+@vertex
+fn vs_blur_composite(@builtin(vertex_index) vertex_id: u32) -> BlurVarying {
+    let position = params.target_rect.xy + unit_vertex(vertex_id) * params.target_rect.zw;
+    let viewport = params.viewport_direction.xy;
+    var out = BlurVarying();
+    out.position = vec4<f32>(position / viewport * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
+    return out;
+}
+
+fn rounded_rect_sdf(point: vec2<f32>) -> f32 {
+    let half_size = params.bounds.zw / 2.0;
+    let center_to_point = point - (params.bounds.xy + half_size);
+    var radius: f32;
+    if (center_to_point.x < 0.0) {
+        radius = select(params.corner_radii.w, params.corner_radii.x, center_to_point.y < 0.0);
+    } else {
+        radius = select(params.corner_radii.z, params.corner_radii.y, center_to_point.y < 0.0);
+    }
+    let corner_center_to_point = abs(center_to_point) - half_size + radius;
+    let inside = min(0.0, max(corner_center_to_point.x, corner_center_to_point.y));
+    return length(max(vec2<f32>(0.0), corner_center_to_point)) + inside - radius;
+}
+
+// The blurred backdrop inside the rounded rectangle, saturated and
+// brightened as CSS `saturate()` and `brightness()` do, premultiplied, at the
+// region's coverage.
+@fragment
+fn fs_blur_composite(input: BlurVarying) -> @location(0) vec4<f32> {
+    let point = input.position.xy;
+    let clip_end = params.clip.xy + params.clip.zw;
+    if (any(point < params.clip.xy) || any(point >= clip_end)) {
+        return vec4<f32>(0.0);
+    }
+    let coverage = saturate(0.5 - rounded_rect_sdf(point)) * params.effect.z;
+    let downsample = params.source_downsample_taps.z;
+    let texel = clamp(
+        (point - params.source_downsample_taps.xy) / downsample,
+        vec2<f32>(0.5),
+        params.reduced_texture.xy - 0.5,
+    );
+    var color = textureSampleLevel(source, source_sampler, texel / params.reduced_texture.zw, 0.0);
+    let luma = dot(color.rgb, vec3<f32>(0.213, 0.715, 0.072));
+    let rgb = mix(vec3<f32>(luma), color.rgb, params.effect.x) * params.effect.y;
+    color = vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(color.a)), color.a);
+    return color * coverage;
+}
+
+// Copies the finished frame to the surface.
+@fragment
+fn fs_blur_present(input: BlurVarying) -> @location(0) vec4<f32> {
+    return textureLoad(source, vec2<i32>(input.position.xy), 0);
+}
diff --color -ruN gpui-pre-wgpu-0.3.12/src/gpui_wgpu.rs b/gpui-pre-wgpu/src/gpui_wgpu.rs
--- a/gpui-pre-wgpu/src/gpui_wgpu.rs	2006-07-24 03:21:28
+++ b/gpui-pre-wgpu/src/gpui_wgpu.rs	2026-11-13 20:48:32
@@ -1,3 +1,4 @@
+mod backdrop_blur;
 mod cosmic_text_system;
 mod wgpu_atlas;
 mod wgpu_context;
diff --color -ruN gpui-pre-wgpu-0.3.12/src/shaders.wgsl b/gpui-pre-wgpu/src/shaders.wgsl
--- a/gpui-pre-wgpu/src/shaders.wgsl	2006-07-24 03:21:28
+++ b/gpui-pre-wgpu/src/shaders.wgsl	2026-11-13 20:29:08
@@ -959,7 +959,7 @@
     // clip so the shadow never escapes the element.
     element_bounds: Bounds,
     element_corner_radii: Corners,
-    // 0 = drop shadow, 1 = inset shadow.
+    // 0 = drop shadow, 1 = inset shadow, 2 = drop shadow with the element cut out.
     inset: u32,
     pad: u32, // align to 8 bytes
 }
@@ -978,7 +978,7 @@
     var shadow = load_shadow(instance_id);
 
     var geometry: Bounds;
-    if (shadow.inset != 0u) {
+    if (shadow.inset == 1u) {
         geometry = shadow.element_bounds;
     } else {
         // Leave room for the gaussian tail outside the shadow rect.
@@ -1040,6 +1040,11 @@
         let element_distance = quad_sdf(input.position.xy, shadow.element_bounds,
                                         shadow.element_corner_radii);
         alpha *= saturate(0.5 - element_distance);
+    } else if (shadow.inset == 2u) {
+        // A drop shadow with the element's shape cut out of it.
+        let element_distance = quad_sdf(input.position.xy, shadow.element_bounds,
+                                        shadow.element_corner_radii);
+        alpha *= saturate(0.5 + element_distance);
     }
 
     return blend_color(input.color, alpha);
diff --color -ruN gpui-pre-wgpu-0.3.12/src/wgpu_renderer.rs b/gpui-pre-wgpu/src/wgpu_renderer.rs
--- a/gpui-pre-wgpu/src/wgpu_renderer.rs	2006-07-24 03:21:28
+++ b/gpui-pre-wgpu/src/wgpu_renderer.rs	2026-11-13 20:48:32
@@ -1,4 +1,5 @@
 // Modified for gpui-pre (snapshot of zed@d2a4fb6): adapt device queues and sRGB detection to wgpu 30.0.1.
+use crate::backdrop_blur::BackdropBlurRenderer;
 use crate::{CompositorGpuHint, DeviceErrorState, WgpuAtlas, WgpuContext};
 use anyhow::{Context as _, Result};
 use bytemuck::{Pod, Zeroable};
@@ -195,6 +196,8 @@
     path_intermediate_view: Option<wgpu::TextureView>,
     path_msaa_texture: Option<wgpu::Texture>,
     path_msaa_view: Option<wgpu::TextureView>,
+    /// Made when the first backdrop blur is drawn.
+    backdrop_blur: Option<BackdropBlurRenderer>,
 }
 
 struct CachedTextureBindGroup {
@@ -1378,6 +1381,7 @@
                 path_intermediate_view: None,
                 path_msaa_texture: None,
                 path_msaa_view: None,
+                backdrop_blur: None,
             },
             atlas,
             path_globals_offset,
@@ -1493,7 +1497,7 @@
             bytemuck::bytes_of(&gamma_params),
         );
 
-        self.record_frame(scene, target_view, clear_color)
+        self.record_frame(scene, target_view, size, clear_color)
             .inspect_err(|_| {
                 // Queue writes are staged before encoding; flush them even if the frame fails.
                 self.resources.queue.submit(std::iter::empty());
@@ -1503,7 +1507,8 @@
     fn record_frame(
         &mut self,
         scene: &Scene,
-        frame_view: &wgpu::TextureView,
+        target_view: &wgpu::TextureView,
+        size: Size<DevicePixels>,
         clear_color: wgpu::Color,
     ) -> Result<wgpu::SubmissionIndex> {
         let mut instance_offset = 0;
@@ -1530,6 +1535,22 @@
                     label: Some("main_encoder"),
                 });
 
+        // A frame with backdrop blurs is drawn into the blur renderer's
+        // texture, which the blurs can read, and copied to the target last.
+        let device = Arc::clone(&self.resources.device);
+        let queue = Arc::clone(&self.resources.queue);
+        let blur_count = scene.backdrop_blurs.len();
+        let blur_frame_view = (blur_count > 0).then(|| {
+            let target_format = self.target_format;
+            self.resources
+                .backdrop_blur
+                .get_or_insert_with(|| BackdropBlurRenderer::new(&device, target_format))
+                .begin_frame(&device, size, blur_count)
+        });
+        let frame_view = blur_frame_view.as_ref().unwrap_or(target_view);
+        let mut backdrop_blurs = scene.backdrop_blurs.iter().enumerate().peekable();
+        let mut batches = scene.batches().peekable();
+
         {
             let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                 label: Some("main_pass"),
@@ -1546,7 +1567,38 @@
                 ..Default::default()
             });
 
-            for batch in scene.batches() {
+            loop {
+                // The blurs ordered before the next batch (all that are left
+                // after the last) read what has been drawn so far.
+                let next_order = batches.peek().map(|batch| scene.batch_order(batch));
+                while let Some((index, blur)) = backdrop_blurs
+                    .next_if(|(_, blur)| next_order.is_none_or(|order| blur.order <= order))
+                {
+                    let (Some(plan), Some(blur_renderer)) =
+                        (blur.plan(size), self.resources.backdrop_blur.as_mut())
+                    else {
+                        continue;
+                    };
+                    drop(pass);
+                    blur_renderer.draw(&device, &queue, &mut encoder, index, blur, &plan, size);
+                    pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
+                        label: Some("main_pass_continued"),
+                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
+                            view: frame_view,
+                            resolve_target: None,
+                            ops: wgpu::Operations {
+                                load: wgpu::LoadOp::Load,
+                                store: wgpu::StoreOp::Store,
+                            },
+                            depth_slice: None,
+                        })],
+                        depth_stencil_attachment: None,
+                        ..Default::default()
+                    });
+                }
+                let Some(batch) = batches.next() else {
+                    break;
+                };
                 match batch {
                     PrimitiveBatch::Quads(range) => self.draw_instances(
                         &instance_bindings.quads,
@@ -1639,6 +1691,11 @@
                     PrimitiveBatch::Surfaces(_surfaces) => {}
                 }
             }
+        }
+        if blur_frame_view.is_some()
+            && let Some(blur_renderer) = &self.resources.backdrop_blur
+        {
+            blur_renderer.present(&device, &mut encoder, target_view);
         }
 
         let submission = self
diff --color -ruN gpui-pre-windows-0.3.12/src/shaders.hlsl b/gpui-pre-windows/src/shaders.hlsl
--- a/gpui-pre-windows/src/shaders.hlsl	2006-07-24 03:21:28
+++ b/gpui-pre-windows/src/shaders.hlsl	2026-11-13 20:29:00
@@ -887,7 +887,7 @@
     Shadow shadow = shadows[shadow_id];
 
     Bounds bounds;
-    if (shadow.inset != 0u) {
+    if (shadow.inset == 1u) {
         bounds = shadow.element_bounds;
     } else {
         // Leave room for the gaussian tail outside the shadow rect.
@@ -948,6 +948,11 @@
         float element_distance = quad_sdf(input.position.xy, shadow.element_bounds,
                                           shadow.element_corner_radii);
         alpha *= saturate(0.5 - element_distance);
+    } else if (shadow.inset == 2u) {
+        // A drop shadow with the element's shape cut out of it.
+        float element_distance = quad_sdf(input.position.xy, shadow.element_bounds,
+                                          shadow.element_corner_radii);
+        alpha *= saturate(0.5 + element_distance);
     }
 
     return input.color * float4(1., 1., 1., alpha);
```
