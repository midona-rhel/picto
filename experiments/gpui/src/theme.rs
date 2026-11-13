//! Design tokens. `src/shared/styles/tokens.css` is the source: a theme is one
//! background, one contrast colour and one text colour, and every surface,
//! border and overlay is the contrast colour at some strength. Views take
//! colours and sizes from here and write none of their own.
use gpui::*;

#[derive(Clone, Copy)]
pub struct Theme {
    background: u32,
    contrast: u32,
    text: u32,
    primary: u32,
}

impl Global for Theme {}

/// `--color-negative`, the same in every theme.
const NEGATIVE: u32 = 0xff4757;

/// `:root` in `tokens.css`.
pub const DARK: Theme = Theme {
    background: 0x18191c,
    contrast: 0xf8f9fb,
    text: 0xf8f9fb,
    primary: 0x3297ff,
};

pub fn theme(cx: &App) -> Theme {
    *cx.global::<Theme>()
}

/// `color-mix(in srgb, <color> <percent>%, transparent)`.
fn translucent(color: u32, percent: f32) -> Rgba {
    rgba(color << 8 | (percent / 100. * 255.).round() as u32)
}

/// `color-mix(in srgb, <color> <percent>%, <base>)`.
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
    /// `--color-bg-app`
    pub fn background(&self) -> Rgba {
        rgb(self.background)
    }
    /// `--color-rail-surface`, the sidebar and inspector.
    pub fn rail(&self) -> Rgba {
        mixed(self.contrast, 3., self.background)
    }
    /// `--color-surface-2`, placeholders.
    pub fn surface_2(&self) -> Rgba {
        mixed(self.contrast, 5., self.background)
    }
    /// `--color-surface-hover`
    pub fn surface_hover(&self) -> Rgba {
        translucent(self.contrast, 5.)
    }
    /// `--color-surface-active`
    pub fn surface_active(&self) -> Rgba {
        translucent(self.contrast, 10.)
    }
    /// `--color-control-surface`, text fields.
    pub fn control_surface(&self) -> Rgba {
        rgba(0x00000033)
    }
    /// `--color-hover-overlay`
    pub fn hover_overlay(&self) -> Rgba {
        translucent(self.contrast, 8.)
    }
    /// `--color-text-primary`
    pub fn text(&self) -> Rgba {
        rgb(self.text)
    }
    /// `--color-text-secondary`
    pub fn text_secondary(&self) -> Rgba {
        translucent(self.text, 70.)
    }
    /// `--color-text-tertiary`
    pub fn text_tertiary(&self) -> Rgba {
        translucent(self.text, 60.)
    }
    /// `--color-border-primary`
    pub fn border(&self) -> Rgba {
        translucent(self.contrast, 10.)
    }
    /// `--color-border-secondary`, rail edges and separators.
    pub fn border_secondary(&self) -> Rgba {
        translucent(self.contrast, 8.)
    }
    /// `color-mix(in srgb, var(--color-text-primary) NN%, transparent)`.
    pub fn text_at(&self, percent: f32) -> Rgba {
        translucent(self.text, percent)
    }
    /// `--color-contrast-NN`
    pub fn contrast(&self, percent: f32) -> Rgba {
        translucent(self.contrast, percent)
    }
    /// `--color-primary`, selection and focus.
    pub fn primary(&self) -> Rgba {
        rgb(self.primary)
    }
    /// `::selection`: `--color-primary-30`.
    pub fn selection(&self) -> Rgba {
        translucent(self.primary, 30.)
    }
    /// `--glass-bg`: floating surfaces.
    pub fn glass(&self) -> Rgba {
        translucent(self.background, 92.)
    }
    /// `LibraryAvatar`: `color-mix(currentColor 10%, control-surface)` with
    /// the secondary text colour.
    pub fn avatar_surface(&self) -> Rgba {
        blend(self.control_surface(), self.text_secondary(), 0.1)
    }
    /// `--glass-border-top`
    pub fn glass_border_top(&self) -> Rgba {
        rgba(0xffffff40)
    }
    /// `--color-border-focus`: `--color-primary-60`.
    pub fn border_focus(&self) -> Rgba {
        translucent(self.primary, 60.)
    }
    /// `--sidebar-tree-guide-color`: a quarter of the secondary text colour.
    pub fn tree_guide(&self) -> Rgba {
        translucent(self.text, 17.5)
    }
    /// `--color-negative`: the destructive row of a menu.
    pub fn negative(&self) -> Rgba {
        rgb(NEGATIVE)
    }
    /// `color-mix(in srgb, var(--color-negative) NN%, transparent)`
    pub fn negative_at(&self, percent: f32) -> Rgba {
        translucent(NEGATIVE, percent)
    }
    /// `--glass-border-side`
    pub fn glass_border(&self) -> Rgba {
        rgba(0xffffff26)
    }
    /// `--color-subtle-overlay`: `--color-contrast-10`.
    pub fn subtle_overlay(&self) -> Rgba {
        translucent(self.contrast, 10.)
    }
    /// `--color-pressed-overlay`: `--color-contrast-20`.
    pub fn pressed_overlay(&self) -> Rgba {
        translucent(self.contrast, 20.)
    }
    /// `--color-footer-surface`: table headers, tab tracks and dialog footers.
    pub fn footer_surface(&self) -> Rgba {
        translucent(self.contrast, 3.)
    }
    /// `color-mix(in srgb, var(--color-bg-app) 90%, var(--color-text-primary) 10%)`:
    /// the select button's fill.
    pub fn select_surface(&self) -> Rgba {
        mixed(self.text, 10., self.background)
    }
    /// `--color-primary-NN`
    pub fn primary_at(&self, percent: f32) -> Rgba {
        translucent(self.primary, percent)
    }
    /// `--color-on-primary`: text on a primary surface.
    pub fn on_primary(&self) -> Rgba {
        rgb(0xffffff)
    }
    /// `--glass-border-bottom`
    pub fn glass_border_bottom(&self) -> Rgba {
        rgba(0xffffff1a)
    }
    /// Chromium's `::placeholder` colour (`darkGray`), for inputs the app
    /// does not give a placeholder colour of its own.
    pub fn placeholder(&self) -> Rgba {
        rgb(0xa9a9a9)
    }
    /// `color-mix(in srgb, var(--color-control-surface) 50%, transparent)`:
    /// the footer of the library switcher.
    pub fn control_surface_half(&self) -> Rgba {
        rgba(0x0000001a)
    }
    /// `--color-text-disabled`
    pub fn text_disabled(&self) -> Rgba {
        translucent(self.text, 22.)
    }
    /// `.shortcutConflict`: `var(--color-warning, #FFA500)`. No theme
    /// defines `--color-warning`, and this rule's fallback differs from the
    /// subscription styles' (`WARNING`).
    pub fn shortcut_conflict(&self) -> Rgba {
        rgb(0xffa500)
    }
}

