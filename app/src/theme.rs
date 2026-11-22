//! The colours, fonts and sizes that the views of the interface share. A
//! theme is one background colour, one contrast colour, one text colour and
//! one primary colour. Every surface, border and overlay is the contrast
//! colour at some opacity, or the contrast colour mixed into the background.
//! Views take their colours from this file. The exceptions are tables of
//! colours that are data (the theme swatches in Settings, the presets of the
//! colour editor), which are kept with the code that lists them. A size that
//! only one component uses is kept in that component's file.
use gpui::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    background: u32,
    contrast: u32,
    text: u32,
    primary: u32,
    /// Whether this is a light theme. A light theme has more opaque
    /// borders, white control surfaces, its own glass fill and backdrop
    /// blur, and lighter shadows.
    light: bool,
}

impl Global for Theme {}

/// The red of destructive actions, errors and excluded choices, which is
/// the same in every theme.
const NEGATIVE: u32 = 0xff4757;

/// The Dark theme.
pub const DARK: Theme = Theme {
    background: 0x18191c,
    contrast: 0xf8f9fb,
    text: 0xf8f9fb,
    primary: 0x3297ff,
    light: false,
};
/// The Blue theme: the Dark theme with a dark blue background.
const BLUE: Theme = Theme {
    background: 0x0d1630,
    ..DARK
};
/// The Purple theme: the Dark theme with a dark purple background.
const PURPLE: Theme = Theme {
    background: 0x1c1424,
    ..DARK
};
/// The Gray theme: the Dark theme with a lighter gray background.
const GRAY: Theme = Theme {
    background: 0x37383c,
    ..DARK
};
/// The Light theme.
pub const LIGHT: Theme = Theme {
    background: 0xffffff,
    contrast: 0x2c2f32,
    text: 0x2c2f32,
    primary: 0x2563eb,
    light: true,
};
/// The Light Gray theme: the Light theme with a light gray background.
const LIGHT_GRAY: Theme = Theme {
    background: 0xe3e4e6,
    ..LIGHT
};

pub fn theme(cx: &App) -> Theme {
    *cx.global::<Theme>()
}

/// Returns the theme that a stored `colorScheme` setting names. "auto" is
/// Dark when the system's appearance is dark and Light otherwise. An unknown
/// name is Dark. Settings written by Picto 0.6.19 and earlier can also name
/// a theme with a transparent window; this app does not draw those and
/// shows Dark for them.
pub fn named(name: &str, system_dark: bool) -> Theme {
    match name {
        "auto" if system_dark => DARK,
        "auto" | "light" => LIGHT,
        "lightgray" => LIGHT_GRAY,
        "gray" => GRAY,
        "blue" => BLUE,
        "purple" => PURPLE,
        _ => DARK,
    }
}

/// Makes the theme that `name` names the current theme and redraws every
/// window. Does nothing when that theme already is the current one.
pub fn show(name: &str, cx: &mut App) {
    let system_dark = matches!(
        cx.window_appearance(),
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    );
    let next = named(name, system_dark);
    if !cx.has_global::<Theme>() || *cx.global::<Theme>() != next {
        cx.set_global(next);
        // A cached view is painted again only when its window is refreshed.
        cx.refresh_windows();
    }
}

/// Returns `color` (0xRRGGBB) at an opacity of `percent` percent.
fn translucent(color: u32, percent: f32) -> Rgba {
    rgba(color << 8 | (percent / 100. * 255.).round() as u32)
}

/// Returns the opaque colour that is `percent` percent `color` and the
/// rest `base`, mixed channel by channel.
fn mixed(color: u32, percent: f32, base: u32) -> Rgba {
    let channel = |shift: u32| {
        let (top, bottom) = (
            (color >> shift & 0xff) as f32,
            (base >> shift & 0xff) as f32,
        );
        ((top * percent + bottom * (100. - percent)) / 100.).round() as u32
    };
    rgb(channel(16) << 16 | channel(8) << 8 | channel(0))
}

impl Theme {
    /// Returns whether this is a light theme.
    pub fn is_light(&self) -> bool {
        self.light
    }

