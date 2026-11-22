//! The colour editor: a square for saturation and brightness beside a strip
//! for the hue, a tolerance slider when the caller asks for one, twelve preset
//! colours and a "no colour" swatch, and a field for the hex code. A change
//! made by dragging or typing is committed 250 ms after the last change.
//! Choosing a preset commits at once.
use super::field::{self, Look};
use super::text_input::{Kind, TextInput};
use crate::i18n::t;
use crate::theme::Text as _;
use crate::{grid::filters, theme};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use std::{cell::Cell, rc::Rc, time::Duration};

/// How long after the last change by dragging or typing the colour is
/// committed.
const SETTLE: Duration = Duration::from_millis(250);
/// The two colours of the saturation and brightness square. From left to right
/// the square goes from white to the hue. Over that lies black, which is
/// opaque at the bottom and transparent at the top.
const SQUARE_WHITE: u32 = 0xffffff;
const SQUARE_BLACK: u32 = 0x000000;

/// The twelve preset colours.
const PRESETS: [u32; 12] = [
    0x111111, 0xFFFFFF, 0x9E9E9E, 0xA48057, 0xFC85B3, 0xFF2727, 0xFFA34B, 0xFFD534, 0x47C595,
    0x51C4C4, 0x2B76E7, 0x6D50ED,
];

/// The width and the height of the saturation and brightness square. The hue
/// strip is `HUE_W` wide and as high as the square.
const SQUARE_W: f32 = 181.;
const SQUARE_H: f32 = 174.;
const HUE_W: f32 = 13.;
/// The editor's width: the square, the gap of 8px and the hue strip. The
/// editor is centred in its row, with 6px above it and 8px below it. `GAP` is
/// also the distance between the editor's rows.
const WIDTH: f32 = 202.;
const GAP: f32 = 8.;
/// The height of the tolerance slider's part: 2px of padding above, the 13px
/// line with the caption, a 6px gap, the 12px slider and 1px of padding below.
const SENSITIVITY: f32 = 2. + 13. + 6. + 12. + 1.;
/// The height of the presets: two rows of swatches that are 20px high, 5px
/// apart.
const PRESETS_H: f32 = 20. + 5. + 20.;

/// Returns the editor's height inside a menu row. `sensitivity` says whether
/// the tolerance slider is shown.
pub fn height(sensitivity: bool) -> f32 {
    6. + SQUARE_H
        + GAP
        + if sensitivity { SENSITIVITY + GAP } else { 0. }
        + PRESETS_H
        + GAP
        + theme::CONTROL_LG
        + 8.
}

/// A colour as hue, saturation and value. The hue is in degrees, and the
/// saturation and the value are from 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsv {
    pub hue: f32,
    pub saturation: f32,
    pub value: f32,
}

/// Returns the colour of the code `hex` (`#rrggbb`). Anything that is not such
/// a code gives mid grey.
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

/// Returns the code of `hsv` as `#RRGGBB`, with capital letters.
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

/// The event that the editor emits when a colour is committed. It holds the
/// colour's code, or `None` for "no colour", and the tolerance.
pub struct ColorCommitted(pub Option<String>, pub f32);

/// The part of the editor that a drag started on.
#[derive(Clone, Copy, PartialEq)]
enum Drag {
    Square,
    Hue,
}

pub struct ColorEditor {
    hsv: Hsv,
    hex: Entity<TextInput>,
    /// The code that the field shows and that the preset swatches are compared
    /// with, in capital letters.
    shown: String,
    has_color: bool,
    /// The colour that was last committed, or last set with `set_value`. The
    /// "no colour" swatch is marked as chosen when it is `None`.
    value: Option<String>,
    delta_e: f32,
    sensitivity: bool,
    /// Whether the "no colour" swatch is shown.
    clear: bool,
    settle: Option<Task<()>>,
    /// Whether the pointer is pressed on the square, and whether it is pressed
    /// on the hue strip.
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

    /// Removes the "no colour" swatch, for a place where a colour must be
    /// chosen.
    pub fn without_clear(mut self) -> Self {
        self.clear = false;
        self
    }

    /// Shows a colour and a tolerance that were set elsewhere, without
    /// committing them.
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

    /// Commits `value` after `SETTLE`. A newer change replaces the waiting
    /// task, so the time starts again.
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

    /// Applies the position `at` in the window to the part `drag`, whose
    /// bounds are `bounds`: it sets the saturation and the value from a
    /// position in the square, or the hue from a position on the strip.
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
            // The hue is a whole number of degrees from 0 to 359.
            Drag::Hue => Hsv {
                hue: (y * 359.).round(),
                ..self.hsv
            },
        };
        self.apply_hsv(hsv, cx);
    }

    /// Returns an invisible element that covers its parent and handles a drag
    /// on it. A press on it starts the drag. After that, every move of the
    /// pointer anywhere in the window is applied, until the mouse button is
    /// released.
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
        // Two gradients, one over the other: from white on the left to the hue
        // on the right, and over it from transparent at the top to black at
        // the bottom.
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
                // The marker of the chosen colour: a white ring 9px across
                // with a dark line around it, centred on the colour.
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
        // From top to bottom the strip goes through red, yellow, green, cyan,
        // blue, magenta and red again. It is drawn as six bands, each a
        // gradient between two neighbouring colours.
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
        // The marker of the hue is 3px high: a light 1px line, a 1px gap and a
        // dark 1px line.
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
                        // The caption's line has the line height of body text.
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
        // The swatches are "no colour" and then the twelve presets, seven in a
        // row.
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
                    // A swatch of the colour that the field holds, 9px from
                    // the field's left edge.
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
        // A text that is not a colour code gives mid grey.
        assert_eq!(hsv_to_hex(hex_to_hsv("nope")), "#808080");
    }
}
