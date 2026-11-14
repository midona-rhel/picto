# Patched GPUI crates

The vendored `gpui-pre` crate leaves out the published package's `examples/`
folder (4.9 MB of sample programs and images) and the `[[example]]` entries
of its manifest that name them; nothing else differs from 0.3.12 apart from
the changes described here.

These are the crates.io sources of `gpui-pre`, `gpui-pre-apple`,
`gpui-pre-wgpu`, `gpui-pre-windows`, `gpui-pre-macos` and `gpui-pre-linux`
version 0.3.12 (snapshot of
zed-industries/zed at d2a4fb6), Apache-2.0 (each keeps its `LICENSE-APACHE`),
with a backdrop-blur primitive, nearest-neighbour image sampling and the
reading of what a drop from another application carries besides files
added.
`../Cargo.toml` substitutes them for the
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
  under its element. `Window::is_dirty` (test support only), so the
  off-screen checks can draw only the frames a real window would.
  `Window::start_platform_drag`, which hands files to the platform's native
  drag session (the published `start_external_drag` of the macOS and Wayland
  windows) for a view that follows the pointer itself: the grid's item drag
  leaving the window. GPUI otherwise does this only for drags started with
  `on_drag` and `external_drag_payload`.
- `src/platform/test/window.rs`: the test window keeps the decorations the
  window asked for and reports them (`Decorations::Client` with nothing
  tiled when the client's were asked for), as a Linux desktop that grants
  them does, so the off-screen checks draw a frameless window's own chrome
  (its buttons and resize edges) as on Linux.
- Nearest-neighbour images (CSS `image-rendering: pixelated`, the Preview
  setting "Scaling: Pixelated"): `PolychromeSprite`'s padding word after
  `order` becomes `nearest` (a `PaddedBool32`, so the layout every shader
  reads is unchanged; `src/scene.rs`); `Window::paint_image_sampled` takes
  the choice and `Window::paint_image` calls it with `false`
  (`src/window.rs`); `ImageStyle::pixelated` and `StyledImage::pixelated`
  pass it from the `img` element (`src/elements/img.rs`). Each renderer's
  sprite fragment shader then reads its linear sampler at the centre of the
  texel the position falls in, which returns that texel alone; no sampler,
  binding or pipeline is added.

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
- `src/metal_atlas.rs`: `PlatformAtlas::contains` (test support only), so
  `Window::has_image_atlas_entry` answers on Metal as it does headless and
  the off-screen checks can tell whether a released picture left the atlas.
- `src/shaders.metal`, `polychrome_sprite_fragment`: the nearest-neighbour
  read. `vendor/gpui/src/scene.rs` (the copy the Metal header is generated
  from) has the same `nearest` field.

`gpui-pre-wgpu` (Linux; also the web)
- `src/backdrop_blur.rs`, `src/backdrop_blur.wgsl`: the same passes. A
  surface texture can only be drawn to, so a frame with blurs is drawn into a
  texture of this module's and copied to the surface at the end; a frame
  without blurs is drawn as before.
- `src/wgpu_renderer.rs`: holds the blur renderer with the other device
  resources (dropped on device loss) and interleaves blurs in `record_frame`
  as the Metal renderer does. `src/gpui_wgpu.rs` declares the module.
- `src/shaders.wgsl`: the shadow shader takes `inset == 2`; the
  `PolychromeSprite` struct names its second word `nearest`, and
  `fs_poly_sprite` does the nearest-neighbour read (by `select`, so the
  texture sample stays in uniform control flow).
- `src/wgpu_context.rs`: on Windows the instance also offers Direct3D 12.
  This crate is used there only by the off-screen checks (GPUI's Windows
  platform draws with DirectX itself), and a machine without a GPU, such as
  a CI runner, has Direct3D 12 through the WARP software adapter but
  neither Vulkan nor a usable OpenGL.

`gpui-pre-windows` (DirectX)
- `src/shaders.hlsl`: the shadow shader takes `inset == 2`; the
  `PolychromeSprite` struct names its second word `nearest`, and
  `polychrome_sprite_fragment` does the nearest-neighbour read
  (`GetDimensions` for the atlas size). The DirectX
  renderer ignores `Scene::backdrop_blurs`, so on Windows a glass panel shows
  its translucent fill over an unblurred backdrop.

Drops from other applications. GPUI hands an application only the paths of
dropped files, so what a web browser drags (an image's data, its address,
the page's HTML for it) never arrived. Each platform now reads it while the
drop is delivered.

`gpui-pre`
- `src/interactive.rs`: `ExternalDropData` (in-memory files as
  `ExternalDropFile`: bytes, name, media type; address lists, HTML, text,
  Chromium `DownloadURL`s) and the hand-over: a platform calls
  `set_external_drop_data` just before `FileDropEvent::Submit`, and the
  application calls `take_external_drop_data` while it handles that drop. A
  drag that carries only such data enters with no paths.

`gpui-pre-macos` (AppKit)
- `src/drop_data.rs`: the types a window registers besides
  `NSFilenamesPboardType` (`public.png`, `public.jpeg`,
  `com.compuserve.gif`, `org.webmproject.webp`, `public.tiff`,
  `public.url`, `public.html`, `public.utf8-plain-text`), and
  `read_drop_pasteboard`, which reads each pasteboard item's first image
  type, link, HTML and text (no image data or link from an item that is a
  file, whose path the drop has). Public so the application's test can read
  a pasteboard of its own.
- `src/window.rs`: registers those types; a drag with no file names that
  offers one of them enters with no paths; `performDragOperation:` reads the
  drag pasteboard and hands it over before `Submit`.
- `src/gpui_macos.rs`: declares the module and exports
  `read_drop_pasteboard`.

`gpui-pre-windows` (OLE drop target)
- `src/drop_data.rs`: whether a data object offers what is read, and the
  reading, for a drag without `CF_HDROP`: virtual files
  (`FileGroupDescriptorW` and each item's `FileContents`, from global memory
  or a stream, at most 1 GiB each; folders skipped), else `PNG`, else
  `CF_DIBV5` or `CF_DIB` made a BMP file; `UniformResourceLocatorW` (else
  `UniformResourceLocator`), `HTML Format` (its fragment) and
  `CF_UNICODETEXT`. `file_group_names` and `html_fragment` are public for
  the application's test.
- `src/window.rs`: `DragEnter` takes such a drag with no paths;
  `Drop` reads the data object and hands it over before `Submit`.
- `src/clipboard.rs`: `convert_dib_to_bmp` and `LockedGlobal` are shared
  with the drop reading (`pub(crate)`).
- `src/gpui_windows.rs`: declares the module and exports the two parsers.

`gpui-pre-linux` (X11 and Wayland)
- `src/linux/drop_data.rs`: which offered media types are taken
  (`takes`), which are read from a drag without files (`wanted`: the first
  of `image/png`, `image/jpeg`, `image/gif`, `image/webp`, `image/bmp`,
  `image/tiff`; `text/uri-list`; `text/html`; the first of
  `text/plain;charset=utf-8`, `UTF8_STRING`, `text/plain`), and what they
  come to (`assemble`; text is UTF-8, or UTF-16 after a byte order mark, as
  Firefox sends HTML). `wanted` and `assemble` are exported for the
  application's test.
- `src/linux/x11/client.rs`: `XdndEnter` notes the offered types' names (the
  three in the message or `XdndTypeList`); a drag with no supported text
  type that offers an image enters with no paths at its first position; on
  `XdndDrop`, before `XdndFinished`, a drag that brought no paths and that
  no window of this process started is read and handed over.
- `src/linux/x11/clipboard.rs`: `Clipboard::read_xdnd` reads given types
  from `XdndSelection` on a connection of its own with the clipboard's
  reader (`INCR` included); `read_single` takes the selection by atom
  (`read_single_from`) and a `SelectionNotify` for `XdndSelection` is
  accepted.
- `src/linux/wayland/client.rs`: an offer's taken types are accepted; on
  `Enter` the file list and the wanted types are received through a pipe
  each and read off the main thread; a drag without paths whose data is not
  empty enters with no paths, its data kept in `DragState::drop_data` and
  handed over on `Drop` (cleared on `Leave`).
- `src/linux/wayland/clipboard.rs`: `DataOffer::mime_types`.
- `src/linux.rs`, `src/gpui_linux.rs`: declare and export the module.

Dragging files out on Windows and X11. `Window::start_platform_drag` had a
platform drag on macOS and Wayland only.

`gpui-pre-windows`
- `src/drag_source.rs`: a data object holding the files as `CF_HDROP`
  (`DROPFILES`, wide names; `GetData`, `QueryGetData` and `EnumFormatEtc`
  answer it, the rest is not implemented) and a drop source (drop when the
  left button is up, cancel on Escape, the system's cursors). `drag` runs
  `DoDragDrop` with copy allowed. `drop_files` is exported
  (`drag_drop_files`) for the application's test.
- `src/window.rs`: `can_start_external_drag` and `start_external_drag`.
  `DoDragDrop` runs the system's drag loop until the drop, so it is run from
  a foreground task, after the event that asked for it has been handled
  (the application is not borrowed then, and the loop's messages reach the
  window as from the main loop); `FileDropEvent::Ended` follows its return.

`gpui-pre-linux`
- `src/linux/x11/xdnd_source.rs`: the source side of XDND. Each motion finds
  the `XdndAware` window under the pointer (down from the root's child under
  it, following an `XdndProxy` that names itself; a window of this process
  is not a target), sends `XdndLeave`/`XdndEnter` (the version the target
  and this side share, at most 5, `text/uri-list` the one type) when it
  changes, and `XdndPosition` with the copy action, one at a time until its
  `XdndStatus`; the release sends `XdndDrop` to a target that accepted and
  `XdndLeave` otherwise; `SelectionRequest`s for `XdndSelection` get
  `TARGETS` or the files' `file:` addresses (`uri_list`, exported as
  `xdnd_uri_list` for the application's test).
- `src/linux/x11/client.rs`: `X11ClientState::xdnd_source`;
  `start_xdnd_drag` takes `XdndSelection` for the window; the window's own
  pointer motion (which its button's implicit grab keeps delivering outside
  it) and release drive the source; `XdndStatus`, `XdndFinished` and
  `SelectionRequest` reach it; the drag ends (selection let go,
  `FileDropEvent::Ended` to the window) at `XdndFinished`, ten seconds after
  a drop that got none, or at once when nothing took the drop.
- `src/linux/x11/window.rs`: `can_start_external_drag`,
  `start_external_drag`, and the atoms `XdndProxy` and `TARGETS`.
- `src/linux/x11.rs`, `src/linux.rs`, `src/gpui_linux.rs`: declare and
  export.

## Re-applying to a newer gpui-pre

Copy the six new crates from `~/.cargo/registry/src/*/` over these folders,
apply the diff below (`patch -p1` from this folder), and fix what moved. The
Metal header is generated from the `vendor/gpui` copy inside
`gpui-pre-apple`; the blur parameters are a struct of floats declared on both
sides by hand, so no generated type changes. Check: `cargo test` in
`experiments/gpui` (the blur arithmetic and batching tests are in
`src/ui/glass.rs`) and an off-screen picture of a menu over the grid; for
nearest sampling, an off-screen picture of a small image zoomed in the
viewer with Settings > Preview > Scaling set to Pixelated. The WGSL was
validated with naga 30.0.1 (the version `wgpu` 30 uses) in all three of its
variants; the HLSL was not compiled here.

## The diff against 0.3.12

```diff
diff --color -ruN gpui-pre-0.3.12/src/elements/img.rs b/gpui-pre/src/elements/img.rs
--- a/gpui-pre/src/elements/img.rs
+++ b/gpui-pre/src/elements/img.rs
@@ -124,6 +124,7 @@
 /// The style of an image element.
 pub struct ImageStyle {
     grayscale: bool,
+    pixelated: bool,
     object_fit: ObjectFit,
     loading: Option<Box<dyn Fn() -> AnyElement>>,
     fallback: Option<Box<dyn Fn() -> AnyElement>>,
@@ -133,6 +134,7 @@
     fn default() -> Self {
         Self {
             grayscale: false,
+            pixelated: false,
             object_fit: ObjectFit::Contain,
             loading: None,
             fallback: None,
@@ -148,6 +150,13 @@
     /// Set the image to be displayed in grayscale.
     fn grayscale(mut self, grayscale: bool) -> Self {
         self.image_style().grayscale = grayscale;
+        self
+    }
+
+    /// Sample the image nearest-neighbour, as CSS `image-rendering:
+    /// pixelated` draws it, rather than linearly.
+    fn pixelated(mut self, pixelated: bool) -> Self {
+        self.image_style().pixelated = pixelated;
         self
     }
 
@@ -489,13 +498,14 @@
                         .get_bounds(bounds, data.size(layout_state.frame_index));
                     let corner_radii = style.corner_radii.to_pixels(window.rem_size());
                     window
-                        .paint_image(
+                        .paint_image_sampled(
                             bounds,
                             new_bounds,
                             corner_radii,
                             data,
                             layout_state.frame_index,
                             self.style.grayscale,
+                            self.style.pixelated,
                         )
                         .log_err();
                 } else if let Some(replacement) = &mut layout_state.replacement {
diff --color -ruN gpui-pre-0.3.12/src/interactive.rs b/gpui-pre/src/interactive.rs
--- a/gpui-pre/src/interactive.rs
+++ b/gpui-pre/src/interactive.rs
@@ -721,6 +721,69 @@
     }
 }
 
+/// What a drag from another application offered besides files on disk: what
+/// a web browser drags (an image's data, its address, the page's HTML for
+/// it). A platform reads it from the drag while the drop is delivered and
+/// hands it over with [`set_external_drop_data`] just before
+/// [`FileDropEvent::Submit`]; the application takes it with
+/// [`take_external_drop_data`] while it handles that drop. A drag that
+/// carries only these enters the window with no paths.
+#[derive(Debug, Clone, Default, Eq, PartialEq)]
+pub struct ExternalDropData {
+    /// Files held in memory rather than on disk, such as an image's data.
+    pub files: Vec<ExternalDropFile>,
+    /// Addresses, as `text/uri-list` lists them (one per line, `#` lines
+    /// being comments), one entry per offer.
+    pub uri_lists: Vec<String>,
+    /// HTML fragments (`text/html`).
+    pub html: Vec<String>,
+    /// Plain text.
+    pub text: Vec<String>,
+    /// Chromium's `DownloadURL` (`<media type>:<file name>:<address>`).
+    pub download_urls: Vec<String>,
+}
+
+impl ExternalDropData {
+    /// Whether the drag offered nothing of these.
+    pub fn is_empty(&self) -> bool {
+        self.files.is_empty()
+            && self.uri_lists.is_empty()
+            && self.html.is_empty()
+            && self.text.is_empty()
+            && self.download_urls.is_empty()
+    }
+}
+
+/// A file a drag held in memory.
+#[derive(Debug, Clone, Default, Eq, PartialEq)]
+pub struct ExternalDropFile {
+    /// Its contents.
+    pub bytes: Vec<u8>,
+    /// Its name, when the source gave one.
+    pub name: Option<String>,
+    /// Its media type, as the platform named the data (`image/png`).
+    pub media_type: Option<String>,
+}
+
+static EXTERNAL_DROP_DATA: std::sync::Mutex<Option<ExternalDropData>> = std::sync::Mutex::new(None);
+
+/// Called by a platform just before it delivers [`FileDropEvent::Submit`]:
+/// what that drop offered besides its paths (empty when nothing).
+pub fn set_external_drop_data(data: ExternalDropData) {
+    *EXTERNAL_DROP_DATA
+        .lock()
+        .unwrap_or_else(|poison| poison.into_inner()) = Some(data);
+}
+
+/// What the drop being delivered offered besides its paths, once; `None`
+/// after it was taken or when the platform reads nothing of the kind.
+pub fn take_external_drop_data() -> Option<ExternalDropData> {
+    EXTERNAL_DROP_DATA
+        .lock()
+        .unwrap_or_else(|poison| poison.into_inner())
+        .take()
+}
+
 /// Data offered to the platform when an internal drag leaves the window and is
 /// promoted to a native drag session.
 #[derive(Debug, Clone, Eq, PartialEq)]
diff --color -ruN gpui-pre-0.3.12/src/platform/test/window.rs b/gpui-pre/src/platform/test/window.rs
--- a/gpui-pre/src/platform/test/window.rs
+++ b/gpui-pre/src/platform/test/window.rs
@@ -3,8 +3,8 @@
     Pixels, PlatformAtlas, PlatformDisplay, PlatformHeadlessRenderer, PlatformInput,
     PlatformInputHandler, PlatformWindow, Point, PromptButton, RequestFrameOptions, Scene, Size,
     TestDisplay, TestPlatform, TextInputConfiguration, TextInputStateChange, WindowAppearance,
-    WindowBackgroundAppearance, WindowBounds, WindowControlArea, WindowInsets, WindowParams,
-    WindowVisibility,
+    WindowBackgroundAppearance, WindowBounds, WindowControlArea, WindowDecorations, WindowInsets,
+    WindowParams, WindowVisibility,
 };
 use gpui_util::ResultExt as _;
 #[cfg(any(test, feature = "test-support"))]