/// `.btnDanger`'s label colour.
pub const DANGER_TEXT: u32 = 0xff9aa4;
/// The error banner's text (`TagManagerScreen.module.css` `.error`).
pub const ERROR_TEXT: u32 = 0xffb4b4;
/// `GlassModal` `.backdrop`: black at 55%.
pub const MODAL_BACKDROP: u32 = 0x0000008c;

/// `color` with no alpha: what a CSS transition from `transparent` toward
/// it starts from, keeping its hue (see [`blend`]).
pub fn clear(color: Rgba) -> Rgba {
    Rgba { a: 0., ..color }
}

/// `#rrggbb` as a number; the `#` may be left out.
pub fn parse_hex(text: &str) -> Option<u32> {
    let digits = text.strip_prefix('#').unwrap_or(text);
    (digits.len() == 6 && digits.chars().all(|digit| digit.is_ascii_hexdigit()))
        .then(|| u32::from_str_radix(digits, 16).ok())
        .flatten()
}

/// `#RRGGBB` with its `#`, as the colour editor's field takes a code.
pub fn parse_hex_code(text: &str) -> Option<u32> {
    text.starts_with('#').then(|| parse_hex(text)).flatten()
}

/// A colour as the settings store one: `#rgb`, `#rrggbb` or `rgb(r, g, b)`.
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

