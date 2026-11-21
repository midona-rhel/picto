# Patched GPUI crates

The vendored `gpui-pre` crate does not contain the `examples/` folder of the
published package (4.9 MB of sample programs and images), and its manifest
does not contain the `[[example]]` entries that name those examples. Nothing
else in it differs from 0.3.12 apart from the changes described here.

The six folders next to this file hold the crates.io sources of `gpui-pre`,
`gpui-pre-apple`, `gpui-pre-wgpu`, `gpui-pre-windows`, `gpui-pre-macos` and
`gpui-pre-linux`, version 0.3.12. That version is a snapshot of
zed-industries/zed at d2a4fb6. The licence is Apache-2.0, and each crate
keeps its `LICENSE-APACHE` file. This project added the following to them:

- a backdrop-blur primitive;
- nearest-neighbour image sampling;
- an interface zoom;
- the reading of what a drop from another application carries besides
  files;
- a way to ask whether the system blurs what is behind a window;
- Acrylic on Windows 11, set through the system's documented window
  attribute.

The section "What changed, and why" describes these and the smaller changes
that this list leaves out. `../Cargo.toml` substitutes the six folders for
the published crates with `[patch.crates-io]`. Every other GPUI crate still
comes from crates.io. Everything else in the six crates is byte-for-byte the
published package, so the diff at the end of this file is the whole change.