@@ -59,6 +59,7 @@
     appearance: WindowAppearance,
     external_drag_files: Vec<(PathBuf, bool)>,
     start_external_drag_result: bool,
+    decorations: WindowDecorations,
 }
 
 #[derive(Clone)]
@@ -124,6 +125,7 @@
             appearance_change_callback: None,
             request_frame_callback: None,
             frame_wake_count: Rc::new(Cell::new(0)),
+            decorations: WindowDecorations::Server,
             frame_scheduled: false,
             frame_callback_pending: false,
             input_handler: None,
@@ -354,6 +356,21 @@
         false
     }
 
+    // The decorations the window asked for, as a desktop that grants them
+    // reports them, so a test sees a frameless window's own chrome.
+    fn request_decorations(&self, decorations: WindowDecorations) {
+        self.0.lock().decorations = decorations;
+    }
+
+    fn window_decorations(&self) -> crate::Decorations {
+        match self.0.lock().decorations {
+            WindowDecorations::Server => crate::Decorations::Server,
+            WindowDecorations::Client => crate::Decorations::Client {
+                tiling: crate::Tiling::default(),
+            },
+        }
+    }
+
     fn content_size(&self) -> Size<Pixels> {
         self.bounds().size
     }
diff --color -ruN gpui-pre-0.3.12/src/scene.rs b/gpui-pre/src/scene.rs
--- a/gpui-pre/src/scene.rs
+++ b/gpui-pre/src/scene.rs
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
@@ -136,12 +142,52 @@
         }
         self.paint_operations
             .push(PaintOperation::Primitive(primitive));
+    }
+
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
     }
 
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
@@ -588,9 +650,176 @@
 impl From<Shadow> for Primitive {
     fn from(shadow: Shadow) -> Self {
         Primitive::Shadow(shadow)
+    }
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
     }
+
+    /// How many texels from the centre the furthest tap reads.
+    pub fn reach(&self) -> i32 {
+        self.offsets[self.tap_count as usize - 1].ceil() as i32
+    }
 }
 
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
+}
+
 /// The style of a border.
 #[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
 #[repr(C)]
@@ -748,7 +977,10 @@
 #[expect(missing_docs)]
 pub struct PolychromeSprite {
     pub order: DrawOrder,
-    pub pad: u32,
+    /// Sampled nearest-neighbour (CSS `image-rendering: pixelated`) rather
+    /// than linearly. Takes the place of the struct's former padding word,
+    /// so the layout the shaders read is unchanged.
+    pub nearest: PaddedBool32,
     pub tile: AtlasTile,
     pub grayscale: PaddedBool32,
     pub opacity: f32,
diff --color -ruN gpui-pre-0.3.12/src/window.rs b/gpui-pre/src/window.rs
--- a/gpui-pre/src/window.rs
+++ b/gpui-pre/src/window.rs
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
@@ -2958,6 +2985,14 @@
         count
     }
 
+    /// Whether the window has been invalidated since it was last drawn: what
+    /// the platform's frame loop asks before drawing a frame. Tests use it to
+    /// draw only the frames a real window would.
+    #[cfg(any(test, feature = "test-support"))]
+    pub fn is_dirty(&self) -> bool {
+        self.invalidator.is_dirty()
+    }
+
     /// Spawn the future returned by the given closure on the application thread pool.
     /// The closure is provided a handle to the current window and an `AsyncWindowContext` for
     /// use within your future.
@@ -4811,6 +4846,31 @@
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
@@ -4832,12 +4892,52 @@
                 color: shadow.color.opacity(opacity),
                 element_bounds,
                 element_corner_radii,
-                inset: 0,
+                inset: if outside { 2 } else { 0 },
                 pad: 0,
             });
         }
     }
 
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
+    }
+
     /// Paint the inset shadows from `shadows` into the scene at the current z-index. Should
     /// be called after the element's background so the shadow layers on top of the fill.
     /// Drop shadows are skipped; paint those with [`Self::paint_drop_shadows`] before the background.
