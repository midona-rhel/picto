//! How long each animation in the app lasts and which easing curve it
//! follows. The comment on each constant says which animations use it.
//!
//! This file also has [`CubicBezier`], which computes the CSS
//! `cubic-bezier()` timing function those curves are written in, and
//! [`Fade`], which moves a value between 0 and 1 the way a CSS transition
//! does.
use std::time::Duration;

/// The easing curve most transitions in the app follow. [`Fade::value`]
/// uses it.
pub const EASE_DEFAULT: CubicBezier = CubicBezier::new(0.4, 0., 0.22, 1.);
/// The CSS `ease-in` curve. A dismissed context menu and a closing dialog
/// fade out on it.
pub const EASE_IN: CubicBezier = CubicBezier::new(0.42, 0., 1., 1.);
/// The CSS `ease-out` curve. A dialog's backdrop fades in on it. The flash
/// of light on the Accept or Reject button of the Inbox review pill, which
/// shows after the button is pressed, dims on it.
pub const EASE_OUT: CubicBezier = CubicBezier::new(0., 0., 0.58, 1.);
/// The CSS `ease-in-out` curve. The "Return to Top" button, the moving
/// segment of an indeterminate progress bar, the tag manager's loading
/// placeholders and the pulse of a syncing subscription's badge follow it.
pub const EASE_IN_OUT: CubicBezier = CubicBezier::new(0.42, 0., 0.58, 1.);
/// The curve a popover, a context menu and the task dialog follow while
/// they appear. It moves quickly at first and then slows down.
pub const EASE_SCALE_IN: CubicBezier = CubicBezier::new(0., 0.89, 0.44, 1.);
/// How long a popover or a context menu takes to appear. A popover's glass
/// panel grows from 96% of its size to its full size in this time. A context
/// menu fades in over this time. On macOS, where a context menu is shown in a
/// pop-up window of its own, the window is scaled from 96% to its full size
/// in this time instead.
pub const MENU_IN: Duration = Duration::from_millis(70);
/// The scale a popover's panel, or a context menu's pop-up window on
/// macOS, has at the moment it opens: 96% of its full size. It grows to its
/// full size over [`MENU_IN`].
pub const MENU_IN_SCALE: f32 = 0.96;
/// The scale a dismissed context menu shrinks to while it fades out: 95%
/// of its full size. [`menu_out`] computes the scale at each moment of that
/// animation.
const MENU_OUT_SCALE: f32 = 0.95;
/// The CSS `ease` curve. A tooltip, a toggle switch and the grid's hover
/// preview follow it, among others.
pub const EASE: CubicBezier = CubicBezier::new(0.25, 0.1, 0.25, 1.);
/// How long the "First item" or "Last item" notice of the media view and
/// the detail window takes to fade in and to fade out, on [`EASE`]. The
/// notice appears when the user tries to step back from the first item or
/// forward from the last one.
pub const BOUNDARY_FADE: Duration = Duration::from_millis(200);
/// How long after the "First item" or "Last item" notice is shown it starts
/// to fade out.
pub const BOUNDARY_HOLD: Duration = Duration::from_millis(800);
/// How long the viewer waits, after the image first becomes larger than the
/// pane, before it shows the navigator. The navigator is the small square in
/// the bottom-right corner of the viewer that shows the whole image and marks
/// the part that is on screen.
pub const NAVIGATOR_DELAY: Duration = Duration::from_millis(80);
/// How long the background of a button in Quick Look's previous and next
/// strip takes to change when the pointer moves onto or off the button, on
/// [`EASE`].
pub const STRIP_BUTTON: Duration = Duration::from_millis(200);
/// How long the detail window's toolbar takes to fade in and to fade out,
/// on [`EASE`].
pub const DETAIL_TOOLBAR_FADE: Duration = Duration::from_millis(250);
/// The detail window's toolbar starts to fade out when the pointer has not
/// moved in the window for this long.
pub const DETAIL_TOOLBAR_HIDE: Duration = Duration::from_millis(1000);
/// The curve the background of an empty state's button follows when the
/// pointer moves onto or off the button.
pub const EASE_STANDARD: CubicBezier = CubicBezier::new(0.4, 0., 0.2, 1.);
/// How long the "Return to Top" button takes to move up into its place and
/// fade in, and to move back down and fade out.
pub const RETURN_TO_TOP: Duration = Duration::from_millis(150);
/// How long a context menu row's background takes to change when the row
/// becomes the highlighted row or stops being it.
pub const MENU_ROW: Duration = Duration::from_millis(30);
/// How long a dismissed context menu takes to fade out.
pub const MENU_OUT: Duration = Duration::from_millis(80);
/// How long an open submenu stays open after the pointer moves onto
/// another row of its menu that has no submenu. If the pointer reaches the
/// submenu within this time, the submenu stays open.
pub const MENU_SUBMENU_GRACE: Duration = Duration::from_millis(150);
/// How long the grid and the scope title take to fade out when the user
/// goes to another scope (for example from one folder to another), and how
/// long they take to fade back in once the new scope is ready to show.
pub const SCOPE_FADE: Duration = Duration::from_millis(170);
/// How long a dialog's panel and its backdrop take to appear.
pub const MODAL_IN: Duration = Duration::from_millis(150);
/// How long a dialog's panel and its backdrop take to disappear after the
/// dialog is dismissed. The dialog is removed after the same time.
pub const MODAL_OUT: Duration = Duration::from_millis(120);
/// How long the grid's hover preview takes to fade in. Its dark backdrop
/// and the original image fade in together.
pub const HOVER_PREVIEW_FADE: Duration = Duration::from_millis(150);
/// How long the background of an empty state's button takes to change
/// when the pointer moves onto or off the button.
pub const EMPTY_ACTION_HOVER: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug)]
pub struct CubicBezier {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
}