The design follows dux-web's port of backdrop materials to gpui-pre 0.3.4
(github.com/dux-web/gpui, commit b9b7df7, from crabtalk/zed's `bezel-gpui`;
proposed upstream in longbridge/gpui-component#3598). Two parts of that
design are used here. The first is how the scene stores blurs: it keeps them
in a list of their own, outside the batches of primitives, sorted by draw
order, and the batch iterator ends a batch at each blur. A renderer can
therefore stop at a blur, read what it has drawn so far, and then go on. The
second is a drop shadow that has the shape of its element cut out of it. The
blur itself is written anew: dux-web uses Metal Performance Shaders on
Metal, and the blur here uses shaders of its own. The blur here also leaves
out what this app does not need: lens, dispersion, tint and rim light.

## What changed, and why

`gpui-pre`
- `src/scene.rs`: This file has these changes.
  - `BackdropBlur` is a new struct. It describes one region in which
    everything drawn beneath it so far is shown blurred. It holds the
    rounded rectangle, the clip, the standard deviation of the gaussian, the
    factors of CSS `saturate()` and `brightness()`, and an opacity.
  - `Scene::backdrop_blurs` is the list of the blurs of a frame. It is kept
    in draw order. Each blur is also recorded with the cached paint
    operations, as `PaintOperation::BackdropBlur`, so that it is inserted
    again when those operations are replayed.
  - `Scene::insert_backdrop_blur` gives a blur the next draw order over its
    bounds, in the same way that a primitive gets its draw order.
  - `BatchIterator` ends a batch before the next blur. `Scene::batch_order`
    returns the draw order of the first primitive of a batch, so that a
    renderer can tell which blurs come before the batch.
  - `BackdropBlur::plan` and `BackdropBlurKernel` are the arithmetic that
    the Metal renderer and the wgpu renderer share. They work out three
    things. The first is which pixels the blur covers and which pixels it
    reads. The second is the downsample factor: it is 1, or an even number
    chosen so that a gaussian of about three reduced texels remains. The
    third is a separable gaussian whose taps are sampled linearly: each tap
    is placed between two texels, so that one linear sample reads both
    texels at their weights.
  - The documentation of `Shadow::inset` now says that the value 2 means a
    drop shadow with its element cut out.
- `src/window.rs`: This file has four additions.
  - `BackdropFilter` is a new struct. It holds the blur radius, the
    saturation, the brightness and an opacity.
    `Window::paint_backdrop_blur` takes one and inserts a blur into the
    scene at the element opacity, so a panel that fades also fades its
    blur.
  - `Window::paint_drop_shadows_outside` paints drop shadows with
    `inset: 2`. A shadow painted this way is not drawn where its element
    is, so a translucent panel does not show its own shadow through its
    fill. It can also be painted after the panel's backdrop blur, so the
    blur does not blur the panel's own shadow into the panel. CSS behaves
    the same way: it never draws a `box-shadow` under its element.
  - `Window::is_dirty` (test support only) answers whether the window has
    been invalidated since it was last drawn. With it, the off-screen
    checks can draw only the frames that a real window would draw.
  - `Window::start_platform_drag` hands files to the platform's native drag
    session. It calls `start_external_drag`, which the macOS and Wayland
    windows of the published crates already have. It is for a view that
    follows the pointer itself. The app uses it when the grid's item drag
    leaves the window. Without it, GPUI starts a native drag only for drags
    that were started with `on_drag` and `external_drag_payload`.
- Interface zoom. The app's Cmd+= / Cmd+− / Cmd+0 change it, and it works
  like the page zoom of a browser.
  - `App::set_ui_zoom` and `App::ui_zoom` (`src/app.rs`) hold one zoom for
    every window.
  - A window (`src/window.rs`) takes the zoom when it opens. When the zoom
    changes, an open window takes the new value at the start of its next
    frame (`Window::apply_ui_zoom`). The window's `scale_factor` becomes
    the display's scale factor multiplied by the zoom, and its
    `viewport_size` becomes the platform's content size divided by the
    zoom. This has four effects. Layout, hit-testing and the scene stay in
    one space of points. Text and paths are rasterized at the zoomed size,
    and the sprite atlas keys glyphs by that scale factor. Blur radii and
    corner radii scale with them. The renderers, which draw the scene in
    device pixels, are unchanged. `Window::ui_zoom` returns the zoom that
    the window is laid out at.
  - Input from the platform goes through the new
    `Window::dispatch_platform_input`. It divides every position, and the
    distance of a precise scroll, by the zoom, and then calls
    `dispatch_event`. The platform's input callback calls it. The app's
    playback of input (`src/pointer.rs` in the app, `experiments/gpui`)
    calls it too.
  - What the window gives back to the platform is multiplied by the zoom.
    This applies to the rectangles for the input method
    (`PlatformInputHandler::bounds_for_range`, `selection_bounds_for_range`
    and `element_bounds` in `src/platform.rs`, and the candidate position
    that `Window` sends) and to what the window passes to
    `show_window_menu`, `set_input_region`, `set_client_inset` and
    `set_client_corner_radii`.
  - Two values that come from the platform are divided by the zoom: the
    point that the input method passes to
    `PlatformInputHandler::character_index_for_point` (`src/platform.rs`),
    and what `visual_viewport_bounds` returns.
  - The frame of the window stays in screen points: `Window::bounds`,
    `window_bounds`, the minimum size and the position of the traffic
    lights are not converted.
  - At zoom 1 every conversion returns its input unchanged, so nothing is
    drawn or hit-tested differently.
  - `Window::insets` and `Window::resize` are not converted either. They
    use the platform's points. `Window::insets` reports safe areas, which
    the desktop platforms do not report.
- `src/platform/test/window.rs`: The test window now keeps the decorations
  that the window asked for, and reports them. When the client's
  decorations were asked for, it reports `Decorations::Client` with nothing
  tiled. A Linux desktop that grants the request reports the same. The
  off-screen checks therefore draw the chrome that a frameless window draws
  itself (its buttons and resize edges), as it is drawn on Linux.
- Nearest-neighbour images. This is CSS `image-rendering: pixelated`. The
  app uses it for the Preview setting "Scaling: Pixelated".
  - `src/scene.rs`: In `PolychromeSprite`, the padding word after `order`
    becomes the field `nearest`. It is a `PaddedBool32`, so the layout that
    every shader reads is unchanged.
  - `src/window.rs`: `Window::paint_image_sampled` takes the choice between
    linear and nearest-neighbour sampling. `Window::paint_image` calls it
    with `false`.
  - `src/elements/img.rs`: `ImageStyle::pixelated` and
    `StyledImage::pixelated` pass the choice on from the `img` element.
  - In each renderer, the fragment shader for these sprites then reads its
    linear sampler at the centre of the texel that the position falls in,
    when `nearest` is set. A read at the centre of a texel returns that
    texel alone. No sampler, binding or pipeline is added.
- `src/elements/img.rs`: These changes are to when an animated image (GIF,
  animated WebP) plays and to how often its window is redrawn.
  - An animated image now plays whenever its window draws, as a browser
    plays it. Before, it played only while its window was the active one.
    A detail window beside the main window, or the main window while
    another application was in front, kept its GIFs on one frame. A covered
    or minimised window still draws nothing.
  - The view that the image is in is now drawn again when the frame on
    screen has been shown for its delay. A timer that the element keeps in
    its state does this. Before, the view was drawn again on every frame
    that the display can show, which redrew the whole window at the
    display's rate for as long as an animated image was on screen.
  - A frame with a delay of 10 ms or less is shown for 100 ms, as browsers
    show it (`shown_for`). Before, the image advanced by one such frame for
    each frame that was drawn.
- `src/elements/img.rs`: An image element takes its picture's shape only
  when the caller leaves its width or its height open. Before, the element
  always gave the layout the picture's width-to-height ratio. For an element
  with both a width and a height (for example `size_full`), the layout then
  replaced the given height with one worked out from the width. A tall
  picture that fills a square box (`ObjectFit::Cover`) was therefore laid out
  taller than the box, starting at the box's top, so the box showed the top
  part of the picture and not its middle, and the rest spilled out below.
  The grid's Fit Thumbnails and the Set cover photo dialog showed this.
- Accessibility. 0.3.12 already builds an AccessKit tree from every element
  that has an id and a role. It hands the tree to each platform's adapter
  (macOS NSAccessibility, Windows UI Automation, Linux AT-SPI) only while
  assistive technology is attached, and it routes the actions that come
  from the adapter back to the elements. The following were added because
  the app needs them and they cannot be set from outside the crate.
  - `src/elements/div.rs`: This file has four additions.
    - The states `aria_disabled`, `aria_read_only`, `aria_modal`,
      `aria_busy` and `aria_multiselectable`, and a live region
      (`aria_live`). They are written to the node in the same way as the
      published `aria_*` properties. A disabled node does not offer the
      `Click` action.
    - `StatefulInteractiveElement::a11y_focus(&FocusHandle)` makes the node
      of an element stand for a focus handle that a descendant without a
      node tracks. The node is reported as focused while the handle is
      focused, and the `Focus` action of the node focuses the handle. The
      app needs this because its controls keep their tab stop in a child of
      no size, so that a click leaves the keyboard focus where it was.
    - `a11y_presentational()` marks an element whose click is not a
      control, such as the backdrop of a dialog or a drag strip.
    - While the tree is built, every element that takes a click but has no
      role is recorded. An element marked with `a11y_presentational()` is
      left out of this record.
  - `src/window/a11y.rs`: The root node carries the interface zoom as a
    scale transform. The transform is needed because the bounds of the
    nodes are in the window's zoomed points.
  - `src/window.rs`, `src/window/a11y/debug.rs` (test support only):
    `Window::set_a11y_active_for_test`, `last_a11y_tree_update`,
    `dispatch_a11y_action_for_test` and `a11y_unexposed_clickables` are
    new. With them, tests and the off-screen tool can build the tree
    without a screen reader, read it and act on it.

`gpui-pre-apple` (Metal)
- `src/shaders.metal`: This file has the shadow change and the blur shaders.
  - The shadow shader accepts `inset == 2`. For this value it uses the
    geometry of a drop shadow, and it multiplies the coverage of the shadow
    by the coverage of the area outside the element.
  - The inset branch of the vertex shader and the inset branch of the
    fragment shader are now taken for `inset == 1` only. Upstream tests
    `inset != 0`. With that test a shadow with `inset == 2` was drawn as an
    inset shadow, which left every glass panel without its outer shadow.
    The WGSL and the HLSL have the same change.
  - There are three new blur shaders. The downsample shader writes each
    reduced texel as the average of a box of frame pixels, which it reads
    with bilinear taps. The gaussian shader blurs in one direction, and its
    reads are clamped to the reduced region. The composite shader draws the
    blurred backdrop at the coverage of the rounded rectangle, applies the
    clip, the saturation and the brightness, and writes premultiplied
    output.
- `src/metal_renderer.rs`: This file draws the blurs.
  - It has three new pipelines and two scratch textures. The textures are
    made on the first blur and grown in steps of 128 texels.
  - The frame loop draws the blurs between the batches. Before each batch,
    for every blur whose draw order is at or before the order of the batch,
    the renderer ends the render pass, runs the downsample pass, the
    gaussian pass across, the gaussian pass down and the composite pass,
    and then starts a render pass that loads what the frame already holds.
  - A drawable from a framebuffer-only layer cannot be read. The renderer
    therefore makes the layer of the window readable the first time a frame
    has a blur, and the layer stays readable after that. A frame whose
    drawable is still framebuffer-only skips its blurs.
- `src/metal_atlas.rs`: The Metal atlas now implements
  `PlatformAtlas::contains` (test support only). With it,
  `Window::has_image_atlas_entry` answers on Metal as it does headless, and
  the off-screen checks can tell whether a released picture has left the
  atlas.
- `src/shaders.metal`, `polychrome_sprite_fragment`: This shader does the
  nearest-neighbour read. `vendor/gpui/src/scene.rs` is the copy of
  `scene.rs` that the Metal header is generated from, and it has the same
  `nearest` field.

`gpui-pre-wgpu` (Linux; also the web)
- `src/backdrop_blur.rs`, `src/backdrop_blur.wgsl`: These new files run the
  same passes as the Metal renderer. A surface texture can only be drawn
  to; it cannot be read. A frame with blurs is therefore drawn into a
  texture that this module owns, and is copied to the surface at the end. A
  frame without blurs is drawn as before.
- `src/wgpu_renderer.rs`: The renderer holds the blur renderer with its
  other device resources, so the blur renderer is dropped with them when
  the device is lost. `record_frame` draws the blurs between the batches as
  the Metal renderer does. `src/gpui_wgpu.rs` declares the module.
- `src/shaders.wgsl`: The shadow shader accepts `inset == 2`. The
  `PolychromeSprite` struct names its second word `nearest`, and
  `fs_poly_sprite` does the nearest-neighbour read. It chooses the position
  with `select`, so the texture sample stays in uniform control flow.
- `src/wgpu_context.rs`: On Windows the instance also offers Direct3D 12.
  On Windows this crate is used only by the off-screen checks, because
  GPUI's Windows platform draws with DirectX itself. A machine without a
  GPU, such as a CI runner, has Direct3D 12 through the WARP software
  adapter, but it has neither Vulkan nor a usable OpenGL.

`gpui-pre-windows` (DirectX)
- `src/shaders.hlsl`: The shadow shader accepts `inset == 2`. The
  `PolychromeSprite` struct names its second word `nearest`, and
  `polychrome_sprite_fragment` does the nearest-neighbour read. It gets the
  size of the atlas from `GetDimensions`. The DirectX renderer ignores
  `Scene::backdrop_blurs`, so on Windows a glass panel shows its
  translucent fill over a backdrop that is not blurred.

Drops from other applications. GPUI hands an application only the paths of
dropped files. What a web browser drags (the data of an image, its address,
the page's HTML for it) therefore never arrived. Files that a drag only
promises did not arrive either: macOS's file promises, Windows' virtual
files and X11's direct save. Each platform now reads the data while the
drop is delivered, and has the promised files written into a folder that is
made for the drop. In the lists below, a drag "enters with no paths" when
the window receives `FileDropEvent::Entered` with an empty list of paths.

`gpui-pre`
- `src/interactive.rs`: This file has the types for the data of a drop and
  the functions that hand the data over.
  - `ExternalDropData` holds what a drop carries besides files on disk:
    in-memory files (`ExternalDropFile`, with bytes, name and media type),
    address lists, HTML, text and Chromium `DownloadURL`s.
  - The data is handed over in two steps. A platform calls
    `set_external_drop_data` just before `FileDropEvent::Submit`. The
    application calls `take_external_drop_data` while it handles that
    drop.
  - A drag that carries only such data enters with no paths.
  - `ExternalDropData::promised` holds `PromisedFiles`. That struct has the
    folder that was made for the drop, the number of files that were
    promised, and a stream (`arrival`, which can be taken once). On the
    stream the platform reports each file once: either its path, or why it
    was not written. The stream ends when the platform drops its sender.
  - `set_promised_files_directory` names the folder in which the folder of
    each drop is made. `promised_files_directory` returns it. Until it is
    set, it is the system's temporary folder.

`gpui-pre-macos` (AppKit)
- `src/drop_data.rs`: This new file has the pasteboard types that a window
  registers besides `NSFilenamesPboardType`: `public.png`, `public.jpeg`,
  `com.compuserve.gif`, `org.webmproject.webp`, `public.tiff`,
  `public.url`, `public.html` and `public.utf8-plain-text`. It also has
  `read_drop_pasteboard`. For each pasteboard item, that function reads the
  data of the first of those image types that the item has, its link, its
  HTML and its text. It reads no image data and no link from an item that
  is a file, because the drop already has the path of that file. The
  function is public so that the application's test can read a pasteboard
  of its own.
- `src/drop_data.rs` also: Three functions are for promised files.
  `promise_types` returns the `readableDraggedTypes` of
  `NSFilePromiseReceiver`. `offers_promised_files` answers whether a
  pasteboard holds a file promise. `receive_promised_files` reads the
  `NSFilePromiseReceiver`s of the pasteboard and asks each one to deliver
  into a new folder in `promised_files_directory`. The delivery runs on an
  `NSOperationQueue` of its own. The reader block reports each file through
  `PromisedFiles`; there is one file for each promised type. AppKit allows
  this request only during the drop.
- `src/window.rs`: The window registers those types and the promise types.
  A drag that has no file names and offers one of them enters with no
  paths. `performDragOperation:` reads the drag pasteboard. For a drag
  without file names that holds a promise, it also asks for the promised
  files. It then hands both over before `Submit`.
- `src/gpui_macos.rs`: This file declares the module and exports
  `read_drop_pasteboard`.

`gpui-pre-windows` (OLE drop target)
- `src/drop_data.rs`: This new file answers whether a data object offers
  anything that is read, and it does the reading. Both are for a drag
  without `CF_HDROP`.
  - It reads virtual files (`FileGroupDescriptorW`) and skips folders. Each
    file is written into a new folder in `promised_files_directory`. The
    contents come from the `FileContents` of the file, either in global
    memory or as a stream. Contents from global memory are cut to the size
    that the descriptor gives.
  - A virtual file is written under the last part of its name, without the
    characters that a Windows name cannot hold. A device name gets the
    prefix `_`. When the name is taken, the file is written as
    `name (2).ext`.
  - A file that is shorter than its descriptor says, or whose writing fails
    part-way, is removed. Each file is reported through `PromisedFiles`.
  - It reads an image from `PNG`. If there is no `PNG`, it reads `CF_DIBV5`
    or `CF_DIB` and makes a BMP file from it.
  - It reads an address from `UniformResourceLocatorW`, or else from
    `UniformResourceLocator`. It reads the fragment of `HTML Format`, and
    it reads `CF_UNICODETEXT`.
  - `html_fragment` and `read_drop_data` are public for the application's
    test. So are `virtual_files_object` and `VirtualFile`, which make a
    data object that holds virtual files as Outlook and browsers drag
    them.
- `src/window.rs`: `DragEnter` takes such a drag, and the drag enters with
  no paths. `Drop` reads the data object and hands the data over before
  `Submit`.
- `src/clipboard.rs`: `convert_dib_to_bmp` and `LockedGlobal` are now
  `pub(crate)`, so that the drop reading can use them too.
- `src/gpui_windows.rs`: This file declares the module and exports
  `html_fragment`, `read_drop_data`, `virtual_files_object` and
  `VirtualFile`.

`gpui-pre-linux` (X11 and Wayland)
- `src/linux/drop_data.rs`: This new file has three functions.
  - `takes` answers whether a drag is taken, given the media types that it
    offers.
  - `wanted` lists the types that are read from a drag without files: the
    first of `image/png`, `image/jpeg`, `image/gif`, `image/webp`,
    `image/bmp` and `image/tiff`; `text/uri-list`; `text/html`; and the
    first of `text/plain;charset=utf-8`, `UTF8_STRING` and `text/plain`.
  - `assemble` builds the drop data from what was read. Text is read as
    UTF-8, or as UTF-16 when it starts with a byte order mark, which is how
    Firefox sends HTML.
  - `wanted` and `assemble` are exported for the application's test.
- `src/linux/x11/client.rs`: This file changes three steps of a drag.
  - On `XdndEnter`, the client notes the names of the offered types. They
    are the three types in the message, or the `XdndTypeList` of the source
    when it offers more.
  - A drag that offers no supported text type, but offers a type that
    `takes` accepts (for example an image), enters with no paths at its
    first position.
  - On `XdndDrop`, before `XdndFinished`, the client reads the data of a
    drag that brought no paths and that no window of this process started,
    and hands the data over.
- `src/linux/x11/clipboard.rs`: `Clipboard::read_xdnd` reads the given
  types from `XdndSelection`. It uses a connection of its own and the
  reader of the clipboard, which includes `INCR` transfers. To allow this,
  `read_single` now calls the new `read_single_from`, which takes the
  selection as an atom, and a `SelectionNotify` for `XdndSelection` is
  accepted.
- `src/linux/wayland/client.rs`: The types of an offer that `takes` accepts
  are accepted. On `Enter`, the file list and the wanted types are
  received, each through a pipe of its own, and are read off the main
  thread. A drag without paths whose data is not empty enters with no
  paths. Its data is kept in `DragState::drop_data`, handed over on `Drop`
  and cleared on `Leave`.
- `src/linux/wayland/clipboard.rs`: `DataOffer::mime_types` is new. It
  returns the types that the offer has announced.
- `src/linux/x11/direct_save.rs`: This new file is the receiving side of
  XDS, the Direct Save protocol (`XdndDirectSave0`). It runs on a thread
  and a connection of its own, in these steps.
  - It reads the name that the source suggests from the `XdndDirectSave0`
    property of the source's window. It uses that name for a file in a new
    folder in `promised_files_directory`.
  - It puts the `file://host/path` address of the file in that property.
  - It asks for the `XdndDirectSave0` target of `XdndSelection`, on a
    window that it creates on its own connection.
  - It waits for the answer of the source, for two minutes at most. If the
    answer is `S` and the file is there, the file is the result. After any
    other answer it removes what is at that path.
  - It sends `XdndFinished` to the source on behalf of the target, and
    reports the file through `PromisedFiles`.
  - `receive_direct_save` and `DirectSaveSource` are exported.
    `DirectSaveSource` is a source that answers one request, for the
    application's test.
- `src/linux/x11/client.rs`: On `XdndDrop`, a drag from another process
  that has no paths and offers `XdndDirectSave0` gets the direct save. The
  direct save sends `XdndFinished` itself. `src/linux/drop_data.rs`:
  `takes` accepts that type, so such a drag enters with no paths.
  `src/linux/x11/clipboard.rs`: `Clipboard::display` is new. It returns the
  X display that the direct save connects to.
- `src/linux.rs`, `src/gpui_linux.rs`, `src/linux/x11.rs`: These files
  declare and export the modules.

Dragging files out on Windows and X11. `Window::start_platform_drag` needs
a platform window that implements `start_external_drag`. In 0.3.12 only the
macOS and Wayland windows implement it, so there was a platform drag on
macOS and Wayland only.

`gpui-pre-windows`
- `src/drag_source.rs`: This new file has four parts.
  - A data object that holds the files as `CF_HDROP` (`DROPFILES` with wide
    names). Its `GetData`, `QueryGetData` and `EnumFormatEtc` answer for
    that format. The rest of the interface is not implemented.
  - A drop source. It drops when the left button is up, cancels on Escape,
    and uses the system's cursors.
  - `drag`, which runs `DoDragDrop` with copy allowed.
  - `drop_files`, which is exported as `drag_drop_files` for the
    application's test.
- `src/window.rs`: `can_start_external_drag` and `start_external_drag` are
  new. `DoDragDrop` runs the system's drag loop until the drop. It is
  therefore run from a foreground task, after the event that asked for the
  drag has been handled. At that point the application is not borrowed, and
  the messages of the loop reach the window as they do from the main loop.
  When `DoDragDrop` returns, the window receives `FileDropEvent::Ended`.

`gpui-pre-linux`
- `src/linux/x11/xdnd_source.rs`: This new file is the source side of XDND.
  - On each motion of the pointer it finds the `XdndAware` window under the
    pointer. It searches downward from the child of the root window that
    is under the pointer. It follows an `XdndProxy` that names itself. A
    window of this process is not a target.
  - When the target changes, it sends `XdndLeave` to the old target and
    `XdndEnter` to the new one. `XdndEnter` carries the version that the
    target and this side share, which is at most 5, and `text/uri-list` as
    the one type.
  - It sends `XdndPosition` with the copy action. It sends one at a time:
    the next one waits until the `XdndStatus` for the last one has arrived.
  - On the release it sends `XdndDrop` to a target that accepted, and
    `XdndLeave` otherwise.
  - It answers a `SelectionRequest` for `XdndSelection` with `TARGETS` or
    with the `file:` addresses of the files. `uri_list` builds those
    addresses. It is exported as `xdnd_uri_list` for the application's
    test.
- `src/linux/x11/client.rs`: This file connects the source to the client's
  events.
  - `X11ClientState::xdnd_source` holds the source of the drag in progress.
  - `start_xdnd_drag` takes `XdndSelection` for the window.
  - The window's own pointer motion and button release drive the source.
    The implicit grab of the button press keeps delivering the motion to
    the window while the pointer is outside it.
  - `XdndStatus`, `XdndFinished` and `SelectionRequest` are passed to the
    source.
  - The drag ends at `XdndFinished`, or ten seconds after a drop that got
    no `XdndFinished`, or at once when nothing took the drop. When the
    drag ends, the selection is let go and the window receives
    `FileDropEvent::Ended`.
- `src/linux/x11/window.rs`: `can_start_external_drag`,
  `start_external_drag`, and the atoms `XdndProxy` and `TARGETS` are new.
- `src/linux/x11.rs`, `src/linux.rs`, `src/gpui_linux.rs`: These files
  declare and export the new modules.

Whether the system blurs behind a window, and Acrylic on Windows. The app
can show the system's frosted material behind the sidebar and the inspector
of its main window (`src/ui/window_chrome.rs`, `show_rail_material`). It
does that by giving the window the background appearance
`WindowBackgroundAppearance::Blurred`. Two things were missing in 0.3.12.
First, the app could not find out whether `Blurred` really blurs on the
system it runs on. Where it does not, the window would show what is behind
it without blur, or show nothing behind it. Second, on Windows `Blurred`
used only an undocumented call.

`gpui-pre`
- `src/platform.rs`: the trait `PlatformWindow` has a new method,
  `is_background_blur_supported`. It answers whether `Blurred` makes the
  system blur what is behind this window, as the system is set up now. Its
  default answer is `false`, so the Linux windows and the test window
  answer `false` without any change to them.
- `src/window.rs`: `Window::is_background_blur_supported` is the public
  method that an application calls. It passes the question on to the
  platform's window.

`gpui-pre-macos`
- `src/window.rs`: `is_background_blur_supported` answers `true` unless the
  user has turned on Reduce Transparency (`NSWorkspace`'s
  `accessibilityDisplayShouldReduceTransparency`). With Reduce Transparency
  on, AppKit fills an `NSVisualEffectView` with a solid colour and blurs
  nothing, and `blurred_view_update_layer` removes that fill, so a `Blurred`
  window would show what is behind it without blur.

`gpui-pre-macos`, dragging many files out of the window
- `src/window.rs`, `start_external_drag`: every dragged file still gets a
  dragging item, because the items are what the drop receives, but only the
  first four items now return a picture from their
  `imageComponentsProvider`; the others return an empty array. In 0.3.12
  every item returns an icon, and AppKit draws all of them, so a drag of
  several thousand files took seconds to start and drew thousands of icons
  on top of one another. The session's formation is also set to
  `NSDraggingFormationPile`, so the four pictures are drawn as a small pile,
  as Finder draws several files. AppKit adds its own badge with the number
  of files. This has not been tried with a real drag of thousands of files.

`gpui-pre-windows`
- `src/window.rs`, `set_background_appearance`: on Windows 11 version 22H2
  (build 22621) and later, `Blurred` now sets the window attribute
  `DWMWA_SYSTEMBACKDROP_TYPE` to `DWMSBT_TRANSIENTWINDOW`, which is Acrylic,
  and turns the accent policy off. This is the documented way to ask for
  Acrylic, and it is the same attribute that 0.3.12 already sets for
  `MicaBackdrop` and `MicaAltBackdrop`. On Windows 10 version 1809 (build
  17763) up to Windows 11 version 21H2, which do not have the attribute,
  `Blurred` is unchanged: it sets the accent policy
  `ACCENT_ENABLE_ACRYLICBLURBEHIND` with the undocumented
  `SetWindowCompositionAttribute`. When a window stops being `Blurred`, the
  attribute is set back to `DWMSBT_AUTO`, the value a new window has;
  otherwise Windows would go on drawing the backdrop behind an opaque
  window.
- `src/window.rs`: `is_background_blur_supported` answers `true` on build
  17763 and later when the window was created without a redirection bitmap
  (`WS_EX_NOREDIRECTIONBITMAP`). That is every window, unless the
  environment variable `GPUI_DISABLE_DIRECT_COMPOSITION` is set. A window
  with a redirection bitmap is drawn through a swap chain without an alpha
  channel, so it cannot show anything behind it.
- `windows_build_number`, `ACRYLIC_ACCENT_BUILD` and `SYSTEM_BACKDROP_BUILD`
  are the helper and the two build numbers that these changes use.
- None of this has been run on Windows. It is compiled and tested by the
  repository's CI on a machine without a desktop, which does not show a
  backdrop. Not known: whether Acrylic shows through the window, and
  whether Windows replaces it with a solid colour while the window is not
  the active one.

`gpui-pre-linux` has no change for this. On Wayland, 0.3.12 asks the
compositor to blur behind a `Blurred` window when the compositor has the
ext-background-effect or the kde-blur protocol. The Linux windows do not
answer `is_background_blur_supported` yet, so the app never asks for
`Blurred` on Linux. To answer it, the Wayland window would have to report
whether the compositor offers one of the two protocols, and for
ext-background-effect also whether the compositor has announced the `blur`
capability, which 0.3.12 does not record.

## Re-applying to a newer gpui-pre

Copy the six new crates from `~/.cargo/registry/src/*/` over these folders.
Apply the diff below with `patch -p1` from this folder. Then fix the parts
that the patch could not apply because the code has moved.

The Metal header is generated from the `vendor/gpui` copy inside
`gpui-pre-apple`. The blur parameters are a struct of floats that is
declared by hand on both sides (in `gpui-pre-apple/src/metal_renderer.rs`
and in `gpui-pre-apple/src/shaders.metal`), so no generated type changes.

Check the result in these ways:

- For the blur, run `cargo test` in `experiments/gpui` (the blur arithmetic
  and batching tests are in `src/ui/glass.rs`), and take an off-screen
  picture of a menu over the grid.
- For the interface zoom, run `zoom::tests`, which check that a press at
  screen points lands on what is drawn there, and look at the pictures that
  `ci/smoke.txt` takes at 150% and 80%.
- For nearest sampling, take an off-screen picture of a small image zoomed
  in the viewer with Settings > Preview > Scaling set to Pixelated.
- For the frosted material, turn on Settings > General > Appearance >
  "Frosted glass sidebar and inspector" in a real window on macOS with
  another window behind it. On Windows, do the same with "Acrylic sidebar
  and inspector".

The WGSL was validated with naga 30.0.1 (the version `wgpu` 30 uses) in all
three of its variants (`STORAGE_BUFFER_SHADERS`, `WEBGL_SHADERS` and
`SUBPIXEL_SHADERS` in `gpui-pre-wgpu/src/wgpu_renderer.rs`). The HLSL was
not compiled here.

## The diff against 0.3.12

```diff
diff --color -ruN gpui-pre-0.3.12/src/app.rs b/gpui-pre/src/app.rs
--- a/gpui-pre/src/app.rs
+++ b/gpui-pre/src/app.rs
@@ -970,6 +970,8 @@
     #[cfg(any(test, feature = "test-support", debug_assertions))]
     pub(crate) name: Option<&'static str>,
     pub(crate) text_rendering_mode: Rc<Cell<TextRenderingMode>>,
+    /// The interface zoom every window draws at (see [`App::set_ui_zoom`]).
+    pub(crate) ui_zoom: Rc<Cell<f32>>,
 
     pub(crate) window_update_stack: Vec<WindowId>,
     pub(crate) mode: GpuiMode,
@@ -1024,6 +1026,7 @@
                 platform: platform.clone(),
                 text_system,
                 text_rendering_mode: Rc::new(Cell::new(TextRenderingMode::default())),
+                ui_zoom: Rc::new(Cell::new(1.0)),
                 mode: GpuiMode::Production,
                 actions: Rc::new(ActionRegistry::default()),
                 pending_updates: 0,
@@ -1856,6 +1859,35 @@
     /// Returns the current text rendering mode for the application.
     pub fn text_rendering_mode(&self) -> TextRenderingMode {
         self.text_rendering_mode.get()
+    }
+
+    /// Zooms the interface of every window, as a browser's page zoom does:
+    /// each window lays out in a viewport of its size divided by `zoom` and
+    /// draws at its display's scale factor times `zoom`, so everything
+    /// sized in pixels (text rasterized at the zoomed size, not stretched)
+    /// grows or shrinks together, and pointer positions from the system are
+    /// divided by `zoom` before they reach the window. What the window
+    /// hands back to the system (input method rectangles, the window menu's
+    /// position, input regions, the client inset) is multiplied by it. The
+    /// window's frame, its minimum size and `Window::bounds` stay in screen
+    /// points. Windows opened later start at this zoom; open windows take
+    /// it on their next frame. A value that is not finite and positive is
+    /// taken as 1.
+    pub fn set_ui_zoom(&mut self, zoom: f32) {
+        let zoom = if zoom.is_finite() && zoom > 0.0 {
+            zoom
+        } else {
+            1.0
+        };
+        if self.ui_zoom.get() != zoom {
+            self.ui_zoom.set(zoom);
+            self.refresh_windows();
+        }
+    }
+
+    /// The interface zoom set by [`App::set_ui_zoom`]; 1 unless set.
+    pub fn ui_zoom(&self) -> f32 {
+        self.ui_zoom.get()
     }
 
     /// Returns the operating system's power mode, or [`PowerMode::Unknown`]
diff --color -ruN gpui-pre-0.3.12/src/elements/div.rs b/gpui-pre/src/elements/div.rs
--- a/gpui-pre/src/elements/div.rs
+++ b/gpui-pre/src/elements/div.rs
@@ -1443,6 +1443,67 @@
         self
     }
 
+    /// Set the disabled state for this element: assistive technology shows
+    /// it but does not offer to press or edit it.
+    fn aria_disabled(mut self, disabled: bool) -> Self {
+        self.interactivity().aria.disabled = disabled;
+        self
+    }
+
+    /// Set the read-only state for this element (a field whose text can be
+    /// read and selected but not changed).
+    fn aria_read_only(mut self, read_only: bool) -> Self {
+        self.interactivity().aria.read_only = read_only;
+        self
+    }
+
+    /// Mark this element as modal: while it is shown, assistive technology
+    /// treats what lies outside it as inert (`aria-modal`).
+    fn aria_modal(mut self, modal: bool) -> Self {
+        self.interactivity().aria.modal = modal;
+        self
+    }
+
+    /// Mark this element as busy (`aria-busy`): its contents are still
+    /// being loaded.
+    fn aria_busy(mut self, busy: bool) -> Self {
+        self.interactivity().aria.busy = busy;
+        self
+    }
+
+    /// Make this element a live region (`aria-live`): changes to its text
+    /// are announced, politely or at once.
+    fn aria_live(mut self, live: accesskit::Live) -> Self {
+        self.interactivity().aria.live = Some(live);
+        self
+    }
+
+    /// Mark this element as allowing more than one of its items to be
+    /// selected (`aria-multiselectable`).
+    fn aria_multiselectable(mut self, multiselectable: bool) -> Self {
+        self.interactivity().aria.multiselectable = multiselectable;
+        self
+    }
+
+    /// Declare that this element's click is not a control assistive
+    /// technology needs (a dialog's backdrop, a window's drag strip, a
+    /// surface whose click only focuses the field inside it), so it is left
+    /// out of [`crate::Window::a11y_unexposed_clickables`].
+    fn a11y_presentational(mut self) -> Self {
+        self.interactivity().aria.presentational = true;
+        self
+    }
+
+    /// Report this element's node as the one standing for `handle`, a focus
+    /// handle a descendant without a node of its own tracks: assistive
+    /// technology sees this node as focused while `handle` is, and its
+    /// `Focus` action focuses `handle`. Unlike `track_focus`, a click on the
+    /// element does not move the focus.
+    fn a11y_focus(mut self, handle: &FocusHandle) -> Self {
+        self.interactivity().a11y_focus_handle = Some(handle.clone());
+        self
+    }
+
     /// Set the expanded state for this element.
     fn aria_expanded(mut self, expanded: bool) -> Self {
         self.interactivity().aria.expanded = Some(expanded);
@@ -2175,6 +2236,13 @@
     pub(crate) column_index: Option<usize>,
     pub(crate) row_count: Option<usize>,
     pub(crate) column_count: Option<usize>,
+    pub(crate) disabled: bool,
+    pub(crate) read_only: bool,
+    pub(crate) modal: bool,
+    pub(crate) busy: bool,
+    pub(crate) multiselectable: bool,
+    pub(crate) live: Option<accesskit::Live>,
+    pub(crate) presentational: bool,
 }
 
 /// The interactivity struct. Powers all of the general-purpose
@@ -2247,6 +2315,7 @@
     pub(crate) report_active_descendant_focus: bool,
     pub(crate) override_role: Option<accesskit::Role>,
     pub(crate) aria: AriaProperties,
+    pub(crate) a11y_focus_handle: Option<FocusHandle>,
 
     #[cfg(any(feature = "inspector", debug_assertions))]
     pub(crate) source_location: Option<&'static core::panic::Location<'static>>,
@@ -2395,6 +2464,33 @@
             }
         }
 
+        if window.a11y.is_active() {
+            if let Some(handle) = self.a11y_focus_handle.as_ref()
+                && let Some(global_id) = global_id
+                && self.override_role.is_some()
+            {
+                let node_id = global_id.accesskit_node_id();
+                window.a11y.set_focusable(node_id, handle.id);
+                if handle.is_focused(window) {
+                    window.a11y.set_focus(node_id);
+                }
+            }
+            #[cfg(any(test, feature = "test-support"))]
+            if self.override_role.is_none()
+                && !self.click_listeners.is_empty()
+                && !self.aria.presentational
+            {
+                #[cfg(any(feature = "inspector", debug_assertions))]
+                let at = self
+                    .source_location
+                    .map(|location| location.to_string())
+                    .unwrap_or_default();
+                #[cfg(not(any(feature = "inspector", debug_assertions)))]
+                let at = String::new();
+                window.a11y.unexposed_clickables.push((at, bounds));
+            }
+        }
+
         if self.report_active_descendant_focus && window.a11y.is_active() {
             if let Some(global_id) = global_id {
                 window
@@ -3652,10 +3748,31 @@
         if let Some(count) = self.aria.column_count {
             node.set_column_count(count);
         }
-        if !self.click_listeners.is_empty() {
+        if self.aria.disabled {
+            node.set_disabled();
+        }
+        if self.aria.read_only {
+            node.set_read_only();
+        }
+        if self.aria.modal {
+            node.set_modal();
+        }
+        if self.aria.busy {
+            node.set_busy();
+        }
+        if self.aria.multiselectable {
+            node.set_multiselectable();
+        }
+        if let Some(live) = self.aria.live {
+            node.set_live(live);
+        }
+        if !self.click_listeners.is_empty() && !self.aria.disabled {
             node.add_action(accesskit::Action::Click);
         }
-        if self.tracked_focus_handle.is_some() || self.focusable {
+        if self.tracked_focus_handle.is_some()
+            || self.focusable
+            || self.a11y_focus_handle.is_some()
+        {
             node.add_action(accesskit::Action::Focus);
         }
         for (action, _) in &self.a11y_action_listeners {
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
@@ -151,6 +153,13 @@
         self
     }
 
+    /// Sample the image nearest-neighbour, as CSS `image-rendering:
+    /// pixelated` draws it, rather than linearly.
+    fn pixelated(mut self, pixelated: bool) -> Self {
+        self.image_style().pixelated = pixelated;
+        self
+    }
+
     /// Set the object fit for the image.
     fn object_fit(mut self, object_fit: ObjectFit) -> Self {
         self.image_style().object_fit = object_fit;
@@ -245,11 +254,24 @@
     }
 }
 
+/// How long frame `index` of an animated image is shown. As browsers do,
+/// a frame of 10 ms or less is shown for 100 ms.
+fn shown_for(data: &RenderImage, index: usize) -> Duration {
+    let delay = Duration::from(data.delay(index));
+    if delay <= Duration::from_millis(10) {
+        Duration::from_millis(100)
+    } else {
+        delay
+    }
+}
+
 /// The image state between frames
 struct ImgState {
     frame_index: usize,
     last_frame_time: Option<Instant>,
     started_loading: Option<(Instant, Task<()>)>,
+    /// The redraw when the frame shown has been shown for its delay.
+    next_frame: Option<Task<()>>,
 }
 
 /// The image layout state between frames
@@ -288,6 +310,7 @@
                     frame_index: 0,
                     last_frame_time: None,
                     started_loading: None,
+                    next_frame: None,
                 })
             });
 
