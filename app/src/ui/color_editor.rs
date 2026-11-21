//! `ColorFilterEditor.tsx`: a saturation and brightness square beside a hue
//! strip, an optional tolerance slider, twelve preset colours and "no
//! colour", and the hex code. Dragging and typing settle for 250 ms before
//! the colour is committed; a preset commits at once.
use super::field::{self, Look};
use super::text_input::{Kind, TextInput};
use crate::i18n::t;
use crate::theme::Text as _;
use crate::{grid::filters, theme};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use std::{cell::Cell, rc::Rc, time::Duration};

/// `COLOR_FILTER_SETTLE_MS`.
const SETTLE: Duration = Duration::from_millis(250);
/// `.saturation`: white toward the hue across, and black up from the
/// bottom to clear, which make the saturation and brightness square.
const SQUARE_WHITE: u32 = 0xffffff;
const SQUARE_BLACK: u32 = 0x000000;

/// `PRESETS`.
const PRESETS: [u32; 12] = [
    0x111111, 0xFFFFFF, 0x9E9E9E, 0xA48057, 0xFC85B3, 0xFF2727, 0xFFA34B, 0xFFD534, 0x47C595,
    0x51C4C4, 0x2B76E7, 0x6D50ED,
];

/// `.saturation` and `.hue`.
const SQUARE_W: f32 = 181.;
const SQUARE_H: f32 = 174.;
const HUE_W: f32 = 13.;
/// `.editor`: two columns 8px apart, centred with 6px above and 8px below.
const WIDTH: f32 = 202.;
const GAP: f32 = 8.;
/// `.sensitivity`: its padding, the caption line, a 6px gap and the 12px
/// slider.
const SENSITIVITY: f32 = 2. + 13. + 6. + 12. + 1.;
/// `.presets`: two rows of 20px swatches, 5px apart.
const PRESETS_H: f32 = 20. + 5. + 20.;

/// The editor's height inside a menu row.
pub fn height(sensitivity: bool) -> f32 {
    6. + SQUARE_H
        + GAP
        + if sensitivity { SENSITIVITY + GAP } else { 0. }
        + PRESETS_H
        + GAP
        + theme::CONTROL_LG
        + 8.
}

/// Hue in degrees, saturation and value from 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsv {
    pub hue: f32,
    pub saturation: f32,
    pub value: f32,
}

/// `hexToHsv`; anything that is not `#rrggbb` is mid grey.
fn hex_to_hsv(hex: &str) -> Hsv {
    let rgb = theme::parse_hex_code(hex).unwrap_or(0x808080);
    let channel = |shift: u32| ((rgb >> shift) & 0xff) as f32 / 255.;
    let (red, green, blue) = (channel(16), channel(8), channel(0));
    let max = red.max(green).max(blue);
    let min = red.min(green).min(blue);
    let delta = max - min;
    let hue = if delta == 0. {
        0.
    } else if max == red {
        60. * (((green - blue) / delta) % 6.)
    } else if max == green {
        60. * ((blue - red) / delta + 2.)
    } else {
        60. * ((red - green) / delta + 4.)
    };
    Hsv {
        hue: if hue < 0. { hue + 360. } else { hue },
        saturation: if max == 0. { 0. } else { delta / max },
        value: max,
    }
}

/// `hsvToHex`, in capitals.
fn hsv_to_hex(hsv: Hsv) -> String {
    let chroma = hsv.value * hsv.saturation;
    let segment = hsv.hue / 60.;
    let secondary = chroma * (1. - ((segment % 2.) - 1.).abs());
    let (red, green, blue) = if segment < 1. {
        (chroma, secondary, 0.)
    } else if segment < 2. {
        (secondary, chroma, 0.)
    } else if segment < 3. {
        (0., chroma, secondary)
    } else if segment < 4. {
        (0., secondary, chroma)
    } else if segment < 5. {
        (secondary, 0., chroma)
    } else {
        (chroma, 0., secondary)
    };
    let matched = hsv.value - chroma;
    let byte = |component: f32| ((component + matched) * 255.).round().clamp(0., 255.) as u8;
    format!("#{:02X}{:02X}{:02X}", byte(red), byte(green), byte(blue))
}

/// The colour committed: a code or none, and the tolerance.
pub struct ColorCommitted(pub Option<String>, pub f32);

/// What a drag started on.
#[derive(Clone, Copy, PartialEq)]
enum Drag {
    Square,
    Hue,
}