impl CubicBezier {
    pub const fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        Self { x1, y1, x2, y2 }
    }

    /// Returns the curve's two control points as `(x1, y1, x2, y2)`, the
    /// order in which CSS `cubic-bezier()` and Core Animation's timing
    /// function take them.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn points(&self) -> (f32, f32, f32, f32) {
        (self.x1, self.y1, self.x2, self.y2)
    }

    fn axis(t: f32, p1: f32, p2: f32) -> f32 {
        let inverse = 1. - t;
        3. * inverse * inverse * t * p1 + 3. * inverse * t * t * p2 + t * t * t
    }

    /// Returns the eased progress for the linear progress `x`: how much of
    /// an animation's change has happened when the share `x` of its time
    /// has passed. Both are between 0 and 1.
    pub fn ease(&self, x: f32) -> f32 {
        if x <= 0. {
            return 0.;
        }
        if x >= 1. {
            return 1.;
        }
        // The curve is given as x(t) and y(t). x(t) only increases when the
        // control points' x values are between 0 and 1, so bisection finds
        // the t whose x(t) is this x. The result is y at that t.
        let (mut low, mut high) = (0f32, 1f32);
        for _ in 0..24 {
            let middle = (low + high) / 2.;
            if Self::axis(middle, self.x1, self.x2) < x {
                low = middle;
            } else {
                high = middle;
            }
        }
        Self::axis((low + high) / 2., self.y1, self.y2)
    }
}

/// How long each of these takes to change. The background of a sidebar row
/// or a sidebar section header when the pointer moves onto or off it. The
/// highlight of the library switcher. The ring around a library's avatar.
/// The border of a text field in a dialog's form row when the field gains
/// or loses focus.
pub const FAST: Duration = Duration::from_millis(50);
/// How long a button's fill and icon take to change when the pointer moves
/// onto or off the button, and when an icon button is pressed or released.
/// It applies to icon buttons, a window's close button, the media player's
/// controls, text buttons and the inspector's full-width action buttons.
pub const BUTTON_HOVER: Duration = Duration::from_millis(150);
/// How long the sidebar and the inspector take to slide out of the window
/// when they are hidden, and back in when they are shown.
pub const INSPECTOR_SLIDE: Duration = Duration::from_millis(100);

/// How long the small icon button of a table row or a job strip takes to
/// change its opacity and background when the pointer moves onto or off it,
/// on [`EASE`]. A source's row on a subscription's page also fades its
/// buttons in and out over this time when the pointer moves onto or off the
/// row.
pub const SMALL_BUTTON: Duration = Duration::from_millis(100);
/// How long the minus and plus buttons of a smart folder rule take to
/// change when the pointer moves onto or off them, on [`EASE`].
pub const CONDITION_BUTTON: Duration = Duration::from_millis(120);
/// How long the "+" button after a row of tag chips in the inspector takes
/// to go from half opacity to full opacity when the pointer moves onto it,
/// and back when the pointer leaves, on [`EASE_DEFAULT`].
pub const TAG_ADD_BUTTON: Duration = Duration::from_millis(200);
/// How long one pulse of a syncing subscription takes, from its dimmest to
/// full strength and back, on [`EASE_IN_OUT`]. The status badge on the
/// subscription screens and the activity dot in the subscription's sidebar
/// row both pulse. [`pulse`] uses it.
const STATUS_PULSE: Duration = Duration::from_millis(2400);

/// In a collection's reader, how long a member's original image takes to fade
/// in over its thumbnail once the original has decoded. A member that is not
/// an image (a video or a document, for example) fades its thumbnail out over
/// the same time once its own viewer has drawn its first frame or page.
pub const FULL_IMAGE_FADE: Duration = Duration::from_millis(130);
/// How long the Duplicates screen waits before it shows its loading
/// message or a scan's progress bar, so that a load or a scan that finishes
/// sooner shows neither.
pub const LOADING_MESSAGE_DELAY: Duration = Duration::from_millis(200);

/// How long an image must stay on screen before its original file is read,
/// both in the viewers and in the grid. Until then only its thumbnail is
/// shown, so stepping or scrolling quickly past many images does not read
/// their original files.
pub const ORIGINAL_DELAY: Duration = Duration::from_millis(200);
/// The longest the viewer keeps the previous item on screen while the next
/// item is opening. A video is shown when its first frame is ready and a
/// document when its first page is ready. If that takes longer than this,
/// the viewer stops showing the previous item anyway.
pub const MEDIA_HOLD: Duration = Duration::from_millis(1500);