@@ -5215,7 +5315,7 @@
 
             self.next_frame.scene.insert_primitive(PolychromeSprite {
                 order: 0,
-                pad: 0,
+                nearest: false.into(),
                 grayscale: false.into(),
                 bounds,
                 corner_radii: Default::default(),
@@ -5309,6 +5409,31 @@
         frame_index: usize,
         grayscale: bool,
     ) -> Result<()> {
+        self.paint_image_sampled(
+            bounds,
+            image_bounds,
+            corner_radii,
+            data,
+            frame_index,
+            grayscale,
+            false,
+        )
+    }
+
+    /// [`Window::paint_image`], sampling the image nearest-neighbour when
+    /// `nearest` is set (CSS `image-rendering: pixelated`) instead of
+    /// linearly.
+    #[allow(clippy::too_many_arguments)]
+    pub fn paint_image_sampled(
+        &mut self,
+        bounds: Bounds<Pixels>,
+        image_bounds: Bounds<Pixels>,
+        corner_radii: Corners<Pixels>,
+        data: Arc<RenderImage>,
+        frame_index: usize,
+        grayscale: bool,
+        nearest: bool,
+    ) -> Result<()> {
         self.invalidator.debug_assert_paint();
 
         let visible_bounds = bounds.intersect(&image_bounds);
@@ -5387,7 +5512,7 @@
 
         self.next_frame.scene.insert_primitive(PolychromeSprite {
             order: 0,
-            pad: 0,
+            nearest: nearest.into(),
             grayscale: grayscale.into(),
             bounds: visible_bounds_snapped,
             content_mask,
@@ -6037,6 +6162,16 @@
         }
     }
 
+    /// Hands `payload` to the platform as a native drag session continuing
+    /// the left-button drag in progress, for a view that follows the pointer
+    /// itself rather than through [`Interactivity::on_drag`] (whose drags
+    /// are handed over on their own when they leave the window). Returns
+    /// whether the platform took it; one without native drags declines.
+    pub fn start_platform_drag(&mut self, payload: &crate::ExternalDragPayload) -> bool {
+        self.platform_window.can_start_external_drag()
+            && self.platform_window.start_external_drag(payload)
+    }
+
     fn promote_external_drag_to_platform(&mut self, event: &PlatformInput, cx: &mut App) {
         let PlatformInput::MouseMove(mouse_move) = event else {
             return;
diff --color -ruN gpui-pre-apple-0.3.12/src/metal_atlas.rs b/gpui-pre-apple/src/metal_atlas.rs
--- a/gpui-pre-apple/src/metal_atlas.rs
+++ b/gpui-pre-apple/src/metal_atlas.rs
@@ -58,6 +58,11 @@
     fn remove(&self, key: &AtlasKey) {
         self.0.lock().remove(key);
     }
+
+    #[cfg(any(test, feature = "test-support", feature = "bench-support"))]
+    fn contains(&self, key: &AtlasKey) -> bool {
+        self.0.lock().contains(key)
+    }
 }
 
 impl AtlasBackend for MetalAtlasTextures {
diff --color -ruN gpui-pre-apple-0.3.12/src/metal_renderer.rs b/gpui-pre-apple/src/metal_renderer.rs
--- a/gpui-pre-apple/src/metal_renderer.rs
+++ b/gpui-pre-apple/src/metal_renderer.rs
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
@@ -945,6 +1021,128 @@
         command_encoder.endEncoding();
 
         Ok(command_buffer)
+    }
+
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
     }
 
     fn draw_paths_to_intermediate(
@@ -1579,12 +1777,124 @@
     color_attachment.setSourceAlphaBlendFactor(MTLBlendFactor::One);
     color_attachment.setDestinationRGBBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
     color_attachment.setDestinationAlphaBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
+
+    device
+        .newRenderPipelineStateWithDescriptor_error(&descriptor)
+        .with_context(|| format!("creating {label} render pipeline"))
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
 
+    let descriptor = MTLRenderPipelineDescriptor::new();
+    descriptor.setLabel(Some(&NSString::from_str(label)));
+    descriptor.setVertexFunction(Some(vertex_fn.as_ref()));
+    descriptor.setFragmentFunction(Some(fragment_fn.as_ref()));
+    // SAFETY: Index zero is within the pipeline's color attachment array.
+    let color_attachment = unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) };
+    color_attachment.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
+    color_attachment.setBlendingEnabled(false);
+
     device
         .newRenderPipelineStateWithDescriptor_error(&descriptor)
         .with_context(|| format!("creating {label} render pipeline"))
 }
 
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
+}
+
 fn build_path_sprite_pipeline_state(
     device: &ProtocolObject<dyn MTLDevice>,
     library: &ProtocolObject<dyn MTLLibrary>,
diff --color -ruN gpui-pre-apple-0.3.12/src/shaders.metal b/gpui-pre-apple/src/shaders.metal
--- a/gpui-pre-apple/src/shaders.metal
+++ b/gpui-pre-apple/src/shaders.metal
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
@@ -715,8 +720,15 @@
   PolychromeSprite sprite = sprites[input.sprite_id];
   constexpr sampler atlas_texture_sampler(mag_filter::linear,
                                           min_filter::linear);
-  float4 sample =
-      atlas_texture.sample(atlas_texture_sampler, input.tile_position);
+  // Nearest-neighbour: the linear sampler read at the centre of the texel
+  // the position falls in returns that texel alone.
+  float2 tile_position = input.tile_position;
+  if (sprite.nearest) {
+    float2 atlas_size =
+        float2(atlas_texture.get_width(), atlas_texture.get_height());
+    tile_position = (floor(tile_position * atlas_size) + 0.5) / atlas_size;
+  }
+  float4 sample = atlas_texture.sample(atlas_texture_sampler, tile_position);
   float distance =
       quad_sdf(input.position.xy, sprite.bounds, sprite.corner_radii);
 
@@ -1277,3 +1289,152 @@
 
   return color;
 }
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
+}
diff --color -ruN gpui-pre-apple-0.3.12/vendor/gpui/src/scene.rs b/gpui-pre-apple/vendor/gpui/src/scene.rs
--- a/gpui-pre-apple/vendor/gpui/src/scene.rs
+++ b/gpui-pre-apple/vendor/gpui/src/scene.rs
@@ -748,7 +748,10 @@
 #[expect(missing_docs)]
 pub struct PolychromeSprite {
     pub order: DrawOrder,
-    pub pad: u32,
+    /// Sampled nearest-neighbour (CSS `image-rendering: pixelated`) rather
+    /// than linearly. Takes the place of the struct's former padding word,
+    /// so the layout the shaders read is unchanged.
+    pub nearest: PaddedBool32,
     pub tile: AtlasTile,
     pub grayscale: PaddedBool32,
     pub opacity: f32,
diff --color -ruN gpui-pre-wgpu-0.3.12/src/backdrop_blur.rs b/gpui-pre-wgpu/src/backdrop_blur.rs
--- a/gpui-pre-wgpu/src/backdrop_blur.rs
+++ b/gpui-pre-wgpu/src/backdrop_blur.rs
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
--- a/gpui-pre-wgpu/src/backdrop_blur.wgsl
+++ b/gpui-pre-wgpu/src/backdrop_blur.wgsl
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
--- a/gpui-pre-wgpu/src/gpui_wgpu.rs
+++ b/gpui-pre-wgpu/src/gpui_wgpu.rs
@@ -1,3 +1,4 @@
+mod backdrop_blur;
 mod cosmic_text_system;
 mod wgpu_atlas;
 mod wgpu_context;
diff --color -ruN gpui-pre-wgpu-0.3.12/src/shaders.wgsl b/gpui-pre-wgpu/src/shaders.wgsl
--- a/gpui-pre-wgpu/src/shaders.wgsl
+++ b/gpui-pre-wgpu/src/shaders.wgsl
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
@@ -1261,7 +1266,7 @@
 
 struct PolychromeSprite {
     order: u32,
-    pad: u32,
+    nearest: u32,
     tile: AtlasTile,
     grayscale: u32,
     opacity: f32,
@@ -1293,13 +1298,18 @@
 
 @fragment
 fn fs_poly_sprite(input: PolySpriteVarying) -> @location(0) vec4<f32> {
-    let sample = textureSample(t_sprite, s_sprite, input.tile_position);
+    let sprite = load_poly_sprite(input.sprite_id);
+    // Nearest-neighbour: the linear sampler read at the centre of the texel
+    // the position falls in returns that texel alone.
+    let atlas_size = vec2<f32>(textureDimensions(t_sprite, 0));
+    let texel_centre = (floor(input.tile_position * atlas_size) + 0.5) / atlas_size;
+    let tile_position = select(input.tile_position, texel_centre, sprite.nearest != 0u);
+    let sample = textureSample(t_sprite, s_sprite, tile_position);
     // Alpha clip after using the derivatives.
     if (any(input.clip_distances < vec4<f32>(0.0))) {
         return vec4<f32>(0.0);
     }
 
-    let sprite = load_poly_sprite(input.sprite_id);
     let distance = quad_sdf(input.position.xy, sprite.bounds, sprite.corner_radii);
 
     var color = sample;
diff --color -ruN gpui-pre-wgpu-0.3.12/src/wgpu_context.rs b/gpui-pre-wgpu/src/wgpu_context.rs
--- a/gpui-pre-wgpu/src/wgpu_context.rs
+++ b/gpui-pre-wgpu/src/wgpu_context.rs
@@ -479,7 +479,14 @@
     #[cfg(not(target_family = "wasm"))]
     pub fn instance(display: Option<Box<dyn wgpu::wgt::WgpuHasDisplayHandle>>) -> wgpu::Instance {
         wgpu::Instance::new(wgpu::InstanceDescriptor {
-            backends: wgpu::Backends::VULKAN | wgpu::Backends::GL,
+            // On Windows this crate draws only off-screen (GPUI's Windows
+            // platform renders with DirectX itself), where Direct3D 12 is the
+            // backend a machine without a GPU still has, through WARP.
+            backends: if cfg!(windows) {
+                wgpu::Backends::DX12 | wgpu::Backends::VULKAN | wgpu::Backends::GL
+            } else {
+                wgpu::Backends::VULKAN | wgpu::Backends::GL
+            },
             flags: wgpu::InstanceFlags::default(),
             backend_options: wgpu::BackendOptions::default(),
             memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
diff --color -ruN gpui-pre-wgpu-0.3.12/src/wgpu_renderer.rs b/gpui-pre-wgpu/src/wgpu_renderer.rs
--- a/gpui-pre-wgpu/src/wgpu_renderer.rs
+++ b/gpui-pre-wgpu/src/wgpu_renderer.rs
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
@@ -1640,6 +1692,11 @@
                 }
             }
         }
+        if blur_frame_view.is_some()
+            && let Some(blur_renderer) = &self.resources.backdrop_blur
+        {
+            blur_renderer.present(&device, &mut encoder, target_view);
+        }
 
         let submission = self
             .resources()
diff --color -ruN gpui-pre-windows-0.3.12/src/clipboard.rs b/gpui-pre-windows/src/clipboard.rs
--- a/gpui-pre-windows/src/clipboard.rs
+++ b/gpui-pre-windows/src/clipboard.rs
@@ -275,7 +275,7 @@
 }
 
 /// DIB is BMP without the 14-byte BITMAPFILEHEADER. Prepend one.
-fn convert_dib_to_bmp(dib: &[u8]) -> Option<Vec<u8>> {
+pub(crate) fn convert_dib_to_bmp(dib: &[u8]) -> Option<Vec<u8>> {
     if dib.len() < 40 {
         return None;
     }
@@ -362,14 +362,14 @@
     }
 }
 
-struct LockedGlobal {
+pub(crate) struct LockedGlobal {
     global: HGLOBAL,
     ptr: *const u8,
     size: usize,
 }
 
 impl LockedGlobal {
-    fn lock(global: HGLOBAL) -> Option<Self> {
+    pub(crate) fn lock(global: HGLOBAL) -> Option<Self> {
         let size = unsafe { GlobalSize(global) };
         let ptr = unsafe { GlobalLock(global) };
         if ptr.is_null() {
@@ -382,7 +382,7 @@
         })
     }
 
-    fn as_bytes(&self) -> &[u8] {
+    pub(crate) fn as_bytes(&self) -> &[u8] {
         unsafe { std::slice::from_raw_parts(self.ptr, self.size) }
     }
 }
diff --color -ruN gpui-pre-windows-0.3.12/src/drag_source.rs b/gpui-pre-windows/src/drag_source.rs
--- a/gpui-pre-windows/src/drag_source.rs
+++ b/gpui-pre-windows/src/drag_source.rs
@@ -0,0 +1,182 @@
+//! Files dragged out of a window to Explorer or another application: an
+//! OLE drag of a data object that holds them as `CF_HDROP`.
+use std::mem::ManuallyDrop;
+use std::path::PathBuf;
+
+use windows::Win32::Foundation::{
+    DATA_S_SAMEFORMATETC, DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS,
+    DV_E_FORMATETC, E_NOTIMPL, E_OUTOFMEMORY, GlobalFree, OLE_E_ADVISENOTSUPPORTED, S_OK,
+};
+use windows::Win32::System::Com::{
+    DATADIR_GET, DVASPECT_CONTENT, FORMATETC, IAdviseSink, IDataObject, IDataObject_Impl,
+    IEnumFORMATETC, IEnumSTATDATA, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
+};
+use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
+use windows::Win32::System::Ole::{
+    CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE, DoDragDrop, IDropSource,
+    IDropSource_Impl,
+};
+use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
+use windows::Win32::UI::Shell::SHCreateStdEnumFmtEtc;
+use windows::core::{BOOL, HRESULT, Ref, implement};
+
+/// A `DROPFILES` header (wide names) followed by each path, nul-terminated,
+/// and a closing nul: what `CF_HDROP` holds.
+pub fn drop_files(paths: &[PathBuf]) -> Vec<u8> {
+    // `pFiles`, `pt` (two `LONG`s), `fNC`, `fWide`.
+    let mut bytes = Vec::new();
+    bytes.extend(20u32.to_le_bytes());
+    bytes.extend([0u8; 8]);
+    bytes.extend(0u32.to_le_bytes());
+    bytes.extend(1u32.to_le_bytes());
+    for path in paths {
+        for unit in path.as_os_str().to_string_lossy().encode_utf16() {
+            bytes.extend(unit.to_le_bytes());
+        }
+        bytes.extend([0, 0]);
+    }
+    bytes.extend([0, 0]);
+    bytes
+}
+
+fn file_format() -> FORMATETC {
+    FORMATETC {
+        cfFormat: CF_HDROP.0,
+        ptd: std::ptr::null_mut(),
+        dwAspect: DVASPECT_CONTENT.0,
+        lindex: -1,
+        tymed: TYMED_HGLOBAL.0 as u32,
+    }
+}
+
+fn is_file_format(format: *const FORMATETC) -> bool {
+    // SAFETY: OLE passes a valid format or null.
+    let Some(format) = (unsafe { format.as_ref() }) else {
+        return false;
+    };
+    format.cfFormat == CF_HDROP.0
+        && format.dwAspect == DVASPECT_CONTENT.0
+        && format.tymed & TYMED_HGLOBAL.0 as u32 != 0
+}
+
+#[implement(IDataObject)]
+struct FileData(Vec<u8>);
+
+#[allow(non_snake_case)]
+impl IDataObject_Impl for FileData_Impl {
+    fn GetData(&self, format: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
+        if !is_file_format(format) {
+            return Err(DV_E_FORMATETC.into());
+        }
+        // SAFETY: the memory is allocated, filled while locked and handed to
+        // the receiver, who frees it with `ReleaseStgMedium` (no release
+        // object is given).
+        unsafe {
+            let global = GlobalAlloc(GMEM_MOVEABLE, self.0.len())?;
+            let target = GlobalLock(global);
+            if target.is_null() {
+                let _ = GlobalFree(Some(global));
+                return Err(E_OUTOFMEMORY.into());
+            }
+            std::ptr::copy_nonoverlapping(self.0.as_ptr(), target.cast::<u8>(), self.0.len());
+            let _ = GlobalUnlock(global);
+            Ok(STGMEDIUM {
+                tymed: TYMED_HGLOBAL.0 as u32,
+                u: STGMEDIUM_0 { hGlobal: global },
+                pUnkForRelease: ManuallyDrop::new(None),
+            })
+        }
+    }
+
+    fn GetDataHere(
+        &self,
+        _format: *const FORMATETC,
+        _medium: *mut STGMEDIUM,
+    ) -> windows::core::Result<()> {
+        Err(E_NOTIMPL.into())
+    }
+
+    fn QueryGetData(&self, format: *const FORMATETC) -> HRESULT {
+        if is_file_format(format) {
+            S_OK
+        } else {
+            DV_E_FORMATETC
+        }
+    }
+
+    fn GetCanonicalFormatEtc(&self, _format: *const FORMATETC, out: *mut FORMATETC) -> HRESULT {
+        // SAFETY: OLE passes a writable format or null.
+        if let Some(out) = unsafe { out.as_mut() } {
+            out.ptd = std::ptr::null_mut();
+        }
+        DATA_S_SAMEFORMATETC
+    }
+
+    fn SetData(
+        &self,
+        _format: *const FORMATETC,
+        _medium: *const STGMEDIUM,
+        _release: BOOL,
+    ) -> windows::core::Result<()> {
+        Err(E_NOTIMPL.into())
+    }
+
+    fn EnumFormatEtc(&self, direction: u32) -> windows::core::Result<IEnumFORMATETC> {
+        if direction == DATADIR_GET.0 as u32 {
+            // SAFETY: one format, copied by the enumerator.
+            unsafe { SHCreateStdEnumFmtEtc(&[file_format()]) }
+        } else {
+            Err(E_NOTIMPL.into())
+        }
+    }
+
+    fn DAdvise(
+        &self,
+        _format: *const FORMATETC,
+        _flags: u32,
+        _sink: Ref<IAdviseSink>,
+    ) -> windows::core::Result<u32> {
+        Err(OLE_E_ADVISENOTSUPPORTED.into())
+    }
+
+    fn DUnadvise(&self, _connection: u32) -> windows::core::Result<()> {
+        Err(OLE_E_ADVISENOTSUPPORTED.into())
+    }
+
+    fn EnumDAdvise(&self) -> windows::core::Result<IEnumSTATDATA> {
+        Err(OLE_E_ADVISENOTSUPPORTED.into())
+    }
+}
+
+#[implement(IDropSource)]
+struct FileSource;
+
+#[allow(non_snake_case)]
+impl IDropSource_Impl for FileSource_Impl {
+    fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
+        if escape.as_bool() {
+            DRAGDROP_S_CANCEL
+        } else if keys.0 & MK_LBUTTON.0 == 0 {
+            DRAGDROP_S_DROP
+        } else {
+            S_OK
+        }
+    }
+
+    fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
+        DRAGDROP_S_USEDEFAULTCURSORS
+    }
+}
+
+/// Drags `paths` until they are dropped or the drag is cancelled, running
+/// the system's drag loop (it pumps the thread's messages), and returns
+/// what the target did with them.
+pub(crate) fn drag(paths: &[PathBuf]) -> DROPEFFECT {
+    let data: IDataObject = FileData(drop_files(paths)).into();
+    let source: IDropSource = FileSource.into();
+    let mut effect = DROPEFFECT_NONE;
+    // SAFETY: OLE is initialised on this thread (`OleInitialize` when the
+    // platform starts); both objects live for the call.
+    let _ = unsafe { DoDragDrop(&data, &source, DROPEFFECT_COPY, &mut effect) };
+    effect
+}
diff --color -ruN gpui-pre-windows-0.3.12/src/drop_data.rs b/gpui-pre-windows/src/drop_data.rs
--- a/gpui-pre-windows/src/drop_data.rs
+++ b/gpui-pre-windows/src/drop_data.rs
@@ -0,0 +1,225 @@
+//! What a drag from another application offers besides files (`CF_HDROP`):
+//! a browser's dragged image as a virtual file (`FileGroupDescriptorW` with
+//! `FileContents`) or as image data (`PNG`, `CF_DIBV5`, `CF_DIB`), its
+//! address (`UniformResourceLocatorW`), the page's HTML for it
+//! (`HTML Format`) and text.
+use std::sync::LazyLock;
+
+use gpui::{ExternalDropData, ExternalDropFile};
+use windows::Win32::Foundation::S_OK;
+use windows::Win32::System::{
+    Com::{DVASPECT_CONTENT, FORMATETC, IDataObject, TYMED_HGLOBAL, TYMED_ISTREAM},
+    DataExchange::RegisterClipboardFormatW,
+    Ole::{CF_DIB, CF_DIBV5, CF_HDROP, CF_UNICODETEXT, ReleaseStgMedium},
+};
+use windows::core::{PCWSTR, w};
+
+/// The most one in-memory file may hold; a larger one is left out.
+const FILE_LIMIT: usize = 1 << 30;
+/// `FILE_ATTRIBUTE_DIRECTORY`, and `FD_ATTRIBUTES` (the attributes field
+/// is valid).
+const DIRECTORY: u32 = 0x10;
+const FD_ATTRIBUTES: u32 = 0x4;
+/// The size of one `FILEDESCRIPTORW`.
+const DESCRIPTOR: usize = 592;
+
+fn format(name: PCWSTR) -> u16 {
+    // SAFETY: a nul-terminated wide string; 0 (failure) matches nothing.
+    unsafe { RegisterClipboardFormatW(name) as u16 }
+}
+
+static PNG: LazyLock<u16> = LazyLock::new(|| format(w!("PNG")));
+static URL_WIDE: LazyLock<u16> = LazyLock::new(|| format(w!("UniformResourceLocatorW")));
+static URL: LazyLock<u16> = LazyLock::new(|| format(w!("UniformResourceLocator")));
+static HTML: LazyLock<u16> = LazyLock::new(|| format(w!("HTML Format")));
+static FILE_GROUP: LazyLock<u16> = LazyLock::new(|| format(w!("FileGroupDescriptorW")));
+static FILE_CONTENTS: LazyLock<u16> = LazyLock::new(|| format(w!("FileContents")));
+
+fn etc(format: u16, lindex: i32, tymed: u32) -> FORMATETC {
+    FORMATETC {
+        cfFormat: format,
+        ptd: std::ptr::null_mut(),
+        dwAspect: DVASPECT_CONTENT.0,
+        lindex,
+        tymed,
+    }
+}
+
+fn offers(data: &IDataObject, format: u16) -> bool {
+    format != 0 && unsafe { data.QueryGetData(&etc(format, -1, TYMED_HGLOBAL.0 as u32)) } == S_OK
+}
+
+/// Whether a drag with no files offers anything [`read`] reads.
+pub(crate) fn offers_drop_data(data: &IDataObject) -> bool {
+    [
+        *FILE_GROUP,
+        *PNG,
+        CF_DIBV5.0,
+        CF_DIB.0,
+        *URL_WIDE,
+        *URL,
+        *HTML,
+        CF_UNICODETEXT.0,
+    ]
+    .into_iter()
+    .any(|format| offers(data, format))
+}
+
+/// The bytes of `format` (item `lindex`), from global memory or a stream.
+fn bytes(data: &IDataObject, format: u16, lindex: i32) -> Option<Vec<u8>> {
+    if format == 0 {
+        return None;
+    }
+    let wanted = etc(format, lindex, (TYMED_HGLOBAL.0 | TYMED_ISTREAM.0) as u32);
+    // SAFETY: `GetData` hands over a medium the caller releases with
+    // `ReleaseStgMedium`, which every path below does once; the union
+    // field read is the one `tymed` names.
+    unsafe {
+        let mut medium = data.GetData(&wanted).ok()?;
+        let read = if medium.tymed == TYMED_HGLOBAL.0 as u32 {
+            crate::clipboard::LockedGlobal::lock(medium.u.hGlobal)
+                .map(|locked| locked.as_bytes().to_vec())
+                .filter(|bytes| bytes.len() <= FILE_LIMIT)
+        } else if medium.tymed == TYMED_ISTREAM.0 as u32 {
+            (*medium.u.pstm).as_ref().and_then(|stream| {
+                let mut all = Vec::new();
+                let mut chunk = vec![0u8; 1 << 16];
+                loop {
+                    let mut got = 0u32;
+                    let result = stream.Read(
+                        chunk.as_mut_ptr().cast(),
+                        chunk.len() as u32,
+                        Some(&mut got as *mut u32),
+                    );
+                    if result.is_err() {
+                        return None;
+                    }
+                    if got == 0 {
+                        return Some(all);
+                    }
+                    all.extend_from_slice(&chunk[..got as usize]);
+                    if all.len() > FILE_LIMIT {
+                        return None;
+                    }
+                }
+            })
+        } else {
+            None
+        };
+        ReleaseStgMedium(&mut medium);
+        read
+    }
+}
+
+/// UTF-16 text up to its terminating nul.
+fn wide_text(bytes: &[u8]) -> String {
+    let units: Vec<u16> = bytes
+        .chunks_exact(2)
+        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
+        .take_while(|unit| *unit != 0)
+        .collect();
+    String::from_utf16_lossy(&units)
+}
+
+/// Narrow text up to its terminating nul.
+fn narrow_text(bytes: &[u8]) -> String {
+    let end = bytes
+        .iter()
+        .position(|byte| *byte == 0)
+        .unwrap_or(bytes.len());
+    String::from_utf8_lossy(&bytes[..end]).into_owned()
+}
+
+/// The names and attributes of a `FILEGROUPDESCRIPTORW`'s files, in order.
+pub fn file_group_names(group: &[u8]) -> Vec<(String, bool)> {
+    let Some(count) = group
+        .get(..4)
+        .map(|count| u32::from_le_bytes(count.try_into().unwrap()))
+    else {
+        return Vec::new();
+    };
+    (0..count as usize)
+        .map_while(|index| {
+            let descriptor = group.get(4 + index * DESCRIPTOR..4 + (index + 1) * DESCRIPTOR)?;
+            let flags = u32::from_le_bytes(descriptor[0..4].try_into().unwrap());
+            let attributes = u32::from_le_bytes(descriptor[36..40].try_into().unwrap());
+            let directory = flags & FD_ATTRIBUTES != 0 && attributes & DIRECTORY != 0;
+            Some((wide_text(&descriptor[72..592]), directory))
+        })
+        .collect()
+}
+
+/// The fragment `CF_HTML` marks (`StartFragment`/`EndFragment` byte
+/// offsets), else its HTML from `StartHTML`, else all of it.
+pub fn html_fragment(cf_html: &str) -> String {
+    let offset = |name: &str| -> Option<usize> {
+        let at = cf_html.find(name)? + name.len();
+        let digits: String = cf_html[at..]
+            .chars()
+            .take_while(char::is_ascii_digit)
+            .collect();
+        digits.parse().ok()
+    };
+    let bytes = cf_html.as_bytes();
+    let slice = |start: Option<usize>, end: Option<usize>| -> Option<String> {
+        let (start, end) = (start?, end.unwrap_or(bytes.len()));
+        let text = bytes.get(start..end.min(bytes.len()))?;
+        Some(String::from_utf8_lossy(text).into_owned())
+    };
+    slice(offset("StartFragment:"), offset("EndFragment:"))
+        .or_else(|| slice(offset("StartHTML:"), offset("EndHTML:")))
+        .unwrap_or_else(|| cf_html.to_owned())
+}
+
+/// What `data` offers besides files: its virtual files (else one image's
+/// data), its address, HTML and text. Nothing of a drag of files.
+pub(crate) fn read(data: &IDataObject) -> ExternalDropData {
+    let mut dropped = ExternalDropData::default();
+    if offers(data, CF_HDROP.0) {
+        return dropped;
+    }
+    if let Some(group) = bytes(data, *FILE_GROUP, -1) {
+        for (index, (name, directory)) in file_group_names(&group).into_iter().enumerate() {
+            if directory {
+                continue;
+            }
+            if let Some(contents) = bytes(data, *FILE_CONTENTS, index as i32) {
+                dropped.files.push(ExternalDropFile {
+                    bytes: contents,
+                    name: Some(name),
+                    media_type: None,
+                });
+            }
+        }
+    }
+    if dropped.files.is_empty() {
+        let image = bytes(data, *PNG, -1)
+            .map(|png| (png, "image/png"))
+            .or_else(|| {
+                [CF_DIBV5.0, CF_DIB.0].into_iter().find_map(|format| {
+                    let dib = bytes(data, format, -1)?;
+                    Some((crate::clipboard::convert_dib_to_bmp(&dib)?, "image/bmp"))
+                })
+            });
+        if let Some((bytes, media_type)) = image {
+            dropped.files.push(ExternalDropFile {
+                bytes,
+                name: None,
+                media_type: Some(media_type.to_owned()),
+            });
+        }
+    }
+    let address = bytes(data, *URL_WIDE, -1)
+        .map(|bytes| wide_text(&bytes))
+        .or_else(|| bytes(data, *URL, -1).map(|bytes| narrow_text(&bytes)));
+    dropped
+        .uri_lists
+        .extend(address.filter(|address| !address.is_empty()));
+    if let Some(html) = bytes(data, *HTML, -1) {
+        dropped.html.push(html_fragment(&narrow_text(&html)));
+    }
+    if let Some(text) = bytes(data, CF_UNICODETEXT.0, -1) {
+        dropped.text.push(wide_text(&text));
+    }
+    dropped
+}
diff --color -ruN gpui-pre-windows-0.3.12/src/gpui_windows.rs b/gpui-pre-windows/src/gpui_windows.rs
--- a/gpui-pre-windows/src/gpui_windows.rs
+++ b/gpui-pre-windows/src/gpui_windows.rs
@@ -10,6 +10,7 @@
 mod directx_renderer;
 mod dispatcher;
 mod display;
+mod drop_data;
 mod events;
 mod keyboard;
 mod platform;
@@ -38,6 +39,8 @@
 pub(crate) use window::*;
 pub(crate) use wrapper::*;
 
+pub use drag_source::drop_files as drag_drop_files;
+pub use drop_data::{file_group_names, html_fragment};
 pub use platform::WindowsPlatform;
 
 pub(crate) use windows::Win32::Foundation::HWND;
diff --color -ruN gpui-pre-windows-0.3.12/src/shaders.hlsl b/gpui-pre-windows/src/shaders.hlsl
--- a/gpui-pre-windows/src/shaders.hlsl
+++ b/gpui-pre-windows/src/shaders.hlsl
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
@@ -1212,7 +1217,7 @@
 
 struct PolychromeSprite {
     uint order;
-    uint pad;
+    uint nearest;
     AtlasTile tile;
     uint grayscale;
     float opacity;
@@ -1255,7 +1260,16 @@
 
 float4 polychrome_sprite_fragment(PolychromeSpriteFragmentInput input): SV_Target {
     PolychromeSprite sprite = poly_sprites[input.sprite_id];
-    float4 sample = t_sprite.Sample(s_sprite, input.tile_position);
+    // Nearest-neighbour: the linear sampler read at the centre of the texel
+    // the position falls in returns that texel alone.
+    float2 tile_position = input.tile_position;
+    if (sprite.nearest != 0u) {
+        float atlas_width, atlas_height;
+        t_sprite.GetDimensions(atlas_width, atlas_height);
+        float2 atlas_size = float2(atlas_width, atlas_height);
+        tile_position = (floor(tile_position * atlas_size) + 0.5) / atlas_size;
+    }
+    float4 sample = t_sprite.Sample(s_sprite, tile_position);
     float distance = quad_sdf(input.position.xy, sprite.bounds, sprite.corner_radii);
 
     float4 color = sample;
diff --color -ruN gpui-pre-windows-0.3.12/src/window.rs b/gpui-pre-windows/src/window.rs
--- a/gpui-pre-windows/src/window.rs
+++ b/gpui-pre-windows/src/window.rs
@@ -972,8 +972,40 @@
 
     fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) {
         self.state.callbacks.request_frame.set(Some(callback));
+    }
+
+    fn can_start_external_drag(&self) -> bool {
+        true
     }
 
+    /// An OLE drag of the files. `DoDragDrop` runs the system's drag loop
+    /// until the drop, so it is run from a task of its own after the event
+    /// that asked for it has been handled, and the drag's end is reported
+    /// as `FileDropEvent::Ended` once it returns.
+    fn start_external_drag(&self, payload: &ExternalDragPayload) -> bool {
+        let ExternalDragPayload::Files(paths) = payload;
+        let paths: Vec<PathBuf> = paths
+            .entries()
+            .iter()
+            .map(|(path, _)| path.clone())
+            .collect();
+        if paths.is_empty() {
+            return false;
+        }
+        let this = self.0.clone();
+        self.0
+            .executor
+            .spawn(async move {
+                crate::drag_source::drag(&paths);
+                if let Some(mut input) = this.state.callbacks.input.take() {
+                    input(PlatformInput::FileDrop(FileDropEvent::Ended));
+                    this.state.callbacks.input.set(Some(input));
+                }
+            })
+            .detach();
+        true
+    }
+
     fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {
         self.state.callbacks.input.set(Some(callback));
     }
@@ -1204,6 +1236,24 @@
                     paths: ExternalPaths(paths),
                 });
                 self.handle_drag_drop(input);
+            } else if crate::drop_data::offers_drop_data(idata_obj) {
+                // What a web browser drags (an image, its address): it
+                // enters with no paths and is read when it is dropped.
+                *pdweffect = DROPEFFECT_COPY;
+                let mut cursor_position = cursor_position;
+                ScreenToClient(self.0.hwnd, &mut cursor_position)
+                    .ok()
+                    .log_err();
+                let scale_factor = self.0.state.scale_factor.get();
+                let input = PlatformInput::FileDrop(FileDropEvent::Entered {
+                    position: logical_point(
+                        cursor_position.x as f32,
+                        cursor_position.y as f32,
+                        scale_factor,
+                    ),
+                    paths: ExternalPaths(SmallVec::new()),
+                });
+                self.handle_drag_drop(input);
             } else {
                 *pdweffect = DROPEFFECT_NONE;
             }