    /// The background colour of the app's windows.
    pub fn background(&self) -> Rgba {
        rgb(self.background)
    }
    /// The background of the sidebar and the inspector: the contrast
    /// colour mixed 3% into the background.
    pub fn rail(&self) -> Rgba {
        mixed(self.contrast, 3., self.background)
    }
    /// The fill of a panel that lies on the rail colour, such as the
    /// content panel of Settings. It is the contrast colour at 3% opacity
    /// over the rail colour, computed as one opaque colour. It has to be
    /// opaque because GPUI paints a box shadow under the whole element and
    /// not only outside it, so the shadow would darken a translucent fill.
    pub fn rail_raised(&self) -> Rgba {
        // 3% over 3% leaves 0.97² of the background, so the share of the
        // contrast colour is 1 - 0.97² = 5.91%.
        mixed(self.contrast, 5.91, self.background)
    }
    /// A fill for an area that stands out a little from the background,
    /// such as the picture frames of the cover dialog. In the dark themes
    /// it is the rail colour. In the light themes it is the contrast colour
    /// at 6% opacity.
    pub fn surface_1(&self) -> Rgba {
        if self.light {
            translucent(self.contrast, 6.)
        } else {
            self.rail()
        }
    }
    /// A fill for an area that stands out more than `surface_1`, such as
    /// the place of a picture that has not loaded: the contrast colour
    /// mixed 5% into the background.
    pub fn surface_2(&self) -> Rgba {
        mixed(self.contrast, 5., self.background)
    }
    /// The fill of a row or control under the pointer: the contrast colour
    /// at 5% opacity.
    pub fn surface_hover(&self) -> Rgba {
        translucent(self.contrast, 5.)
    }
    /// The fill of a row or control that is pressed or selected: the
    /// contrast colour at 10% opacity.
    pub fn surface_active(&self) -> Rgba {
        translucent(self.contrast, 10.)
    }
    /// The fill of text fields and similar controls: white at 50% opacity
    /// in the light themes, black at 20% in the dark themes.
    pub fn control_surface(&self) -> Rgba {
        if self.light {
            rgba(0xffffff80)
        } else {
            rgba(0x00000033)
        }
    }
    /// A fill for a control under the pointer that is more visible than
    /// `surface_hover`: the contrast colour at 8% opacity.
    pub fn hover_overlay(&self) -> Rgba {
        translucent(self.contrast, 8.)
    }
    /// The colour of primary text.
    pub fn text(&self) -> Rgba {
        rgb(self.text)
    }
    /// The colour of secondary text: the text colour at 70% opacity.
    pub fn text_secondary(&self) -> Rgba {
        translucent(self.text, 70.)
    }
    /// The colour of tertiary text: the text colour at 60% opacity.
    pub fn text_tertiary(&self) -> Rgba {
        translucent(self.text, 60.)
    }
    /// The colour of the borders of controls: the contrast colour at 15%
    /// opacity in the light themes and 10% in the dark themes.
    pub fn border(&self) -> Rgba {
        translucent(self.contrast, if self.light { 15. } else { 10. })
    }
    /// The colour of the edges of the sidebar and the inspector and of
    /// separators: the contrast colour at 10% opacity in the light themes
    /// and 8% in the dark themes.
    pub fn border_secondary(&self) -> Rgba {
        translucent(self.contrast, if self.light { 10. } else { 8. })
    }
    /// Returns the text colour at an opacity of `percent` percent.
    pub fn text_at(&self, percent: f32) -> Rgba {
        translucent(self.text, percent)
    }
    /// Returns the contrast colour at an opacity of `percent` percent.
    pub fn contrast(&self, percent: f32) -> Rgba {
        translucent(self.contrast, percent)
    }
    /// The primary colour, which marks what is selected and what has the
    /// keyboard focus.
    pub fn primary(&self) -> Rgba {
        rgb(self.primary)
    }
    /// The fill behind selected text: the primary colour at 30% opacity.
    pub fn selection(&self) -> Rgba {
        translucent(self.primary, 30.)
    }
    /// The fill of a floating panel that blurs what is behind it: the
    /// background colour at 90% opacity in the light themes and 92% in the
    /// dark themes.
    pub fn glass(&self) -> Rgba {
        translucent(self.background, if self.light { 90. } else { 92. })
    }
    /// The fill of a menu: the background colour at 76% opacity in the
    /// light themes and 78% in the dark themes. It is less opaque than
    /// `glass`, so more of what is behind a menu shows through it.
    pub fn menu_glass(&self) -> Rgba {
        translucent(self.background, if self.light { 76. } else { 78. })
    }
    /// The fill of a library's avatar: the secondary text colour mixed 10%
    /// into the control surface.
    pub fn avatar_surface(&self) -> Rgba {
        blend(self.control_surface(), self.text_secondary(), 0.1)
    }
    /// The backdrop filter of a glass panel, applied to what is behind the
    /// panel: a blur of 13px at 70% brightness in the dark themes, and a
    /// blur of 25px at 300% brightness in the light themes.
    pub fn glass_blur(&self) -> BackdropFilter {
        let (radius, brightness) = if self.light { (25., 3.) } else { (13., 0.7) };
        BackdropFilter {
            brightness,
            ..BackdropFilter::blur(px(radius))
        }
    }
    /// The top edge of a glass panel: opaque white in the light themes and
    /// white at 25% opacity in the dark themes.
    pub fn glass_border_top(&self) -> Rgba {
        if self.light {
            rgba(0xffffffff)
        } else {
            rgba(0xffffff40)
        }
    }
    /// The border of a control that has the keyboard focus: the primary
    /// colour at 60% opacity.
    pub fn border_focus(&self) -> Rgba {
        translucent(self.primary, 60.)
    }
    /// The colour of the vertical guide lines in a tree of folders: the
    /// text colour at 17.5% opacity, a quarter of the opacity of secondary
    /// text.
    pub fn tree_guide(&self) -> Rgba {
        translucent(self.text, 17.5)
    }
    /// The colour of destructive actions and errors, such as the row of a
    /// menu that deletes something.
    pub fn negative(&self) -> Rgba {
        rgb(NEGATIVE)
    }
    /// Returns the colour of destructive actions at an opacity of
    /// `percent` percent.
    pub fn negative_at(&self, percent: f32) -> Rgba {
        translucent(NEGATIVE, percent)
    }
    /// The left and right edges of a glass panel: white at 50% opacity in
    /// the light themes and 15% in the dark themes.
    pub fn glass_border(&self) -> Rgba {
        if self.light {
            rgba(0xffffff80)
        } else {
            rgba(0xffffff26)
        }
    }
    /// The fill of some icon buttons under the pointer: the contrast
    /// colour at 10% opacity.
    pub fn subtle_overlay(&self) -> Rgba {
        translucent(self.contrast, 10.)
    }
    /// The fill of some icon buttons while they are pressed, and the ring
    /// of a compact check box: the contrast colour at 20% opacity.
    pub fn pressed_overlay(&self) -> Rgba {
        translucent(self.contrast, 20.)
    }
    /// The fill of table headers, of the track behind a row of tabs and of
    /// the footers of dialogs: the contrast colour at 3% opacity.
    pub fn footer_surface(&self) -> Rgba {
        translucent(self.contrast, 3.)
    }
    /// The fill of a select control: the text colour mixed 10% into the
    /// background.
    pub fn select_surface(&self) -> Rgba {
        mixed(self.text, 10., self.background)
    }
    /// Returns the primary colour at an opacity of `percent` percent.
    pub fn primary_at(&self, percent: f32) -> Rgba {
        translucent(self.primary, percent)
    }
    /// The colour of text and icons on a fill of the primary colour:
    /// white.
    pub fn on_primary(&self) -> Rgba {
        rgb(0xffffff)
    }
    /// The bottom edge of a glass panel: white at 25% opacity in the light
    /// themes and 10% in the dark themes.
    pub fn glass_border_bottom(&self) -> Rgba {
        if self.light {
            rgba(0xffffff40)
        } else {
            rgba(0xffffff1a)
        }
    }
    /// The colour of the placeholder of a text field in a popover: a
    /// medium gray, the same in every theme. The other text fields draw
    /// their placeholder in the tertiary text colour.
    pub fn placeholder(&self) -> Rgba {
        rgb(0xa9a9a9)
    }
    /// The fill of the footer of the library switcher: the control surface
    /// at half its opacity.
    pub fn control_surface_half(&self) -> Rgba {
        if self.light {
            rgba(0xffffff40)
        } else {
            rgba(0x0000001a)
        }
    }
    /// The colour of disabled text: the text colour at 22% opacity.
    pub fn text_disabled(&self) -> Rgba {
        translucent(self.text, 22.)
    }
    /// The border of a shortcut field in Settings whose keys are already
    /// bound to another shortcut: orange, the same in every theme. It is a
    /// different orange from `WARNING`.
    pub fn shortcut_conflict(&self) -> Rgba {
        rgb(0xffa500)
    }
}