@@ -314,25 +337,24 @@
 
                             if let Some(state) = &mut state {
                                 state.frame_index = state.frame_index.min(max_frame_index);
+                                // An animated image plays whenever its window
+                                // draws, active or not, as a browser plays one;
+                                // a covered or minimised window draws nothing.
                                 if frame_count > 1 && !cx.reduce_motion() {
-                                    if window.is_window_active() {
-                                        let current_time = Instant::now();
-                                        if let Some(last_frame_time) = state.last_frame_time {
-                                            let elapsed = current_time - last_frame_time;
-                                            let frame_duration =
-                                                Duration::from(data.delay(state.frame_index));
+                                    let current_time = Instant::now();
+                                    if let Some(last_frame_time) = state.last_frame_time {
+                                        let elapsed = current_time - last_frame_time;
+                                        let frame_duration =
+                                            shown_for(&data, state.frame_index);
 
-                                            if elapsed >= frame_duration {
-                                                state.frame_index =
-                                                    (state.frame_index + 1) % frame_count;
-                                                state.last_frame_time =
-                                                    Some(current_time - (elapsed - frame_duration));
-                                            }
-                                        } else {
-                                            state.last_frame_time = Some(current_time);
+                                        if elapsed >= frame_duration {
+                                            state.frame_index =
+                                                (state.frame_index + 1) % frame_count;
+                                            state.last_frame_time =
+                                                Some(current_time - (elapsed - frame_duration));
                                         }
                                     } else {
-                                        state.last_frame_time = None;
+                                        state.last_frame_time = Some(current_time);
                                     }
                                 } else {
                                     state.last_frame_time = None;
@@ -343,7 +365,19 @@
 
                             let image_size = data.render_size(frame_index);
 
-                            if style.aspect_ratio.is_none() {
+                            // The picture's own shape decides the element's
+                            // shape only when the caller left the width or
+                            // the height open. When both are given (for
+                            // example `size_full`), the element takes that
+                            // box, and `object_fit` places the picture in
+                            // it. Setting the ratio in that case made the
+                            // layout replace the given height with one
+                            // worked out from the width, so a tall picture
+                            // that fills a square box showed its top part
+                            // and not its middle.
+                            let open = matches!(style.size.width, Length::Auto)
+                                || matches!(style.size.height, Length::Auto);
+                            if style.aspect_ratio.is_none() && open {
                                 style.aspect_ratio = Some(image_size.width / image_size.height);
                             }
 
@@ -375,12 +409,22 @@
                                 };
                             }
 
-                            if global_id.is_some()
-                                && data.frame_count() > 1
-                                && window.is_window_active()
-                                && !cx.reduce_motion()
-                            {
-                                window.request_animation_frame();
+                            // The view is drawn again when the frame shown
+                            // has been shown for its delay, not every frame.
+                            if let Some(state) = &mut state {
+                                state.next_frame = None;
+                                if let Some(last_frame_time) = state.last_frame_time
+                                    && frame_count > 1
+                                    && !cx.reduce_motion()
+                                {
+                                    let remaining = shown_for(&data, state.frame_index)
+                                        .saturating_sub(last_frame_time.elapsed());
+                                    let current_view = window.current_view();
+                                    state.next_frame = Some(window.spawn(cx, async move |cx| {
+                                        cx.background_executor().timer(remaining).await;
+                                        cx.update(move |_, cx| cx.notify(current_view)).ok();
+                                    }));
+                                }
                             }
                         }
                         Some(_err) => {
@@ -489,13 +533,14 @@
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
@@ -721,6 +721,155 @@
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
+    /// Files the source promised instead of putting them on the drag
+    /// (macOS's file promises), which it writes after the drop.
+    pub promised: Option<PromisedFiles>,
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
+            && self.promised.is_none()
+    }
+}
+
+/// What a promised file came to: where it was written, or why it was not.
+pub type PromisedFile = Result<PathBuf, String>;
+
+/// Files a drop promised, which their source writes into `folder` after the
+/// drop has been delivered. The platform makes `folder` for the drop inside
+/// [`promised_files_directory`] and reports each file once as it arrives or
+/// fails; the application waits for them with [`PromisedFiles::arrival`] and
+/// owns the folder from then on.
+#[derive(Clone)]
+pub struct PromisedFiles {
+    /// The folder made for this drop, which holds nothing else.
+    pub folder: PathBuf,
+    /// How many files were promised.
+    pub count: usize,
+    arrival: std::sync::Arc<
+        std::sync::Mutex<Option<futures::channel::mpsc::UnboundedReceiver<PromisedFile>>>,
+    >,
+}
+
+impl PromisedFiles {
+    /// Promised files to be written into `folder`, and the sender the
+    /// platform reports each of them through.
+    pub fn new(
+        folder: PathBuf,
+        count: usize,
+    ) -> (Self, futures::channel::mpsc::UnboundedSender<PromisedFile>) {
+        let (sender, receiver) = futures::channel::mpsc::unbounded();
+        let promised = Self {
+            folder,
+            count,
+            arrival: std::sync::Arc::new(std::sync::Mutex::new(Some(receiver))),
+        };
+        (promised, sender)
+    }
+
+    /// The files' reports as they come, once (`None` after the first call).
+    /// The stream ends after `count` reports, or earlier when the platform
+    /// can no longer report (its sender dropped).
+    pub fn arrival(&self) -> Option<futures::channel::mpsc::UnboundedReceiver<PromisedFile>> {
+        self.arrival
+            .lock()
+            .unwrap_or_else(|poison| poison.into_inner())
+            .take()
+    }
+}
+
+impl Debug for PromisedFiles {
+    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
+        f.debug_struct("PromisedFiles")
+            .field("folder", &self.folder)
+            .field("count", &self.count)
+            .finish_non_exhaustive()
+    }
+}
+
+impl PartialEq for PromisedFiles {
+    fn eq(&self, other: &Self) -> bool {
+        std::sync::Arc::ptr_eq(&self.arrival, &other.arrival)
+    }
+}
+
+impl Eq for PromisedFiles {}
+
+static PROMISED_FILES_DIRECTORY: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);
+
+/// Sets the folder in which a platform makes each drop's folder for the
+/// files it promised (the system's temporary folder until set).
+pub fn set_promised_files_directory(directory: PathBuf) {
+    *PROMISED_FILES_DIRECTORY
+        .lock()
+        .unwrap_or_else(|poison| poison.into_inner()) = Some(directory);
+}
+
+/// Where a platform makes each drop's folder for promised files.
+pub fn promised_files_directory() -> PathBuf {
+    PROMISED_FILES_DIRECTORY
+        .lock()
+        .unwrap_or_else(|poison| poison.into_inner())
+        .clone()
+        .unwrap_or_else(std::env::temp_dir)
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
diff --color -ruN gpui-pre-0.3.12/src/platform.rs b/gpui-pre/src/platform.rs
--- a/gpui-pre/src/platform.rs
+++ b/gpui-pre/src/platform.rs
@@ -1300,6 +1300,12 @@
     fn background_appearance(&self) -> WindowBackgroundAppearance;
     fn set_title(&mut self, title: &str);
     fn set_background_appearance(&self, background_appearance: WindowBackgroundAppearance);
+    /// Whether [`WindowBackgroundAppearance::Blurred`] makes the system blur
+    /// what is behind this window, as the system is set up now. A platform
+    /// that does not implement this answers `false`.
+    fn is_background_blur_supported(&self) -> bool {
+        false
+    }
     fn minimize(&self);
     fn zoom(&self);
     fn toggle_fullscreen(&self);
@@ -2147,9 +2153,17 @@
             .ok();
     }
 