pub struct ColorEditor {
    hsv: Hsv,
    hex: Entity<TextInput>,
    /// The code the field and swatches show, in capitals.
    shown: String,
    has_color: bool,
    /// The colour last committed or given, for the "no colour" swatch.
    value: Option<String>,
    delta_e: f32,
    sensitivity: bool,
    /// The "no colour" swatch (`allowClear`).
    clear: bool,
    settle: Option<Task<()>>,
    /// Whether the square and the hue strip are held.
    drag: (Rc<Cell<bool>>, Rc<Cell<bool>>),
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ColorCommitted> for ColorEditor {}

impl ColorEditor {
    pub fn new(
        value: Option<String>,
        delta_e: f32,
        sensitivity: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let shown = value
            .as_deref()
            .map(str::to_uppercase)
            .unwrap_or_else(|| "#808080".into());
        let hex = cx.new(|cx| {
            let mut input = TextInput::new(Kind::Mono, cx).name(t("Hex color"));
            input.set_text(shown.clone(), cx);
            input
        });
        let subscriptions = vec![cx.observe(&hex, |this, input, cx| {
            let typed = input.read(cx).value().to_uppercase();
            if typed == this.shown {
                return;
            }
            this.shown = typed.clone();
            if theme::parse_hex_code(&typed).is_some() {
                this.hsv = hex_to_hsv(&typed);
                this.has_color = true;
                this.schedule(Some(typed), cx);
            }
            cx.notify();
        })];
        Self {
            hsv: hex_to_hsv(&shown),
            hex,
            shown,
            has_color: value.is_some(),
            value,
            delta_e,
            sensitivity,
            clear: true,
            settle: None,
            drag: Default::default(),
            _subscriptions: subscriptions,
        }
    }

    /// Without the "no colour" swatch (`allowClear={false}`), for a colour
    /// that must be one.
    pub fn without_clear(mut self) -> Self {
        self.clear = false;
        self
    }

    /// Follows a colour set elsewhere, as the component's effect on `value`
    /// does.
    pub fn set_value(&mut self, value: Option<String>, delta_e: f32, cx: &mut Context<Self>) {
        self.delta_e = delta_e;
        if value == self.value {
            cx.notify();
            return;
        }
        self.value = value.clone();
        let next = value
            .as_deref()
            .map(str::to_uppercase)
            .unwrap_or_else(|| "#808080".into());
        self.has_color = value.is_some();
        if next != self.shown {
            self.hsv = hex_to_hsv(&next);
            self.shown = next.clone();
            self.hex.update(cx, |input, cx| input.set_text(next, cx));
        }
        cx.notify();
    }

    fn apply_hsv(&mut self, hsv: Hsv, cx: &mut Context<Self>) {
        let hex = hsv_to_hex(hsv);
        self.hsv = hsv;
        self.has_color = true;
        if hex != self.shown {
            self.shown = hex.clone();
            self.hex
                .update(cx, |input, cx| input.set_text(hex.clone(), cx));
        }
        self.schedule(Some(hex), cx);
        cx.notify();
    }

    /// Commits after the settling time; a newer change starts it again.
    fn schedule(&mut self, value: Option<String>, cx: &mut Context<Self>) {
        let delta_e = self.delta_e;
        self.settle = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETTLE).await;
            let _ = this.update(cx, |this, cx| {
                this.settle = None;
                this.value = value.clone();
                cx.emit(ColorCommitted(value, delta_e));
            });
        }));
    }

    fn commit_now(&mut self, value: Option<String>, cx: &mut Context<Self>) {
        self.settle = None;
        self.value = value.clone();
        cx.emit(ColorCommitted(value, self.delta_e));
        cx.notify();
    }

    fn pick_preset(&mut self, preset: u32, cx: &mut Context<Self>) {
        let hex = format!("#{preset:06X}");
        self.has_color = true;
        self.hsv = hex_to_hsv(&hex);
        if hex != self.shown {
            self.shown = hex.clone();
            self.hex
                .update(cx, |input, cx| input.set_text(hex.clone(), cx));
        }
        self.commit_now(Some(hex), cx);
    }

    /// A point in the square or on the strip, from the window position.
    fn point(
        &mut self,
        drag: Drag,
        at: Point<Pixels>,
        bounds: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let x = (f32::from(at.x - bounds.left()) / f32::from(bounds.size.width)).clamp(0., 1.);
        let y = (f32::from(at.y - bounds.top()) / f32::from(bounds.size.height)).clamp(0., 1.);
        let hsv = match drag {
            Drag::Square => Hsv {
                saturation: x,
                value: 1. - y,
                ..self.hsv
            },
            // The strip is a range input of whole degrees, 0 to 359.
            Drag::Hue => Hsv {
                hue: (y * 359.).round(),
                ..self.hsv
            },
        };
        self.apply_hsv(hsv, cx);
    }

    /// A drag surface: a press starts the drag there, and moves anywhere
    /// in the window follow it until the button is released.
    fn drag_surface(&self, drag: Drag, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let held = match drag {
            Drag::Square => self.drag.0.clone(),
            Drag::Hue => self.drag.1.clone(),
        };
        let editor = cx.entity().downgrade();
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let editor = editor.clone();
                super::track_drag(
                    bounds,
                    held.clone(),
                    move |at, _, cx| {
                        let _ = editor.update(cx, |this, cx| this.point(drag, at, bounds, cx));
                    },
                    |_| {},
                    window,
                );
            },
        )
        .absolute()
        .inset_0()
    }
}

