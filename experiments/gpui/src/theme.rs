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
    /// `--color-footer-surface`: table headers and tab tracks.
    pub fn footer_surface(&self) -> Rgba {
        translucent(self.contrast, 3.)
    }
    /// `--color-on-primary`: text on the primary colour.
    pub fn on_primary(&self) -> Rgba {
        rgb(0xffffff)
    }
    /// `--color-primary-60`
    pub fn primary_60(&self) -> Rgba {
        translucent(self.primary, 60.)
    }
    /// `color-mix(in srgb, var(--color-bg-app) 90%, var(--color-text-primary) 10%)`:
    /// the select button's fill.
    pub fn select_surface(&self) -> Rgba {
        mixed(self.text, 10., self.background)
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

/// `GRID_SELECTION_COLOR`: the grid's selection stroke and name pill, the
/// same in every theme.
pub const GRID_SELECTION: u32 = 0x3297ff;

// Grid badges are fixed, not themed (`gridAppearance.ts`).
pub const BADGE_BACKGROUND: u32 = 0x00000080;
pub const BADGE_BORDER: u32 = 0x00000033;
pub const BADGE_TEXT: u32 = 0xffffffcc;

// ── Typography ──────────────────────────────────────────────────
/// `--font-family-ui`. Its first name, SF Pro Text, is not an installed
/// family, so Chromium draws the system font; this is the same face.
pub const FONT_UI: &str = ".SystemUIFont";
/// `Picto Inspector Roboto`
pub const FONT_INSPECTOR: &str = "Roboto";
pub const FONT_INSPECTOR_MONO: &str = "Fira Mono";
/// `--font-family-mono`. Its first name, SF Mono, is not available to apps
/// by name, so Chromium draws the bundled Fira Mono.
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
/// The sidebar's glyph size (`IC` in `Sidebar.tsx`).
pub const SIDEBAR_ICON: f32 = 19.;
/// `GRID_TILE_RADIUS`
pub const TILE_RADIUS: f32 = 4.;

#[cfg(test)]
mod tests {
    use super::{DARK, blend};
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
}