@@ -1274,6 +1324,7 @@
                 .ok()
                 .log_err();
         }
+        gpui::set_external_drop_data(crate::drop_data::read(idata_obj));
         let scale_factor = self.0.state.scale_factor.get();
         let input = PlatformInput::FileDrop(FileDropEvent::Submit {
             position: logical_point(
diff --color -ruN gpui-pre-macos-0.3.12/src/drop_data.rs b/gpui-pre-macos/src/drop_data.rs
--- a/gpui-pre-macos/src/drop_data.rs
+++ b/gpui-pre-macos/src/drop_data.rs
@@ -0,0 +1,93 @@
+//! What a drag from another application offers besides files: image data,
+//! links, HTML and text, as a web browser drags them.
+use gpui::{ExternalDropData, ExternalDropFile};
+use objc2::{msg_send, rc::Retained, runtime::AnyObject};
+use objc2_foundation::{NSArray, NSData, NSString};
+
+/// Image types read from a dragged item, in the order preferred, with the
+/// media type each is.
+const IMAGE_TYPES: &[(&str, &str)] = &[
+    ("public.png", "image/png"),
+    ("public.jpeg", "image/jpeg"),
+    ("com.compuserve.gif", "image/gif"),
+    ("org.webmproject.webp", "image/webp"),
+    ("public.tiff", "image/tiff"),
+];
+const URL: &str = "public.url";
+const HTML: &str = "public.html";
+const TEXT: &str = "public.utf8-plain-text";
+const FILE_URL: &str = "public.file-url";
+
+/// The types a window takes dragged over it besides files.
+pub(crate) fn dragged_types() -> Vec<Retained<NSString>> {
+    IMAGE_TYPES
+        .iter()
+        .map(|(kind, _)| *kind)
+        .chain([URL, HTML, TEXT])
+        .map(NSString::from_str)
+        .collect()
+}
+
+/// Whether `pasteboard` holds anything [`read_drop_pasteboard`] reads.
+pub(crate) fn offers_drop_data(pasteboard: &AnyObject) -> bool {
+    // SAFETY: NSPasteboard's `types`, an array of strings or nil.
+    let types: Vec<String> = unsafe {
+        let types: Option<Retained<NSArray<NSString>>> = msg_send![pasteboard, types];
+        types
+            .iter()
+            .flat_map(|types| types.iter())
+            .map(|kind| kind.to_string())
+            .collect()
+    };
+    dragged_types()
+        .iter()
+        .any(|wanted| types.iter().any(|kind| *kind == wanted.to_string()))
+}
+
+/// What `pasteboard` offers item by item: an item's image data (not for an
+/// item that is a file, whose path the drop already has), its link, HTML
+/// and text.
+pub fn read_drop_pasteboard(pasteboard: &AnyObject) -> ExternalDropData {
+    let mut data = ExternalDropData::default();
+    // SAFETY: NSPasteboard's and NSPasteboardItem's documented selectors,
+    // with the argument types they take; each answer is retained or nil.
+    unsafe {
+        let items: Option<Retained<NSArray<AnyObject>>> = msg_send![pasteboard, pasteboardItems];
+        for item in items.iter().flat_map(|items| items.iter()) {
+            let types: Option<Retained<NSArray<NSString>>> = msg_send![&*item, types];
+            let types: Vec<String> = types
+                .iter()
+                .flat_map(|types| types.iter())
+                .map(|kind| kind.to_string())
+                .collect();
+            let has = |kind: &str| types.iter().any(|offered| offered == kind);
+            if !has(FILE_URL)
+                && let Some((kind, media_type)) = IMAGE_TYPES.iter().find(|(kind, _)| has(kind))
+            {
+                let bytes: Option<Retained<NSData>> =
+                    msg_send![&*item, dataForType: &*NSString::from_str(kind)];
+                if let Some(bytes) = bytes {
+                    data.files.push(ExternalDropFile {
+                        bytes: bytes.to_vec(),
+                        name: None,
+                        media_type: Some((*media_type).to_owned()),
+                    });
+                }
+            }
+            let text = |kind: &str| -> Option<String> {
+                if !has(kind) {
+                    return None;
+                }
+                let value: Option<Retained<NSString>> =
+                    msg_send![&*item, stringForType: &*NSString::from_str(kind)];
+                value.map(|value| value.to_string())
+            };
+            if !has(FILE_URL) {
+                data.uri_lists.extend(text(URL));
+            }
+            data.html.extend(text(HTML));
+            data.text.extend(text(TEXT));
+        }
+    }
+    data
+}
diff --color -ruN gpui-pre-macos-0.3.12/src/gpui_macos.rs b/gpui-pre-macos/src/gpui_macos.rs
--- a/gpui-pre-macos/src/gpui_macos.rs
+++ b/gpui-pre-macos/src/gpui_macos.rs
@@ -6,6 +6,7 @@
 
 mod display;
 mod display_link;
+mod drop_data;
 mod events;
 mod keyboard;
 mod pasteboard;
@@ -42,6 +43,7 @@
 pub(crate) use platform::*;
 pub(crate) use window::*;
 
+pub use drop_data::read_drop_pasteboard;
 pub use platform::MacPlatform;
 
 trait BoolExt {
diff --color -ruN gpui-pre-macos-0.3.12/src/window.rs b/gpui-pre-macos/src/window.rs
--- a/gpui-pre-macos/src/window.rs
+++ b/gpui-pre-macos/src/window.rs
@@ -1092,10 +1092,12 @@
                 deprecated,
                 reason = "File-drop decoding expects a property-list array of paths; replacing NSFilenamesPboardType requires migrating both registration and decoding to file URLs"
             )]
+            let mut dragged_types = vec![NSFilenamesPboardType.retain()];
+            // What a web browser drags: image data, links, HTML and text.
+            dragged_types.extend(crate::drop_data::dragged_types());
             let () = msg_send![
                 native_window,
-                registerForDraggedTypes:
-                    &*NSArray::from_slice(&[NSFilenamesPboardType])
+                registerForDraggedTypes: &*NSArray::from_retained_slice(&dragged_types)
             ];
             let () = msg_send![
                 native_window,
@@ -3749,6 +3751,11 @@
 ) -> Bool {
     let window_state = unsafe { get_window_state(this) };
     let position = drag_event_position(&window_state, dragging_info);
+    let pasteboard: *mut AnyObject = unsafe { msg_send![dragging_info, draggingPasteboard] };
+    let data = unsafe { pasteboard.as_ref() }
+        .map(crate::drop_data::read_drop_pasteboard)
+        .unwrap_or_default();
+    gpui::set_external_drop_data(data);
     send_file_drop_event(window_state, FileDropEvent::Submit { position }).to_objc()
 }
 
@@ -3764,7 +3771,12 @@
     )]
     let filenames: *mut NSArray<NSString> =
         unsafe { msg_send![pasteboard, propertyListForType: NSFilenamesPboardType] };