/// The colour `amount` of the way from `from` to `to`, as a CSS colour
/// transition passes through it (interpolated with alpha premultiplied).
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

/// `--shadow-panel`
pub fn panel_shadow() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(12.), rgba(0x0000004d).into()).blur_radius(px(24.)),
        BoxShadow::new(px(0.), px(0.), rgba(0x00000080).into()).spread_radius(px(1.)),
    ]
}

/// `--shadow-panel-small`
pub fn panel_shadow_small() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(3.), rgba(0x0000004d).into()).blur_radius(px(12.)),
        BoxShadow::new(px(0.), px(0.), rgba(0x00000080).into()).spread_radius(px(1.)),
    ]
}

/// `LibraryAvatar.module.css` `.avatar::after`: its inset shadows.
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

/// `font-variant-numeric: tabular-nums slashed-zero`
pub fn tabular_figures() -> FontFeatures {
    FontFeatures(std::sync::Arc::new(vec![
        ("tnum".into(), 1),
        ("zero".into(), 1),
    ]))
}

/// A set star in `StarRating.module.css`.
pub const RATING: u32 = 0xff9f0a;

/// `--color-accent`, which no theme defines: its `#3297ff` fallback.
pub const ACCENT: u32 = 0x3297ff;
/// `--color-warning`, `--color-danger` and `--color-success`, which no theme
/// defines: the fallbacks the subscription styles give them.
pub const WARNING: u32 = 0xe8a33d;
pub const DANGER: u32 = 0xef8b8b;
pub const SUCCESS: u32 = 0x5fb56b;
/// The subscription tables' warm failure text (`.qFailureText`).
pub const FAILURE_TEXT: u32 = 0xf0ad91;
/// Smart merge's survivor ring (`.mediaCard::after`): a green tint at 8%
/// and a 1px ring at 50% of `rgb(72, 199, 116)`.
pub const MERGE_SURVIVOR_FILL: u32 = 0x48c77414;
pub const MERGE_SURVIVOR_RING: u32 = 0x48c77480;

/// `GRID_SELECTION_COLOR`: the grid's selection stroke and name pill, the
/// same in every theme.
pub const GRID_SELECTION: u32 = 0x3297ff;

// Grid badges are fixed, not themed (`gridAppearance.ts`).
pub const BADGE_BACKGROUND: u32 = 0x00000080;
pub const BADGE_BORDER: u32 = 0x00000033;
pub const BADGE_TEXT: u32 = 0xffffffcc;

// The viewer's overlays are fixed, not themed (`MediaView.module.css`).
/// `.boundaryLeft` / `.boundaryRight`: `rgba(0, 0, 0, 0.6)`.
pub const VIEWER_NOTICE_BACKGROUND: u32 = 0x00000099;
/// `.navigator`: `rgba(0, 0, 0, 0.4)`, bordered `rgba(255, 255, 255, 0.15)`.
pub const NAVIGATOR_BACKGROUND: u32 = 0x00000066;
pub const NAVIGATOR_BORDER: u32 = 0xffffff26;
/// `.navigatorViewport`: border `rgba(248, 249, 251, 0.9)`, fill
/// `rgba(248, 249, 251, 0.06)`, and the `rgba(0, 0, 0, 0.58)` shade around it.
pub const NAVIGATOR_VIEWPORT_BORDER: u32 = 0xf8f9fbe6;
pub const NAVIGATOR_VIEWPORT_FILL: u32 = 0xf8f9fb0f;
pub const NAVIGATOR_SHADE: u32 = 0x00000094;
/// Quick Look's close button: `color: #f8f9fb` (`QuickLookHost.module.css`).
pub const QUICK_LOOK_GLYPH: u32 = 0xf8f9fb;
/// The detail window's toolbar: `rgba(0, 0, 0, 0.55)` (`DetailWindow.module.css`).
pub const DETAIL_TOOLBAR_BACKGROUND: u32 = 0x0000008c;