// The backdrop blurs of the surfaces that do not use `Theme::glass_blur`.
// They are the same in every theme.

/// The backdrop blur of a tooltip: 16px.
pub fn tooltip_blur() -> BackdropFilter {
    BackdropFilter::blur(px(16.))
}
/// The backdrop blur of the toolbar of a detail window: 12px.
pub fn detail_toolbar_blur() -> BackdropFilter {
    BackdropFilter::blur(px(12.))
}
/// The backdrop blur of the format badge on the inspector's preview: 5px.
pub fn preview_label_blur() -> BackdropFilter {
    BackdropFilter::blur(px(5.))
}

/// The colour of the label of a button for a destructive action.
pub const DANGER_TEXT: u32 = 0xff9aa4;
/// The colour of the text of an error message shown in a form.
pub const ERROR_TEXT: u32 = 0xffb4b4;
/// The colour that covers the window behind a modal dialog: black at 55%
/// opacity.
pub const MODAL_BACKDROP: u32 = 0x0000008c;
/// The fill of the square behind the icon in the export and import dialog
/// for Picto packs: white at 6% opacity, the same in every theme.
pub const PACK_GLYPH_TILE: u32 = 0xffffff0f;

/// Returns `color` fully transparent. A fade that starts from this colour
/// and ends at `color` keeps the hue of `color` all the way (see [`blend`]).
pub fn clear(color: Rgba) -> Rgba {
    Rgba { a: 0., ..color }
}

/// Parses a colour written as `#rrggbb` into a number. The `#` may be left
/// out.
pub fn parse_hex(text: &str) -> Option<u32> {
    let digits = text.strip_prefix('#').unwrap_or(text);
    (digits.len() == 6 && digits.chars().all(|digit| digit.is_ascii_hexdigit()))
        .then(|| u32::from_str_radix(digits, 16).ok())
        .flatten()
}

/// Parses a colour written as `#RRGGBB`. The `#` is required, as it is in
/// the code field of the colour editor.
pub fn parse_hex_code(text: &str) -> Option<u32> {
    text.starts_with('#').then(|| parse_hex(text)).flatten()
}

/// Parses a colour in one of the forms the settings store: `#rgb`,
/// `#rrggbb` or `rgb(r, g, b)`.
pub fn parse_css_color(value: &str) -> Option<u32> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        return match hex.len() {
            3 => parse_hex(
                &hex.chars()
                    .flat_map(|digit| [digit, digit])
                    .collect::<String>(),
            ),
            _ => parse_hex(hex),
        };
    }
    let inner = value.strip_prefix("rgb(")?.strip_suffix(')')?;
    let channels = inner
        .split(',')
        .map(|channel| channel.trim().parse::<u8>().ok())
        .collect::<Option<Vec<_>>>()?;
    match channels[..] {
        [r, g, b] => Some((r as u32) << 16 | (g as u32) << 8 | b as u32),
        _ => None,
    }
}

/// Returns the colour that is `amount` (0 to 1) of the way from `from` to
/// `to`. The colour channels are multiplied by the opacity before they are
/// interpolated, so a fade from a transparent colour keeps the hue of `to`.
pub fn blend(from: Rgba, to: Rgba, amount: f32) -> Rgba {
    let amount = amount.clamp(0., 1.);
    let a = from.a + (to.a - from.a) * amount;
    if a <= 0. {
        return Rgba {
            r: to.r,
            g: to.g,
            b: to.b,
            a: 0.,
        };
    }
    let channel =
        |from_c: f32, to_c: f32| (from_c * from.a + (to_c * to.a - from_c * from.a) * amount) / a;
    Rgba {
        r: channel(from.r, to.r),
        g: channel(from.g, to.g),
        b: channel(from.b, to.b),
        a,
    }
}

impl Theme {
    /// The shadow of a floating panel: a soft shadow 12px below it and a
    /// ring of 1px around it.
    pub fn panel_shadow(&self) -> Vec<BoxShadow> {
        let (shade, ring) = self.panel_shadow_colors();
        vec![
            BoxShadow::new(px(0.), px(12.), rgba(shade).into()).blur_radius(px(24.)),
            BoxShadow::new(px(0.), px(0.), rgba(ring).into()).spread_radius(px(1.)),
        ]
    }

    /// The shadow of a small floating panel: a soft shadow 3px below it and
    /// a ring of 1px around it.
    pub fn panel_shadow_small(&self) -> Vec<BoxShadow> {
        let (shade, ring) = self.panel_shadow_colors();
        vec![
            BoxShadow::new(px(0.), px(3.), rgba(shade).into()).blur_radius(px(12.)),
            BoxShadow::new(px(0.), px(0.), rgba(ring).into()).spread_radius(px(1.)),
        ]
    }

    /// Returns the colours of the shadow and of the ring of the panel
    /// shadows. In the dark themes the shadow is black at 30% opacity and
    /// the ring black at 50%. In the light themes both are black at 15%.
    fn panel_shadow_colors(&self) -> (u32, u32) {
        if self.light {
            (0x00000026, 0x00000026)
        } else {
            (0x0000004d, 0x00000080)
        }
    }
}