-    let filenames = unsafe { filenames.as_ref() }?;
+    // A drag of what is not a file (an image or a link from a web page)
+    // enters with no paths; its data is read when it is dropped.
+    let Some(filenames) = (unsafe { filenames.as_ref() }) else {
+        return crate::drop_data::offers_drop_data(unsafe { &*pasteboard })
+            .then(|| ExternalPaths(paths));
+    };
     for file in filenames {
         let path = unsafe {
             let f = file.UTF8String();
diff --color -ruN gpui-pre-linux-0.3.12/src/gpui_linux.rs b/gpui-pre-linux/src/gpui_linux.rs
--- a/gpui-pre-linux/src/gpui_linux.rs
+++ b/gpui-pre-linux/src/gpui_linux.rs
@@ -1,4 +1,8 @@
 #![cfg(any(target_os = "linux", target_os = "freebsd"))]
 mod linux;
 
+#[cfg(any(feature = "wayland", feature = "x11"))]
+pub use linux::drop_data::{assemble as assemble_drop_data, wanted as wanted_drop_types};
+#[cfg(feature = "x11")]
+pub use linux::xdnd_uri_list;
 pub use linux::{current_platform, linux_platform};
diff --color -ruN gpui-pre-linux-0.3.12/src/linux.rs b/gpui-pre-linux/src/linux.rs
--- a/gpui-pre-linux/src/linux.rs
+++ b/gpui-pre-linux/src/linux.rs
@@ -1,5 +1,7 @@
 mod dispatcher;
 mod display_connection;
+#[cfg(any(feature = "wayland", feature = "x11"))]
+pub(crate) mod drop_data;
 mod headless;
 mod keyboard;
 mod platform;
@@ -26,6 +28,8 @@
 #[cfg(feature = "wayland")]
 pub(crate) use wayland::*;
 #[cfg(feature = "x11")]
+pub use x11::xdnd_uri_list;
+#[cfg(feature = "x11")]
 pub(crate) use x11::*;
 
 use std::rc::Rc;
diff --color -ruN gpui-pre-linux-0.3.12/src/linux/drop_data.rs b/gpui-pre-linux/src/linux/drop_data.rs
--- a/gpui-pre-linux/src/linux/drop_data.rs
+++ b/gpui-pre-linux/src/linux/drop_data.rs
@@ -0,0 +1,91 @@
+//! What a drag from another application offers besides files, read from an
+//! XDND selection or a Wayland data offer by media type: an image's data,
+//! its addresses, the page's HTML for it and text, as a web browser drags
+//! them.
+use gpui::{ExternalDropData, ExternalDropFile};
+
+/// Image types read, in the order preferred.
+const IMAGE_TYPES: [&str; 6] = [
+    "image/png",
+    "image/jpeg",
+    "image/gif",
+    "image/webp",
+    "image/bmp",
+    "image/tiff",
+];
+/// Addresses.
+pub(crate) const URI_LIST: &str = "text/uri-list";
+const HTML: &str = "text/html";
+/// Plain text, in the order preferred.
+const TEXT_TYPES: [&str; 3] = ["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"];
+
+/// Whether a drag offering `offered` carries anything [`wanted`] reads.
+pub(crate) fn takes(offered: &[String]) -> bool {
+    offered.iter().any(|kind| {
+        kind == URI_LIST
+            || kind == HTML
+            || IMAGE_TYPES.contains(&kind.as_str())
+            || TEXT_TYPES.contains(&kind.as_str())
+    })
+}
+
+/// The types read from a drag that brought no files: the first image type
+/// offered, the address list, HTML and the first text type offered.
+pub fn wanted(offered: &[String]) -> Vec<String> {
+    let has = |kind: &str| offered.iter().any(|offer| offer == kind);
+    let mut wanted = Vec::new();
+    wanted.extend(IMAGE_TYPES.into_iter().find(|kind| has(kind)));
+    wanted.extend([URI_LIST, HTML].into_iter().filter(|kind| has(kind)));
+    wanted.extend(TEXT_TYPES.into_iter().find(|kind| has(kind)));
+    wanted.into_iter().map(str::to_owned).collect()
+}
+
+/// Text in UTF-8, or UTF-16 when it starts with a byte order mark (Firefox
+/// sends HTML so), up to a terminating nul.
+fn text(bytes: &[u8]) -> String {
+    let text = match bytes {
+        [0xff, 0xfe, rest @ ..] => {
+            let units: Vec<u16> = rest
+                .chunks_exact(2)
+                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
+                .collect();
+            String::from_utf16_lossy(&units)
+        }
+        [0xfe, 0xff, rest @ ..] => {
+            let units: Vec<u16> = rest
+                .chunks_exact(2)
+                .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
+                .collect();
+            String::from_utf16_lossy(&units)
+        }
+        _ => String::from_utf8_lossy(bytes).into_owned(),
+    };
+    match text.find('\0') {
+        Some(end) => text[..end].to_owned(),
+        None => text,
+    }
+}
+
+/// What was read, by type, as the application takes it.
+pub fn assemble(read: Vec<(String, Vec<u8>)>) -> ExternalDropData {
+    let mut data = ExternalDropData::default();
+    for (kind, bytes) in read {
+        if bytes.is_empty() {
+            continue;
+        }
+        if IMAGE_TYPES.contains(&kind.as_str()) {
+            data.files.push(ExternalDropFile {
+                bytes,
+                name: None,
+                media_type: Some(kind),
+            });
+        } else if kind == URI_LIST {
+            data.uri_lists.push(text(&bytes));
+        } else if kind == HTML {
+            data.html.push(text(&bytes));
+        } else if TEXT_TYPES.contains(&kind.as_str()) {
+            data.text.push(text(&bytes));
+        }
+    }
+    data
+}
diff --color -ruN gpui-pre-linux-0.3.12/src/linux/wayland/client.rs b/gpui-pre-linux/src/linux/wayland/client.rs
--- a/gpui-pre-linux/src/linux/wayland/client.rs
+++ b/gpui-pre-linux/src/linux/wayland/client.rs
@@ -426,6 +426,9 @@
     window: Option<Window>,
     position: Point<Pixels>,
     uri_read_generation: u64,
+    /// What the drag offered besides files, read when it entered and handed
+    /// over when it is dropped.
+    drop_data: gpui::ExternalDropData,
 }
 
 impl<DataOffer, Window> DragState<DataOffer, Window> {
@@ -465,6 +468,7 @@
 {
     fn handle_leave(&mut self) -> Option<(Window, PlatformInput)> {
         self.invalidate_uri_read();
+        self.drop_data = Default::default();
         let window = self.window.clone()?;
         let data_offer = self.data_offer.clone()?;
         data_offer.destroy();
@@ -475,6 +479,7 @@
 
     fn handle_drop(&mut self) -> Option<(Window, PlatformInput)> {
         self.invalidate_uri_read();
+        gpui::set_external_drop_data(std::mem::take(&mut self.drop_data));
         let window = self.window.clone()?;
         let data_offer = self.data_offer.clone()?;
         data_offer.finish();
@@ -1095,6 +1100,7 @@
                 window: None,
                 position: Point::default(),
                 uri_read_generation: 0,
+                drop_data: Default::default(),
             },
             external_drag: None,
             click: ClickState {
@@ -2925,17 +2931,45 @@
                     const ACTIONS: DndAction = DndAction::Copy;
                     data_offer.set_actions(ACTIONS, ACTIONS);
 
-                    let pipe = Pipe::new().unwrap();
-                    data_offer.receive(FILE_LIST_MIME_TYPE.to_string(), unsafe {
-                        BorrowedFd::borrow_raw(pipe.write.as_raw_fd())
-                    });
-                    let fd = pipe.read;
-                    drop(pipe.write);
+                    // The file list, and what a web browser drags besides
+                    // (an image's data, its addresses, HTML, text), each
+                    // read through a pipe of its own.
+                    let offered = state
+                        .data_offers
+                        .iter()
+                        .find(|offer| offer.inner.id() == data_offer.id())
+                        .map(|offer| offer.mime_types().to_vec())
+                        .unwrap_or_default();
+                    let mut kinds = vec![FILE_LIST_MIME_TYPE.to_string()];
+                    kinds.extend(
+                        crate::linux::drop_data::wanted(&offered)
+                            .into_iter()
+                            .filter(|kind| kind != FILE_LIST_MIME_TYPE),
+                    );
+                    let mut pipes = Vec::new();
+                    for kind in kinds {
+                        let pipe = Pipe::new().unwrap();
+                        data_offer.receive(kind.clone(), unsafe {
+                            BorrowedFd::borrow_raw(pipe.write.as_raw_fd())
+                        });
+                        drop(pipe.write);
+                        pipes.push((kind, pipe.read));
+                    }
 
                     let read_task = state.common.borrow().background_executor.spawn(async {
-                        let buffer = read_fd_with_timeout(fd, PIPE_READ_TIMEOUT)?;
-                        let text = String::from_utf8(buffer)?;
-                        anyhow::Ok(text)
+                        let mut read = Vec::new();
+                        for (kind, fd) in pipes {
+                            match read_fd_with_timeout(fd, PIPE_READ_TIMEOUT) {
+                                Ok(bytes) => read.push((kind, bytes)),
+                                Err(err) => log::warn!("error reading dragged {kind}: {err:?}"),
+                            }
+                        }
+                        let text = read
+                            .iter()
+                            .find(|(kind, _)| kind == FILE_LIST_MIME_TYPE)
+                            .map(|(_, bytes)| String::from_utf8_lossy(bytes).into_owned())
+                            .unwrap_or_default();
+                        anyhow::Ok((text, read))
                     });
 
                     let this = this.clone();
@@ -2945,8 +2979,8 @@
                         .foreground_executor
                         .clone()
                         .spawn(async move {
-                            let file_list = match read_task.await {
-                                Ok(list) => list,
+                            let (file_list, read) = match read_task.await {
+                                Ok(read) => read,
                                 Err(err) => {
                                     log::error!("error reading drag and drop pipe: {err:?}");
                                     return;
@@ -2966,8 +3000,14 @@
                                 .collect();
                             let position = Point::new(x.into(), y.into());
 
-                            // Prevent dropping text from other programs.
-                            if paths.is_empty() {
+                            // What is not a file is taken only from a drag
+                            // that brings no files.
+                            let drop_data = if paths.is_empty() {
+                                crate::linux::drop_data::assemble(read)
+                            } else {
+                                gpui::ExternalDropData::default()
+                            };
+                            if paths.is_empty() && drop_data.is_empty() {
                                 data_offer.destroy();
                                 return;
                             }
@@ -2984,6 +3024,9 @@
                                 gpui::ExternalPaths(paths),
                             );
 
+                            if input.is_some() {
+                                state.drag.drop_data = drop_data;
+                            }
                             drop(state);
                             if let Some((drag_window, input)) = input {
                                 drag_window.handle_input(input);
@@ -3040,7 +3083,9 @@
 
         if let wl_data_offer::Event::Offer { mime_type } = event {
             // Drag and drop
-            if mime_type == FILE_LIST_MIME_TYPE {
+            if mime_type == FILE_LIST_MIME_TYPE
+                || crate::linux::drop_data::takes(std::slice::from_ref(&mime_type))
+            {
                 let serial = state.serial_tracker.get(SerialKind::DataDevice);
                 let mime_type = mime_type.clone();
                 data_offer.accept(serial.as_raw(), Some(mime_type));
@@ -3296,6 +3341,7 @@
             window: None,
             position: Point::default(),
             uri_read_generation: 0,
+            drop_data: Default::default(),
         }
     }
 
diff --color -ruN gpui-pre-linux-0.3.12/src/linux/wayland/clipboard.rs b/gpui-pre-linux/src/linux/wayland/clipboard.rs
--- a/gpui-pre-linux/src/linux/wayland/clipboard.rs
+++ b/gpui-pre-linux/src/linux/wayland/clipboard.rs
@@ -68,6 +68,11 @@
         }
     }
 
+    /// The types the offer was made in, as announced so far.
+    pub fn mime_types(&self) -> &[String] {
+        &self.mime_types
+    }
+
     pub fn add_mime_type(&mut self, mime_type: String) {
         self.mime_types.push(mime_type)
     }
diff --color -ruN gpui-pre-linux-0.3.12/src/linux/x11.rs b/gpui-pre-linux/src/linux/x11.rs
--- a/gpui-pre-linux/src/linux/x11.rs
+++ b/gpui-pre-linux/src/linux/x11.rs
@@ -3,6 +3,7 @@
 mod display;
 mod event;
 mod window;
+mod xdnd_source;
 mod xim_handler;
 
 pub(crate) use client::*;
@@ -10,4 +11,5 @@
 pub(crate) use display::*;
 pub(crate) use event::*;
 pub(crate) use window::*;
+pub use xdnd_source::uri_list as xdnd_uri_list;
 pub(crate) use xim_handler::*;
diff --color -ruN gpui-pre-linux-0.3.12/src/linux/x11/client.rs b/gpui-pre-linux/src/linux/x11/client.rs
--- a/gpui-pre-linux/src/linux/x11/client.rs
+++ b/gpui-pre-linux/src/linux/x11/client.rs
@@ -157,6 +157,10 @@
     drag_type: u32,
     retrieved: bool,
     position: Point<Pixels>,
+    /// The media types the source offers.
+    offered: Vec<String>,
+    /// The drag brought files, so what else it offers is not read.
+    has_paths: bool,
 }
 
 #[derive(Debug)]
@@ -239,6 +243,8 @@
     pub(crate) clipboard: Clipboard,
     pub(crate) clipboard_item: Option<ClipboardItem>,
     pub(crate) xdnd_state: Xdnd,
+    /// Files being dragged out of one of this process's windows.
+    pub(crate) xdnd_source: Option<super::xdnd_source::XdndSource>,
 }
 
 #[derive(Clone)]
@@ -554,6 +560,7 @@
                 clipboard,
                 clipboard_item: None,
                 xdnd_state: Xdnd::default(),
+                xdnd_source: None,
             })
         })));
         // From here on, `Drop` removes whatever was registered if a later step fails.
@@ -879,6 +886,52 @@
             .map(|window_reference| window_reference.window.clone())
     }
 
+    /// Starts dragging `paths` out of `window`, which takes
+    /// `XdndSelection`; a drag still under way is left first.
+    pub(crate) fn start_xdnd_drag(&self, window: xproto::Window, paths: &[PathBuf]) -> bool {
+        self.end_xdnd_drag();
+        let mut state = self.0.borrow_mut();
+        let owned = check_reply(
+            || "Failed to own XdndSelection",
+            state.xcb_connection.set_selection_owner(
+                window,
+                state.atoms.XdndSelection,
+                x11rb::CURRENT_TIME,
+            ),
+        )
+        .log_err()
+        .is_some();
+        if !owned {
+            return false;
+        }
+        state.xdnd_source = Some(super::xdnd_source::XdndSource::new(window, paths));
+        xcb_flush(&state.xcb_connection);
+        true
+    }
+
+    /// Ends the drag out, if there is one: the selection is let go and the
+    /// window it started in is told (`FileDropEvent::Ended`).
+    fn end_xdnd_drag(&self) {
+        let mut state = self.0.borrow_mut();
+        let Some(source) = state.xdnd_source.take() else {
+            return;
+        };
+        check_reply(
+            || "Failed to let XdndSelection go",
+            state.xcb_connection.set_selection_owner(
+                x11rb::NONE,
+                state.atoms.XdndSelection,
+                x11rb::CURRENT_TIME,
+            ),
+        )
+        .log_err();
+        xcb_flush(&state.xcb_connection);
+        drop(state);
+        if let Some(window) = self.get_window(source.window) {
+            window.handle_input(PlatformInput::FileDrop(FileDropEvent::Ended));
+        }
+    }
+
     fn handle_event(&self, event: Event) -> Option<()> {
         match event {
             Event::UnmapNotify(event) => {
@@ -925,8 +978,35 @@
                         })
                 }
 
+                if event.type_ == state.atoms.XdndStatus {
+                    // A target answering this process's drag out.
+                    let data = event.data.as_data32();
+                    let state = &mut *state;
+                    if let Some(source) = state.xdnd_source.as_mut() {
+                        source.status(&state.xcb_connection, &state.atoms, data);
+                    }
+                    return Some(());
+                }
+                if event.type_ == state.atoms.XdndFinished {
+                    let finished = state
+                        .xdnd_source
+                        .as_ref()
+                        .is_some_and(|source| source.is_finished_by(event.data.as_data32()));
+                    drop(state);
+                    if finished {
+                        self.end_xdnd_drag();
+                    }
+                    return Some(());
+                }
                 if event.type_ == state.atoms.XdndEnter {
                     state.xdnd_state.other_window = atom;
+                    state.xdnd_state.offered = xdnd_offered_types(
+                        &state.xcb_connection,
+                        &state.atoms,
+                        atom,
+                        arg1,
+                        [arg2, arg3, arg4],
+                    );
                     if (arg1 & 0x1) == 0x1 {
                         state.xdnd_state.drag_type = xdnd_get_supported_atom(
                             &state.xcb_connection,
@@ -956,6 +1036,28 @@
                         state.xdnd_state.position =
                             Point::new(px(pos.win_x as f32), px(pos.win_y as f32));
                     }
+                    if !state.xdnd_state.retrieved
+                        && state.xdnd_state.drag_type == 0
+                        && crate::linux::drop_data::takes(&state.xdnd_state.offered)
+                    {
+                        // Nothing to read paths from (an image alone): it
+                        // enters with none, and its data is read on the drop.
+                        state.xdnd_state.retrieved = true;
+                        let input = PlatformInput::FileDrop(FileDropEvent::Entered {
+                            position: state.xdnd_state.position,
+                            paths: gpui::ExternalPaths(SmallVec::new()),
+                        });
+                        xdnd_send_status(
+                            &state.xcb_connection,
+                            &state.atoms,
+                            event.window,
+                            state.xdnd_state.other_window,
+                            arg4,
+                        );
+                        drop(state);
+                        window.handle_input(input);
+                        return Some(());
+                    }
                     if !state.xdnd_state.retrieved {
                         check_reply(
                             || "Failed to convert selection for drag and drop",
@@ -981,6 +1083,19 @@
                     window
                         .handle_input(PlatformInput::FileDrop(FileDropEvent::Pending { position }));
                 } else if event.type_ == state.atoms.XdndDrop {
+                    // What a drag without files offers (a web browser's
+                    // image, its addresses), read while the source still
+                    // holds the selection. Never from a drag this process
+                    // makes, which could not answer while it waits here.
+                    let source = state.xdnd_state.other_window;
+                    let data = if state.xdnd_state.has_paths || state.windows.contains_key(&source)
+                    {
+                        gpui::ExternalDropData::default()
+                    } else {
+                        let kinds = crate::linux::drop_data::wanted(&state.xdnd_state.offered);
+                        crate::linux::drop_data::assemble(state.clipboard.read_xdnd(&kinds))
+                    };
+                    gpui::set_external_drop_data(data);
                     xdnd_send_finished(
                         &state.xcb_connection,
                         &state.atoms,
@@ -994,6 +1109,15 @@
                     self.0.borrow_mut().xdnd_state = Xdnd::default();
                 }
             }
+            Event::SelectionRequest(event) => {
+                // A drop target asking for the files dragged out.
+                let state = self.0.borrow();
+                if event.selection == state.atoms.XdndSelection
+                    && let Some(source) = state.xdnd_source.as_ref()
+                {
+                    source.answer(&state.xcb_connection, &state.atoms, &event);
+                }
+            }
             Event::SelectionNotify(event) => {
                 let window = self.get_window(event.requestor)?;
                 let state = self.0.borrow_mut();
@@ -1024,13 +1148,16 @@
                             }
                         })
                         .collect();
+                    let has_paths = !paths.is_empty();
                     let input = PlatformInput::FileDrop(FileDropEvent::Entered {
                         position: state.xdnd_state.position,
                         paths: gpui::ExternalPaths(paths),
                     });
                     drop(state);
                     window.handle_input(input);
-                    self.0.borrow_mut().xdnd_state.retrieved = true;
+                    let mut state = self.0.borrow_mut();
+                    state.xdnd_state.retrieved = true;
+                    state.xdnd_state.has_paths = has_paths;
                 }
             }
             Event::ConfigureNotify(event) => {
@@ -1309,6 +1436,47 @@
             }
             Event::XinputButtonRelease(event) => {
                 let window = self.get_window(event.event)?;
+                // The release of a drag out: a drop on a target that took
+                // it (the drag ends at its `XdndFinished`, or after a
+                // while), else the drag's end.
+                let dragging_out = {
+                    let state = &mut *self.0.borrow_mut();
+                    match state.xdnd_source.as_mut() {
+                        Some(source) if source.window == event.event && !source.dropped => {
+                            Some(source.release(&state.xcb_connection, &state.atoms, event.time))
+                        }
+                        _ => None,
+                    }
+                };
+                match dragging_out {
+                    Some(true) => {
+                        let this = X11ClientStatePtr(Rc::downgrade(&self.0));
+                        let window_id = event.event;
+                        self.0
+                            .borrow()
+                            .loop_handle
+                            .insert_source(
+                                calloop::timer::Timer::from_duration(Duration::from_secs(10)),
+                                move |_, (), _| {
+                                    if let Some(client) = this.get_client() {
+                                        let stale =
+                                            client.0.borrow().xdnd_source.as_ref().is_some_and(
+                                                |source| {
+                                                    source.window == window_id && source.dropped
+                                                },
+                                            );
+                                        if stale {
+                                            client.end_xdnd_drag();
+                                        }
+                                    }
+                                    calloop::timer::TimeoutAction::Drop
+                                },
+                            )
+                            .log_err();
+                    }
+                    Some(false) => self.end_xdnd_drag(),
+                    None => {}
+                }
                 let mut state = self.0.borrow_mut();
                 let modifiers = modifiers_from_xinput_info(event.mods);
                 state.modifiers = modifiers;
@@ -1334,6 +1502,26 @@
             }
             Event::XinputMotion(event) => {
                 let window = self.get_window(event.event)?;
+                {
+                    // A drag out follows the pointer outside the window.
+                    let state = &mut *self.0.borrow_mut();
+                    if let Some(source) = state.xdnd_source.as_mut()
+                        && source.window == event.event
+                    {
+                        let windows = &state.windows;
+                        source.motion(
+                            &state.xcb_connection,
+                            &state.atoms,
+                            event.root,
+                            (
+                                (event.root_x >> 16) as i16,
+                                (event.root_y >> 16) as i16,
+                                event.time,
+                            ),
+                            |window| windows.contains_key(&window),
+                        );
+                    }
+                }
                 let mut state = self.0.borrow_mut();
                 state.restore_cursor_after_hide();
                 if window.is_blocked() {
@@ -2408,6 +2596,39 @@
     0
 }
 
+/// The names of the types an XDND source offers: the three in its
+/// `XdndEnter`, or its `XdndTypeList` when it offers more.
+fn xdnd_offered_types(
+    xcb_connection: &XCBConnection,
+    atoms: &XcbAtoms,
+    source: xproto::Window,
+    flags: u32,
+    listed: [u32; 3],
+) -> Vec<String> {
+    let offered: Vec<u32> = if flags & 0x1 == 0x1 {
+        get_reply(
+            || "Failed to get XDnD type list",
+            xcb_connection.get_property(false, source, atoms.XdndTypeList, AtomEnum::ANY, 0, 1024),
+        )
+        .log_with_level(Level::Warn)
+        .and_then(|reply| reply.value32().map(|atoms| atoms.collect()))
+        .unwrap_or_default()
+    } else {
+        listed.into_iter().filter(|atom| *atom != 0).collect()
+    };
+    offered
+        .into_iter()
+        .filter_map(|atom| {
+            let reply = get_reply(
+                || "Failed to get an XDnD type's name",
+                xcb_connection.get_atom_name(atom),
+            )
+            .log_err()?;
+            String::from_utf8(reply.name).ok()
+        })
+        .collect()
+}
+
 fn xdnd_send_finished(
     xcb_connection: &XCBConnection,
     atoms: &XcbAtoms,
diff --color -ruN gpui-pre-linux-0.3.12/src/linux/x11/clipboard.rs b/gpui-pre-linux/src/linux/x11/clipboard.rs
--- a/gpui-pre-linux/src/linux/x11/clipboard.rs
+++ b/gpui-pre-linux/src/linux/x11/clipboard.rs
@@ -92,6 +92,8 @@
         // This is just some random name for the property on our window, into which
         // the clipboard owner writes the data we requested.
         ARBOARD_CLIPBOARD,
+        // The selection a drag-and-drop source offers its data through.
+        XdndSelection,
     }
 }
 