// The media player's controls are fixed white on black, not themed
// (`VideoPlayer.module.css`); alpha rounded to the nearest byte.
/// `.root`: `#000` behind video.
pub const MEDIA_BACKGROUND: u32 = 0x000000ff;
/// `.controls`: `linear-gradient(to top, rgba(0,0,0,0.6) 0%, transparent)`.
pub const MEDIA_CONTROLS_SHADE: u32 = 0x00000099;
/// `.icBtn` glyph `rgba(255,255,255,0.8)`, hovered or active `0.95`.
pub const MEDIA_GLYPH: u32 = 0xffffffcc;
pub const MEDIA_GLYPH_STRONG: u32 = 0xfffffff2;
/// `.icBtn:hover` `rgba(255,255,255,0.15)`, `:active` `0.25`.
pub const MEDIA_BUTTON_HOVER: u32 = 0xffffff26;
pub const MEDIA_BUTTON_ACTIVE: u32 = 0xffffff40;
/// `.timeDisplay`, `.volumePercent`, `.rateMenuItem`: `rgba(255,255,255,0.7)`.
pub const MEDIA_TEXT: u32 = 0xffffffb3;
/// `.separator` and `.progressBuffered`: `rgba(255,255,255,0.2)`;
/// `.waveformBase` the same.
pub const MEDIA_FAINT: u32 = 0xffffff33;
/// `.progressTrack` and `.volumeTrack`: `rgba(255,255,255,0.15)`.
pub const MEDIA_TRACK: u32 = 0xffffff26;
/// `.progressFill`, `.volumeFill`, `.waveformImage`, `.waveformCursor`:
/// `rgba(255,255,255,0.95)`.
pub const MEDIA_FILL: u32 = 0xfffffff2;
/// `.progressHoverTime` and `.volumePopup`: `rgba(0,0,0,0.8)`.
pub const MEDIA_POPUP: u32 = 0x000000cc;
/// `.rateMenu`: `rgba(0,0,0,0.9)`.
pub const MEDIA_MENU: u32 = 0x000000e6;
/// `.volumePopup` and `.rateMenu` border: `rgba(255,255,255,0.1)`; a rate
/// row's hover the same.
pub const MEDIA_POPUP_BORDER: u32 = 0xffffff1a;
/// `.volumeHud`: `rgba(0,0,0,0.7)`.
pub const MEDIA_HUD: u32 = 0x000000b3;
/// `.marquee` in `CanvasGrid.module.css`: the rubber band's fill,
/// rgba(51, 154, 240, 0.12), and its 1px edge at 0.5.
pub const MARQUEE_FILL: u32 = 0x339af01f;
pub const MARQUEE_BORDER: u32 = 0x339af080;
/// `HoverPreviewPortal.tsx`: the backdrop, rgba(0, 0, 0, 0.6), and the
/// image's shadow colour, rgba(0, 0, 0, 0.7).
pub const PREVIEW_BACKDROP: u32 = 0x00000099;
pub const PREVIEW_SHADOW: u32 = 0x000000b3;
/// `drawBase.ts`: an audio tile's tint in the dark theme,
/// rgba(247, 248, 248, 0.05). The radial gradient laid over it runs from
/// 0.05 at the top centre to 0.03 at a full tile's distance; a linear
/// gradient from 0.05 to 0.04 stands in for it.
pub const AUDIO_TINT: u32 = 0xf7f8f80d;
pub const AUDIO_TINT_FAR: u32 = 0xf7f8f80a;
/// `--color-black-20`
pub const BLACK_20: u32 = 0x00000033;
/// `--color-black-30`
pub const BLACK_30: u32 = 0x0000004d;
/// `--color-black-10`, rgba(0, 0, 0, 0.1): thin dark rings and soft
/// shadows (the bottom edge of `EmptyState`'s inner frame, a ticked check
/// box, the inspector preview's edge).
pub const BLACK_10: u32 = 0x0000001a;
// Black at the other strengths component shadows are written in.
/// rgba(0, 0, 0, 0.15): the subscription dialog's and the settings card's
/// soft shadows.
pub const BLACK_15: u32 = 0x00000026;
/// rgba(0, 0, 0, 0.18): a field's dropdown (`0 1px 3px`) and the source
/// list (`0 4px 10px`).
pub const BLACK_18: u32 = 0x0000002e;
/// rgba(0, 0, 0, 0.22): a field's dropdown (`0 8px 20px`).
pub const BLACK_22: u32 = 0x00000038;
/// rgba(0, 0, 0, 0.25): a slider thumb's shadow, and the subscription
/// dialog's ring.
pub const BLACK_25: u32 = 0x00000040;
/// `CanvasGrid`'s magnifier corner: rgba(0, 0, 0, 0.4) under a glyph of
/// rgba(255, 255, 255, 0.7).
pub const GRID_MAGNIFIER_BACKGROUND: u32 = 0x00000066;
pub const GRID_MAGNIFIER_GLYPH: u32 = 0xffffffb3;
/// The grid's selected name pill and the inspector preview's format badge:
/// white text, the same in every theme.
pub const BADGE_STRONG_TEXT: u32 = 0xffffff;
/// `.differenceComposite`: duplicate review's difference picture is drawn
/// on black.
pub const DIFFERENCE_BACKGROUND: u32 = 0x000000;
/// `DatePickerButton.module.css` `.daySelected`:
/// `inset 0 1px 0 rgba(255, 255, 255, 0.45)`.
pub const DATE_SELECTED_HIGHLIGHT: u32 = 0xffffff73;
/// `ToggleSwitch.module.css`: the off track, rgba(128, 128, 128, 0.25), and
/// the white knob.
pub const TOGGLE_TRACK_OFF: u32 = 0x80808040;
pub const TOGGLE_KNOB: u32 = 0xffffff;
/// The ring of a colour swatch beside a smart folder's colour field:
/// rgba(255, 255, 255, 0.22).
pub const COLOR_SWATCH_RING: u32 = 0xffffff38;
/// Settings' "Auto" theme swatch: `linear-gradient(135deg, #f0f0f0 50%,
/// #1a1a1e 50%)`.
pub const THEME_SWATCH_AUTO: (u32, u32) = (0xf0f0f0, 0x1a1a1e);