/// How long each of these takes to change, among others. A disclosure arrow
/// turning when its inspector section or sidebar row is folded or unfolded. A
/// tag chip's look when the pointer moves onto or off it. An inspector
/// field's border and background when the field gains or loses focus. The
/// resize edge of the sidebar or the inspector when the pointer moves onto or
/// off it.
pub const NORMAL: Duration = Duration::from_millis(200);
/// How long each of these takes to change when the pointer moves onto or
/// off it, on [`EASE`]: the background of a check box row, the background
/// of a section in the Settings window's navigation, and the outline of a
/// theme swatch in Settings.
pub const ROW_HOVER: Duration = Duration::from_millis(100);
/// How long the up and down arrows of a number field take to change
/// opacity when the pointer moves onto or off them, and how long the number
/// field's border takes to change when the field gains or loses focus.
pub const STEPPER: Duration = Duration::from_millis(120);
/// How long the border and background of the grid toolbar's search field
/// take to change when the field gains or loses focus.
pub const FIELD_FOCUS: Duration = Duration::from_millis(160);
/// How long an inspector field takes to fade from 92% to full opacity when
/// it expands. A focused inspector field whose text does not fit on one
/// line expands over the fields below it.
pub const FIELD_EXPAND: Duration = Duration::from_millis(100);
/// How long the background of a button in a picker's header takes to
/// change when the pointer moves onto or off the button.
pub const PIN_BUTTON: Duration = Duration::from_millis(120);
/// How long the sidebar on the left of the tag picker takes to fade in
/// when it is shown. The AI tagger's sidebar takes the same time.
pub const PICKER_RAIL_IN: Duration = Duration::from_millis(150);
/// How long the tag picker's sidebar, and the AI tagger's, takes to fade
/// out when it is hidden.
pub const PICKER_RAIL_OUT: Duration = Duration::from_millis(100);
/// How long the border of a text field in a dialog takes to change colour
/// when the field gains or loses focus.
pub const FIELD_BORDER: Duration = Duration::from_millis(120);
/// How long the pointer must stay on a control before the control's
/// tooltip starts to appear.
pub const TOOLTIP_DELAY: Duration = Duration::from_millis(400);
/// How long a tooltip takes to appear and to disappear.
pub const TOOLTIP_FADE: Duration = Duration::from_millis(150);
/// The gap between a control and its tooltip, in pixels.
pub const TOOLTIP_OFFSET: f32 = 6.;
/// The scale of a tooltip when it is fully hidden: 90% of its size, about
/// its centre. The tooltip grows from this scale while it appears.
const TOOLTIP_POP_SCALE: f32 = 0.9;
/// How many pixels below its place a tooltip is when it is fully hidden,
/// before the scale is applied. The tooltip moves up while it appears.
const TOOLTIP_POP_RISE: f32 = 10.;

/// How long the inspector takes to fade out when the user opens a screen that
/// has no inspector, and to fade in when the user leaves that screen. When
/// the user hides or shows the inspector with its toggle, it slides instead
/// ([`INSPECTOR_SLIDE`]).
pub const INSPECTOR_FADE: Duration = Duration::from_millis(170);

/// The scale of a dialog's panel when it is fully hidden: 97% of its size.
/// The panel grows from this scale while it appears and shrinks to it while
/// it disappears.
const MODAL_SCALE: f32 = 0.97;
/// How many pixels below its place a dialog's panel is when it is fully
/// hidden, before the scale is applied.
const MODAL_DROP: f32 = 4.;

/// After the pointer leaves the resize edge of the sidebar or the
/// inspector, the edge waits this long before it starts to change back to
/// its resting width and colour.
pub const RESIZE_EDGE_DELAY: Duration = Duration::from_millis(100);

/// How long a notification stays on screen when the code that shows it
/// does not give a duration.
pub const NOTIFICATION_DURATION: Duration = Duration::from_millis(4000);
/// How long a notification takes to move up and fade out when it leaves.
pub const NOTIFICATION_EXIT: Duration = Duration::from_millis(400);
/// The curve a leaving notification follows. Its output goes slightly
/// above 1 before the end, so the notification moves a little further up
/// than [`NOTIFICATION_RISE`] before it is removed.
pub const NOTIFICATION_EASE: CubicBezier = CubicBezier::new(0., 1.1, 0.79, 1.03);
/// How many pixels above its place a notification is when it has finished
/// leaving.
pub const NOTIFICATION_RISE: f32 = 48.;

/// How long the "Return to Top" button waits before it starts to appear or
/// to disappear.
pub const RETURN_TO_TOP_DELAY: Duration = Duration::from_millis(30);

