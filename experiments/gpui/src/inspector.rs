//! Layout follows InspectorSkeleton, InspectorField, TagChip and PropertyRow CSS.
use super::*;

fn field(input: Entity<input::TextInput>) -> impl IntoElement {
    div()
        .h(px(32.))
        .mb(px(8.))
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(0x303236))
        .overflow_hidden()
        .child(input)
}
fn property(label: &'static str, value: String) -> impl IntoElement {
    div()
        .h(px(20.))
        .flex()
        .items_center()
        .gap(px(12.))
        .text_size(px(11.))
        .child(div().w(px(88.)).flex_none().child(label))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_right()
                .text_ellipsis()
                .overflow_hidden()
                .font_family("Fira Mono")
                .text_color(rgba(0xf8f9fbcc))
                .child(value),
        )
}
fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1048576 {
        format!("{:.1} KB", bytes as f64 / 1024.)
    } else {
        format!("{:.1} MB", bytes as f64 / 1048576.)
    }
}
// Same D65 Lab conversion and namespace presentation as Picto's renderer.
fn palette_rgb(color: &picto_library::LabColor) -> u32 {
    let fy = (color.l + 16.) / 116.;
    let inv = |v: f32| {
        if v.powi(3) > 0.008856 {
            v.powi(3)
        } else {
            (v - 16. / 116.) / 7.787
        }
    };
    let x = 0.95047 * inv(color.a / 500. + fy);
    let y = inv(fy);
    let z = 1.08883 * inv(fy - color.b / 200.);
    [
        x * 3.2406 - y * 1.5372 - z * 0.4986,
        -x * 0.9689 + y * 1.8758 + z * 0.0415,
        x * 0.0557 - y * 0.204 + z * 1.057,
    ]
    .into_iter()
    .fold(0, |rgb, c| {
        let c = if c <= 0.0031308 {
            12.92 * c
        } else {
            1.055 * c.powf(1. / 2.4) - 0.055
        };
        (rgb << 8) | (c.clamp(0., 1.) * 255.).round() as u32
    })
}
fn tag_style(namespace: &str) -> (u32, u32, u8) {
    match namespace {
        "creator" => (0xff6667, 0xf8e7e6, 0),
        "series" => (0xc499ff, 0xf3eff9, 1),
        "character" => (0x30d159, 0xe4f5e9, 2),
        "species" => (0x00aaff, 0xdff1f9, 3),
        "rating" => (0xffd60a, 0xf8f5e1, 4),
        _ => (0xbdbec0, 0xf8f9fb, 10),
    }
}
impl Browser {
    fn section(
        &self,
        title: &'static str,
        content: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let collapsed = self.collapsed_sections.contains(title);
        let mut section = div()
            .mt(px(10.))
            .pt(px(10.))
            .border_t_1()
            .border_color(rgb(0x303236))
            .child(
                div()
                    .id(title)
                    .h(px(26.))
                    .flex()
                    .items_center()
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(MUTED))
                    .hover(|s| s.text_color(rgb(theme::TEXT)))
                    .child(div().flex_1().child(title))
                    .child(
                        div()
                            .size(px(24.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(theme::icon(if collapsed {
                                "chevron-right"
                            } else {
                                "chevron-down"
                            })),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.collapsed_sections.remove(title) {
                            this.collapsed_sections.insert(title);
                        }
                        cx.notify();
                    })),
            );
        if !collapsed {
            section = section.child(div().mt(px(4.)).child(content));
        }
        section.into_any_element()
    }

    pub(super) fn inspector(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut panel = div()
            .id("inspector")
            .w(px(280.))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .px(px(16.))
            .pt(px(16.))
            .pb(px(48.))
            .font_family("Roboto")
            .bg(rgb(PANEL))
            .border_l_1()
            .border_color(rgb(0x303236));
        if let Some((name, mime)) = self.demo {
            return panel
                .child(property("Example", name.to_owned()))
                .child(property("Type", mime.to_owned()))
                .into_any_element();
        }
        let Some(details) = &self.details else {
            return panel.into_any_element();
        };
        let root = &details.root;
        let cover = details
            .media
            .iter()
            .find(|m| m.media_id == root.cover_media_id)
            .or(details.media.first());
        if let Some(media) = cover {
            panel = panel.child(
                div()
                    .h(px(200.))
                    .mb(px(8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        img(self.thumbnail(&media.facts.content_hash))
                            .image_cache(&self.thumbnails)
                            .w(px(200.))
                            .h(px(200.))
                            .rounded(px(6.))
                            .object_fit(ObjectFit::Contain),
                    ),
            );
            let mut palette = div()
                .h(px(28.))
                .mb(px(8.))
                .flex()
                .items_center()
                .justify_center();
            if !media.facts.palette.is_empty() {
                let mut swatches = div()
                    .h(px(26.))
                    .px(px(6.))
                    .rounded_full()
                    .border_1()
                    .border_color(rgb(0x303236))
                    .bg(rgb(0x141518))
                    .flex()
                    .items_center()
                    .gap(px(2.));
                for (i, color) in media.facts.palette.iter().take(12).enumerate() {
                    let color = color.clone();
                    swatches = swatches.child(
                        div()
                            .id(("palette", i))
                            .size(px(16.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .hover(|s| s.bg(rgb(0x96999e)))
                            .child(
                                div()
                                    .size(px(14.))
                                    .rounded_full()
                                    .bg(rgb(palette_rgb(&color))),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.extra_filter =
                                    Some(picto_library::predicate::FilterExpr::Clause(
                                        picto_library::predicate::FilterClause::Color {
                                            color: color.clone(),
                                            delta_e: 20.,
                                        },
                                    ));
                                this.preview = false;
                                this.query(false, cx);
                            })),
                    );
                }
                palette = palette.child(swatches);
            }
            panel = panel.child(palette);
        }
        panel = panel
            .child(field(self.name_field.clone()))
            .child(field(self.notes_field.clone()));
        let source = root
            .source_urls
            .first()
            .cloned()
            .unwrap_or_else(|| "http://".into());
        panel = panel.child(
            div()
                .id("source-url")
                .h(px(32.))
                .px(px(10.))
                .mb(px(8.))
                .rounded(px(6.))
                .border_1()
                .border_color(rgb(0x303236))
                .bg(rgb(0x141518))
                .flex()
                .items_center()
                .text_size(px(13.))
                .text_color(rgb(MUTED))
                .overflow_hidden()
                .text_ellipsis()
                .child(source.clone())
                .on_click(cx.listener(move |_, _, _, cx| {
                    if source.starts_with("https://") {
                        cx.open_url(&source);
                    }
                })),
        );
        let mut folders = div().flex().flex_wrap().gap(px(4.));
        for folder in &details.folder_ids {
            if let Some((index, nav)) = self.nav.iter().enumerate().find(|(_, n)| matches!(n.scope, picto_library::query::ItemScope::FolderTree {folder_id} if folder_id == *folder)) {
                folders = folders.child(div().id(("inspector-folder", index)).h(px(26.)).px(px(8.)).rounded(px(6.))
                    .border_1().border_color(rgb(0x48494c)).text_size(px(12.)).flex().items_center().child(nav.label.clone())
                    .on_click(cx.listener(move |this, _, window, cx| this.navigate(index, window, cx))));
            }
        }
        panel = panel.child(self.section("Folders", folders.into_any_element(), cx));
        let mut tags = div().flex().flex_wrap().gap(px(4.));
        let mut selected_tags = details
            .tag_ids
            .iter()
            .filter_map(|id| self.tags.get(&id.0))
            .collect::<Vec<_>>();
        selected_tags.sort_by_key(|tag| (tag_style(&tag.namespace).2, tag.subname.to_lowercase()));
        for tag in selected_tags {
            let (color, text, _) = tag_style(&tag.namespace);
            let tag_id = tag.tag_id;
            let name = tag.subname.clone();
            tags = tags.child(
                div()
                    .id(("inspector-tag", tag.tag_id.0 as usize))
                    .h(px(26.))
                    .px(px(8.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgba((color << 8) | 64))
                    .bg(rgba((color << 8) | 26))
                    .text_color(rgb(text))
                    .text_size(px(12.))
                    .flex()
                    .items_center()
                    .max_w(px(248.))
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(name.clone())
                    .hover(move |s| s.bg(rgba((color << 8) | 51)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.extra_filter = Some(picto_library::predicate::FilterExpr::Clause(
                            picto_library::predicate::FilterClause::Tags {
                                tag_ids: vec![tag_id],
                                mode: picto_library::predicate::SetMatchMode::All,
                            },
                        ));
                        this.preview = false;
                        this.query(false, cx);
                    })),
            );
        }
        panel = panel.child(self.section("Tags", tags.into_any_element(), cx));
        let mut rating = div()
            .h(px(20.))
            .flex()
            .items_center()
            .text_size(px(11.))
            .child(div().flex_1().child("Rating"));
        for value in 1..=5 {
            let active = details.rating.bitmap_key() >= value as u32;
            let id = root.root_id;
            rating = rating.child(
                div()
                    .id(("rating", value))
                    .w(px(16.))
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(15.))
                    .text_color(if active {
                        rgba(0xff9f0aff)
                    } else {
                        rgba(0xf8f9fb40)
                    })
                    .child(if active { "★" } else { "☆" })
                    .on_click(cx.listener(move |this, _, _, _| {
                        let _ = this
                            .sender
                            .send(Request::Rate(id, picto_library::Rating::ALL[value]));
                    })),
            );
        }
        let mut properties = div()
            .child(rating)
            .child(property("Items", root.media_count.to_string()));
        if let Some(media) = cover {
            properties = properties
                .child(property(
                    "Dimensions",
                    format!(
                        "{} × {}",
                        media.facts.width.unwrap_or(0),
                        media.facts.height.unwrap_or(0)
                    ),
                ))
                .child(property("Size", format_size(root.total_size_bytes)))
                .child(property("Type", media.facts.mime.clone()));
        }
        panel
            .child(self.section("Properties", properties.into_any_element(), cx))
            .into_any_element()
    }
}