// `ColorFilterEditor.module.css`, fixed, not themed.
/// `.cursor`: a white ring with a `0 0 0 1px rgba(0, 0, 0, .65)` halo.
pub const COLOR_CURSOR_RING: u32 = 0xffffff;
pub const COLOR_CURSOR_HALO: u32 = 0x000000a6;
/// `.hue::-webkit-slider-thumb`: a `rgba(255,255,255,.9)` line over a
/// `rgba(0,0,0,.6)` one.
pub const COLOR_HUE_THUMB_LIGHT: u32 = 0xffffffe6;
pub const COLOR_HUE_THUMB_DARK: u32 = 0x00000099;
/// `.preset`: `border: 1px solid rgba(128,128,128,.3)`.
pub const COLOR_PRESET_RING: u32 = 0x8080804d;
/// `.none`: `#ccc`, struck through in `#6a6a6a`.
pub const COLOR_NONE_FILL: u32 = 0xcccccc;
pub const COLOR_NONE_STROKE: u32 = 0x6a6a6a;

// The font viewer's preview papers (`FontViewer.module.css`), fixed, not
// themed.
/// `.root[data-preview-theme]`: each paper's `--font-page-bg` and
/// `--font-page-fg`.
pub const FONT_PAPER_LIGHT: (u32, u32) = (0xf7f8f8, 0x242529);
pub const FONT_PAPER_DARK: (u32, u32) = (0x18191c, 0xf8f9fb);
pub const FONT_PAPER_PURPLE: (u32, u32) = (0x2b2140, 0xfbf7ff);
pub const FONT_PAPER_YELLOW: (u32, u32) = (0xf5df91, 0x312b1d);
/// The paper swatches (`.theme`): `.light`, `.dark`, `.purple` and
/// `.yellow`'s backgrounds and, where they set one, their colour; `.auto`
/// is the light and dark swatches split diagonally, in `#888`.
pub const FONT_SWATCH_LIGHT: (u32, u32) = (0xf7f8f8, 0x35363a);
pub const FONT_SWATCH_DARK: u32 = 0x25262a;
pub const FONT_SWATCH_PURPLE: u32 = 0x6e4ea2;
pub const FONT_SWATCH_YELLOW: (u32, u32) = (0xd3a82e, 0x3a3017);
pub const FONT_SWATCH_AUTO_TEXT: u32 = 0x888888;
/// `.theme`: `color: rgba(255,255,255,.82)` and
/// `box-shadow: inset 0 0 0 1px rgba(255,255,255,.12)`.
pub const FONT_SWATCH_TEXT: u32 = 0xffffffd1;
pub const FONT_SWATCH_RING: u32 = 0xffffff1f;
/// `.glyphGrid span:hover`: `0 8px 22px rgba(0,0,0,.35)`.
pub const FONT_GLYPH_SHADOW: u32 = 0x00000059;