@@ -379,6 +381,16 @@
         selection: ClipboardKind,
         target_format: Atom,
     ) -> Result<ClipboardData> {
+        self.read_single_from(reader, self.atom_of(selection), target_format)
+    }
+
+    /// [`Self::read_single`] from the selection named by `selection`.
+    fn read_single_from(
+        &self,
+        reader: &XContext,
+        selection: Atom,
+        target_format: Atom,
+    ) -> Result<ClipboardData> {
         // Delete the property so that we can detect (using property notify)
         // when the selection owner receives our request.
         reader
@@ -391,7 +403,7 @@
             .conn
             .convert_selection(
                 reader.win_id,
-                self.atom_of(selection),
+                selection,
                 target_format,
                 self.atoms.ARBOARD_CLIPBOARD,
                 Time::CURRENT_TIME,
@@ -550,7 +562,7 @@
         if event.property == NONE || event.target != target_format {
             return Err(Error::ContentNotAvailable);
         }
-        if self.kind_of(event.selection).is_none() {
+        if self.kind_of(event.selection).is_none() && event.selection != self.atoms.XdndSelection {
             log::info!(
                 "Received a SelectionNotify for a selection other than CLIPBOARD, PRIMARY or SECONDARY. This is unexpected."
             );
@@ -979,6 +991,32 @@
             server_handle: join_handle,
         });
         Ok(Self { inner: ctx })
+    }
+
+    /// What a drag-and-drop source offers in each of `kinds` (media types),
+    /// read from `XdndSelection` on a connection of its own, as the
+    /// clipboard is read; a kind that cannot be read is left out.
+    pub(crate) fn read_xdnd(&self, kinds: &[String]) -> Vec<(String, Vec<u8>)> {
+        let Ok(reader) = XContext::new(&self.inner.display) else {
+            return Vec::new();
+        };
+        kinds
+            .iter()
+            .filter_map(|kind| {
+                let atom = reader
+                    .conn
+                    .intern_atom(false, kind.as_bytes())
+                    .ok()?
+                    .reply()
+                    .ok()?
+                    .atom;
+                let data = self
+                    .inner
+                    .read_single_from(&reader, self.inner.atoms.XdndSelection, atom)
+                    .ok()?;
+                Some((kind.clone(), data.bytes))
+            })
+            .collect()
     }
 
     pub(crate) fn set_text(
diff --color -ruN gpui-pre-linux-0.3.12/src/linux/x11/window.rs b/gpui-pre-linux/src/linux/x11/window.rs
--- a/gpui-pre-linux/src/linux/x11/window.rs
+++ b/gpui-pre-linux/src/linux/x11/window.rs
@@ -3,12 +3,12 @@
 
 use crate::linux::X11ClientStatePtr;
 use gpui::{
-    AnyWindowHandle, Bounds, Decorations, DevicePixels, ForegroundExecutor, GpuSpecs, Modifiers,
-    Pixels, PlatformAtlas, PlatformDisplay, PlatformInput, PlatformInputHandler, PlatformWindow,
-    Point, PromptButton, PromptLevel, RequestFrameOptions, ResizeEdge, ScaledPixels, Scene, Size,
-    Tiling, WindowAppearance, WindowBackgroundAppearance, WindowBounds, WindowControlArea,
-    WindowDecorations, WindowKind, WindowParams, WindowVisibility, popup::PopupNotSupportedError,
-    px,
+    AnyWindowHandle, Bounds, Decorations, DevicePixels, ExternalDragPayload, ForegroundExecutor,
+    GpuSpecs, Modifiers, Pixels, PlatformAtlas, PlatformDisplay, PlatformInput,
+    PlatformInputHandler, PlatformWindow, Point, PromptButton, PromptLevel, RequestFrameOptions,
+    ResizeEdge, ScaledPixels, Scene, Size, Tiling, WindowAppearance, WindowBackgroundAppearance,
+    WindowBounds, WindowControlArea, WindowDecorations, WindowKind, WindowParams, WindowVisibility,
+    popup::PopupNotSupportedError, px,
 };
 use gpui_wgpu::{CompositorGpuHint, WgpuRenderer, WgpuSurfaceConfig};
 
@@ -30,7 +30,8 @@
 };
 
 use std::{
-    cell::RefCell, ffi::c_void, fmt::Display, num::NonZeroU32, ptr::NonNull, rc::Rc, sync::Arc,
+    cell::RefCell, ffi::c_void, fmt::Display, num::NonZeroU32, path::PathBuf, ptr::NonNull, rc::Rc,
+    sync::Arc,
 };
 
 use super::{X11Display, XINPUT_ALL_DEVICE_GROUPS, XINPUT_ALL_DEVICES};
@@ -48,6 +49,8 @@
         XdndFinished,
         XdndTypeList,
         XdndActionCopy,
+        XdndProxy,
+        TARGETS,
         TextUriList: b"text/uri-list",
         UTF8_STRING,
         TEXT,
@@ -1827,8 +1830,27 @@
             ),
         )
         .log_err();