/// How long the tag manager's list takes to fade out when the user changes
/// which tags the list shows, by choosing another tag group or by turning
/// the filter for starred tags on or off. The list changes its contents
/// after this fade.
pub const TAG_VIEW_LEAVE: Duration = Duration::from_millis(100);
/// How long the tag manager's list takes to fade back in with the new
/// tags.
pub const TAG_VIEW_ENTER: Duration = Duration::from_millis(140);
/// How long the rows of the tag manager's list take to fade in each time
/// the list's contents are replaced.
pub const TAG_LIST_FADE: Duration = Duration::from_millis(90);
/// How long the moving highlight takes to cross a loading placeholder in
/// the tag manager's list once.
pub const TAG_SHIMMER: Duration = Duration::from_millis(800);

/// In the inspector's stacked preview of a multiple selection, how long a
/// card that rests turned to the left takes to move into its place in the
/// stack, shrinking and fading in on the way.
pub const STACK_ENTER_LEFT: Duration = Duration::from_millis(200);
/// The same as [`STACK_ENTER_LEFT`], for a card that rests turned to the
/// right.
pub const STACK_ENTER_RIGHT: Duration = Duration::from_millis(250);
/// How long a card that leaves the inspector's stacked preview takes to
/// fade out.
pub const STACK_EXIT_FADE: Duration = Duration::from_millis(200);
/// How long after a card starts to leave the inspector's stacked preview
/// the card is removed. This is 20 ms after its fade ends.
pub const STACK_EXIT_REMOVE: Duration = Duration::from_millis(220);

/// How long the pointer must rest on a grid tile's magnifier before the
/// hover preview opens. The magnifier is the icon in the bottom-right
/// corner of the image under the pointer.
pub const HOVER_PREVIEW_DELAY: Duration = Duration::from_millis(200);
/// How long the hover preview stays open after the pointer moves off the
/// magnifier while it is still over the grid.
pub const HOVER_PREVIEW_HIDE: Duration = Duration::from_millis(90);
/// The hover preview shows nothing for at least this long after it opens,
/// even when the image has already decoded.
pub const HOVER_PREVIEW_MIN_WAIT: Duration = Duration::from_millis(150);
/// Scrolling the grid removes the hover highlight and the magnifier from
/// the tile under the pointer. They come back when this long has passed
/// since the last scroll.
pub const MAGNIFIER_RESUME: Duration = Duration::from_millis(80);
/// How long a grid tile waits, after it enters the area the grid treats as
/// visible, before its thumbnail starts to fade in.
pub const THUMBNAIL_REVEAL_DELAY: Duration = Duration::from_millis(100);
/// How long a grid thumbnail takes to fade in.
pub const THUMBNAIL_REVEAL: Duration = Duration::from_millis(500);

/// How long a zoom animation lasts. In the viewer, a zoom step from the
/// keyboard or from one notch of the wheel animates the picture for this
/// long. In the grid, a change of zoom animates the tiles to their new
/// positions and sizes for this long.
pub const ZOOM_GLIDE: Duration = Duration::from_millis(150);

/// Returns the share of a zoom animation's distance that is covered `elapsed`
/// after it started, from 0 to 1. The curve is an ease-out cubic: the speed
/// is highest at the start and falls to zero at 150 ms. A second zoom step
/// during the animation therefore starts from tiles that are already moving
/// fast, and does not look like a stop followed by a restart.
pub fn zoom_glide_progress(elapsed: Duration) -> f32 {
    1. - (1. - progress(elapsed, ZOOM_GLIDE)).powi(3)
}

/// The caret of a focused text field blinks: it is drawn for this long,
/// then not drawn for this long, and so on.
pub const CARET_BLINK: Duration = Duration::from_millis(500);
/// How long a toggle switch takes to change between off and on, on
/// [`EASE`].
pub const TOGGLE_SWITCH: Duration = Duration::from_millis(200);
/// How long the moving segment of an indeterminate progress bar takes to
/// cross the bar once.
pub const PROGRESS_SWEEP: Duration = Duration::from_millis(900);
/// How long the task dialog and its backdrop take to fade in, on
/// [`EASE_SCALE_IN`].
pub const TASK_DIALOG_IN: Duration = Duration::from_millis(200);
/// How long the filled part of a progress bar takes to grow or shrink to
/// its new width when the progress changes, on [`EASE_DEFAULT`]. The shared
/// progress bar and the task dialog's progress bar both use it.
pub const PROGRESS_FILL: Duration = Duration::from_millis(250);
/// How long the moving segment of a task's progress bar takes to cross the
/// bar once, while the task's total is not known.
pub const TASK_PROGRESS_SWEEP: Duration = Duration::from_millis(1100);
/// A Flash movie is advanced and redrawn this often (about 60 frames a
/// second).
pub const FLASH_FRAME: Duration = Duration::from_millis(16);
/// Settings shows "Saving…" only for a save that takes longer than this.
pub const SAVING_MESSAGE_DELAY: Duration = Duration::from_millis(160);