/// The inset shadows drawn over a library's avatar: dark ones at its top
/// and left and light ones at its bottom and right, which make it look
/// pressed in, and a faint white ring.
pub fn avatar_bevel() -> Vec<BoxShadow> {
    let inset = |x: f32, y: f32, blur: f32, color: u32| {
        BoxShadow::new(px(x), px(y), rgba(color).into())
            .blur_radius(px(blur))
            .inset()
    };
    vec![
        inset(0., 2., 3., 0x0000006b),
        inset(2., 0., 2., 0x00000029),
        inset(0., -1.5, 1., 0xffffff33),
        inset(-1., 0., 1., 0xffffff17),
        BoxShadow::new(px(0.), px(0.), rgba(0xffffff0f).into())
            .spread_radius(px(1.))
            .inset(),
    ]
}

/// The font features for numbers in columns: every digit has the same
/// width, and the zero has a slash.
pub fn tabular_figures() -> FontFeatures {
    FontFeatures(std::sync::Arc::new(vec![
        ("tnum".into(), 1),
        ("zero".into(), 1),
    ]))
}

// The colours of the tag groups. `tags.rs` chooses among them by the name
// of a tag's group, for the tag's chip and for the group's preset colour.
/// The colour of the groups `creator` and `studio`.
pub const TAG_CREATOR: u32 = 0xff6667;
/// The colour of the groups `character` and `person`.
pub const TAG_CHARACTER: u32 = 0x30d159;
pub const TAG_SERIES: u32 = 0xc499ff;
pub const TAG_SPECIES: u32 = 0x00aaff;
pub const TAG_RATING: u32 = 0xffd60a;
/// The colour of the group `system`.
pub const TAG_SYSTEM: u32 = 0xff9f0a;
/// The colour of every other group, including `general`.
pub const TAG_GENERAL: u32 = 0xbdbec0;

/// The colour of the chip of a folder that has no colour of its own.
pub const FOLDER_DEFAULT: u32 = 0x868e96;

/// The colour of the 1px highlight along the inner top edge of the colour
/// swatch in the smart folder dialog: white at 18% opacity.
pub const COLOR_SWATCH_HIGHLIGHT: u32 = 0xffffff2e;

/// The three colours of the audio visualizer's gradient: cyan, violet and
/// pink.
pub const VISUALIZER_CYAN: u32 = 0x31d7ff;
pub const VISUALIZER_VIOLET: u32 = 0x735cff;
pub const VISUALIZER_PINK: u32 = 0xff4fa3;

/// The colour of a star of a rating that is set.
pub const RATING: u32 = 0xff9f0a;

/// The colours of a status that is a warning, a failure or a success, as
/// status badges and status texts show them. They are the same in every
/// theme.
pub const WARNING: u32 = 0xe8a33d;
pub const DANGER: u32 = 0xef8b8b;
pub const SUCCESS: u32 = 0x5fb56b;
/// The colour of the text of a failure in the tables of the Subscriptions
/// screen: a light orange.
pub const FAILURE_TEXT: u32 = 0xf0ad91;

/// The colours of the task dialog (the dialog of an import), which are the
/// same in every theme. A task's icon, the square behind the icon and the
/// fill of its progress bar depend on whether the task is working, done or
/// has a warning. This one is the icon of a task that is working.
pub const TASK_WORKING_GLYPH: u32 = 0x8cc4ff;
pub const TASK_DONE_GLYPH: u32 = 0x7be39b;
/// The square behind the icon of a task that is done: green at 18%
/// opacity.
pub const TASK_DONE_TILE: u32 = 0x34c7592e;
/// The fill of the progress bar of a task that is done: green.
pub const TASK_DONE_FILL: u32 = 0x34c759;
pub const TASK_WARNING_GLYPH: u32 = 0xffb4a0;
/// The square behind the icon of a task that has a warning: orange at 16%
/// opacity.
pub const TASK_WARNING_TILE: u32 = 0xff785a29;
/// The border and the fill of a selected choice that is destructive: red
/// at 50% and at 8% opacity.
pub const TASK_DANGER_BORDER: u32 = 0xff475780;
pub const TASK_DANGER_FILL: u32 = 0xff475714;
/// The colour of the mark of a choice that is destructive.
pub const TASK_DANGER_MARK: u32 = 0xff8a95;

/// The colour of the border around a selected tile of the grid and of the
/// fill behind its name. It is the same in every theme.
pub const GRID_SELECTION: u32 = 0x3297ff;

// The colours of the badges on the grid's tiles are the same in every
// theme.
pub const BADGE_BACKGROUND: u32 = 0x00000080;
pub const BADGE_BORDER: u32 = 0x00000033;
pub const BADGE_TEXT: u32 = 0xffffffcc;

// The colours of what the viewer draws over a picture are the same in
// every theme.
/// The fill behind the notice the viewer shows at the first and the last
/// item: black at 60% opacity.
pub const VIEWER_NOTICE_BACKGROUND: u32 = 0x00000099;
/// The two colours of the checkerboard that the viewer draws behind a
/// picture with transparency: white and light gray.
pub const TRANSPARENCY_GRID_LIGHT: u32 = 0xffffff;
pub const TRANSPARENCY_GRID_DARK: u32 = 0xd9d9d9;
/// The fill and the border of the navigator, the small picture of the
/// whole image that the viewer shows while the image is zoomed in: black at
/// 40% opacity and white at 15%.
pub const NAVIGATOR_BACKGROUND: u32 = 0x00000066;
pub const NAVIGATOR_BORDER: u32 = 0xffffff26;
/// The rectangle in the navigator that marks the visible part of the
/// image: its border (near-white at 90% opacity), its fill (near-white at
/// 6%), and the shade over the rest of the navigator (black at 58%).
pub const NAVIGATOR_VIEWPORT_BORDER: u32 = 0xf8f9fbe6;
pub const NAVIGATOR_VIEWPORT_FILL: u32 = 0xf8f9fb0f;
pub const NAVIGATOR_SHADE: u32 = 0x00000094;
/// The colour of the icon of Quick Look's close button.
pub const QUICK_LOOK_GLYPH: u32 = 0xf8f9fb;
/// The fill of the toolbar of a detail window: black at 55% opacity.
pub const DETAIL_TOOLBAR_BACKGROUND: u32 = 0x0000008c;

