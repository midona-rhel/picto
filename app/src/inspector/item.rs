//! Draws the parts of the inspector that show one item: its cover in a frame
//! fitted to the cover's shape, the swatches of its palette with their menu,
//! and its rating.
use super::*;

impl Inspector {
    /// Draws the cover in a frame fitted to its shape, with the format badge
    /// `label` over it. A right-click on it emits `PreviewMenuRequested`.
    pub(super) fn preview(
        &mut self,
        thumbnail: std::path::PathBuf,
        dimensions: Option<(u32, u32)>,
        label: AnyElement,
        cx: &mut Context<Self>,
    ) -> Div {
        // The frame shows the thumbnail at its own size, at most 200px each
        // way. Until the thumbnail is decoded, the frame takes its shape from
        // the media's dimensions.
        let resource = Resource::Path(thumbnail.clone().into());
        let natural = self
            .thumbnails
            .read(cx)
            .size(&resource)
            .map(|(width, height)| (width.round() as u32, height.round() as u32));
        let (width, height) = fitted(natural.or(dimensions));
        // A thumbnail that cannot be loaded is replaced by the picture of a
        // torn page, fitted inside the same frame.
        let failed = self.thumbnails.read(cx).is_failed(&resource);
        let picture = if failed {
            use crate::grid::art;
            let surface = theme::theme(cx).surface_2();
            let broken = art::Art::for_color(&mut self.art, surface).broken.clone();
            let (art_width, art_height) = art::BROKEN_SIZE;
            let scale = (width / art_width).min(height / art_height);
            let (drawn_width, drawn_height) = (
                (art_width + 2. * art::MARGIN) * scale,
                (art_height + 2. * art::MARGIN) * scale,
            );
            div()
                .size_full()
                .rounded(px(theme::RADIUS_MD))
                .bg(surface)
                .child(
                    img(broken)
                        .absolute()
                        .left(px((width.round() - drawn_width) / 2.))
                        .top(px((height.round() - drawn_height) / 2.))
                        .w(px(drawn_width))
                        .h(px(drawn_height)),
                )
                .into_any_element()
        } else {
            img(thumbnail)
                .image_cache(&self.thumbnails)
                .size_full()
                .rounded(px(theme::RADIUS_MD))
                .object_fit(ObjectFit::Cover)
                .into_any_element()
        };
        div()
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|_, event: &MouseDownEvent, _, cx| {
                    cx.emit(PreviewMenuRequested(event.position))
                }),
            )
            .h(px(PREVIEW))
            .mb(px(8.))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .relative()
                    .w(px(width.round()))
                    .h(px(height.round()))
                    // The frame's shadow, black at 10% with a blur of 5px, is
                    // drawn only outside the frame. A shadow under the frame
                    // would show through the transparent parts of a picture,
                    // which must show the panel.
                    .child(
                        canvas(
                            |_, _, _| {},
                            |bounds, _, window, _| {
                                window.paint_drop_shadows_outside(
                                    bounds,
                                    Corners::all(px(theme::RADIUS_MD)),
                                    &[BoxShadow::new(px(0.), px(0.), rgba(theme::BLACK_10).into())
                                        .blur_radius(px(5.))],
                                );
                            },
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .rounded(px(theme::RADIUS_MD))
                            .overflow_hidden()
                            .child(picture)
                            // A 1px ring, black at 10%, is drawn over the edge of
                            // the image.
                            .child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .rounded(px(theme::RADIUS_MD))
                                    .border_1()
                                    .border_color(rgba(theme::BLACK_10)),
                            )
                            .child(label),
                    ),
            )
    }

    /// Draws the pill that holds the swatches of the item's colours. It keeps
    /// its height when there are no colours, so that the fields below it
    /// never move.
    pub(super) fn palette(
        &self,
        colors: &[picto_library::LabColor],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = theme::theme(cx);
        let mut swatches = div()
            .h(px(theme::CONTROL_SM))
            .max_w_full()
            .px(px(6.))
            .rounded_full()
            .border_1()
            .border_color(theme.border_secondary())
            .bg(theme.control_surface())
            .overflow_hidden()
            .flex()
            .items_center()
            .gap(px(2.));
        if colors.is_empty() {
            swatches = swatches.invisible();
        }
        for (index, color) in colors.iter().enumerate() {
            let color = format::lab_to_rgb(color.l, color.a, color.b);
            let group = SharedString::from(format!("swatch-{index}"));
            let swatch = div()
                .id(("palette", index))
                .role(Role::Button)
                .aria_label(format!("#{color:06X}"))
                .group(group.clone())
                .relative()
                .size(px(16.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                // The ring that is shown while the pointer is over the swatch
                // lies one pixel outside the swatch's cell.
                .child(
                    div()
                        .absolute()
                        .inset(px(-1.))
                        .rounded_full()
                        .border_1()
                        .border_color(gpui::transparent_black())
                        .group_hover(group, move |style| {
                            style.border_color(theme.text_tertiary())
                        }),
                )
                .child(
                    div()
                        .size(px(14.))
                        .rounded_full()
                        .bg(rgb(color))
                        .border_1()
                        .border_color(theme.contrast(10.)),
                )
                // A click opens the filter bar with this colour as its colour
                // filter.
                .on_click(cx.listener(move |this, _, _, cx| this.filter_by_color(color, cx)))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.open_swatch_menu(color, event.position, window, cx);
                    }),
                );
            let tip = if self.copied == Some(color) {
                t("Copied!").to_owned()
            } else {
                crate::tf!(
                    "{value0} · Click to filter · Right-click for actions",
                    colors::hex(color)
                )
            };
            swatches = swatches.child(ui::tooltip::attach(
                swatch,
                SharedString::from(format!("swatch-tip-{index}")),
                ui::tip(tip),
                16.,
                window,
                cx,
            ));
        }
        div()
            .h(px(theme::CONTROL_SM))
            .mb(px(8.))
            .flex()
            .justify_center()
            .child(swatches)
    }

    /// Opens the filter bar and sets `color` as its colour filter.
    pub(super) fn filter_by_color(&mut self, color: u32, cx: &mut Context<Self>) {
        let hex = colors::hex(color);
        self.session.update(cx, |session, cx| {
            session.set_filter_bar_open(true, cx);
            let mut filters = session.filters.clone();
            filters.color = Some(hex);
            session.set_filters(filters, cx);
        });
    }

    /// Opens the menu of a swatch at `position`. Its rows are Filter by color
    /// and seven rows that each copy the colour in one format. After a copy,
    /// the swatch's tooltip says "Copied!" for 1.5 seconds.
    pub(super) fn open_swatch_menu(
        &mut self,
        color: u32,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let copy = |action: &'static str, label: &'static str| -> menu::Entry {
            menu::item(action, label).icon("menu-copy").into()
        };
        let entries = vec![
            menu::item("filter", "Filter by color")
                .icon("menu-funnel")
                .into(),
            menu::Entry::Separator,
            copy("hex", t("Copy HEX")),
            copy("rgb", t("Copy RGB")),
            copy("rgba", t("Copy RGBA")),
            copy("hsl", t("Copy HSL")),
            menu::Entry::Separator,
            copy("hsv", t("Copy HSV")),
            copy("hwb", t("Copy HWB")),
            copy("cmyk", t("Copy CMYK")),
        ];
        let menu = cx.new(|cx| menu::Menu::new(entries, position, cx).search(false));
        self.menu
            .open(menu, window, cx, move |this, _, action, _, cx| {
                let text = match action {
                    "filter" => return this.filter_by_color(color, cx),
                    "hex" => colors::hex(color).to_uppercase(),
                    "rgb" => colors::rgb(color),
                    "rgba" => colors::rgba(color),
                    "hsl" => colors::hsl(color),
                    "hsv" => colors::hsv(color),
                    "hwb" => colors::hwb(color),
                    "cmyk" => colors::cmyk(color),
                    _ => return,
                };
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                this.copied = Some(color);
                cx.notify();
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(1500))
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        if this.copied == Some(color) {
                            this.copied = None;
                            cx.notify();
                        }
                    });
                })
                .detach();
            });
        self.menu_target = Some(ChipMenu::Swatch);
    }

    /// Draws the rating row. While the pointer is over a star, the stars up
    /// to that one are filled, which shows the rating a click would set. A
    /// click on the star of the current rating clears the rating.
    pub(super) fn rating(&self, target: Rate, current: usize, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let shown = self.hovered_star.unwrap_or(current);
        let mut row = div()
            .h(px(20.))
            .flex()
            .items_center()
            .text(theme::TEXT_CAPTION)
            .child(div().flex_1().child(t("Rating")));
        // The stars are drawn at 80% opacity, as the values of the other
        // property rows are.
        for value in 1..=5 {
            let star = if value <= shown {
                ui::icon("star-filled", 12., rgb(theme::RATING)).opacity(0.8)
            } else {
                ui::icon("star", 12., theme.text_secondary()).opacity(0.25 * 0.8)
            };
            row = row.child(
                div()
                    .id(("rating", value))
                    .role(Role::Button)
                    .aria_label(if value == current {
                        t("Clear rating").to_owned()
                    } else {
                        crate::tf!(
                            "Set rating to {value0} star{value1}",
                            value,
                            crate::i18n::plural_s(value as i128)
                        )
                    })
                    .aria_toggled(ui::a11y::toggled(value <= current))
                    .w(px(16.))
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(star)
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.hovered_star = Some(value);
                        } else if this.hovered_star == Some(value) {
                            this.hovered_star = None;
                        }
                        cx.notify();
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let next =
                            picto_library::Rating::ALL[if value == current { 0 } else { value }];
                        match target {
                            Rate::Item(id) => {
                                this.rate_item(id, next, cx);
                                // The session reads the details of the selected
                                // item again by itself. A pinned item need not be
                                // the selected one, so the inspector reads its
                                // details again here.
                                if this.pinned {
                                    this.reread(membership::Touches::Text, cx);
                                }
                            }
                            Rate::Selection => this.rate_selection(next, cx),
                        }
                    })),
            );
        }
        row
    }
}