+    // The rectangles and points exchanged with the system are in the
+    // window's screen points; the handler's are in the points the window is
+    // laid out in, which differ by the interface zoom (`App::set_ui_zoom`).
+
     pub fn bounds_for_range(&mut self, range_utf16: Range<usize>) -> Option<Bounds<Pixels>> {
         self.cx
-            .update(|window, cx| self.handler.bounds_for_range(range_utf16, window, cx))
+            .update(|window, cx| {
+                self.handler
+                    .bounds_for_range(range_utf16, window, cx)
+                    .map(|bounds| crate::window::zoomed_out_bounds(bounds, window.ui_zoom()))
+            })
             .ok()
             .flatten()
     }
@@ -2157,8 +2171,12 @@
     pub fn selection_bounds_for_range(&mut self, range_utf16: Range<usize>) -> Vec<Bounds<Pixels>> {
         self.cx
             .update(|window, cx| {
+                let zoom = window.ui_zoom();
                 self.handler
                     .selection_bounds_for_range(range_utf16, window, cx)
+                    .into_iter()
+                    .map(|bounds| crate::window::zoomed_out_bounds(bounds, zoom))
+                    .collect()
             })
             .unwrap_or_default()
     }
@@ -2226,7 +2244,10 @@
     #[allow(unused)]
     pub fn character_index_for_point(&mut self, point: Point<Pixels>) -> Option<usize> {
         self.cx
-            .update(|window, cx| self.handler.character_index_for_point(point, window, cx))
+            .update(|window, cx| {
+                let point = point / window.ui_zoom();
+                self.handler.character_index_for_point(point, window, cx)
+            })
             .ok()
             .flatten()
     }
@@ -2244,7 +2265,11 @@
     /// See [`InputHandler::element_bounds`].
     pub fn element_bounds(&mut self) -> Option<Bounds<Pixels>> {
         self.cx
-            .update(|window, cx| self.handler.element_bounds(window, cx))
+            .update(|window, cx| {
+                self.handler
+                    .element_bounds(window, cx)
+                    .map(|bounds| crate::window::zoomed_out_bounds(bounds, window.ui_zoom()))
+            })
             .ok()
             .flatten()
     }
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
@@ -352,6 +354,21 @@
 
     fn is_maximized(&self) -> bool {
         false
+    }
+
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
     }
 
     fn content_size(&self) -> Size<Pixels> {
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
@@ -591,6 +653,173 @@
     }
 }
 
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
@@ -1266,7 +1266,13 @@
     mouse_hit_test: HitTest,
     modifiers: Modifiers,
     capslock: Capslock,
+    /// The display's scale factor times [`Self::ui_zoom`]: what layout
+    /// pixels are multiplied by to reach device pixels.
     scale_factor: f32,
+    /// The interface zoom this window was last laid out at
+    /// ([`App::set_ui_zoom`]). `viewport_size` and `mouse_position` are in
+    /// zoomed points: the platform's divided by it.
+    ui_zoom: f32,
     pub(crate) bounds_observers: SingleSubscriberSet<AnyObserver>,
     appearance: WindowAppearance,
     pub(crate) appearance_observers: SingleSubscriberSet<AnyObserver>,
@@ -1634,6 +1640,33 @@
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
@@ -1705,11 +1738,12 @@
 
         let display_id = platform_window.display().map(|display| display.id());
         let sprite_atlas = platform_window.sprite_atlas();
-        let mouse_position = platform_window.mouse_position();
+        let ui_zoom = cx.ui_zoom.get();
+        let mouse_position = platform_window.mouse_position() / ui_zoom;
         let modifiers = platform_window.modifiers();
         let capslock = platform_window.capslock();
-        let content_size = platform_window.content_size();
-        let scale_factor = platform_window.scale_factor();
+        let content_size = zoomed_size(platform_window.content_size(), ui_zoom);
+        let scale_factor = platform_window.scale_factor() * ui_zoom;
         let appearance = platform_window.appearance();
         let text_system = Rc::new(WindowTextSystem::new(cx.text_system().clone()));
         let invalidator = WindowInvalidator::new(handle.window_id());