/// How long the media player's controls take to fade in and to fade out,
/// on [`EASE`]. The Flash player's controls take the same time.
pub const MEDIA_CONTROLS_FADE: Duration = Duration::from_millis(300);
/// How long the media player's scrubber takes to get thicker when the
/// pointer moves onto it or a seek starts, and to get thinner again
/// afterwards.
pub const MEDIA_SCRUBBER_EXPAND: Duration = Duration::from_millis(100);
/// How long the media player's volume display takes to fade in after a
/// volume key is pressed.
pub const MEDIA_VOLUME_HUD_IN: Duration = Duration::from_millis(150);
/// How long the media player's volume slider stays open after the pointer
/// leaves both the volume button and the slider.
pub const MEDIA_VOLUME_PANEL_GRACE: Duration = Duration::from_millis(300);

/// How long the Inbox review pill (the Accept and Reject buttons) takes to
/// move up into its place when it appears, on [`EASE_DEFAULT`].
pub const REVIEW_PILL_IN: Duration = Duration::from_millis(130);
/// How long the light behind the Accept or Reject button, and the shadow
/// of the button's icon, take to change when the pointer moves onto or off
/// the button, on [`EASE`].
pub const REVIEW_GLOW: Duration = Duration::from_millis(160);
/// How long the icon of the Accept or Reject button takes to change colour
/// when the pointer moves onto or off the button, on [`EASE`].
pub const REVIEW_TINT: Duration = Duration::from_millis(140);

/// A value that is 0 in one state and 1 in the other, and that moves
/// gradually from one to the other when the state changes, as a CSS
/// transition does. For example, a component keeps one per element for the
/// element's hover highlight: 0 while the pointer is off the element and 1
/// while it is on it.
pub struct Fade {
    on: bool,
    since: std::time::Instant,
    from: f32,
    duration: Duration,
    /// How long the value waits after a state change before it starts to
    /// move, as a CSS `transition-delay` does.
    delay: Duration,
}

impl Default for Fade {
    fn default() -> Self {
        Self {
            on: false,
            since: std::time::Instant::now(),
            from: 0.,
            duration: Duration::ZERO,
            delay: Duration::ZERO,
        }
    }
}

impl Fade {
    /// Returns a fade that is already fully in the given state and is not
    /// moving: its value is 1 when `on` is true and 0 when `on` is false.
    pub fn at(on: bool) -> Self {
        Self {
            on,
            ..Self::default()
        }
    }

    /// Changes the state to `on`. The value starts from where it is now.
    /// `duration` is the time for the whole way between 0 and 1; a value
    /// that has less far to go takes the matching share of it. Setting the
    /// state the fade is already in does nothing.
    pub fn set(&mut self, on: bool, duration: Duration) {
        self.set_after(on, duration, Duration::ZERO);
    }

    /// The same as [`Fade::set`], but the value starts to move only after
    /// `delay`, as with a CSS `transition-delay`.
    pub fn set_after(&mut self, on: bool, duration: Duration, delay: Duration) {
        if self.on == on {
            return;
        }
        self.from = self.value();
        self.on = on;
        self.since = std::time::Instant::now();
        self.delay = delay;
        // When a transition is reversed before it has finished, CSS shortens
        // the way back in proportion to the distance left to cover. The same
        // is done here: a fade that got halfway takes half of `duration` to
        // go back.
        let distance = if on { 1. - self.from } else { self.from };
        self.duration = duration.mul_f32(distance.clamp(0., 1.));
    }

    /// Returns the value now: 0 when the fade is fully off, 1 when it is
    /// fully on, and in between while it moves. It moves on
    /// [`EASE_DEFAULT`].
    pub fn value(&self) -> f32 {
        self.value_with(EASE_DEFAULT)
    }

    /// The same as [`Fade::value`], but the value moves on `curve`.
    pub fn value_with(&self, curve: CubicBezier) -> f32 {
        let target = if self.on { 1. } else { 0. };
        if self.duration.is_zero() {
            return target;
        }
        let elapsed = self.since.elapsed().saturating_sub(self.delay);
        self.from + (target - self.from) * curve.ease(progress(elapsed, self.duration))
    }

    pub fn on(&self) -> bool {
        self.on
    }

    pub fn animating(&self) -> bool {
        self.since.elapsed() < self.delay + self.duration
    }
}

/// Keeps a [`Fade`] for an element's hover highlight in the window's state
/// under `id`, so that it lasts from one frame to the next. Returns two
/// things. The first is the highlight's strength now: 0 while the pointer
/// is off the element, 1 once it has been on the element for `duration`,
/// and moving between them on `curve`. The second is the handler to pass to
/// the element's `on_hover`. While the strength is changing, this function
/// requests the next animation frame itself, so the caller does not have
/// to.
pub fn hover_fade(
    id: impl Into<gpui::ElementId>,
    duration: Duration,
    curve: CubicBezier,
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) -> (
    f32,
    impl Fn(&bool, &mut gpui::Window, &mut gpui::App) + 'static,
) {
    let fade = window.use_keyed_state(id, cx, |_, _| Fade::default());
    let (value, animating) = {
        let fade = fade.read(cx);
        (fade.value_with(curve), fade.animating())
    };
    if animating {
        window.request_animation_frame();
    }
    let set = move |hovered: &bool, _: &mut gpui::Window, cx: &mut gpui::App| {
        fade.update(cx, |fade, cx| {
            fade.set(*hovered, duration);
            cx.notify();
        })
    };
    (value, set)
}