+    }
+
+    fn can_start_external_drag(&self) -> bool {
+        true
     }
 
+    /// An XDND drag of the files, from this window (`xdnd_source.rs`).
+    fn start_external_drag(&self, payload: &ExternalDragPayload) -> bool {
+        let ExternalDragPayload::Files(paths) = payload;
+        let paths: Vec<PathBuf> = paths
+            .entries()
+            .iter()
+            .map(|(path, _)| path.clone())
+            .collect();
+        if paths.is_empty() {
+            return false;
+        }
+        let client = self.0.state.borrow().client.get_client();
+        client.is_some_and(|client| client.start_xdnd_drag(self.0.x_window, &paths))
+    }
+
     fn start_window_move(&self) {
         const MOVERESIZE_MOVE: u32 = 8;
         self.send_moveresize(MOVERESIZE_MOVE).log_err();
diff --color -ruN gpui-pre-linux-0.3.12/src/linux/x11/xdnd_source.rs b/gpui-pre-linux/src/linux/x11/xdnd_source.rs
--- a/gpui-pre-linux/src/linux/x11/xdnd_source.rs
+++ b/gpui-pre-linux/src/linux/x11/xdnd_source.rs
@@ -0,0 +1,307 @@
+//! Files dragged out of a window to another application: the source side of
+//! XDND (version 3 to 5). The window that started the drag owns
+//! `XdndSelection` and, through the implicit grab of its button press, keeps
+//! receiving the pointer's motion and release wherever the pointer is. Each
+//! motion finds the XDND-aware window under the pointer and speaks to it
+//! (`XdndEnter`, `XdndPosition`, `XdndLeave`); the release drops on a target
+//! that accepted, which then asks for `text/uri-list` and answers
+//! `XdndFinished`.
+use std::path::PathBuf;
+
+use gpui_util::ResultExt as _;
+use x11rb::{
+    connection::Connection as _,
+    protocol::xproto::{
+        self, AtomEnum, ClientMessageData, ClientMessageEvent, ConnectionExt as _, EventMask,
+        PropMode, SelectionNotifyEvent, SelectionRequestEvent,
+    },
+    wrapper::ConnectionExt as _,
+    xcb_ffi::XCBConnection,
+};
+
+use super::XcbAtoms;
+
+/// The highest XDND version spoken.
+const VERSION: u32 = 5;
+
+#[derive(Clone, Copy, Debug, PartialEq, Eq)]
+struct Target {
+    /// The XDND-aware window under the pointer.
+    window: xproto::Window,
+    /// Where its messages go: the window, or the proxy it names.
+    deliver_to: xproto::Window,
+    version: u32,
+}
+
+pub(crate) struct XdndSource {
+    /// The window the drag started in, which owns `XdndSelection`.
+    pub(crate) window: xproto::Window,
+    uri_list: Vec<u8>,
+    target: Option<Target>,
+    /// A position was sent and its status has not arrived.
+    waiting: bool,
+    /// The latest position while one is waiting.
+    pending: Option<(i16, i16, u32)>,
+    accepted: bool,
+    /// The drop was sent; the target's `XdndFinished` ends the drag.
+    pub(crate) dropped: bool,
+}
+
+/// `text/uri-list` for `paths`: one `file:` address per line, each line
+/// ended by CR LF.
+pub fn uri_list(paths: &[PathBuf]) -> Vec<u8> {
+    let mut list = String::new();
+    for path in paths {
+        if let Ok(url) = url::Url::from_file_path(path) {
+            list.push_str(url.as_str());
+            list.push_str("\r\n");
+        }
+    }
+    list.into_bytes()
+}
+
+fn send(connection: &XCBConnection, target: Target, kind: xproto::Atom, data: [u32; 5]) {
+    let message = ClientMessageEvent {
+        format: 32,
+        window: target.window,
+        type_: kind,
+        data: ClientMessageData::from(data),
+        sequence: 0,
+        response_type: xproto::CLIENT_MESSAGE_EVENT,
+    };
+    connection
+        .send_event(false, target.deliver_to, EventMask::NO_EVENT, message)
+        .log_err();
+}
+
+fn property32(
+    connection: &XCBConnection,
+    window: xproto::Window,
+    property: xproto::Atom,
+    kind: impl Into<xproto::Atom>,
+) -> Option<u32> {
+    connection
+        .get_property(false, window, property, kind, 0, 1)
+        .ok()?
+        .reply()
+        .ok()?
+        .value32()?
+        .next()
+}
+
+/// The XDND-aware window at root point (`x`, `y`): the first window, going
+/// down from the root's child under the point, that has `XdndAware`, with
+/// the proxy it names when that proxy names itself.
+fn find_target(
+    connection: &XCBConnection,
+    atoms: &XcbAtoms,
+    root: xproto::Window,
+    x: i16,
+    y: i16,
+) -> Option<Target> {
+    let mut parent = root;
+    for _ in 0..16 {
+        let child = connection
+            .translate_coordinates(root, parent, x, y)
+            .ok()?
+            .reply()
+            .ok()?
+            .child;
+        if child == x11rb::NONE {
+            return None;
+        }
+        if let Some(version) = property32(connection, child, atoms.XdndAware, AtomEnum::ATOM) {
+            if version < 3 {
+                return None;
+            }
+            let deliver_to = property32(connection, child, atoms.XdndProxy, AtomEnum::WINDOW)
+                .filter(|proxy| {
+                    property32(connection, *proxy, atoms.XdndProxy, AtomEnum::WINDOW)
+                        == Some(*proxy)
+                })
+                .unwrap_or(child);
+            return Some(Target {
+                window: child,
+                deliver_to,
+                version: version.min(VERSION),
+            });
+        }
+        parent = child;
+    }
+    None
+}
+
+impl XdndSource {
+    pub(crate) fn new(window: xproto::Window, paths: &[PathBuf]) -> Self {
+        Self {
+            window,
+            uri_list: uri_list(paths),
+            target: None,
+            waiting: false,
+            pending: None,
+            accepted: false,
+            dropped: false,
+        }
+    }
+
+    fn position(&mut self, connection: &XCBConnection, atoms: &XcbAtoms, at: (i16, i16, u32)) {
+        let Some(target) = self.target else {
+            return;
+        };
+        let (x, y, time) = at;
+        let point = ((x as u16 as u32) << 16) | y as u16 as u32;
+        send(
+            connection,
+            target,
+            atoms.XdndPosition,
+            [self.window, 0, point, time, atoms.XdndActionCopy],
+        );
+        self.waiting = true;
+    }
+
+    /// The pointer moved to root point (`x`, `y`). A window `own` says is
+    /// this process's is not a target: the app takes its own items back
+    /// itself.
+    pub(crate) fn motion(
+        &mut self,
+        connection: &XCBConnection,
+        atoms: &XcbAtoms,
+        root: xproto::Window,
+        at: (i16, i16, u32),
+        own: impl Fn(xproto::Window) -> bool,
+    ) {
+        if self.dropped {
+            return;
+        }
+        let target =
+            find_target(connection, atoms, root, at.0, at.1).filter(|target| !own(target.window));
+        if target.map(|target| target.window) != self.target.map(|target| target.window) {
+            if let Some(old) = self.target {
+                send(connection, old, atoms.XdndLeave, [self.window, 0, 0, 0, 0]);
+            }
+            self.target = target;
+            self.accepted = false;
+            self.waiting = false;
+            self.pending = None;
+            if let Some(new) = target {
+                send(
+                    connection,
+                    new,
+                    atoms.XdndEnter,
+                    [self.window, new.version << 24, atoms.TextUriList, 0, 0],
+                );
+            }
+        }
+        if self.target.is_some() {
+            if self.waiting {
+                self.pending = Some(at);
+            } else {
+                self.position(connection, atoms, at);
+            }
+        }
+        connection.flush().log_err();
+    }
+
+    /// The target's `XdndStatus`: whether it would take the drop.
+    pub(crate) fn status(&mut self, connection: &XCBConnection, atoms: &XcbAtoms, data: [u32; 5]) {
+        if self.target.is_none_or(|target| target.window != data[0]) {
+            return;
+        }
+        self.accepted = data[1] & 1 == 1;
+        self.waiting = false;
+        if let Some(at) = self.pending.take() {
+            self.position(connection, atoms, at);
+            connection.flush().log_err();
+        }
+    }
+
+    /// The button was let go: a target that accepted gets the drop (and
+    /// `true` is returned until its `XdndFinished`); any other is left.
+    pub(crate) fn release(
+        &mut self,
+        connection: &XCBConnection,
+        atoms: &XcbAtoms,
+        time: u32,
+    ) -> bool {
+        let dropped = match self.target {
+            Some(target) if self.accepted => {
+                send(
+                    connection,
+                    target,
+                    atoms.XdndDrop,
+                    [self.window, 0, time, 0, 0],
+                );
+                true
+            }
+            Some(target) => {
+                send(
+                    connection,
+                    target,
+                    atoms.XdndLeave,
+                    [self.window, 0, 0, 0, 0],
+                );
+                false
+            }
+            None => false,
+        };
+        self.dropped = dropped;
+        connection.flush().log_err();
+        dropped
+    }
+
+    /// Whether `data` is the `XdndFinished` of the target dropped on.
+    pub(crate) fn is_finished_by(&self, data: [u32; 5]) -> bool {
+        self.dropped && self.target.is_some_and(|target| target.window == data[0])
+    }
+
+    /// Answers a request for `XdndSelection`: its targets, or the files as
+    /// `text/uri-list`; anything else is refused.
+    pub(crate) fn answer(
+        &self,
+        connection: &XCBConnection,
+        atoms: &XcbAtoms,
+        request: &SelectionRequestEvent,
+    ) {
+        let property = if request.property == x11rb::NONE {
+            request.target
+        } else {
+            request.property
+        };
+        let written = if request.target == atoms.TARGETS {
+            connection
+                .change_property32(
+                    PropMode::REPLACE,
+                    request.requestor,
+                    property,
+                    AtomEnum::ATOM,
+                    &[atoms.TARGETS, atoms.TextUriList],
+                )
+                .is_ok()
+        } else if request.target == atoms.TextUriList {
+            connection
+                .change_property8(
+                    PropMode::REPLACE,
+                    request.requestor,
+                    property,
+                    atoms.TextUriList,
+                    &self.uri_list,
+                )
+                .is_ok()
+        } else {
+            false
+        };
+        let notify = SelectionNotifyEvent {
+            response_type: xproto::SELECTION_NOTIFY_EVENT,
+            sequence: 0,
+            time: request.time,
+            requestor: request.requestor,
+            selection: request.selection,
+            target: request.target,
+            property: if written { property } else { x11rb::NONE },
+        };
+        connection
+            .send_event(false, request.requestor, EventMask::NO_EVENT, notify)
+            .log_err();
+        connection.flush().log_err();
+    }
+}
```