// The document viewers' page colours are fixed, not themed.
/// `DocumentViewerShell.module.css` `.viewport`: `rgb(42, 42, 46)`.
pub const DOCUMENT_BACKDROP: u32 = 0x2a2a2e;
/// A page's paper, `#fff`.
pub const DOCUMENT_PAPER: u32 = 0xffffff;
/// `TextDocumentViewer.module.css` `.page`: text `#1f2023`, links
/// `#2773c9`, quotes `#5b5e65` behind a `#c7c9cd` rule, code on `#eef0f2`.
pub const DOCUMENT_TEXT: u32 = 0x1f2023;
pub const DOCUMENT_LINK: u32 = 0x2773c9;
pub const DOCUMENT_QUOTE: u32 = 0x5b5e65;
pub const DOCUMENT_QUOTE_RULE: u32 = 0xc7c9cd;
pub const DOCUMENT_CODE: u32 = 0xeef0f2;
/// `ArchiveDocumentViewer.module.css` `.bookPage`: text `#222`.
pub const BOOK_TEXT: u32 = 0x222222;
/// `.bookPage`'s `Georgia, serif`.
pub const FONT_BOOK: &str = "Georgia";

/// `PdfViewer.module.css` `.page` and the text page:
/// `0 8px 28px rgba(0, 0, 0, 0.34)`.
pub fn page_shadow() -> Vec<BoxShadow> {
    vec![BoxShadow::new(px(0.), px(8.), rgba(0x00000057).into()).blur_radius(px(28.))]
}

/// The comic, DjVu and book pages: `0 4px 18px rgba(0, 0, 0, .32)`.
pub fn sheet_shadow() -> Vec<BoxShadow> {
    vec![BoxShadow::new(px(0.), px(4.), rgba(0x00000052).into()).blur_radius(px(18.))]
}

// ── Typography ──────────────────────────────────────────────────
/// `--font-family-ui`. Its first name, SF Pro Text, is not an installed
/// family, so Chromium draws the system font; this is the same face.
pub const FONT_UI: &str = ".SystemUIFont";
/// `Picto Inspector Roboto`
pub const FONT_INSPECTOR: &str = "Roboto";
pub const FONT_INSPECTOR_MONO: &str = "Fira Mono";
/// `--font-family-mono`: SF Mono is not an installed family, so Chromium
/// falls through to Fira Mono, which the app bundles.
pub const FONT_MONO: &str = "Fira Mono";
/// CSS `line-height: normal` for 13px UI text. Chromium rounds a font's
/// ascent and descent to whole pixels, so the line is 15px, not 1.18em; a
/// fractional line box would put centred text half a pixel off.
pub const LINE_HEIGHT: f32 = 15.;
/// Text at one of the UI sizes, on the line Chromium gives it. Its
/// `line-height: normal` is the font's ascent and descent each rounded to a
/// whole pixel, which for the UI fonts at these sizes is the size plus two.
pub trait Text: Styled + Sized {
    fn text(self, size: f32) -> Self {
        self.text_size(px(size)).line_height(px(size + 2.))
    }
}

impl<T: Styled> Text for T {}