// The colours of the Accept and Reject control of the Inbox review are the
// same in every theme.
/// The colours of the glow of the Accept button (green) and of the Reject
/// button (red).
pub const REVIEW_ACCEPT_GLOW: u32 = 0x36dc77;
pub const REVIEW_REJECT_GLOW: u32 = 0xff4b54;
/// The colours of the icons of the Accept and the Reject button while the
/// button glows: a pale green and a pale red, each at 98% opacity.
pub const REVIEW_ACCEPT_TINT: u32 = 0xe2ffedfa;
pub const REVIEW_REJECT_TINT: u32 = 0xffe6e7fa;
/// The colours of the vertical line between the Accept and the Reject
/// button. The line is a gradient: transparent at its ends, white at 25%
/// opacity a fifth of the way down and white at 16% four fifths of the way
/// down. A line of black at 18% opacity lies 1px to its right. The ends are
/// white at no opacity, not black at no opacity, so that the gradient does
/// not pass through gray.
pub const REVIEW_DIVIDER_EDGE: u32 = 0xffffff00;
pub const REVIEW_DIVIDER_TOP: u32 = 0xffffff40;
pub const REVIEW_DIVIDER_BOTTOM: u32 = 0xffffff29;
pub const REVIEW_DIVIDER_SHADE: u32 = 0x0000002e;
/// The outline of the Accept or Reject button that has the keyboard
/// focus: white at 75% opacity.
pub const REVIEW_FOCUS_RING: u32 = 0xffffffbf;

// The media player's controls are white on black in every theme.
/// The colour behind a video: black.
pub const MEDIA_BACKGROUND: u32 = 0x000000ff;
/// The colour at the bottom of the gradient behind the player's controls:
/// black at 60% opacity. The gradient is transparent at its top.
pub const MEDIA_CONTROLS_SHADE: u32 = 0x00000099;
/// The icon of a button of the player: white at 80% opacity, and at 95%
/// while the button is under the pointer or active.
pub const MEDIA_GLYPH: u32 = 0xffffffcc;
pub const MEDIA_GLYPH_STRONG: u32 = 0xfffffff2;
/// The fill of a button of the player under the pointer (white at 15%
/// opacity) and while pressed (white at 25%).
pub const MEDIA_BUTTON_HOVER: u32 = 0xffffff26;
pub const MEDIA_BUTTON_ACTIVE: u32 = 0xffffff40;
/// The text of the player's time, of the volume in percent and of the
/// rows of the speed menu: white at 70% opacity.
pub const MEDIA_TEXT: u32 = 0xffffffb3;
/// The separators between the player's controls, the buffered part of the
/// progress bar and the unplayed part of the waveform: white at 20%
/// opacity.
pub const MEDIA_FAINT: u32 = 0xffffff33;
/// The tracks of the progress bar and of the volume slider: white at 15%
/// opacity.
pub const MEDIA_TRACK: u32 = 0xffffff26;
/// The filled parts of the progress bar and of the volume slider, the
/// played part of the waveform and the waveform's cursor: white at 95%
/// opacity.
pub const MEDIA_FILL: u32 = 0xfffffff2;
/// The fill of the time shown above the progress bar under the pointer
/// and of the volume popup: black at 80% opacity.
pub const MEDIA_POPUP: u32 = 0x000000cc;
/// The fill of the speed menu: black at 90% opacity.
pub const MEDIA_MENU: u32 = 0x000000e6;
/// The border of the volume popup and of the speed menu, and the fill of
/// a row of the speed menu under the pointer: white at 10% opacity.
pub const MEDIA_POPUP_BORDER: u32 = 0xffffff1a;
/// The fill of the volume indicator that appears over the video when the
/// volume changes: black at 70% opacity.
pub const MEDIA_HUD: u32 = 0x000000b3;
/// The fill and the 1px border of the rectangle the user drags to select
/// tiles: blue at 12% opacity and at 50%.
pub const MARQUEE_FILL: u32 = 0x339af01f;
pub const MARQUEE_BORDER: u32 = 0x339af080;
/// The colour of the mark that shows where dragged tiles will be dropped
/// when the grid is reordered: a 2px line with a triangle at its top, in
/// blue.
pub const REORDER_INDICATOR: u32 = 0x228be6;
/// The fill and the text colour of an error message shown in the view of
/// a collection: red at 8% opacity behind light red text.
pub const GROUP_ERROR_BACKGROUND: u32 = 0xff475714;
pub const GROUP_ERROR_TEXT: u32 = 0xff9aa4;
/// The colours of the large preview of a tile: the colour that covers the
/// window behind it (black at 60% opacity) and the colour of the picture's
/// shadow (black at 70%).
pub const PREVIEW_BACKDROP: u32 = 0x00000099;
pub const PREVIEW_SHADOW: u32 = 0x000000b3;
/// The fill of the tile of an audio file: near-white at 5% opacity. Over
/// it lies a linear gradient from the same colour at the top to
/// `AUDIO_TINT_FAR`, near-white at 4% opacity, at the bottom.
pub const AUDIO_TINT: u32 = 0xf7f8f80d;
pub const AUDIO_TINT_FAR: u32 = 0xf7f8f80a;
/// Black at 20% opacity, for shadows and dark fills.
pub const BLACK_20: u32 = 0x00000033;
/// Black at 30% opacity, for shadows.
pub const BLACK_30: u32 = 0x0000004d;
/// Black at 10% opacity, for thin dark rings and soft shadows (the bottom
/// edge of the inner frame of an empty state, the ring of a ticked check
/// box, the edge of the inspector's preview).
pub const BLACK_10: u32 = 0x0000001a;
// Black at the other opacities that the shadows of components use.
/// Black at 15% opacity: the ring around the content panel of Settings.
pub const BLACK_15: u32 = 0x00000026;
/// Black at 18% opacity: the near shadow of a field's dropdown and the
/// shadow of the inspector's list of sources.
pub const BLACK_18: u32 = 0x0000002e;
/// Black at 22% opacity: the far shadow of a field's dropdown.
pub const BLACK_22: u32 = 0x00000038;
/// Black at 25% opacity: the shadow of a slider's thumb and the fill of
/// a text field in a row of a form.
pub const BLACK_25: u32 = 0x00000040;
/// The magnifier button in the corner of a tile of the grid: its fill,
/// black at 40% opacity, and its icon, white at 70%.
pub const GRID_MAGNIFIER_BACKGROUND: u32 = 0x00000066;
pub const GRID_MAGNIFIER_GLYPH: u32 = 0xffffffb3;