impl Render for ColorEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let pure_hue = rgb(theme::parse_hex_code(&hsv_to_hex(Hsv {
            hue: self.hsv.hue,
            saturation: 1.,
            value: 1.,
        }))
        .unwrap_or(0xff0000));
        let clear = |color: Rgba| Rgba { a: 0., ..color };
        // `linear-gradient(to top, #000, transparent)` over
        // `linear-gradient(to right, #fff, <hue>)`.
        let square = div()
            .id("color-square")
            .role(Role::Slider)
            .aria_label(t("Color saturation and brightness"))
            .aria_value(self.shown.clone())
            .relative()
            .w(px(SQUARE_W))
            .h(px(SQUARE_H))
            .overflow_hidden()
            .rounded(px(2.))
            .cursor(CursorStyle::Crosshair)
            .child(div().absolute().inset_0().bg(linear_gradient(
                90.,
                linear_color_stop(rgb(SQUARE_WHITE), 0.),
                linear_color_stop(pure_hue, 1.),
            )))
            .child(div().absolute().inset_0().bg(linear_gradient(
                0.,
                linear_color_stop(rgb(SQUARE_BLACK), 0.),
                linear_color_stop(clear(rgb(SQUARE_BLACK)), 1.),
            )))
            .child(
                // `.cursor`: a 9px white ring with a dark halo, centred on the
                // colour.
                div()
                    .absolute()
                    .left(px(self.hsv.saturation * SQUARE_W - 4.5))
                    .top(px((1. - self.hsv.value) * SQUARE_H - 4.5))
                    .size(px(9.))
                    .rounded_full()
                    .border_1()
                    .border_color(rgb(theme::COLOR_CURSOR_RING))
                    .shadow(vec![
                        BoxShadow::new(px(0.), px(0.), rgba(theme::COLOR_CURSOR_HALO).into())
                            .spread_radius(px(1.)),
                    ]),
            )
            .child(self.drag_surface(Drag::Square, cx));
        // Red, yellow, green, cyan, blue, magenta and red again, top to
        // bottom, as six two-stop bands.
        let stops = [
            0xff0000, 0xffff00, 0x00ff00, 0x00ffff, 0x0000ff, 0xff00ff, 0xff0000,
        ];
        let band = SQUARE_H / 6.;
        let mut strip = div()
            .id("color-hue")
            .role(Role::Slider)
            .aria_label(t("Color hue"))
            .aria_numeric_value(self.hsv.hue as f64)
            .aria_min_numeric_value(0.)
            .aria_max_numeric_value(359.)
            .relative()
            .w(px(HUE_W))
            .h(px(SQUARE_H))
            .overflow_hidden()
            .rounded(px(2.))
            .cursor_pointer();
        for (index, pair) in stops.windows(2).enumerate() {
            strip = strip.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(index as f32 * band))
                    .h(px(band))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(rgb(pair[0]), 0.),
                        linear_color_stop(rgb(pair[1]), 1.),
                    )),
            );
        }
        // The range input's 3px thumb: a light line over a dark one.
        let thumb_top = (self.hsv.hue.round() / 359.) * (SQUARE_H - 3.);
        strip = strip
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(thumb_top))
                    .h(px(1.))
                    .bg(rgba(theme::COLOR_HUE_THUMB_LIGHT)),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(thumb_top + 2.))
                    .h(px(1.))
                    .bg(rgba(theme::COLOR_HUE_THUMB_DARK)),
            )
            .child(self.drag_surface(Drag::Hue, cx));

        let mut editor = div()
            .w(px(WIDTH))
            .mx_auto()
            .mt(px(6.))
            .mb(px(8.))
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(div().flex().gap(px(GAP)).child(square).child(strip));
        if self.sensitivity {
            let delta_e = self.delta_e;
            let editor_handle = cx.entity().downgrade();
            editor = editor.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .pt(px(2.))
                    .px(px(1.))
                    .pb(px(1.))
                    .text(theme::TEXT_CAPTION)
                    .text_color(theme.text_secondary())
                    .child(
                        // `.sensitivityHeader` keeps the body's line height.
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .line_height(px(theme::TEXT_CAPTION * theme::LINE_HEIGHT_BODY))
                            .child(t("Tolerance"))
                            .child(
                                div()
                                    .font_family(theme::FONT_MONO)
                                    .text_color(theme.text())
                                    .child(format!("{}", delta_e.round() as i64)),
                            ),
                    )
                    .child(
                        super::slider(
                            "color-tolerance",
                            delta_e as f64,
                            filters::MINIMUM_DELTA_E as f64,
                            filters::MAXIMUM_DELTA_E as f64,
                            1.,
                        )
                        .name(t("Tolerance"))
                        .look(super::slider::Look::Filled)
                        .on_change(move |value, cx| {
                            let _ = editor_handle.update(cx, |this, cx| {
                                this.delta_e = value as f32;
                                let color = this.has_color.then(|| this.shown.clone());
                                this.schedule(color, cx);
                                cx.notify();
                            });
                        }),
                    ),
            );
        }
        // `.presets`: "no colour" then the twelve, seven to a row.
        let none_active = self.value.is_none();
        let mut swatches = Vec::new();
        swatches.extend(self.clear.then(|| {
            div()
                .id("color-none")
                .role(Role::Button)
                .aria_label(t("No color"))
                .aria_toggled(super::a11y::toggled(none_active))
                .relative()
                .size(px(20.))
                .rounded(px(5.))
                .overflow_hidden()
                .bg(rgb(theme::COLOR_NONE_FILL))
                .border_1()
                .border_color(if none_active {
                    theme.text()
                } else {
                    rgba(theme::COLOR_PRESET_RING)
                })
                .opacity(if none_active { 1. } else { 0.8 })
                .hover(|style| style.opacity(1.))
                .cursor_pointer()
                .child(
                    super::icon("color-none", 20., rgb(theme::COLOR_NONE_STROKE))
                        .absolute()
                        .left(px(-1.))
                        .top(px(-1.)),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.has_color = false;
                    this.commit_now(None, cx);
                }))
        }));
        for (index, preset) in PRESETS.into_iter().enumerate() {
            let active = self.shown == format!("#{preset:06X}");
            swatches.push(
                div()
                    .id(("color-preset", index))
                    .role(Role::Button)
                    .aria_label(format!("#{preset:06X}"))
                    .aria_toggled(super::a11y::toggled(active))
                    .size(px(20.))
                    .rounded(px(5.))
                    .bg(rgb(preset))
                    .border_1()
                    .border_color(if active {
                        theme.text()
                    } else {
                        rgba(theme::COLOR_PRESET_RING)
                    })
                    .opacity(if active { 1. } else { 0.8 })
                    .hover(|style| style.opacity(1.))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.pick_preset(preset, cx))),
            );
        }
        let mut rows = div().flex().flex_col().gap(px(5.));
        let mut swatches = swatches.into_iter().peekable();
        while swatches.peek().is_some() {
            rows = rows.child(div().flex().gap(px(5.)).children(swatches.by_ref().take(7)));
        }
        editor = editor.child(rows);
        let preview = theme::parse_hex_code(&self.shown);
        editor.child(
            div()
                .relative()
                .child(field::field("color-hex", &self.hex, Look::Hex))
                .child(
                    // `.preview`: the colour typed, 9px in from the left.
                    div()
                        .absolute()
                        .left(px(9.))
                        .top(px((theme::CONTROL_LG - 14.) / 2.))
                        .size(px(14.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(theme.border_secondary())
                        .when_some(preview, |swatch, color| swatch.bg(rgb(color))),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Hsv, hex_to_hsv, hsv_to_hex};

    #[test]
    fn hex_and_hsv_round_trip() {
        for hex in ["#FF2727", "#2B76E7", "#47C595", "#111111", "#FFFFFF"] {
            assert_eq!(hsv_to_hex(hex_to_hsv(hex)), hex);
        }
        let red = hex_to_hsv("#ff0000");
        assert_eq!(
            red,
            Hsv {
                hue: 0.,
                saturation: 1.,
                value: 1.
            }
        );
        // Anything else is mid grey.
        assert_eq!(hsv_to_hex(hex_to_hsv("nope")), "#808080");
    }
}
