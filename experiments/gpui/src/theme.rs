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
    /// `--color-active-overlay`
    pub fn active_overlay(&self) -> Rgba {
        translucent(self.contrast, 14.)
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
    /// `--glass-bg`: floating surfaces.
    pub fn glass(&self) -> Rgba {
        translucent(self.background, 92.)
    }
    /// `--glass-border-side`
    pub fn glass_border(&self) -> Rgba {
        rgba(0xffffff26)
    }
}

/// `--shadow-panel`
pub fn panel_shadow() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(12.), rgba(0x0000004d).into()).blur_radius(px(24.)),
        BoxShadow::new(px(0.), px(0.), rgba(0x00000080).into()).spread_radius(px(1.)),
    ]
}

// Grid badges are fixed, not themed (`gridAppearance.ts`).
pub const BADGE_BACKGROUND: u32 = 0x00000080;
pub const BADGE_BORDER: u32 = 0x00000033;
pub const BADGE_TEXT: u32 = 0xffffffcc;

// ── Typography ──────────────────────────────────────────────────
/// `--font-family-ui` on macOS.
pub const FONT_UI: &str = "SF Pro Text";
/// `--font-family-mono`
pub const FONT_MONO: &str = "SF Mono";
/// `Picto Inspector Roboto`
pub const FONT_INSPECTOR: &str = "Roboto";
pub const FONT_INSPECTOR_MONO: &str = "Fira Mono";
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

// ── Shape and size ──────────────────────────────────────────────
/// `--radius-sm`
pub const RADIUS_SM: f32 = 4.;
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
/// `--icon-button-size`
pub const ICON_BUTTON: f32 = 24.;
/// `--titlebar-height`
pub const TITLEBAR_HEIGHT: f32 = 48.;
/// `--sidebar-width`
pub const SIDEBAR_WIDTH: f32 = 300.;
pub const INSPECTOR_WIDTH: f32 = 280.;
/// `--sidebar-row-height`
pub const SIDEBAR_ROW: f32 = 27.;
/// `--sidebar-indent`
pub const SIDEBAR_INDENT: f32 = 20.;
/// `--sidebar-content-inset`: the left edge rows and section labels share.
pub const SIDEBAR_INSET: f32 = 16.;
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
    use super::DARK;
    use gpui::{rgb, rgba};

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