/// A set of [`Fade`]s, one per key. The context menu keeps one for the
/// highlight of each of its rows. The shell keeps one for the sidebar and
/// one for the inspector, to slide each out of the window and back in.
#[derive(Default)]
pub struct Fades {
    fades: std::collections::HashMap<(&'static str, usize), Fade>,
}

impl Fades {
    pub fn set(&mut self, key: (&'static str, usize), hovered: bool, duration: Duration) {
        // A fade that is off and has stopped moving is removed from the map,
        // so the map does not grow without limit. A key that is not in the
        // map counts as a fade at 0.
        self.fades.retain(|_, fade| fade.on() || fade.animating());
        self.fades.entry(key).or_default().set(hovered, duration);
    }

    /// Returns the value of the fade under `key`: 0 when it is fully off
    /// or has never been set, 1 when it is fully on.
    pub fn value(&self, key: (&'static str, usize)) -> f32 {
        self.fades.get(&key).map_or(0., Fade::value)
    }

    pub fn animating(&self) -> bool {
        self.fades.values().any(Fade::animating)
    }
}

/// Returns where a rectangle of `w` × `h` is drawn when it is scaled by
/// `scale` about `origin`, the way CSS `transform: scale()` with
/// `transform-origin` places it. The result is `(left, top, width, height)`,
/// relative to the unscaled rectangle's top-left corner. `origin` is given
/// as fractions of the width and the height: 0 is the left or top edge, 0.5
/// is the centre, and 1 is the right or bottom edge.
///
/// GPUI cannot scale an element's contents, so a panel that grows or shrinks
/// draws its frame (surface, border and shadow) in this rectangle and keeps
/// its contents at full size, clipped to it.
pub fn scaled_frame(w: f32, h: f32, scale: f32, origin: (f32, f32)) -> (f32, f32, f32, f32) {
    (
        origin.0 * w * (1. - scale),
        origin.1 * h * (1. - scale),
        w * scale,
        h * scale,
    )
}

/// Returns the scale a popover's panel is drawn at when `elapsed` has
/// passed since it opened. The scale grows from [`MENU_IN_SCALE`] to 1 over
/// [`MENU_IN`] on [`EASE_SCALE_IN`].
pub fn menu_in(elapsed: Duration) -> f32 {
    MENU_IN_SCALE + (1. - MENU_IN_SCALE) * EASE_SCALE_IN.ease(progress(elapsed, MENU_IN))
}

/// Returns the scale and the opacity of a dismissed context menu when
/// `elapsed` has passed since it was dismissed, as `(scale, opacity)`. Over
/// [`MENU_OUT`], on [`EASE_IN`], the scale goes from 1 to 0.95 and the
/// opacity from 1 to 0. The context menu in `ui/menu.rs` uses only the
/// opacity, because GPUI cannot scale a panel's contents.
pub fn menu_out(elapsed: Duration) -> (f32, f32) {
    let eased = EASE_IN.ease(progress(elapsed, MENU_OUT));
    (1. - (1. - MENU_OUT_SCALE) * eased, 1. - eased)
}

/// Returns where a tooltip is drawn when it is `shown` of the way from
/// hidden to shown (0 is hidden, 1 is shown; the caller has already applied
/// the easing curve). The result is the tooltip's scale about its centre,
/// and how many pixels below its final place its centre is. The tooltip is
/// moved down first and scaled afterwards, so its centre is the scale times
/// the distance below its place.
pub fn tooltip_pop(shown: f32) -> (f32, f32) {
    let scale = TOOLTIP_POP_SCALE + (1. - TOOLTIP_POP_SCALE) * shown;
    (scale, scale * TOOLTIP_POP_RISE * (1. - shown))
}

/// Returns where a dialog's panel is drawn when it is `shown` of the way
/// from hidden to shown (0 is hidden, 1 is shown; the caller has already
/// applied the easing curve). The result is the panel's scale about its
/// centre, and how many pixels below its final place its centre is. The
/// panel is moved down first and scaled afterwards, so a fully hidden panel
/// has its centre `0.97 × 4` pixels down.
pub fn modal_pop(shown: f32) -> (f32, f32) {
    let scale = MODAL_SCALE + (1. - MODAL_SCALE) * shown;
    (scale, scale * MODAL_DROP * (1. - shown))
}

/// Returns the time since this function was first called. Every animation
/// that repeats for as long as something is running (the moving segment of an
/// indeterminate progress bar, the pulse of a syncing subscription's badge)
/// computes its position from this one clock. Two animations of the same kind
/// are therefore always at the same point of their cycle, and an animation
/// does not start again from its beginning when its view is drawn again.
pub fn loop_clock() -> Duration {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed()
}

/// How often a view with a repeating animation is drawn again: every 33 ms,
/// which is about 30 times a second. A repeating animation is a pulse or a
/// moving progress segment that runs for as long as some work does.
/// Requesting every animation frame instead would redraw the whole window
/// at the display's refresh rate, which is 120 times a second on some
/// displays, for an indicator that changes slowly.
pub const LOOP_FRAME: Duration = Duration::from_millis(33);

/// The views that already have a redraw scheduled by [`loop_frame`]. Each
/// view is identified by its window and its entity.
#[derive(Default)]
struct LoopFrames(std::collections::HashSet<(gpui::WindowId, gpui::EntityId)>);

impl gpui::Global for LoopFrames {}

/// Schedules the view that is being drawn to be drawn again after
/// [`LOOP_FRAME`]. A view calls this each time it draws a repeating
/// animation whose position it reads from [`loop_clock`]. If the view
/// already has a redraw scheduled, this does nothing, so a view with
/// several such animations is still redrawn only once per interval. Each
/// redraw has to call this again to schedule the next one, so a window that
/// is not being drawn (because it is hidden or covered) schedules no more
/// redraws.
pub fn loop_frame(window: &gpui::Window, cx: &mut gpui::App) {
    let view = window.current_view();
    let key = (window.window_handle().window_id(), view);
    if !cx.default_global::<LoopFrames>().0.insert(key) {
        return;
    }
    cx.spawn(async move |cx| {
        cx.background_executor().timer(LOOP_FRAME).await;
        let _ = cx.update(|cx| {
            cx.default_global::<LoopFrames>().0.remove(&key);
            cx.notify(view);
        });
    })
    .detach();
}

/// Returns how far along a syncing subscription's pulse is when the loop
/// clock reads `elapsed`. The result goes from 0 to 1 during the first half
/// of [`STATUS_PULSE`] and back to 0 during the second half, each half on
/// [`EASE_IN_OUT`], and then repeats.
pub fn pulse(elapsed: Duration) -> f32 {
    let period = STATUS_PULSE.as_secs_f32();
    let phase = (elapsed.as_secs_f32() % period) / (period / 2.);
    EASE_IN_OUT.ease(if phase < 1. { phase } else { 2. - phase })
}

/// Returns the share of an animation's `duration` that has passed when the
/// animation began `elapsed` ago: 0 at the start and 1 once `duration` has
/// passed. No easing curve is applied.
pub fn progress(elapsed: Duration, duration: Duration) -> f32 {
    // An animation with a duration of zero is already finished. Returning 1
    // here also avoids dividing by zero below.
    if duration.is_zero() {
        return 1.;
    }
    (elapsed.as_secs_f32() / duration.as_secs_f32()).clamp(0., 1.)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_control_points_are_the_identity() {
        let linear = CubicBezier::new(0.25, 0.25, 0.75, 0.75);
        for step in 0..=20 {
            let x = step as f32 / 20.;
            assert!((linear.ease(x) - x).abs() < 1e-3, "{x}");
        }
    }

    #[test]
    fn default_ease_matches_the_sampled_curve() {
        // Compute points (x, y) directly from the curve's formula at 1999
        // values of t, and check that `ease(x)` returns each point's y.
        for step in 1..2000 {
            let t = step as f32 / 2000.;
            let x = CubicBezier::axis(t, 0.4, 0.22);
            let y = CubicBezier::axis(t, 0., 1.);
            assert!((EASE_DEFAULT.ease(x) - y).abs() < 2e-3, "t={t}");
        }
        assert_eq!(EASE_DEFAULT.ease(0.), 0.);
        assert_eq!(EASE_DEFAULT.ease(1.), 1.);
        // The curve starts slowly and then catches up: at 10% of the time
        // less than 10% of the change has happened, and at 60% of the time
        // more than 60% of it has.
        assert!(EASE_DEFAULT.ease(0.1) < 0.1);
        assert!(EASE_DEFAULT.ease(0.6) > 0.6);
    }

    #[test]
    fn keyed_fades_start_at_rest_and_forget_what_has_settled() {
        let mut fades = Fades::default();
        assert_eq!(fades.value(("row", 1)), 0.);
        fades.set(("row", 1), true, Duration::ZERO);
        assert_eq!(fades.value(("row", 1)), 1.);
        fades.set(("row", 1), false, Duration::ZERO);
        // Row 1 is now off and not moving, so the next call to `set` removes
        // it from the map.
        fades.set(("row", 2), true, Duration::from_secs(10));
        assert!(!fades.fades.contains_key(&("row", 1)));
        assert!(fades.animating());
    }

    #[test]
    fn a_fade_reversed_part_way_takes_less_than_its_full_time() {
        let mut fade = Fade::default();
        fade.set(true, Duration::from_secs(10));
        // The fade is reversed almost at once, so its value is still near 0
        // and the way back takes far less than the full 10 seconds.
        fade.set(false, Duration::from_secs(10));
        assert!(fade.duration < Duration::from_secs(1));
        assert!(!fade.on());
        // Setting the state the fade is already in does not restart it:
        // `since` stays the same.
        let since = fade.since;
        fade.set(false, Duration::from_secs(10));
        assert_eq!(fade.since, since);
        assert!(Fade::at(true).value() == 1. && !Fade::at(true).animating());
    }

    #[test]
    fn a_scaled_frame_keeps_its_origin_in_place() {
        // Scaled about the top-left corner, that corner stays where it is
        // and the right and bottom edges move inward.
        assert_eq!(
            scaled_frame(200., 100., 0.96, (0., 0.)),
            (0., 0., 192., 96.)
        );
        // Scaled about the bottom-right corner, that corner stays where it
        // was.
        let (x, y, w, h) = scaled_frame(200., 100., 0.96, (1., 1.));
        assert!((x + w - 200.).abs() < 1e-4 && (y + h - 100.).abs() < 1e-4);
        assert!((x - 8.).abs() < 1e-4 && (y - 4.).abs() < 1e-4);
        // Scaled about the centre, each side moves inward by half of the
        // lost width or height, and the centre stays where it was.
        let (x, y, w, h) = scaled_frame(200., 100., 0.9, (0.5, 0.5));
        assert!((x - 10.).abs() < 1e-4 && (y - 5.).abs() < 1e-4);
        assert!((x + w / 2. - 100.).abs() < 1e-4 && (y + h / 2. - 50.).abs() < 1e-4);
        // At a scale of 1 the rectangle is unchanged.
        assert_eq!(scaled_frame(200., 100., 1., (1., 0.)), (0., 0., 200., 100.));
    }

    #[test]
    fn a_menu_grows_in_then_shrinks_and_fades_out() {
        assert_eq!(menu_in(Duration::ZERO), MENU_IN_SCALE);
        assert_eq!(menu_in(MENU_IN), 1.);
        // `EASE_SCALE_IN` moves quickly at first: after 10 of the 70 ms
        // the scale is already more than halfway from 0.96 to 1.
        assert!(menu_in(Duration::from_millis(10)) > 0.98);
        assert_eq!(menu_out(Duration::ZERO), (1., 1.));
        assert_eq!(menu_out(MENU_OUT), (MENU_OUT_SCALE, 0.));
        // `EASE_IN` starts slowly: after a quarter of the 80 ms the opacity
        // is still above 0.9 and the scale above 0.995.
        let (scale, opacity) = menu_out(MENU_OUT / 4);
        assert!(opacity > 0.9 && scale > 0.995, "{scale} {opacity}");
    }

    #[test]
    fn a_tooltip_pops_up_from_below_and_a_modal_rises() {
        assert_eq!(tooltip_pop(1.), (1., 0.));
        let (scale, below) = tooltip_pop(0.);
        assert_eq!(scale, 0.9);
        // The 10px translation is multiplied by the 0.9 scale, which gives
        // 9px.
        assert!((below - 9.).abs() < 1e-5);
        assert_eq!(modal_pop(1.), (1., 0.));
        let (scale, below) = modal_pop(0.);
        assert_eq!(scale, 0.97);
        assert!((below - 3.88).abs() < 1e-5);
    }

    #[test]
    fn a_delayed_fade_waits_before_it_moves() {
        let mut fade = Fade::default();
        fade.set_after(true, Duration::from_secs(10), Duration::from_secs(10));
        // The delay has not passed yet, so the value has not moved.
        // `animating` is already true, so that the view that holds the
        // fade keeps drawing frames until the fade starts.
        assert_eq!(fade.value(), 0.);
        assert!(fade.animating());
    }

    #[test]
    fn progress_is_clamped() {
        let duration = Duration::from_millis(170);
        assert_eq!(progress(Duration::ZERO, duration), 0.);
        assert_eq!(progress(Duration::from_millis(85), duration), 0.5);
        assert_eq!(progress(Duration::from_secs(1), duration), 1.);
    }
}

#[cfg(test)]
mod loop_tests {
    use super::{LOOP_FRAME, loop_frame};
    use gpui::{AppContext as _, Context, IntoElement, Render, Window, div};
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::Duration;

    /// A view that calls [`loop_frame`] twice each time it is drawn, as a
    /// view with two repeating animations would.
    struct Dots;

    impl Render for Dots {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            loop_frame(window, cx);
            loop_frame(window, cx);
            div()
        }
    }

    #[test]
    fn a_looping_view_is_drawn_again_once_an_interval() {
        let mut cx = gpui::TestAppContext::single();
        let window = cx.add_window(|_, _| Dots);
        let handle: gpui::AnyWindowHandle = window.into();
        let view = window.root(&mut cx).unwrap();
        let count = Rc::new(Cell::new(0));
        let seen = count.clone();
        cx.update(|cx| {
            cx.observe(&view, move |_, _| seen.set(seen.get() + 1)).detach();
        });
        cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        cx.executor().advance_clock(LOOP_FRAME - Duration::from_millis(1));
        cx.run_until_parked();
        assert_eq!(count.get(), 0);
        cx.executor().advance_clock(Duration::from_millis(1));
        cx.run_until_parked();
        // The view called `loop_frame` twice but was notified only once.
        assert_eq!(count.get(), 1);

        // Each redraw calls `loop_frame` again, which schedules the next
        // one. After ten more intervals the view has been notified ten more
        // times, however many frames the display could have shown in that
        // time.
        cx.executor().advance_clock(LOOP_FRAME * 10);
        cx.run_until_parked();
        assert_eq!(count.get(), 11);
    }
}