/// Holds the colours of the card of a subfolder's tile in the grid: its
/// fill as a gradient from top to bottom, its top edge, its side edges, and
/// the top edges of the two cards drawn behind it (`middle` and `back`).
#[derive(Clone, Copy)]
pub struct FolderCard {
    pub fill: (u32, u32),
    pub top: u32,
    pub side: u32,
    pub middle: u32,
    pub back: u32,
}

/// The card of a subfolder's tile in the dark themes. Its fill is a linear
/// gradient from white at 15% opacity at the top to white at 5% at the
/// bottom.
pub const FOLDER_CARD_DARK: FolderCard = FolderCard {
    fill: (0xffffff26, 0xffffff0d),
    top: 0xffffff26,
    side: 0xffffff0d,
    middle: 0xffffff26,
    back: 0xffffff1a,
};

/// The card of a subfolder's tile in the light themes. Its fill is black
/// at 10% opacity.
pub const FOLDER_CARD_LIGHT: FolderCard = FolderCard {
    fill: (0x0000001a, 0x0000001a),
    top: 0x00000033,
    side: 0x0000001a,
    middle: 0x00000033,
    back: 0x0000001a,
};
/// The colour of the name of a selected tile of the grid and of the format
/// badge on the inspector's preview: white, the same in every theme.
pub const BADGE_STRONG_TEXT: u32 = 0xffffff;
/// The colour behind the picture of the differences between two
/// duplicates on the Duplicates screen: black.
pub const DIFFERENCE_BACKGROUND: u32 = 0x000000;
/// The colour of the 1px highlight along the inner top edge of the
/// selected day in the date picker: white at 45% opacity.
pub const DATE_SELECTED_HIGHLIGHT: u32 = 0xffffff73;
/// The colours of a toggle switch: its track while it is off, gray at 25%
/// opacity, and its knob, white.
pub const TOGGLE_TRACK_OFF: u32 = 0x80808040;
pub const TOGGLE_KNOB: u32 = 0xffffff;
/// The border of the colour swatch in the smart folder dialog: white at
/// 22% opacity.
pub const COLOR_SWATCH_RING: u32 = 0xffffff38;
/// The two colours of the swatch of the "Auto" theme in Settings, which is
/// split diagonally into a light half and a dark half.
pub const THEME_SWATCH_AUTO: (u32, u32) = (0xf0f0f0, 0x1a1a1e);

// The colours of the colour editor are the same in every theme.
/// The marker of the chosen colour in the colour editor's square of
/// saturation and brightness: a white ring with a 1px ring of black at 65%
/// opacity around it.
pub const COLOR_CURSOR_RING: u32 = 0xffffff;
pub const COLOR_CURSOR_HALO: u32 = 0x000000a6;
/// The thumb of the hue slider: a line of white at 90% opacity over a
/// line of black at 60%.
pub const COLOR_HUE_THUMB_LIGHT: u32 = 0xffffffe6;
pub const COLOR_HUE_THUMB_DARK: u32 = 0x00000099;
/// The border of a preset colour: gray at 30% opacity.
pub const COLOR_PRESET_RING: u32 = 0x8080804d;
/// The preset that stands for no colour: a light gray fill with a darker
/// gray line through it.
pub const COLOR_NONE_FILL: u32 = 0xcccccc;
pub const COLOR_NONE_STROKE: u32 = 0x6a6a6a;

// The colours of the font viewer are the same in every theme.
/// The background colour and the text colour of each paper that the font
/// viewer can show a font's preview on.
pub const FONT_PAPER_LIGHT: (u32, u32) = (0xf7f8f8, 0x242529);
pub const FONT_PAPER_DARK: (u32, u32) = (0x18191c, 0xf8f9fb);
pub const FONT_PAPER_PURPLE: (u32, u32) = (0x2b2140, 0xfbf7ff);
pub const FONT_PAPER_YELLOW: (u32, u32) = (0xf5df91, 0x312b1d);
/// The colours of the buttons that choose the paper. A constant with one
/// value is the button's fill. A constant with two values is the button's
/// fill and the colour of its label. The button for the automatic paper is
/// the light and the dark fill split diagonally, with a gray label
/// (`FONT_SWATCH_AUTO_TEXT`).
pub const FONT_SWATCH_LIGHT: (u32, u32) = (0xf7f8f8, 0x35363a);
pub const FONT_SWATCH_DARK: u32 = 0x25262a;
pub const FONT_SWATCH_PURPLE: u32 = 0x6e4ea2;
pub const FONT_SWATCH_YELLOW: (u32, u32) = (0xd3a82e, 0x3a3017);
pub const FONT_SWATCH_AUTO_TEXT: u32 = 0x888888;
/// The colour of the label of a paper button that sets no colour of its
/// own, white at 82% opacity, and the 1px ring inside every paper button,
/// white at 12%.
pub const FONT_SWATCH_TEXT: u32 = 0xffffffd1;
pub const FONT_SWATCH_RING: u32 = 0xffffff1f;
/// The colour of the shadow of a glyph under the pointer in the font
/// viewer's table of glyphs: black at 35% opacity.
pub const FONT_GLYPH_SHADOW: u32 = 0x00000059;