/// `--font-size-xs`
pub const TEXT_XS: f32 = 10.;
/// `--font-size-caption`
pub const TEXT_CAPTION: f32 = 11.;
/// `--font-size-sm`
pub const TEXT_SM: f32 = 12.;
/// `--font-size-md`
pub const TEXT_MD: f32 = 13.;
/// `--font-size-lg`
pub const TEXT_LG: f32 = 14.;
/// `--line-height-md`
pub const LINE_HEIGHT_MD: f32 = 20.;
/// `--font-size-xl`
pub const TEXT_XL: f32 = 16.;
/// `--font-size-title`
pub const TEXT_TITLE: f32 = 18.;

// ── Shape and size ──────────────────────────────────────────────
/// `--radius-sm`
pub const RADIUS_SM: f32 = 4.;
/// The 5px radius of the search field and the sidebar's small buttons.
pub const RADIUS_CONTROL: f32 = 5.;
/// `--radius-md`
pub const RADIUS_MD: f32 = 6.;
/// `--radius-lg`
pub const RADIUS_LG: f32 = 8.;
/// `--control-height-sm`
pub const CONTROL_SM: f32 = 26.;
/// `--control-height-md`
pub const CONTROL_MD: f32 = 28.;
/// `--control-height-lg`
pub const CONTROL_LG: f32 = 32.;
/// `--action-button-height`
pub const ACTION_BUTTON: f32 = 30.;
/// `--icon-button-size`
pub const ICON_BUTTON: f32 = 24.;
/// The macOS window's corner radius, which its rim follows.
pub const WINDOW_RADIUS: f32 = 10.;
/// `--titlebar-height`
pub const TITLEBAR_HEIGHT: f32 = 48.;
/// `--sidebar-width`
pub const SIDEBAR_WIDTH: f32 = 300.;
/// `INSPECTOR_DEFAULT_WIDTH` and the limits its edge drags between.
pub const INSPECTOR_WIDTH: f32 = 320.;
pub const INSPECTOR_MIN: f32 = 260.;
pub const INSPECTOR_MAX: f32 = 550.;
/// `--sidebar-row-height`
pub const SIDEBAR_ROW: f32 = 27.;
/// `--sidebar-indent`
pub const SIDEBAR_INDENT: f32 = 20.;
/// `--sidebar-content-inset`: the left edge rows and section labels share.
pub const SIDEBAR_INSET: f32 = 16.;
/// `--sidebar-section-control-inset`: section labels sit this far in.
pub const SIDEBAR_SECTION_INSET: f32 = 5.;
/// `--sidebar-list-right-padding`, beside the scrollbar gutter.
pub const SIDEBAR_LIST_PADDING: f32 = 8.;
/// `--sidebar-glyph-column`
pub const SIDEBAR_GLYPH_COLUMN: f32 = 26.;
/// `--sidebar-disclosure-column`
pub const SIDEBAR_DISCLOSURE: f32 = 14.;
/// `Sidebar.module.css` `.renameInput`: the in-place rename field's height
/// and corner radius.
pub const SIDEBAR_RENAME_HEIGHT: f32 = 20.;
pub const SIDEBAR_RENAME_RADIUS: f32 = 3.;
/// The sidebar's glyph size (`IC` in `Sidebar.tsx`).
pub const SIDEBAR_ICON: f32 = 19.;
/// `GRID_TILE_RADIUS`
pub const TILE_RADIUS: f32 = 4.;

#[cfg(test)]
mod tests {
    use super::{DARK, blend, parse_css_color, parse_hex, parse_hex_code};
    use gpui::{Rgba, rgb, rgba};

    #[test]
    fn a_blend_from_transparent_keeps_the_target_hue() {
        // CSS interpolates premultiplied, so fading in from `transparent`
        // never passes through grey.
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
        // "Inspector surface over the dark base resolves to rgb(31 32 35)."
        assert_eq!(DARK.rail(), rgb(0x1f2023));
        assert_eq!(DARK.surface_2(), rgb(0x232427));
        assert_eq!(DARK.border(), rgba(0xf8f9fb1a));
        assert_eq!(DARK.border_secondary(), rgba(0xf8f9fb14));
        assert_eq!(DARK.text_tertiary(), rgba(0xf8f9fb99));
        assert_eq!(DARK.glass(), rgba(0x18191ceb));
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