@@ -2242,7 +2276,9 @@
             let mut cx = cx.to_async();
             Box::new(move |event| {
                 handle
-                    .update(&mut cx, |_, window, cx| window.dispatch_event(event, cx))
+                    .update(&mut cx, |_, window, cx| {
+                        window.dispatch_platform_input(event, cx)
+                    })
                     .with_context(|| {
                         format!("dispatching input for window {:?}", handle.window_id())
                     })
@@ -2390,6 +2426,7 @@
             modifiers,
             capslock,
             scale_factor,
+            ui_zoom,
             bounds_observers: SingleSubscriberSet::new(),
             appearance,
             appearance_observers: SingleSubscriberSet::new(),
@@ -2731,7 +2768,14 @@
     /// - `Some(&[])` is an empty region, so the window receives no pointer or touch input.
     /// - `None` resets the region to the default, so the whole window receives input again.
     pub fn set_input_region(&self, region: Option<&[Bounds<Pixels>]>) {
-        self.platform_window.set_input_region(region);
+        let zoom = self.ui_zoom;
+        let zoomed: Option<Vec<Bounds<Pixels>>> = region.map(|rects| {
+            rects
+                .iter()
+                .map(|rect| zoomed_out_bounds(*rect, zoom))
+                .collect()
+        });
+        self.platform_window.set_input_region(zoomed.as_deref());
     }
 
     /// Return the `WindowBounds` to indicate that how a window should be opened
@@ -2958,6 +3002,14 @@
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
@@ -3001,9 +3053,9 @@
     /// the platform window, then notifies observers. Normally called automatically
     /// by the platform's resize callback, but exposed publicly for test infrastructure.
     pub fn bounds_changed(&mut self, cx: &mut App) {
-        self.scale_factor = self.platform_window.scale_factor();
-        self.viewport_size = self.platform_window.content_size();
-        self.mouse_position = self.platform_window.mouse_position();
+        self.scale_factor = self.platform_window.scale_factor() * self.ui_zoom;
+        self.viewport_size = zoomed_size(self.platform_window.content_size(), self.ui_zoom);
+        self.mouse_position = self.platform_window.mouse_position() / self.ui_zoom;
 
         self.refresh();
 
@@ -3166,7 +3218,11 @@
     /// During drawing this is a consistent frame snapshot. Outside drawing it
     /// reflects the latest platform sample, not a synchronous geometry query.
     pub fn visual_viewport_bounds(&self) -> Bounds<Pixels> {
-        self.platform_window.visual_viewport_bounds()
+        let bounds = self.platform_window.visual_viewport_bounds();
+        Bounds::new(
+            bounds.origin / self.ui_zoom,
+            zoomed_size(bounds.size, self.ui_zoom),
+        )
     }
 
     /// Returns a conservative rectangle avoiding platform-known obscured content.
@@ -3227,7 +3283,8 @@
 
     /// Opens the native title bar context menu, useful when implementing client side decorations (Wayland and X11)
     pub fn show_window_menu(&self, position: Point<Pixels>) {
-        self.platform_window.show_window_menu(position)
+        self.platform_window
+            .show_window_menu(position * self.ui_zoom)
     }
 
     /// Handle window movement for Linux and macOS.
@@ -3241,12 +3298,14 @@
     /// When using client side decorations, set this to the width of the invisible decorations (Wayland and X11)
     pub fn set_client_inset(&mut self, inset: Pixels) {
         self.client_inset = Some(inset);
-        self.platform_window.set_client_inset(inset);
+        self.platform_window.set_client_inset(inset * self.ui_zoom);
     }
 
     /// Sets the client-drawn corner radii so platform effects can match the window's shape.
     pub fn set_client_corner_radii(&mut self, radii: Corners<Pixels>) {
-        self.platform_window.set_client_corner_radii(radii);
+        let zoom = self.ui_zoom;
+        self.platform_window
+            .set_client_corner_radii(radii.map(|radius| *radius * zoom));
     }
 
     /// Returns the client_inset value by [`Self::set_client_inset`].
@@ -3297,6 +3356,22 @@
             .set_background_appearance(background_appearance);
     }
 
+    /// Whether [`WindowBackgroundAppearance::Blurred`] makes the system blur
+    /// what is behind this window, as the system is set up now. Where this is
+    /// `false`, a window with that appearance shows what is behind it without
+    /// blur, or shows nothing behind it at all, so an application that wants
+    /// the blur should keep the window opaque.
+    ///
+    /// - macOS: `true` unless the user has turned on Reduce Transparency.
+    /// - Windows: `true` on Windows 10 version 1809 (build 17763) and later,
+    ///   in a window that is drawn through DirectComposition.
+    /// - Linux: always `false`. The Linux windows do not answer the question
+    ///   yet, although a Wayland compositor with a blur protocol does blur
+    ///   behind a `Blurred` window.
+    pub fn is_background_blur_supported(&self) -> bool {
+        self.platform_window.is_background_blur_supported()
+    }
+
     /// Mark the window as dirty at the platform level.
     pub fn set_window_edited(&mut self, edited: bool) {
         self.platform_window.set_edited(edited);
@@ -3331,10 +3406,32 @@
     /// Overrides the display scale factor for tests.
     #[cfg(any(test, feature = "test-support"))]
     pub fn set_scale_factor(&mut self, scale_factor: f32) {
-        self.scale_factor = scale_factor;
+        self.scale_factor = scale_factor * self.ui_zoom;
         self.refresh();
     }
 
+    /// The interface zoom this window is laid out at ([`App::set_ui_zoom`]):
+    /// a point of the window's content is this many screen points.
+    pub fn ui_zoom(&self) -> f32 {
+        self.ui_zoom
+    }
+
+    /// Takes the application's interface zoom if it changed: the viewport,
+    /// scale factor and pointer position follow, and the whole window is
+    /// laid out again.
+    fn apply_ui_zoom(&mut self, cx: &App) {
+        let zoom = cx.ui_zoom.get();
+        if zoom == self.ui_zoom {
+            return;
+        }
+        let device_scale = self.scale_factor / self.ui_zoom;
+        self.mouse_position = self.mouse_position * (self.ui_zoom / zoom);
+        self.ui_zoom = zoom;
+        self.scale_factor = device_scale * zoom;
+        self.viewport_size = zoomed_size(self.platform_window.content_size(), zoom);
+        self.refresh();
+    }
+
     /// The size of an em for the base font of the application. Adjusting this value allows the
     /// UI to scale, just like zooming a web page.
     pub fn rem_size(&self) -> Pixels {
@@ -3672,6 +3769,7 @@
         if self.platform_window.prepare_frame() {
             self.refresh();
         }
+        self.apply_ui_zoom(cx);
         self.invalidate_entities();
         cx.entities.clear_accessed();
         debug_assert!(self.rendered_entity_stack.is_empty());
@@ -3907,7 +4005,7 @@
 
         self.a11y.sync_active_flag();
         if self.a11y.is_active() {
-            self.a11y.begin_frame();
+            self.a11y.begin_frame(self.ui_zoom);
         }
 
         let _inspector_width: Pixels = rems(30.0).to_pixels(self.rem_size());
@@ -4811,6 +4909,31 @@
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
@@ -4832,12 +4955,52 @@
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
@@ -5215,7 +5378,7 @@
 
             self.next_frame.scene.insert_primitive(PolychromeSprite {
                 order: 0,
-                pad: 0,
+                nearest: false.into(),
                 grayscale: false.into(),
                 bounds,
                 corner_radii: Default::default(),
@@ -5309,6 +5472,31 @@
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
@@ -5387,7 +5575,7 @@
 
         self.next_frame.scene.insert_primitive(PolychromeSprite {
             order: 0,
-            pad: 0,
+            nearest: nearest.into(),
             grayscale: grayscale.into(),
             bounds: visible_bounds_snapped,
             content_mask,
@@ -5861,7 +6049,23 @@
             .unwrap_or_else(|| action.name().to_string())
     }
 
-    /// Dispatch a mouse, keyboard, or touch event on the window.
+    /// Dispatch an event as the system reports it, with positions in the
+    /// window's screen points: they are divided by the interface zoom
+    /// ([`App::set_ui_zoom`]) to the points the window is laid out in, then
+    /// the event goes to [`Self::dispatch_event`]. The platform's input
+    /// callback comes here; so should anything that plays input as though
+    /// from the system.
+    pub fn dispatch_platform_input(
+        &mut self,
+        event: PlatformInput,
+        cx: &mut App,
+    ) -> DispatchEventResult {
+        let event = input_in_zoomed_points(event, self.ui_zoom);
+        self.dispatch_event(event, cx)
+    }
+
+    /// Dispatch a mouse, keyboard, or touch event on the window, with
+    /// positions in the points the window is laid out in.
     #[profiling::function]
     pub fn dispatch_event(&mut self, event: PlatformInput, cx: &mut App) -> DispatchEventResult {
         #[cfg(feature = "profiler")]
@@ -6037,6 +6241,16 @@
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
@@ -6946,7 +7160,9 @@
         self.on_next_frame(|window, cx| {
             if let Some(mut input_handler) = window.platform_window.take_input_handler() {
                 if let Some(bounds) = input_handler.selected_bounds(window, cx) {
-                    window.platform_window.update_ime_position(bounds);
+                    window
+                        .platform_window
+                        .update_ime_position(zoomed_out_bounds(bounds, window.ui_zoom));
                 }
                 window.platform_window.set_input_handler(input_handler);
             }
@@ -7304,6 +7520,41 @@
         self.a11y.debug_tree_json()
     }
 
+    /// Builds the accessibility tree from the next frame on as though
+    /// assistive technology had asked for it (or stops, with `false`), for
+    /// tests and off-screen tools, whose platform never asks.
+    #[cfg(any(test, feature = "test-support"))]
+    pub fn set_a11y_active_for_test(&mut self, active: bool) {
+        self.a11y.set_active_flag(active);
+        self.refresh();
+    }
+
+    /// The tree the last frame built (what was, or would have been, sent to
+    /// the platform's adapter), while the tree is being built.
+    #[cfg(any(test, feature = "test-support"))]
+    pub fn last_a11y_tree_update(&self) -> Option<accesskit::TreeUpdate> {
+        self.a11y.last_tree_update()
+    }
+
+    /// Performs an action assistive technology asked for, as the platform's
+    /// adapter hands it over.
+    #[cfg(all(any(test, feature = "test-support"), not(target_family = "wasm")))]
+    pub fn dispatch_a11y_action_for_test(
+        &mut self,
+        request: accesskit::ActionRequest,
+        cx: &mut App,
+    ) {
+        self.handle_a11y_action(request, cx);
+    }
+
+    /// Elements in the last frame that took a click but had no role, so
+    /// assistive technology could not see or press them: each one's source
+    /// location (debug builds) and bounds. Recorded while the tree is built.
+    #[cfg(any(test, feature = "test-support"))]
+    pub fn a11y_unexposed_clickables(&self) -> Vec<(String, Bounds<Pixels>)> {
+        self.a11y.unexposed_clickables.clone()
+    }
+
     /// Register a listener for an accessibility action on a specific node.
     /// The listener will be called when a screen reader requests the given
     /// action on the node identified by `node_id`.
@@ -8160,6 +8411,104 @@
         Ok(())
     } else {
         ELEMENT_ARENA.try_with(|arena| callback(&mut arena.borrow_mut()))
+    }
+}
+
+/// A size the platform reports in screen points, in the points a window
+/// zoomed by `zoom` is laid out in.
+fn zoomed_size(size: Size<Pixels>, zoom: f32) -> Size<Pixels> {
+    if zoom == 1.0 {
+        return size;
+    }
+    Size {
+        width: size.width / zoom,
+        height: size.height / zoom,
+    }
+}
+
+/// Bounds in a window zoomed by `zoom`, in the window's screen points.
+pub(crate) fn zoomed_out_bounds(bounds: Bounds<Pixels>, zoom: f32) -> Bounds<Pixels> {
+    if zoom == 1.0 {
+        return bounds;
+    }
+    Bounds::new(bounds.origin * zoom, bounds.size.map(|length| length * zoom))
+}
+
+/// An event with positions in a window's screen points, with them (and a
+/// precise scroll's distance) in the points a window zoomed by `zoom` is
+/// laid out in.
+fn input_in_zoomed_points(event: PlatformInput, zoom: f32) -> PlatformInput {
+    if zoom == 1.0 {
+        return event;
+    }
+    let at = |position: Point<Pixels>| position / zoom;
+    match event {
+        PlatformInput::MouseDown(mut event) => {
+            event.position = at(event.position);
+            PlatformInput::MouseDown(event)
+        }
+        PlatformInput::MouseUp(mut event) => {
+            event.position = at(event.position);
+            PlatformInput::MouseUp(event)
+        }
+        PlatformInput::MousePressure(mut event) => {
+            event.position = at(event.position);
+            PlatformInput::MousePressure(event)
+        }
+        PlatformInput::MouseMove(mut event) => {
+            event.position = at(event.position);
+            PlatformInput::MouseMove(event)
+        }
+        PlatformInput::MouseExited(mut event) => {
+            event.position = at(event.position);
+            PlatformInput::MouseExited(event)
+        }
+        PlatformInput::Scroll(mut event) => {
+            event.position = at(event.position);
+            if let crate::ScrollDelta::Pixels(delta) = event.delta {
+                event.delta = crate::ScrollDelta::Pixels(at(delta));
+            }
+            PlatformInput::Scroll(event)
+        }
+        PlatformInput::Pinch(mut event) => {
+            event.position = at(event.position);
+            PlatformInput::Pinch(event)
+        }
+        PlatformInput::LongPress(mut event) => {
+            event.start_position = at(event.start_position);
+            event.position = at(event.position);
+            PlatformInput::LongPress(event)
+        }
+        PlatformInput::TouchDrag(mut event) => {
+            event.start_position = at(event.start_position);
+            event.position = at(event.position);
+            PlatformInput::TouchDrag(event)
+        }
+        PlatformInput::BackGesture(mut event) => {
+            event.start_position = at(event.start_position);
+            PlatformInput::BackGesture(event)
+        }
+        PlatformInput::FileDrop(event) => PlatformInput::FileDrop(match event {
+            FileDropEvent::Entered { position, paths } => FileDropEvent::Entered {
+                position: at(position),
+                paths,
+            },
+            FileDropEvent::Pending { position } => FileDropEvent::Pending {
+                position: at(position),
+            },
+            FileDropEvent::Submit { position } => FileDropEvent::Submit {
+                position: at(position),
+            },
+            other => other,
+        }),
+        PlatformInput::Touch(mut event) => {
+            event.position = at(event.position);
+            event.predicted_position = event.predicted_position.map(at);
+            PlatformInput::Touch(event)
+        }
+        event @ (PlatformInput::KeyDown(_)
+        | PlatformInput::KeyUp(_)
+        | PlatformInput::ModifiersChanged(_)) => event,
     }
 }
 
diff --color -ruN gpui-pre-0.3.12/src/window/a11y.rs b/gpui-pre/src/window/a11y.rs
--- a/gpui-pre/src/window/a11y.rs
+++ b/gpui-pre/src/window/a11y.rs
@@ -149,6 +149,9 @@
     pub(crate) focus_ids: FxHashMap<NodeId, FocusId>,
     pub(crate) node_bounds: FxHashMap<NodeId, Bounds<Pixels>>,
     pub(crate) action_listeners: FxHashMap<NodeId, Vec<(Action, A11yActionListener)>>,
+    /// Elements this frame that took a click without having a role (see
+    /// [`crate::Window::a11y_unexposed_clickables`]).
+    pub(crate) unexposed_clickables: Vec<(String, Bounds<Pixels>)>,
     /// The window's title, used to label the root node so assistive
     /// technology can tell windows apart.
     window_title: Option<SharedString>,
@@ -177,6 +180,7 @@
             focus_ids: FxHashMap::default(),
             node_bounds: FxHashMap::default(),
             action_listeners: FxHashMap::default(),
+            unexposed_clickables: Vec::new(),
             window_title,
             last_focus_without_node: None,
             debug: debug::A11yDebug::default(),
@@ -213,6 +217,16 @@
         self.active_this_frame = self.is_enabled() && self.active_flag.load(Ordering::SeqCst);
     }
 
+    #[cfg(any(test, feature = "test-support"))]
+    pub(crate) fn set_active_flag(&mut self, active: bool) {
+        self.active_flag.store(active, Ordering::SeqCst);
+    }
+
+    #[cfg(any(test, feature = "test-support"))]
+    pub(crate) fn last_tree_update(&self) -> Option<TreeUpdate> {
+        self.debug.last_tree_update()
+    }
+
     pub(crate) fn is_enabled(&self) -> bool {
         !self.force_disabled
     }
@@ -272,11 +286,20 @@
     }
 
     /// Clear per-frame state and push the root node to start a new frame.
-    pub(crate) fn begin_frame(&mut self) {
+    ///
+    /// `ui_zoom` is the window's interface zoom: nodes' bounds are in its
+    /// points, which the root's transform scales to the window's.
+    pub(crate) fn begin_frame(&mut self, ui_zoom: f32) {
         self.focus_ids.clear();
         self.node_bounds.clear();
         self.action_listeners.clear();
+        self.unexposed_clickables.clear();
         self.nodes.begin_frame(self.window_title.as_ref());
+        if ui_zoom != 1.0
+            && let Some(root) = self.nodes.current_node_mut()
+        {
+            root.set_transform(accesskit::Affine::scale(ui_zoom as f64));
+        }
     }
 
     /// Finalize the tree and produce a [`TreeUpdate`] for the platform adapter.
@@ -654,7 +677,7 @@
 
     fn new_a11y() -> A11y {
         let mut a11y = A11y::new(Arc::new(AtomicBool::new(true)), false, None);
-        a11y.begin_frame();
+        a11y.begin_frame(1.0);
         a11y
     }
 
diff --color -ruN gpui-pre-0.3.12/src/window/a11y/debug.rs b/gpui-pre/src/window/a11y/debug.rs
--- a/gpui-pre/src/window/a11y/debug.rs
+++ b/gpui-pre/src/window/a11y/debug.rs
@@ -96,6 +96,11 @@
         self.last_node_info = node_info.clone();
     }
 
+    #[cfg(any(test, feature = "test-support"))]
+    pub(crate) fn last_tree_update(&self) -> Option<TreeUpdate> {
+        self.last_tree_update.clone()
+    }
+
     /// Serialize the last tree update to a readable JSON string. Node ids are
     /// replaced with short ephemeral ids (`a`, `b`, ..., `z`, `aa`, ...).
     pub(crate) fn to_json(&self) -> Option<String> {
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
@@ -542,13 +542,18 @@
     }
   }
 
-  if (shadow.inset != 0u) {
+  if (shadow.inset == 1u) {
     // The inset shadow is the complement of the (blurred) hole rect, clipped to the element.
     // `saturate(0.5 - d)` gives a 1-pixel antialiased edge: d <= -0.5 -> 1, d >= 0.5 -> 0.
     alpha = 1. - alpha;
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
 
@@ -1276,4 +1288,153 @@
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
@@ -1033,13 +1033,18 @@
         }
     }
 
-    if (shadow.inset != 0u) {
+    if (shadow.inset == 1u) {
         // The inset shadow is the complement of the (blurred) hole rect, clipped to the element.
         // `saturate(0.5 - d)` gives a 1-pixel antialiased edge: d <= -0.5 -> 1, d >= 0.5 -> 0.
         alpha = 1.0 - alpha;
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
@@ -0,0 +1,561 @@
+//! What a drag from another application offers besides files (`CF_HDROP`):
+//! virtual files (`FileGroupDescriptorW` with `FileContents`, what Outlook,
+//! a browser and Photos drag), written into a folder made for the drop; a
+//! browser's dragged image as image data (`PNG`, `CF_DIBV5`, `CF_DIB`), its
+//! address (`UniformResourceLocatorW`), the page's HTML for it
+//! (`HTML Format`) and text.
+use std::io::Write;
+use std::path::{Path, PathBuf};
+use std::sync::LazyLock;
+
+use gpui::{ExternalDropData, ExternalDropFile, PromisedFile, PromisedFiles};
+use windows::Win32::Foundation::S_OK;
+use windows::Win32::System::{
+    Com::{
+        DVASPECT_CONTENT, FORMATETC, IDataObject, IDataObject_Impl, TYMED_HGLOBAL, TYMED_ISTREAM,
+    },
+    DataExchange::RegisterClipboardFormatW,
+    Ole::{CF_DIB, CF_DIBV5, CF_HDROP, CF_UNICODETEXT, ReleaseStgMedium},
+};
+use windows::core::{PCWSTR, implement, w};
+
+/// The most one in-memory file may hold; a larger one is left out.
+const FILE_LIMIT: usize = 1 << 30;
+/// `FILE_ATTRIBUTE_DIRECTORY`, `FD_ATTRIBUTES` (the attributes field is
+/// valid) and `FD_FILESIZE` (the size fields are).
+const DIRECTORY: u32 = 0x10;
+const FD_ATTRIBUTES: u32 = 0x4;
+const FD_FILESIZE: u32 = 0x40;
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
+/// Copies the data of `format` (item `lindex`), from global memory or a
+/// stream, into `sink`: at most `limit` bytes of it (global memory, which
+/// the system may round up, is cut there); `Err` with the reason when it
+/// cannot be read or a stream holds more.
+fn copy(
+    data: &IDataObject,
+    format: u16,
+    lindex: i32,
+    limit: u64,
+    sink: &mut dyn Write,
+) -> Result<(), String> {
+    if format == 0 {
+        return Err("format not registered".into());
+    }
+    let wanted = etc(format, lindex, (TYMED_HGLOBAL.0 | TYMED_ISTREAM.0) as u32);
+    // SAFETY: `GetData` hands over a medium the caller releases with
+    // `ReleaseStgMedium`, which every path below does once; the union
+    // field read is the one `tymed` names.
+    unsafe {
+        let mut medium = data.GetData(&wanted).map_err(|error| error.message())?;
+        let copied = if medium.tymed == TYMED_HGLOBAL.0 as u32 {
+            match crate::clipboard::LockedGlobal::lock(medium.u.hGlobal) {
+                Some(locked) => {
+                    let bytes = locked.as_bytes();
+                    let end = bytes
+                        .len()
+                        .min(usize::try_from(limit).unwrap_or(usize::MAX));
+                    sink.write_all(&bytes[..end])
+                        .map_err(|error| error.to_string())
+                }
+                None => Err("the data could not be locked".into()),
+            }
+        } else if medium.tymed == TYMED_ISTREAM.0 as u32 {
+            match (*medium.u.pstm).as_ref() {
+                Some(stream) => {
+                    let mut chunk = vec![0u8; 1 << 16];
+                    let mut total = 0u64;
+                    loop {
+                        let mut got = 0u32;
+                        let result = stream.Read(
+                            chunk.as_mut_ptr().cast(),
+                            chunk.len() as u32,
+                            Some(&mut got as *mut u32),
+                        );
+                        if result.is_err() {
+                            break Err(result.message());
+                        }
+                        if got == 0 {
+                            break Ok(());
+                        }
+                        total += u64::from(got);
+                        if total > limit {
+                            break Err("the data is longer than it was said to be".into());
+                        }
+                        if let Err(error) = sink.write_all(&chunk[..got as usize]) {
+                            break Err(error.to_string());
+                        }
+                    }
+                }
+                None => Err("no stream".into()),
+            }
+        } else {
+            Err("the data is in a form not read".into())
+        };
+        ReleaseStgMedium(&mut medium);
+        copied
+    }
+}
+
+/// The bytes of `format` (item `lindex`), at most [`FILE_LIMIT`] of them.
+fn bytes(data: &IDataObject, format: u16, lindex: i32) -> Option<Vec<u8>> {
+    let mut all = Vec::new();
+    copy(data, format, lindex, FILE_LIMIT as u64, &mut all).ok()?;
+    Some(all)
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
+/// A file a `FILEGROUPDESCRIPTORW` describes.
+struct Descriptor {
+    name: String,
+    directory: bool,
+    /// Its size, when the descriptor gives it.
+    size: Option<u64>,
+}
+
+/// The files a `FILEGROUPDESCRIPTORW` describes, in order.
+fn file_group(group: &[u8]) -> Vec<Descriptor> {
+    let Some(count) = group
+        .get(..4)
+        .map(|count| u32::from_le_bytes(count.try_into().unwrap()))
+    else {
+        return Vec::new();
+    };
+    let word = |descriptor: &[u8], at: usize| {
+        u32::from_le_bytes(descriptor[at..at + 4].try_into().unwrap())
+    };
+    (0..count as usize)
+        .map_while(|index| {
+            let descriptor = group.get(4 + index * DESCRIPTOR..4 + (index + 1) * DESCRIPTOR)?;
+            let flags = word(descriptor, 0);
+            let directory = flags & FD_ATTRIBUTES != 0 && word(descriptor, 36) & DIRECTORY != 0;
+            let size = (flags & FD_FILESIZE != 0)
+                .then(|| (u64::from(word(descriptor, 64)) << 32) | u64::from(word(descriptor, 68)));
+            Some(Descriptor {
+                name: wide_text(&descriptor[72..592]),
+                directory,
+                size,
+            })
+        })
+        .collect()
+}
+
+/// The name a virtual file is written under: the last part of the name it
+/// was given (a file in a dragged folder names the folder too), without
+/// the characters a Windows name cannot hold, never a device's name, and
+/// none when nothing is left.
+fn virtual_file_name(given: &str) -> Option<String> {
+    let last = given.rsplit(['\\', '/']).next().unwrap_or(given);
+    let cleaned: String = last
+        .chars()
+        .filter(|character| !matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
+        .filter(|character| !character.is_control())
+        .collect();
+    let cleaned = cleaned.trim_end_matches(['.', ' ']).trim_start();
+    if cleaned.is_empty() {
+        return None;
+    }
+    let stem = cleaned
+        .split('.')
+        .next()
+        .unwrap_or(cleaned)
+        .trim_end()
+        .to_ascii_uppercase();
+    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
+        || (stem.len() == 4
+            && (stem.starts_with("COM") || stem.starts_with("LPT"))
+            && stem.as_bytes()[3].is_ascii_digit());
+    Some(if device {
+        format!("_{cleaned}")
+    } else {
+        cleaned.to_owned()
+    })
+}
+
+/// `name` in `folder`, or `stem (2).ext` and on when that is taken.
+fn unused_path(folder: &Path, name: &str) -> PathBuf {
+    let path = folder.join(name);
+    if !path.exists() {
+        return path;
+    }
+    let (stem, extension) = match name.rsplit_once('.') {
+        Some((stem, extension)) if !stem.is_empty() => (stem, Some(extension)),
+        _ => (name, None),
+    };
+    (2..)
+        .map(|number| {
+            folder.join(match extension {
+                Some(extension) => format!("{stem} ({number}).{extension}"),
+                None => format!("{stem} ({number})"),
+            })
+        })
+        .find(|path| !path.exists())
+        .unwrap()
+}
+
+/// A folder no other drop of this process has used, inside `directory`.
+fn drop_folder(directory: &Path) -> std::io::Result<PathBuf> {
+    use std::sync::atomic::{AtomicU64, Ordering};
+    static COUNT: AtomicU64 = AtomicU64::new(0);
+    let now = std::time::SystemTime::now()
+        .duration_since(std::time::UNIX_EPOCH)
+        .map_or(0, |since| since.as_nanos());
+    let folder = directory.join(format!(
+        "promised-{now:x}-{:x}-{:x}",
+        std::process::id(),
+        COUNT.fetch_add(1, Ordering::Relaxed)
+    ));
+    std::fs::create_dir_all(&folder)?;
+    Ok(folder)
+}
+
+/// Writes the virtual files `group` describes (folders left out) into a
+/// folder made for this drop, while the source still holds them, and
+/// reports each: its path, or why it was not written (a file that failed
+/// part-way is removed).
+fn write_virtual_files(data: &IDataObject, group: &[u8]) -> Option<PromisedFiles> {
+    let files: Vec<(usize, Descriptor)> = file_group(group)
+        .into_iter()
+        .enumerate()
+        .filter(|(_, file)| !file.directory)
+        .collect();
+    if files.is_empty() {
+        return None;
+    }
+    let folder = drop_folder(&gpui::promised_files_directory())
+        .map_err(|error| log::error!("no folder for dropped virtual files: {error}"))
+        .ok()?;
+    let (promised, sender) = PromisedFiles::new(folder.clone(), files.len());
+    for (index, file) in files {
+        let report: PromisedFile = (|| {
+            let name = virtual_file_name(&file.name)
+                .ok_or_else(|| format!("no usable name in \"{}\"", file.name))?;
+            let path = unused_path(&folder, &name);
+            let mut out = std::fs::File::create(&path).map_err(|error| error.to_string())?;
+            let limit = file.size.unwrap_or(u64::MAX);
+            let written = copy(data, *FILE_CONTENTS, index as i32, limit, &mut out)
+                .and_then(|()| out.flush().map_err(|error| error.to_string()))
+                .and_then(|()| match (file.size, out.metadata()) {
+                    (Some(size), Ok(meta)) if meta.len() != size => {
+                        Err("the file is shorter than it was said to be".into())
+                    }
+                    _ => Ok(()),
+                });
+            drop(out);
+            match written {
+                Ok(()) => Ok(path),
+                Err(error) => {
+                    let _ = std::fs::remove_file(&path);
+                    Err(error)
+                }
+            }
+        })();
+        let _ = sender.unbounded_send(report);
+    }
+    Some(promised)
+}
+
+/// [`read`], for the application's test of a drop of its own making.
+pub fn read_drop_data(data: &IDataObject) -> ExternalDropData {
+    read(data)
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
+/// What `data` offers besides files: its virtual files, written into a
+/// folder of the drop's own, one image's data, its address, HTML and text.
+/// Nothing of a drag of files.
+pub(crate) fn read(data: &IDataObject) -> ExternalDropData {
+    let mut dropped = ExternalDropData::default();
+    if offers(data, CF_HDROP.0) {
+        return dropped;
+    }
+    if let Some(group) = bytes(data, *FILE_GROUP, -1) {
+        dropped.promised = write_virtual_files(data, &group);
+    }
+    let image = bytes(data, *PNG, -1)
+        .map(|png| (png, "image/png"))
+        .or_else(|| {
+            [CF_DIBV5.0, CF_DIB.0].into_iter().find_map(|format| {
+                let dib = bytes(data, format, -1)?;
+                Some((crate::clipboard::convert_dib_to_bmp(&dib)?, "image/bmp"))
+            })
+        });
+    if let Some((bytes, media_type)) = image {
+        dropped.files.push(ExternalDropFile {
+            bytes,
+            name: None,
+            media_type: Some(media_type.to_owned()),
+        });
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
+
+/// A data object holding virtual files as Outlook and a browser drag them,
+/// for the application's test of a drop of its own making: one
+/// `FILEGROUPDESCRIPTORW` with each file's name, flags (`FD_ATTRIBUTES`,
+/// and `FD_FILESIZE` with the size given) and attributes, and each file's
+/// `FileContents` at its index, from global memory or, when asked, a
+/// stream. Every other format is refused.
+pub fn virtual_files_object(files: Vec<VirtualFile>) -> IDataObject {
+    VirtualFiles(files).into()
+}
+
+/// A file [`virtual_files_object`] holds.
+pub struct VirtualFile {
+    pub name: String,
+    pub contents: Vec<u8>,
+    /// A folder (its contents are never asked for).
+    pub directory: bool,
+    /// The size its descriptor gives, when it gives one.
+    pub size: Option<u64>,
+    /// Handed over as a stream rather than global memory.
+    pub stream: bool,
+}
+
+#[implement(IDataObject)]
+struct VirtualFiles(Vec<VirtualFile>);
+
+impl VirtualFiles {
+    fn group(&self) -> Vec<u8> {
+        let mut group = (self.0.len() as u32).to_le_bytes().to_vec();
+        for file in &self.0 {
+            let mut descriptor = vec![0u8; DESCRIPTOR];
+            let flags = FD_ATTRIBUTES | file.size.map_or(0, |_| FD_FILESIZE);
+            descriptor[0..4].copy_from_slice(&flags.to_le_bytes());
+            let attributes: u32 = if file.directory { DIRECTORY } else { 0x80 };
+            descriptor[36..40].copy_from_slice(&attributes.to_le_bytes());
+            let size = file.size.unwrap_or(0);
+            descriptor[64..68].copy_from_slice(&((size >> 32) as u32).to_le_bytes());
+            descriptor[68..72].copy_from_slice(&(size as u32).to_le_bytes());
+            for (index, unit) in file.name.encode_utf16().take(259).enumerate() {
+                descriptor[72 + index * 2..74 + index * 2].copy_from_slice(&unit.to_le_bytes());
+            }
+            group.extend(descriptor);
+        }
+        group
+    }
+}
+
+fn global_medium(bytes: &[u8]) -> windows::core::Result<windows::Win32::System::Com::STGMEDIUM> {
+    use windows::Win32::System::Com::{STGMEDIUM, STGMEDIUM_0};
+    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
+    // SAFETY: the memory is allocated, filled while locked and handed to the
+    // receiver, who frees it with `ReleaseStgMedium`.
+    unsafe {
+        let global = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1))?;
+        let target = GlobalLock(global);
+        if target.is_null() {
+            return Err(windows::Win32::Foundation::E_OUTOFMEMORY.into());
+        }
+        std::ptr::copy_nonoverlapping(bytes.as_ptr(), target.cast::<u8>(), bytes.len());
+        let _ = GlobalUnlock(global);
+        Ok(STGMEDIUM {
+            tymed: TYMED_HGLOBAL.0 as u32,
+            u: STGMEDIUM_0 { hGlobal: global },
+            pUnkForRelease: std::mem::ManuallyDrop::new(None),
+        })
+    }
+}
+
+#[allow(non_snake_case)]
+impl IDataObject_Impl for VirtualFiles_Impl {
+    fn GetData(
+        &self,
+        format: *const FORMATETC,
+    ) -> windows::core::Result<windows::Win32::System::Com::STGMEDIUM> {
+        use windows::Win32::Foundation::DV_E_FORMATETC;
+        use windows::Win32::System::Com::{STGMEDIUM, STGMEDIUM_0};
+        // SAFETY: OLE passes a valid format or null.
+        let Some(format) = (unsafe { format.as_ref() }) else {
+            return Err(DV_E_FORMATETC.into());
+        };
+        if format.cfFormat == *FILE_GROUP && format.tymed & TYMED_HGLOBAL.0 as u32 != 0 {
+            return global_medium(&self.group());
+        }
+        let file = usize::try_from(format.lindex)
+            .ok()
+            .and_then(|index| self.0.get(index))
+            .filter(|file| !file.directory);
+        let (true, Some(file)) = (format.cfFormat == *FILE_CONTENTS, file) else {
+            return Err(DV_E_FORMATETC.into());
+        };
+        if file.stream && format.tymed & TYMED_ISTREAM.0 as u32 != 0 {
+            // SAFETY: a stream over a copy of the bytes, handed to the
+            // receiver, who releases it with `ReleaseStgMedium`.
+            let stream = unsafe {
+                windows::Win32::UI::Shell::SHCreateMemStream(Some(file.contents.as_slice()))
+            }
+            .ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_OUTOFMEMORY))?;
+            return Ok(STGMEDIUM {
+                tymed: TYMED_ISTREAM.0 as u32,
+                u: STGMEDIUM_0 {
+                    pstm: std::mem::ManuallyDrop::new(Some(stream)),
+                },
+                pUnkForRelease: std::mem::ManuallyDrop::new(None),
+            });
+        }
+        if format.tymed & TYMED_HGLOBAL.0 as u32 != 0 {
+            return global_medium(&file.contents);
+        }
+        Err(DV_E_FORMATETC.into())
+    }
+
+    fn GetDataHere(
+        &self,
+        _format: *const FORMATETC,
+        _medium: *mut windows::Win32::System::Com::STGMEDIUM,
+    ) -> windows::core::Result<()> {
+        Err(windows::Win32::Foundation::E_NOTIMPL.into())
+    }
+
+    fn QueryGetData(&self, format: *const FORMATETC) -> windows::core::HRESULT {
+        // SAFETY: OLE passes a valid format or null.
+        let offered = unsafe { format.as_ref() }.is_some_and(|format| {
+            format.cfFormat == *FILE_GROUP || format.cfFormat == *FILE_CONTENTS
+        });
+        if offered {
+            S_OK
+        } else {
+            windows::Win32::Foundation::DV_E_FORMATETC
+        }
+    }
+
+    fn GetCanonicalFormatEtc(
+        &self,
+        _format: *const FORMATETC,
+        out: *mut FORMATETC,
+    ) -> windows::core::HRESULT {
+        // SAFETY: OLE passes a writable format or null.
+        if let Some(out) = unsafe { out.as_mut() } {
+            out.ptd = std::ptr::null_mut();
+        }
+        windows::Win32::Foundation::DATA_S_SAMEFORMATETC
+    }
+
+    fn SetData(
+        &self,
+        _format: *const FORMATETC,
+        _medium: *const windows::Win32::System::Com::STGMEDIUM,
+        _release: windows::core::BOOL,
+    ) -> windows::core::Result<()> {
+        Err(windows::Win32::Foundation::E_NOTIMPL.into())
+    }
+
+    fn EnumFormatEtc(
+        &self,
+        _direction: u32,
+    ) -> windows::core::Result<windows::Win32::System::Com::IEnumFORMATETC> {
+        Err(windows::Win32::Foundation::E_NOTIMPL.into())
+    }
+
+    fn DAdvise(
+        &self,
+        _format: *const FORMATETC,
+        _flags: u32,
+        _sink: windows::core::Ref<windows::Win32::System::Com::IAdviseSink>,
+    ) -> windows::core::Result<u32> {
+        Err(windows::Win32::Foundation::OLE_E_ADVISENOTSUPPORTED.into())
+    }
+
+    fn DUnadvise(&self, _connection: u32) -> windows::core::Result<()> {
+        Err(windows::Win32::Foundation::OLE_E_ADVISENOTSUPPORTED.into())
+    }
+
+    fn EnumDAdvise(&self) -> windows::core::Result<windows::Win32::System::Com::IEnumSTATDATA> {
+        Err(windows::Win32::Foundation::OLE_E_ADVISENOTSUPPORTED.into())
+    }
+}
diff --color -ruN gpui-pre-windows-0.3.12/src/gpui_windows.rs b/gpui-pre-windows/src/gpui_windows.rs
--- a/gpui-pre-windows/src/gpui_windows.rs
+++ b/gpui-pre-windows/src/gpui_windows.rs
@@ -10,6 +10,8 @@
 mod directx_renderer;
 mod dispatcher;
 mod display;
+mod drag_source;
+mod drop_data;
 mod events;
 mod keyboard;
 mod platform;
@@ -38,6 +40,8 @@
 pub(crate) use window::*;
 pub(crate) use wrapper::*;
 
+pub use drag_source::drop_files as drag_drop_files;
+pub use drop_data::{VirtualFile, html_fragment, read_drop_data, virtual_files_object};
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
@@ -941,13 +941,18 @@
         }
     }
 
-    if (shadow.inset != 0u) {
+    if (shadow.inset == 1u) {
         // The inset shadow is the complement of the (blurred) hole rect, clipped to the element.
         // `saturate(0.5 - d)` gives a 1-pixel antialiased edge: d <= -0.5 -> 1, d >= 0.5 -> 0.
         alpha = 1.0 - alpha;
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
@@ -909,6 +909,18 @@
         true
     }
 
+    fn is_background_blur_supported(&self) -> bool {
+        // `Blurred` is Acrylic, which Windows has had since Windows 10 version
+        // 1809 (build 17763). Acrylic shows only where the window's pixels are
+        // transparent, and they can be transparent only in a window that
+        // DirectComposition draws. Such a window is created without a
+        // redirection bitmap. A window created while
+        // `GPUI_DISABLE_DIRECT_COMPOSITION` is set has one, and is opaque.
+        let ex_style = WINDOW_EX_STYLE(unsafe { get_window_long(self.0.hwnd, GWL_EXSTYLE) } as _);
+        ex_style.contains(WS_EX_NOREDIRECTIONBITMAP)
+            && windows_build_number() >= ACRYLIC_ACCENT_BUILD
+    }
+
     fn set_title(&mut self, title: &str) {
         unsafe { SetWindowTextW(self.0.hwnd, &HSTRING::from(title)) }
             .inspect_err(|e| log::error!("Set title failed: {e}"))
@@ -916,9 +928,22 @@
     }
 
     fn set_background_appearance(&self, background_appearance: WindowBackgroundAppearance) {
-        self.state.background_appearance.set(background_appearance);
+        let previous = self
+            .state
+            .background_appearance
+            .replace(background_appearance);
         let hwnd = self.0.hwnd;
 
+        // On Windows 11 version 22H2 and later, `Blurred` sets a system
+        // backdrop (see below). The backdrop stays on the window until it is
+        // taken off, so take it off when the window stops being `Blurred`.
+        // DWMSBT_AUTO (0) is the value a new window has.
+        if previous == WindowBackgroundAppearance::Blurred
+            && background_appearance != WindowBackgroundAppearance::Blurred
+        {
+            dwm_set_window_composition_attribute(hwnd, 0);
+        }
+
         // using Dwm APIs for Mica and MicaAlt backdrops.
         // others follow the set_window_composition_attribute approach
         match background_appearance {
@@ -929,7 +954,20 @@
                 set_window_composition_attribute(hwnd, None, 2);
             }
             WindowBackgroundAppearance::Blurred => {
-                set_window_composition_attribute(hwnd, Some((0, 0, 0, 0)), 4);
+                if windows_build_number() >= SYSTEM_BACKDROP_BUILD {
+                    // Windows 11 version 22H2 and later: Acrylic through the
+                    // documented attribute `DWMWA_SYSTEMBACKDROP_TYPE`, as
+                    // DWMSBT_TRANSIENTWINDOW (3). The accent policy is turned
+                    // off, so the window shows only this backdrop.
+                    set_window_composition_attribute(hwnd, None, 0);
+                    dwm_set_window_composition_attribute(hwnd, 3);
+                } else {
+                    // Windows 10 version 1809 up to Windows 11 version 21H2
+                    // have no such attribute. There Acrylic is the accent
+                    // policy ACCENT_ENABLE_ACRYLICBLURBEHIND (4) of the
+                    // undocumented `SetWindowCompositionAttribute`.
+                    set_window_composition_attribute(hwnd, Some((0, 0, 0, 0)), 4);
+                }
             }
             WindowBackgroundAppearance::MicaBackdrop => {
                 // DWMSBT_MAINWINDOW => MicaBase
@@ -974,6 +1012,38 @@
         self.state.callbacks.request_frame.set(Some(callback));
     }
 
+    fn can_start_external_drag(&self) -> bool {
+        true
+    }
+
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
@@ -1204,6 +1274,24 @@
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
@@ -1274,6 +1362,7 @@
                 .ok()
                 .log_err();
         }
+        gpui::set_external_drop_data(crate::drop_data::read(idata_obj));
         let scale_factor = self.0.state.scale_factor.get();
         let input = PlatformInput::FileDrop(FileDropEvent::Submit {
             position: logical_point(
@@ -1601,6 +1690,26 @@
     let bounds = bounds.to_device_pixels(display.scale_factor());
     placement.rcNormalPosition = calculate_window_rect(bounds, border_offset);
     Ok(placement)
+}
+
+/// The first build of Windows that has the Acrylic accent policy of
+/// `SetWindowCompositionAttribute`: Windows 10 version 1809.
+const ACRYLIC_ACCENT_BUILD: u32 = 17763;
+
+/// The first build of Windows that has the window attribute
+/// `DWMWA_SYSTEMBACKDROP_TYPE`: Windows 11 version 22H2.
+const SYSTEM_BACKDROP_BUILD: u32 = 22621;
+
+/// The build number of the Windows that is running, or 0 when it cannot be
+/// read.
+fn windows_build_number() -> u32 {
+    let mut version = unsafe { std::mem::zeroed() };
+    let status = unsafe { windows::Wdk::System::SystemServices::RtlGetVersion(&mut version) };
+    if status.is_ok() {
+        version.dwBuildNumber
+    } else {
+        0
+    }
 }
 
 fn dwm_set_window_composition_attribute(hwnd: HWND, backdrop_type: u32) {
diff --color -ruN gpui-pre-macos-0.3.12/src/drop_data.rs b/gpui-pre-macos/src/drop_data.rs
--- a/gpui-pre-macos/src/drop_data.rs
+++ b/gpui-pre-macos/src/drop_data.rs
@@ -0,0 +1,206 @@
+//! What a drag from another application offers besides files: image data,
+//! links, HTML and text, as a web browser drags them, and files it promises
+//! (`NSFilePromiseReceiver`), as Photos, Mail, Safari and Messages drag
+//! theirs.
+use block2::RcBlock;
+use gpui::{ExternalDropData, ExternalDropFile, PromisedFile, PromisedFiles};
+use objc2::{class, msg_send, rc::Retained, runtime::AnyObject};
+use objc2_foundation::{NSArray, NSData, NSString};
+use std::path::PathBuf;
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
+
+/// The pasteboard types of a file promise, old and new
+/// (`NSFilePromiseReceiver.readableDraggedTypes`).
+pub(crate) fn promise_types() -> Vec<Retained<NSString>> {
+    // SAFETY: a class method that answers an array of strings.
+    unsafe {
+        let types: Option<Retained<NSArray<NSString>>> =
+            msg_send![class!(NSFilePromiseReceiver), readableDraggedTypes];
+        types.map(|types| types.to_vec()).unwrap_or_default()
+    }
+}
+
+/// Whether `pasteboard` holds a file promise.
+pub(crate) fn offers_promised_files(pasteboard: &AnyObject) -> bool {
+    let promised = promise_types();
+    // SAFETY: NSPasteboard's `types`, an array of strings or nil.
+    unsafe {
+        let types: Option<Retained<NSArray<NSString>>> = msg_send![pasteboard, types];
+        types
+            .iter()
+            .flat_map(|types| types.iter())
+            .any(|kind| promised.iter().any(|wanted| **wanted == *kind))
+    }
+}
+
+/// A folder no other drop of this process has used, inside `directory`.
+fn drop_folder(directory: &std::path::Path) -> std::io::Result<PathBuf> {
+    use std::sync::atomic::{AtomicU64, Ordering};
+    static COUNT: AtomicU64 = AtomicU64::new(0);
+    let now = std::time::SystemTime::now()
+        .duration_since(std::time::UNIX_EPOCH)
+        .map_or(0, |since| since.as_nanos());
+    let folder = directory.join(format!(
+        "promised-{now:x}-{:x}-{:x}",
+        std::process::id(),
+        COUNT.fetch_add(1, Ordering::Relaxed)
+    ));
+    std::fs::create_dir_all(&folder)?;
+    Ok(folder)
+}
+
+/// Asks the promises on the dragging pasteboard to write their files into a
+/// folder made for this drop. AppKit allows this only while the drop is
+/// being performed; the files are written afterwards, each reported on a
+/// queue of its own (never the main thread, where a source in this process
+/// would wait on itself). `None` when nothing is promised or the folder
+/// cannot be made.
+pub(crate) fn receive_promised_files(pasteboard: &AnyObject) -> Option<PromisedFiles> {
+    // SAFETY: NSPasteboard's, NSFilePromiseReceiver's, NSURL's,
+    // NSOperationQueue's and NSError's documented selectors with the
+    // argument types they take; each answer is retained or nil.
+    unsafe {
+        let classes: Retained<NSArray<AnyObject>> =
+            msg_send![class!(NSArray), arrayWithObject: class!(NSFilePromiseReceiver)];
+        let receivers: Option<Retained<NSArray<AnyObject>>> = msg_send![
+            pasteboard,
+            readObjectsForClasses: &*classes,
+            options: std::ptr::null::<AnyObject>()
+        ];
+        let receivers = receivers?.to_vec();
+        if receivers.is_empty() {
+            return None;
+        }
+        // One file for each type a receiver promises.
+        let count: usize = receivers
+            .iter()
+            .map(|receiver| {
+                let types: Option<Retained<NSArray<NSString>>> = msg_send![&**receiver, fileTypes];
+                types.map_or(0, |types| types.len()).max(1)
+            })
+            .sum();
+        let folder = drop_folder(&gpui::promised_files_directory())
+            .map_err(|error| log::error!("no folder for promised files: {error}"))
+            .ok()?;
+        let (promised, sender) = PromisedFiles::new(folder.clone(), count);
+        let destination: Retained<AnyObject> = msg_send![
+            class!(NSURL),
+            fileURLWithPath: &*NSString::from_str(&folder.to_string_lossy()),
+            isDirectory: objc2::runtime::Bool::YES
+        ];
+        let options: Retained<AnyObject> = msg_send![class!(NSDictionary), dictionary];
+        let queue: Retained<AnyObject> = msg_send![class!(NSOperationQueue), new];
+        let reader = RcBlock::new(move |url: *mut AnyObject, error: *mut AnyObject| {
+            let report: PromisedFile = match (url.as_ref(), error.as_ref()) {
+                (_, Some(error)) => {
+                    let text: Option<Retained<NSString>> = msg_send![error, localizedDescription];
+                    Err(text.map_or_else(String::new, |text| text.to_string()))
+                }
+                (Some(url), None) => {
+                    let path: Option<Retained<NSString>> = msg_send![url, path];
+                    path.map(|path| PathBuf::from(path.to_string()))
+                        .ok_or_else(String::new)
+                }
+                (None, None) => Err(String::new()),
+            };
+            let _ = sender.unbounded_send(report);
+        });
+        for receiver in &receivers {
+            let () = msg_send![
+                &**receiver,
+                receivePromisedFilesAtDestination: &*destination,
+                options: &*options,
+                operationQueue: &*queue,
+                reader: &*reader
+            ];
+        }
+        Some(promised)
+    }
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
@@ -1092,10 +1092,14 @@
                 deprecated,
                 reason = "File-drop decoding expects a property-list array of paths; replacing NSFilenamesPboardType requires migrating both registration and decoding to file URLs"
             )]
+            let mut dragged_types = vec![NSFilenamesPboardType.retain()];
+            // What a web browser drags: image data, links, HTML and text;
+            // and files promised rather than put on the drag.
+            dragged_types.extend(crate::drop_data::dragged_types());
+            dragged_types.extend(crate::drop_data::promise_types());
             let () = msg_send![
                 native_window,
-                registerForDraggedTypes:
-                    &*NSArray::from_slice(&[NSFilenamesPboardType])
+                registerForDraggedTypes: &*NSArray::from_retained_slice(&dragged_types)
             ];
             let () = msg_send![
                 native_window,
@@ -1927,6 +1931,20 @@
         self.0.as_ref().lock().background_appearance
     }
 
+    fn is_background_blur_supported(&self) -> bool {
+        // With Reduce Transparency turned on (System Settings > Accessibility >
+        // Display), AppKit does not blur what is behind an `NSVisualEffectView`.
+        // It fills the view with a solid colour, and `blurred_view_update_layer`
+        // removes that fill, so the window would show what is behind it without
+        // any blur.
+        unsafe {
+            let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
+            let reduce_transparency: Bool =
+                msg_send![workspace, accessibilityDisplayShouldReduceTransparency];
+            !reduce_transparency.as_bool()
+        }
+    }
+
     fn is_subpixel_rendering_supported(&self) -> bool {
         false
     }
@@ -2311,7 +2329,16 @@
                 NSSize::new(32., 32.),
             );
 
-            for (path, is_directory) in paths.entries() {
+            // Every file gets a dragging item, because the items are what the
+            // drop receives. Only the first few items get a picture. AppKit
+            // asks each item for its picture and draws them all, so a drag
+            // of thousands of files with a picture each took seconds to
+            // start and drew thousands of icons on top of one another. With
+            // a picture on only the first few, the drag shows a small pile
+            // of icons, and AppKit adds its own badge with the number of
+            // files.
+            const ITEMS_WITH_A_PICTURE: usize = 4;
+            for (index, (path, is_directory)) in paths.entries().iter().enumerate() {
                 // Preserve non-UTF-8 paths
                 let Ok(path_bytes) = CString::new(path.as_os_str().as_bytes()) else {
                     log::warn!("start_external_drag skipped path containing an interior nul byte");
@@ -2340,7 +2367,12 @@
                     .extension()
                     .and_then(|extension| extension.to_str())
                     .map(String::from);
+                let has_picture = index < ITEMS_WITH_A_PICTURE;
                 let provider = RcBlock::new(move || {
+                    if !has_picture {
+                        let none = NSArray::<NSDraggingImageComponent>::new();
+                        return NonNull::new_unchecked(Retained::autorelease_return(none));
+                    }
                     let component = NSDraggingImageComponent::draggingImageComponentWithKey(
                         NSDraggingImageComponentIconKey,
                     );
@@ -2382,6 +2414,9 @@
 
             let started = !session.is_null();
             if started {
+                // `NSDraggingFormationPile` (2): the pictures are drawn on
+                // top of one another, as Finder draws several files.
+                let _: () = msg_send![session, setDraggingFormation: 2isize];
                 self.0.lock().synthetic_drag_counter += 1;
             }
             log::debug!(
@@ -3749,6 +3784,20 @@
 ) -> Bool {
     let window_state = unsafe { get_window_state(this) };
     let position = drag_event_position(&window_state, dragging_info);
+    let pasteboard: *mut AnyObject = unsafe { msg_send![dragging_info, draggingPasteboard] };
+    let pasteboard = unsafe { pasteboard.as_ref() };
+    let mut data = pasteboard
+        .map(crate::drop_data::read_drop_pasteboard)
+        .unwrap_or_default();
+    // Files promised by a drag that carries no files of its own are asked
+    // for now, the only time AppKit allows it; they arrive after the drop.
+    if let Some(pasteboard) = pasteboard
+        && external_paths_from_event(dragging_info).is_some_and(|paths| paths.paths().is_empty())
+        && crate::drop_data::offers_promised_files(pasteboard)
+    {
+        data.promised = crate::drop_data::receive_promised_files(pasteboard);
+    }
+    gpui::set_external_drop_data(data);
     send_file_drop_event(window_state, FileDropEvent::Submit { position }).to_objc()
 }
 
@@ -3764,7 +3813,15 @@
     )]
     let filenames: *mut NSArray<NSString> =
         unsafe { msg_send![pasteboard, propertyListForType: NSFilenamesPboardType] };
-    let filenames = unsafe { filenames.as_ref() }?;
+    // A drag of what is not a file (an image or a link from a web page, a
+    // promised file) enters with no paths; its data is read, and promised
+    // files asked for, when it is dropped.
+    let Some(filenames) = (unsafe { filenames.as_ref() }) else {
+        let pasteboard = unsafe { &*pasteboard };
+        return (crate::drop_data::offers_drop_data(pasteboard)
+            || crate::drop_data::offers_promised_files(pasteboard))
+        .then(|| ExternalPaths(paths));
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
+pub use linux::{DirectSaveSource, receive_direct_save, xdnd_uri_list};
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
@@ -27,6 +29,8 @@
 pub(crate) use wayland::*;
 #[cfg(feature = "x11")]
 pub(crate) use x11::*;
+#[cfg(feature = "x11")]
+pub use x11::{DirectSaveSource, receive_direct_save, xdnd_uri_list};
 
 use std::rc::Rc;
 
diff --color -ruN gpui-pre-linux-0.3.12/src/linux/drop_data.rs b/gpui-pre-linux/src/linux/drop_data.rs
--- a/gpui-pre-linux/src/linux/drop_data.rs
+++ b/gpui-pre-linux/src/linux/drop_data.rs
@@ -0,0 +1,96 @@
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
+/// The type of an X11 drag that offers to save its file (XDS).
+pub(crate) const DIRECT_SAVE: &str = "XdndDirectSave0";
+
+/// Whether a drag offering `offered` carries anything [`wanted`] reads, or
+/// a file it offers to save.
+pub(crate) fn takes(offered: &[String]) -> bool {
+    offered.iter().any(|kind| {
+        kind == URI_LIST
+            || kind == DIRECT_SAVE
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
@@ -1,13 +1,17 @@
 mod client;
 mod clipboard;
+mod direct_save;
 mod display;
 mod event;
 mod window;
+mod xdnd_source;
 mod xim_handler;
 
 pub(crate) use client::*;
 pub(crate) use clipboard::wait_for_clipboard_handovers;
+pub use direct_save::{DirectSaveSource, receive_direct_save};
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
@@ -981,12 +1083,45 @@
                     window
                         .handle_input(PlatformInput::FileDrop(FileDropEvent::Pending { position }));
                 } else if event.type_ == state.atoms.XdndDrop {
-                    xdnd_send_finished(
-                        &state.xcb_connection,
-                        &state.atoms,
-                        event.window,
-                        state.xdnd_state.other_window,
-                    );
+                    // What a drag without files offers (a web browser's
+                    // image, its addresses), read while the source still
+                    // holds the selection. Never from a drag this process
+                    // makes, which could not answer while it waits here.
+                    let source = state.xdnd_state.other_window;
+                    let foreign =
+                        !state.xdnd_state.has_paths && !state.windows.contains_key(&source);
+                    let mut data = if foreign {
+                        let kinds = crate::linux::drop_data::wanted(&state.xdnd_state.offered);
+                        crate::linux::drop_data::assemble(state.clipboard.read_xdnd(&kinds))
+                    } else {
+                        gpui::ExternalDropData::default()
+                    };
+                    // A file the source offers to save (XDS) is asked for on
+                    // a thread, which tells the source the drop is finished
+                    // once it has the source's answer.
+                    let direct_save = foreign
+                        && state
+                            .xdnd_state
+                            .offered
+                            .iter()
+                            .any(|kind| kind == crate::linux::drop_data::DIRECT_SAVE);
+                    if direct_save {
+                        data.promised = Some(super::direct_save::receive(
+                            state.clipboard.display(),
+                            source,
+                            event.window,
+                            arg2,
+                        ));
+                    }
+                    gpui::set_external_drop_data(data);
+                    if !direct_save {
+                        xdnd_send_finished(
+                            &state.xcb_connection,
+                            &state.atoms,
+                            event.window,
+                            state.xdnd_state.other_window,
+                        );
+                    }
                     let position = state.xdnd_state.position;
                     drop(state);
                     window
@@ -994,6 +1129,15 @@
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
@@ -1024,13 +1168,16 @@
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
@@ -1309,6 +1456,47 @@
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
@@ -1334,6 +1522,26 @@
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
@@ -2406,6 +2614,39 @@
         }
     }
     0
+}
+
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
 }
 
 fn xdnd_send_finished(
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
@@ -979,6 +991,37 @@
             server_handle: join_handle,
         });
         Ok(Self { inner: ctx })
+    }
+
+    /// What a drag-and-drop source offers in each of `kinds` (media types),
+    /// read from `XdndSelection` on a connection of its own, as the
+    /// clipboard is read; a kind that cannot be read is left out.
+    /// The X display the clipboard's connections use.
+    pub(crate) fn display(&self) -> &str {
+        &self.inner.display
+    }
+
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
diff --color -ruN gpui-pre-linux-0.3.12/src/linux/x11/direct_save.rs b/gpui-pre-linux/src/linux/x11/direct_save.rs
--- a/gpui-pre-linux/src/linux/x11/direct_save.rs
+++ b/gpui-pre-linux/src/linux/x11/direct_save.rs
@@ -0,0 +1,413 @@
+//! Files an X11 drag offers to save rather than put on the drag: the Direct
+//! Save protocol (XDS, `XdndDirectSave0`), the X11 form of a file promise,
+//! which web browsers use for a dragged image or link and archive managers
+//! for a file inside an archive. The source names the file in the
+//! `XdndDirectSave0` property of its window; on the drop the target puts a
+//! `file:` address there, asks for the `XdndDirectSave0` target of
+//! `XdndSelection`, and the source writes the file and answers `S` (saved),
+//! `E` (an error) or `F` (it will not; the target may read the data itself,
+//! which this side does not do).
+use std::path::{Path, PathBuf};
+use std::time::{Duration, Instant};
+
+use gpui::{PromisedFile, PromisedFiles};
+use x11rb::{
+    COPY_DEPTH_FROM_PARENT, COPY_FROM_PARENT,
+    connection::Connection,
+    protocol::{
+        Event,
+        xproto::{
+            AtomEnum, ClientMessageData, ClientMessageEvent, ConnectionExt as _, CreateWindowAux,
+            EventMask, PropMode, Window, WindowClass,
+        },
+    },
+    rust_connection::RustConnection,
+    wrapper::ConnectionExt as _,
+};
+
+use crate::linux::drop_data::DIRECT_SAVE;
+/// How long the source may take to write the file (a browser may fetch it).
+const ANSWER_WAIT: Duration = Duration::from_secs(120);
+
+/// The name a source suggests, as a name in one folder: its last part, and
+/// none when nothing usable is left.
+fn file_name(suggested: &str) -> Option<String> {
+    let name = suggested.rsplit('/').next().unwrap_or(suggested).trim();
+    let name: String = name
+        .chars()
+        .filter(|character| *character != '\0')
+        .collect();
+    (!name.is_empty() && name != "." && name != "..").then_some(name)
+}
+
+/// `name` in `folder`, or `stem (2).ext` and on when that is taken.
+fn unused_path(folder: &Path, name: &str) -> PathBuf {
+    let path = folder.join(name);
+    if !path.exists() {
+        return path;
+    }
+    let (stem, extension) = match name.rsplit_once('.') {
+        Some((stem, extension)) if !stem.is_empty() => (stem, Some(extension)),
+        _ => (name, None),
+    };
+    (2..)
+        .map(|number| {
+            folder.join(match extension {
+                Some(extension) => format!("{stem} ({number}).{extension}"),
+                None => format!("{stem} ({number})"),
+            })
+        })
+        .find(|path| !path.exists())
+        .unwrap()
+}
+
+/// A folder no other drop of this process has used, inside `directory`.
+fn drop_folder(directory: &Path) -> std::io::Result<PathBuf> {
+    use std::sync::atomic::{AtomicU64, Ordering};
+    static COUNT: AtomicU64 = AtomicU64::new(0);
+    let now = std::time::SystemTime::now()
+        .duration_since(std::time::UNIX_EPOCH)
+        .map_or(0, |since| since.as_nanos());
+    let folder = directory.join(format!(
+        "promised-{now:x}-{:x}-{:x}",
+        std::process::id(),
+        COUNT.fetch_add(1, Ordering::Relaxed)
+    ));
+    std::fs::create_dir_all(&folder)?;
+    Ok(folder)
+}
+
+/// This machine's name, which a `file:` address for XDS carries.
+fn host_name() -> String {
+    let mut buffer = [0u8; 256];
+    // SAFETY: the buffer is writable for its length; the name is cut at its
+    // nul (or the buffer's end).
+    let result = unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) };
+    if result != 0 {
+        return String::new();
+    }
+    let end = buffer
+        .iter()
+        .position(|byte| *byte == 0)
+        .unwrap_or(buffer.len());
+    String::from_utf8_lossy(&buffer[..end]).into_owned()
+}
+
+fn atom(conn: &RustConnection, name: &str) -> Result<u32, String> {
+    Ok(conn
+        .intern_atom(false, name.as_bytes())
+        .map_err(|error| error.to_string())?
+        .reply()
+        .map_err(|error| error.to_string())?
+        .atom)
+}
+
+/// Asks the source of the drag being dropped (`source`, its window) to save
+/// the file it offers into a folder made for this drop, on a thread and a
+/// connection of its own, and then tells it the drop is finished on
+/// `target`'s behalf (`XdndFinished`, which the caller does not send).
+/// `time` is the drop's.
+pub(crate) fn receive(display: &str, source: Window, target: Window, time: u32) -> PromisedFiles {
+    let folder = drop_folder(&gpui::promised_files_directory());
+    let (promised, sender) = PromisedFiles::new(
+        folder
+            .as_ref()
+            .cloned()
+            .unwrap_or_else(|_| gpui::promised_files_directory()),
+        1,
+    );
+    let display = display.to_owned();
+    std::thread::spawn(move || {
+        let report: PromisedFile = match (folder, RustConnection::connect(Some(&display))) {
+            (Err(error), _) => Err(error.to_string()),
+            (_, Err(error)) => Err(error.to_string()),
+            (Ok(folder), Ok((conn, screen))) => {
+                let report = save(&conn, screen, source, time, &folder);
+                let finished = finish(&conn, source, target, report.is_ok());
+                match (report, finished) {
+                    (Err(error), Err(also)) => Err(format!("{error}; {also}")),
+                    (report, finished) => {
+                        if let Err(error) = finished {
+                            log::warn!("{error}");
+                        }
+                        report
+                    }
+                }
+            }
+        };
+        if let Err(error) = &report {
+            log::warn!("a drag's direct save failed: {error}");
+        }
+        let _ = sender.unbounded_send(report);
+    });
+    promised
+}
+
+/// The exchange: the file's name from the source, the address it is to be
+/// saved at, the request and the source's answer.
+fn save(
+    conn: &RustConnection,
+    screen: usize,
+    source: Window,
+    time: u32,
+    folder: &Path,
+) -> PromisedFile {
+    let fail = |error: x11rb::errors::ConnectionError| error.to_string();
+    let direct_save = atom(conn, DIRECT_SAVE)?;
+    let text = atom(conn, "text/plain")?;
+    let selection = atom(conn, "XdndSelection")?;
+    let suggested = conn
+        .get_property(false, source, direct_save, AtomEnum::ANY, 0, 1024)
+        .map_err(fail)?
+        .reply()
+        .map_err(|error| error.to_string())?;
+    let name = file_name(&String::from_utf8_lossy(&suggested.value))
+        .ok_or("the source named no file to save")?;
+    let path = unused_path(folder, &name);
+    let mut address = url::Url::from_file_path(&path).map_err(|()| "not an absolute path")?;
+    let _ = address.set_host(
+        Some(&host_name())
+            .filter(|host| !host.is_empty())
+            .map(String::as_str),
+    );
+    conn.change_property8(
+        PropMode::REPLACE,
+        source,
+        direct_save,
+        text,
+        address.as_str().as_bytes(),
+    )
+    .map_err(fail)?;
+    // A window of this connection's own receives the answer.
+    let requestor = conn.generate_id().map_err(|error| error.to_string())?;
+    let root = conn.setup().roots.get(screen).ok_or("no screen")?.root;
+    conn.create_window(
+        COPY_DEPTH_FROM_PARENT,
+        requestor,
+        root,
+        0,
+        0,
+        1,
+        1,
+        0,
+        WindowClass::INPUT_ONLY,
+        COPY_FROM_PARENT,
+        &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
+    )
+    .map_err(fail)?;
+    conn.convert_selection(requestor, selection, direct_save, direct_save, time)
+        .map_err(fail)?;
+    conn.flush().map_err(fail)?;
+    let answer = wait_for_answer(conn, requestor, direct_save);
+    let _ = conn.destroy_window(requestor);
+    let _ = conn.flush();
+    let saved = match answer? {
+        answer if answer.starts_with(b"S") => {
+            std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file())
+        }
+        _ => false,
+    };
+    if saved {
+        Ok(path)
+    } else {
+        // Whatever the source left there is not taken.
+        let _ = std::fs::remove_file(&path);
+        Err("the source did not save the file".into())
+    }
+}
+
+/// The source's answer: the property it set on `requestor`.
+fn wait_for_answer(
+    conn: &RustConnection,
+    requestor: Window,
+    property: u32,
+) -> Result<Vec<u8>, String> {
+    let until = Instant::now() + ANSWER_WAIT;
+    while Instant::now() < until {
+        let event = conn.poll_for_event().map_err(|error| error.to_string())?;
+        let Some(event) = event else {
+            std::thread::sleep(Duration::from_millis(5));
+            continue;
+        };
+        if let Event::SelectionNotify(notify) = event
+            && notify.requestor == requestor
+        {
+            if notify.property == u32::from(AtomEnum::NONE) {
+                return Err("the source refused to save the file".into());
+            }
+            let reply = conn
+                .get_property(true, requestor, property, AtomEnum::ANY, 0, 16)
+                .map_err(|error| error.to_string())?
+                .reply()
+                .map_err(|error| error.to_string())?;
+            return Ok(reply.value);
+        }
+    }
+    Err("the source did not answer".into())
+}
+
+/// `XdndFinished` from `target` to `source`.
+fn finish(
+    conn: &RustConnection,
+    source: Window,
+    target: Window,
+    accepted: bool,
+) -> Result<(), String> {
+    let finished = atom(conn, "XdndFinished")?;
+    let copy = atom(conn, "XdndActionCopy")?;
+    let message = ClientMessageEvent {
+        response_type: x11rb::protocol::xproto::CLIENT_MESSAGE_EVENT,
+        format: 32,
+        sequence: 0,
+        window: source,
+        type_: finished,
+        data: ClientMessageData::from([
+            target,
+            u32::from(accepted),
+            if accepted { copy } else { 0 },
+            0,
+            0,
+        ]),
+    };
+    conn.send_event(false, source, EventMask::NO_EVENT, message)
+        .map_err(|error| format!("XdndFinished not sent: {error}"))?
+        .check()
+        .map_err(|error| format!("XdndFinished refused: {error:?}"))?;
+    conn.flush().map_err(|error| error.to_string())
+}
+
+/// A source of a direct-save drag, for the application's test: a window of
+/// a connection of its own that owns `XdndSelection`, names `name` in its
+/// `XdndDirectSave0` property and, asked to save, writes `contents` (or,
+/// with `fail`, half of them and answers `E`) at the address given. It
+/// answers once, then waits for `XdndFinished`.
+pub struct DirectSaveSource {
+    /// The source's window.
+    pub window: Window,
+    answered: std::thread::JoinHandle<Result<bool, String>>,
+}
+
+impl DirectSaveSource {
+    pub fn start(
+        display: Option<&str>,
+        name: &str,
+        contents: Vec<u8>,
+        fail: bool,
+    ) -> Result<Self, String> {
+        let (conn, screen) = RustConnection::connect(display).map_err(|error| error.to_string())?;
+        let fail_with = |error: x11rb::errors::ConnectionError| error.to_string();
+        let window = conn.generate_id().map_err(|error| error.to_string())?;
+        let root = conn.setup().roots[screen].root;
+        conn.create_window(
+            COPY_DEPTH_FROM_PARENT,
+            window,
+            root,
+            0,
+            0,
+            1,
+            1,
+            0,
+            WindowClass::INPUT_ONLY,
+            COPY_FROM_PARENT,
+            &CreateWindowAux::new(),
+        )
+        .map_err(fail_with)?;
+        let direct_save = atom(&conn, DIRECT_SAVE)?;
+        let text = atom(&conn, "text/plain")?;
+        let selection = atom(&conn, "XdndSelection")?;
+        let finished = atom(&conn, "XdndFinished")?;
+        conn.change_property8(
+            PropMode::REPLACE,
+            window,
+            direct_save,
+            text,
+            name.as_bytes(),
+        )
+        .map_err(fail_with)?;
+        conn.set_selection_owner(window, selection, x11rb::CURRENT_TIME)
+            .map_err(fail_with)?;
+        conn.flush().map_err(fail_with)?;
+        let answered = std::thread::spawn(move || -> Result<bool, String> {
+            let until = Instant::now() + Duration::from_secs(30);
+            let mut saved = None;
+            // What the source saw, for a failure's message.
+            let mut seen = Vec::new();
+            while Instant::now() < until {
+                let event = conn.poll_for_event().map_err(|error| error.to_string())?;
+                let Some(event) = event else {
+                    std::thread::sleep(Duration::from_millis(5));
+                    continue;
+                };
+                seen.push(format!("{event:?}"));
+                match event {
+                    Event::SelectionRequest(request) if request.target == direct_save => {
+                        let address = conn
+                            .get_property(false, window, direct_save, AtomEnum::ANY, 0, 4096)
+                            .map_err(|error| error.to_string())?
+                            .reply()
+                            .map_err(|error| error.to_string())?;
+                        let address = String::from_utf8_lossy(&address.value).into_owned();
+                        seen.push(address.clone());
+                        // The address names this machine, which the `url`
+                        // crate does not take for a path; its path is the
+                        // file's.
+                        let path = url::Url::parse(&address).ok().and_then(|address| {
+                            url::Url::parse(&format!("file://{}", address.path()))
+                                .ok()?
+                                .to_file_path()
+                                .ok()
+                        });
+                        let wrote = path.is_some_and(|path| {
+                            let bytes = if fail {
+                                &contents[..contents.len() / 2]
+                            } else {
+                                &contents[..]
+                            };
+                            std::fs::write(path, bytes).is_ok()
+                        }) && !fail;
+                        let answer: &[u8] = if wrote { b"S" } else { b"E" };
+                        conn.change_property8(
+                            PropMode::REPLACE,
+                            request.requestor,
+                            request.property,
+                            AtomEnum::STRING,
+                            answer,
+                        )
+                        .map_err(fail_with)?;
+                        let notify = x11rb::protocol::xproto::SelectionNotifyEvent {
+                            response_type: x11rb::protocol::xproto::SELECTION_NOTIFY_EVENT,
+                            sequence: 0,
+                            time: request.time,
+                            requestor: request.requestor,
+                            selection: request.selection,
+                            target: request.target,
+                            property: request.property,
+                        };
+                        conn.send_event(false, request.requestor, EventMask::NO_EVENT, notify)
+                            .map_err(fail_with)?;
+                        conn.flush().map_err(fail_with)?;
+                        saved = Some(wrote);
+                    }
+                    Event::ClientMessage(message) if message.type_ == finished => {
+                        return saved.ok_or_else(|| "finished before any request".to_owned());
+                    }
+                    _ => {}
+                }
+            }
+            Err(format!("no XdndFinished; the source saw {seen:?}"))
+        });
+        Ok(Self { window, answered })
+    }
+
+    /// Whether the file was written, once the drop was finished.
+    pub fn finished(self) -> Result<bool, String> {
+        self.answered
+            .join()
+            .map_err(|_| "the source's thread panicked".to_owned())?
+    }
+}
+
+/// [`receive`], for the application's test of a drop of its own making.
+pub fn receive_direct_save(display: &str, source: Window, target: Window) -> PromisedFiles {
+    receive(display, source, target, x11rb::CURRENT_TIME)
+}
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
@@ -1827,6 +1830,25 @@
             ),
         )
         .log_err();
+    }
+
+    fn can_start_external_drag(&self) -> bool {
+        true
+    }
+
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
     }
 
     fn start_window_move(&self) {
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