// The colours of the document viewers' pages are the same in every theme.
/// The colour behind the pages of a document: a dark gray.
pub const DOCUMENT_BACKDROP: u32 = 0x2a2a2e;
/// The colour of a page: white.
pub const DOCUMENT_PAPER: u32 = 0xffffff;
/// The colours of a page of a text document: its text, its links, the
/// text of a quotation, the vertical line to the left of a quotation, and
/// the fill behind code.
pub const DOCUMENT_TEXT: u32 = 0x1f2023;
pub const DOCUMENT_LINK: u32 = 0x2773c9;
pub const DOCUMENT_QUOTE: u32 = 0x5b5e65;
pub const DOCUMENT_QUOTE_RULE: u32 = 0xc7c9cd;
pub const DOCUMENT_CODE: u32 = 0xeef0f2;
/// The colour of the text of a page of a book.
pub const BOOK_TEXT: u32 = 0x222222;
/// The font family of the text of a page of a book.
pub const FONT_BOOK: &str = "Georgia";

/// The shadow of a page of a PDF and of a page of a text document: black
/// at 34% opacity, 8px below the page, with a blur of 28px.
pub fn page_shadow() -> Vec<BoxShadow> {
    vec![BoxShadow::new(px(0.), px(8.), rgba(0x00000057).into()).blur_radius(px(28.))]
}

/// The shadow of a page of a comic, a DjVu document or a book: black at
/// 32% opacity, 4px below the page, with a blur of 18px.
pub fn sheet_shadow() -> Vec<BoxShadow> {
    vec![BoxShadow::new(px(0.), px(4.), rgba(0x00000052).into()).blur_radius(px(18.))]
}

// The colours of a notification's status icon are the same in every
// theme. The icon of an informational notification is drawn in the theme's
// primary colour.
/// The status icon of a notification of a success.
pub const NOTIFICATION_SUCCESS: u32 = 0x30d159;
/// The status icon of a notification of a warning.
pub const NOTIFICATION_WARNING: u32 = 0xff9f0a;
/// The status icon of a notification of an error.
pub const NOTIFICATION_ERROR: u32 = 0xff453a;
/// The distance from the top of the window to the top of a notification,
/// in pixels. It places the notification below the titlebar.
pub const NOTIFICATION_TOP: f32 = 76.;
/// The height of a notification, in pixels.
pub const NOTIFICATION_HEIGHT: f32 = 36.;

/// The fill of the window's close button under the pointer and while it
/// is pressed, where the app draws the window's buttons itself: the red
/// that Windows uses for its close button.
pub const CAPTION_CLOSE_HOVER: u32 = 0xe81123ff;
pub const CAPTION_CLOSE_PRESS: u32 = 0xc50f1fff;

// The fonts, the text sizes and the line heights.
/// The font family of the interface's text. `.SystemUIFont` is GPUI's name
/// for the system's interface font.
pub const FONT_UI: &str = ".SystemUIFont";
/// The font family of the text in the inspector and in the text fields of
/// the sidebar and the inspector.
pub const FONT_INSPECTOR: &str = "Roboto";
pub const FONT_INSPECTOR_MONO: &str = "Fira Mono";
/// The monospaced font family. The app bundles Fira Mono
/// (`assets/FiraMono.ttf`).
pub const FONT_MONO: &str = "Fira Mono";
/// The line height of 13px interface text, in pixels. It is a whole
/// number because a line of a fractional height would put centred text
/// half a pixel off.
pub const LINE_HEIGHT: f32 = 15.;
/// Sets the size of an element's text to one of the interface's text
/// sizes, and the line height to the size plus two pixels. For the
/// interface's fonts at these sizes, that is the font's ascent plus its
/// descent, each rounded to a whole pixel.
pub trait Text: Styled + Sized {
    fn text(self, size: f32) -> Self {
        self.text_size(px(size)).line_height(px(size + 2.))
    }
}

impl<T: Styled> Text for T {}

/// The smallest text size, in pixels.
pub const TEXT_XS: f32 = 10.;
/// The text size of captions, in pixels.
pub const TEXT_CAPTION: f32 = 11.;
/// The small text size, in pixels.
pub const TEXT_SM: f32 = 12.;
/// The medium text size, in pixels.
pub const TEXT_MD: f32 = 13.;
/// The large text size, in pixels.
pub const TEXT_LG: f32 = 14.;
/// The line height of a line of text in a dialog or form, in pixels.
pub const LINE_HEIGHT_MD: f32 = 20.;
/// The line height of running text as a multiple of its size, for text
/// that is given no other line height: 13px text has a line of 20.15px.
pub const LINE_HEIGHT_BODY: f32 = 1.55;
/// The largest text size, in pixels.
pub const TEXT_XL: f32 = 16.;

// The corner radii and the sizes of controls and of parts of the window.
/// The small corner radius, in pixels.
pub const RADIUS_SM: f32 = 4.;
/// The 5px radius of the search field and the sidebar's small buttons.
pub const RADIUS_CONTROL: f32 = 5.;
/// The medium corner radius, in pixels. Most controls have it.
pub const RADIUS_MD: f32 = 6.;
/// The large corner radius, in pixels, which panels and dialogs have.
pub const RADIUS_LG: f32 = 8.;
/// The height of a small control, in pixels.
pub const CONTROL_SM: f32 = 26.;
/// The height of a medium control, in pixels.
pub const CONTROL_MD: f32 = 28.;
/// The height of a large control, in pixels.
pub const CONTROL_LG: f32 = 32.;
/// The height of an action button, in pixels.
pub const ACTION_BUTTON: f32 = 30.;
/// The width and height of an icon button, in pixels.
pub const ICON_BUTTON: f32 = 24.;
/// The corner radius of a window on macOS, in pixels. The edge the app
/// draws around a window has the same radius.
pub const WINDOW_RADIUS: f32 = 10.;
/// The height of the titlebar, in pixels.
pub const TITLEBAR_HEIGHT: f32 = 48.;
/// The width of the sidebar until the user changes it, and the smallest
/// and largest width its edge can be dragged to, in pixels.
pub const SIDEBAR_WIDTH: f32 = 300.;
pub const SIDEBAR_MIN: f32 = 220.;
pub const SIDEBAR_MAX: f32 = 500.;
/// The width of the inspector until the user changes it, and the smallest
/// and largest width its edge can be dragged to, in pixels.
pub const INSPECTOR_WIDTH: f32 = 320.;
pub const INSPECTOR_MIN: f32 = 260.;
pub const INSPECTOR_MAX: f32 = 550.;
/// The height of a row of the sidebar, in pixels.
pub const SIDEBAR_ROW: f32 = 27.;
/// The indent of each level of the sidebar's trees, in pixels.
pub const SIDEBAR_INDENT: f32 = 20.;
/// The distance from the sidebar's left edge to the content of its rows,
/// in pixels. The positions of the section labels are derived from it too.
pub const SIDEBAR_INSET: f32 = 16.;
/// The horizontal padding of a section label of the sidebar, in pixels.
/// The label's left margin is smaller by the same amount, so the padding
/// does not move the label's text.
pub const SIDEBAR_SECTION_INSET: f32 = 5.;
/// The space to the right of the sidebar's rows, in pixels, in addition
/// to the scrollbar's gutter.
pub const SIDEBAR_LIST_PADDING: f32 = 8.;
/// The width of the column that holds a sidebar row's icon, in pixels.
pub const SIDEBAR_GLYPH_COLUMN: f32 = 26.;
/// The width of the column that holds the arrow which folds and unfolds
/// a sidebar row, in pixels.
pub const SIDEBAR_DISCLOSURE: f32 = 14.;
/// The height and the corner radius of the text field that renames a
/// sidebar row in place, in pixels.
pub const SIDEBAR_RENAME_HEIGHT: f32 = 20.;
pub const SIDEBAR_RENAME_RADIUS: f32 = 3.;
/// The size of a sidebar row's icon, in pixels.
pub const SIDEBAR_ICON: f32 = 19.;
/// The corner radius of a tile of the grid, in pixels.
pub const TILE_RADIUS: f32 = 4.;

#[cfg(test)]
mod tests {
    use super::{
        BLUE, DARK, GRAY, LIGHT, LIGHT_GRAY, PURPLE, blend, named, parse_css_color, parse_hex,
        parse_hex_code,
    };
    use gpui::{Rgba, rgb, rgba};

    #[test]
    fn a_blend_from_transparent_keeps_the_target_hue() {
        // `blend` multiplies the colour channels by the opacity before it
        // interpolates, so a fade from transparent black to a colour has
        // that colour's hue all the way and never passes through gray.
        let clear = Rgba {
            r: 0.,
            g: 0.,
            b: 0.,
            a: 0.,
        };
        let half = blend(clear, DARK.primary(), 0.5);
        assert_eq!(
            (half.r, half.g, half.b),
            (DARK.primary().r, DARK.primary().g, DARK.primary().b)
        );
        assert_eq!(half.a, 0.5);
        assert_eq!(blend(DARK.border(), DARK.primary(), 0.), DARK.border());
        assert_eq!(blend(DARK.border(), DARK.primary(), 1.), DARK.primary());
    }

    #[test]
    fn derived_surfaces_match_the_resolved_css() {
        // The rail of the Dark theme is rgb(31, 32, 35).
        assert_eq!(DARK.rail(), rgb(0x1f2023));
        assert_eq!(DARK.surface_2(), rgb(0x232427));
        assert_eq!(DARK.border(), rgba(0xf8f9fb1a));
        assert_eq!(DARK.border_secondary(), rgba(0xf8f9fb14));
        assert_eq!(DARK.text_tertiary(), rgba(0xf8f9fb99));
        assert_eq!(DARK.glass(), rgba(0x18191ceb));
    }

    #[test]
    fn stored_theme_names_resolve_as_resolve_theme_does() {
        assert_eq!(named("auto", true), DARK);
        assert_eq!(named("auto", false), LIGHT);
        assert_eq!(named("lightgray", true), LIGHT_GRAY);
        assert_eq!(named("gray", false), GRAY);
        assert_eq!(named("blue", false), BLUE);
        assert_eq!(named("purple", false), PURPLE);
        // An unknown name, and the name of a theme with a transparent
        // window, give Dark.
        assert_eq!(named("vibrancy", false), DARK);
        assert_eq!(named("nonsense", false), DARK);
    }

    #[test]
    fn light_themes_resolve_their_own_surfaces() {
        assert_eq!(LIGHT.border(), rgba(0x2c2f3226));
        assert_eq!(LIGHT.border_secondary(), rgba(0x2c2f321a));
        assert_eq!(LIGHT.control_surface(), rgba(0xffffff80));
        assert_eq!(LIGHT_GRAY.glass(), rgba(0xe3e4e6e6));
        assert_eq!(BLUE.rail(), rgb(0x141d36));
    }

    #[test]
    fn colour_codes_parse_as_strictly_as_their_callers_need() {
        assert_eq!(parse_hex("#3297ff"), Some(0x3297ff));
        assert_eq!(parse_hex("868e96"), Some(0x868e96));
        assert_eq!(parse_hex("#fff"), None);
        assert_eq!(parse_hex("#zzzzzz"), None);
        assert_eq!(parse_hex("+12345"), None);
        assert_eq!(parse_hex_code("#0000FF"), Some(0xff));
        assert_eq!(parse_hex_code("0000FF"), None);
        assert_eq!(parse_hex_code("#12345g"), None);
        assert_eq!(parse_css_color("rgb(1, 2, 3)"), Some(0x010203));
        assert_eq!(parse_css_color("#abc"), Some(0xaabbcc));
        assert_eq!(parse_css_color(" #FA5252 "), Some(0xfa5252));
        assert_eq!(parse_css_color("blue"), None);
    }
}
