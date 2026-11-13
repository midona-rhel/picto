//! The inspector rail's scrolling body. Layout follows InspectorSkeleton,
//! InspectorField, TagChip and PropertyRow.
use crate::grid::{format, thumbnails::Thumbnails};
use crate::session::{Change, Session};
use crate::theme;
use crate::ui::{
    self,
    text_input::{self, TextInput},
};
use gpui::*;
use picto_library::predicate::{FilterClause, FilterExpr, SetMatchMode};
use picto_library::query::ItemScope;
use std::collections::HashSet;

/// Namespace presentation: chip colour, text colour and sort order.
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

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1048576 {
        format!("{:.1} KB", bytes as f64 / 1024.)
    } else {
        format!("{:.1} MB", bytes as f64 / 1048576.)
    }
}

pub struct Inspector {
    session: Entity<Session>,
    thumbnails: Entity<Thumbnails>,
    name: Entity<TextInput>,
    notes: Entity<TextInput>,
    collapsed: HashSet<&'static str>,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl Inspector {
    pub fn new(
        session: Entity<Session>,
        thumbnails: Entity<Thumbnails>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| TextInput::field("Name", window, cx));
        let notes = cx.new(|cx| TextInput::field("Notes", window, cx));
        let subscriptions = vec![
            cx.subscribe(&name, |this, _, event: &text_input::Committed, cx| {
                this.session.read(cx).rename(event.0, event.1.clone());
            }),
            cx.subscribe(&notes, |this, _, event: &text_input::Committed, cx| {
                this.session.read(cx).set_notes(event.0, event.1.clone());
            }),
            cx.subscribe(
                &session,
                |this, session, change: &Change, cx| match change {
                    Change::Selection => {
                        // A pending edit belongs to the item it was typed for.
                        this.name.update(cx, |field, cx| field.commit(cx));
                        this.notes.update(cx, |field, cx| field.commit(cx));
                        cx.notify();
                    }
                    Change::Details => {
                        if let Some(details) = session.read(cx).details.clone() {
                            let id = details.root.root_id;
                            this.name.update(cx, |field, cx| {
                                field.set_field(id, details.root.name.clone(), cx)
                            });
                            this.notes.update(cx, |field, cx| {
                                field.set_field(
                                    id,
                                    details.root.notes.clone().unwrap_or_default(),
                                    cx,
                                )
                            });
                        }
                        cx.notify();
                    }
                    Change::Navigation => cx.notify(),
                    _ => {}
                },
            ),
        ];
        Self {
            session,
            thumbnails,
            name,
            notes,
            collapsed: HashSet::new(),
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    fn field(&self, input: &Entity<TextInput>, cx: &App) -> Div {
        div()
            .h(px(theme::CONTROL_LG))
            .mb(px(8.))
            .rounded(px(theme::RADIUS_MD))
            .border_1()
            .border_color(theme::theme(cx).border_secondary())
            .overflow_hidden()
            .child(input.clone())
    }

    fn property(label: &'static str, value: String, cx: &App) -> Div {
        div()
            .h(px(20.))
            .flex()
            .items_center()
            .gap(px(12.))
            .text_size(px(theme::TEXT_CAPTION))
            .child(div().w(px(88.)).flex_none().child(label))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_right()
                    .text_ellipsis()
                    .overflow_hidden()
                    .font_family(theme::FONT_INSPECTOR_MONO)
                    .text_color(theme::theme(cx).contrast(80.))
                    .child(value),
            )
    }

    fn section(
        &self,
        title: &'static str,
        content: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = theme::theme(cx);
        let collapsed = self.collapsed.contains(title);
        let mut section = div()
            .mt(px(10.))
            .pt(px(10.))
            .border_t_1()
            .border_color(theme.border_secondary())
            .child(
                div()
                    .id(title)
                    .h(px(theme::CONTROL_SM))
                    .flex()
                    .items_center()
                    .text_size(px(theme::TEXT_SM))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.text_tertiary())
                    .hover(move |style| style.text_color(theme.text()))
                    .child(div().flex_1().child(title))
                    .child(
                        div()
                            .size(px(theme::ICON_BUTTON))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(ui::icon(
                                if collapsed {
                                    "chevron-right"
                                } else {
                                    "chevron-down"
                                },
                                16.,
                                theme.text(),
                            )),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.collapsed.remove(title) {
                            this.collapsed.insert(title);
                        }
                        cx.notify();
                    })),
            );
        if !collapsed {
            section = section.child(div().mt(px(4.)).child(content));
        }
        section.into_any_element()
    }

    fn narrow(&mut self, clause: FilterClause, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| {
            session.set_filter(Some(FilterExpr::Clause(clause)), cx)
        });
    }
}

impl Inspector {
    /// The scrolling column of fields and sections for the selected item.
    fn body(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let mut panel = div()
            .id("inspector")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .px(px(24.))
            .pt(px(10.))
            .pb(px(48.))
            .font_family(theme::FONT_INSPECTOR);
        let (demo, details, entries, tags, cover_thumbnail) = {
            let session = self.session.read(cx);
            let details = session.details.clone();
            let cover_thumbnail = details.as_ref().and_then(|details| {
                details
                    .media
                    .iter()
                    .find(|media| media.media_id == details.root.cover_media_id)
                    .or(details.media.first())
                    .map(|media| session.thumbnail(&media.facts.content_hash))
            });
            let tags = details.as_ref().map_or_else(Vec::new, |details| {
                details
                    .tag_ids
                    .iter()
                    .filter_map(|id| session.tags.get(&id.0).cloned())
                    .collect::<Vec<_>>()
            });
            (
                session.demo,
                details,
                session.nav.clone(),
                tags,
                cover_thumbnail,
            )
        };
        if let Some((name, mime)) = demo {
            return panel
                .child(Self::property("Example", name.to_owned(), cx))
                .child(Self::property("Type", mime.to_owned(), cx));
        }
        let Some(details) = details else {
            return panel;
        };
        let root = &details.root;
        let cover = details
            .media
            .iter()
            .find(|media| media.media_id == root.cover_media_id)
            .or(details.media.first());
        if let (Some(media), Some(thumbnail)) = (cover, cover_thumbnail) {
            panel = panel.child(
                div()
                    .h(px(200.))
                    .mb(px(8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        img(thumbnail)
                            .image_cache(&self.thumbnails)
                            .size(px(200.))
                            .rounded(px(theme::RADIUS_MD))
                            .object_fit(ObjectFit::Contain),
                    ),
            );
            let mut palette = div()
                .h(px(theme::CONTROL_MD))
                .mb(px(8.))
                .flex()
                .items_center()
                .justify_center();
            if !media.facts.palette.is_empty() {
                let mut swatches = div()
                    .h(px(theme::CONTROL_SM))
                    .px(px(6.))
                    .rounded_full()
                    .border_1()
                    .border_color(theme.border_secondary())
                    .bg(theme.control_surface())
                    .flex()
                    .items_center()
                    .gap(px(2.));
                for (index, color) in media.facts.palette.iter().take(12).enumerate() {
                    let color = color.clone();
                    swatches = swatches.child(
                        div()
                            .id(("palette", index))
                            .size(px(16.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .hover(move |style| style.bg(theme.text_tertiary()))
                            .child(
                                div()
                                    .size(px(14.))
                                    .rounded_full()
                                    .bg(rgb(format::lab_to_rgb(color.l, color.a, color.b))),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.narrow(
                                    FilterClause::Color {
                                        color: color.clone(),
                                        delta_e: 20.,
                                    },
                                    cx,
                                );
                            })),
                    );
                }
                palette = palette.child(swatches);
            }
            panel = panel.child(palette);
        }
        panel = panel
            .child(self.field(&self.name, cx))
            .child(self.field(&self.notes, cx));
        let source = root
            .source_urls
            .first()
            .cloned()
            .unwrap_or_else(|| "http://".into());
        panel = panel.child(
            div()
                .id("source-url")
                .h(px(theme::CONTROL_LG))
                .px(px(10.))
                .mb(px(8.))
                .rounded(px(theme::RADIUS_MD))
                .border_1()
                .border_color(theme.border_secondary())
                .bg(theme.control_surface())
                .flex()
                .items_center()
                .text_size(px(theme::TEXT_MD))
                .text_color(theme.text_tertiary())
                .overflow_hidden()
                .text_ellipsis()
                .child(source.clone())
                .on_click(move |_, _, cx| {
                    if source.starts_with("https://") {
                        cx.open_url(&source);
                    }
                }),
        );

        let mut folders = div().flex().flex_wrap().gap(px(4.));
        for folder in &details.folder_ids {
            let found = entries.iter().enumerate().find(|(_, entry)| {
                matches!(entry.scope, ItemScope::FolderTree { folder_id } if folder_id == *folder)
            });
            if let Some((index, entry)) = found {
                folders = folders.child(
                    div()
                        .id(("inspector-folder", index))
                        .h(px(theme::CONTROL_SM))
                        .px(px(8.))
                        .rounded(px(theme::RADIUS_MD))
                        .border_1()
                        .border_color(theme.border())
                        .text_size(px(theme::TEXT_SM))
                        .flex()
                        .items_center()
                        .child(entry.label.clone())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.session
                                .update(cx, |session, cx| session.navigate(index, cx));
                        })),
                );
            }
        }
        panel = panel.child(self.section("Folders", folders.into_any_element(), cx));

        let mut chips = div().flex().flex_wrap().gap(px(4.));
        let mut tags = tags;
        tags.sort_by_key(|tag| (tag_style(&tag.namespace).2, tag.subname.to_lowercase()));
        for tag in tags {
            let (color, text, _) = tag_style(&tag.namespace);
            let tag_id = tag.tag_id;
            chips = chips.child(
                div()
                    .id(("inspector-tag", tag.tag_id.0 as usize))
                    .h(px(theme::CONTROL_SM))
                    .px(px(8.))
                    .rounded(px(theme::RADIUS_MD))
                    .border_1()
                    .border_color(rgba((color << 8) | 64))
                    .bg(rgba((color << 8) | 26))
                    .text_color(rgb(text))
                    .text_size(px(theme::TEXT_SM))
                    .flex()
                    .items_center()
                    .max_w(px(248.))
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(tag.subname.clone())
                    .hover(move |style| style.bg(rgba((color << 8) | 51)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.narrow(
                            FilterClause::Tags {
                                tag_ids: vec![tag_id],
                                mode: SetMatchMode::All,
                            },
                            cx,
                        );
                    })),
            );
        }
        panel = panel.child(self.section("Tags", chips.into_any_element(), cx));

        let mut rating = div()
            .h(px(20.))
            .flex()
            .items_center()
            .text_size(px(theme::TEXT_CAPTION))
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
                        rgb(theme::RATING)
                    } else {
                        theme.contrast(25.)
                    })
                    .child(if active { "★" } else { "☆" })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.session
                            .read(cx)
                            .rate(id, picto_library::Rating::ALL[value]);
                    })),
            );
        }
        let mut properties =
            div()
                .child(rating)
                .child(Self::property("Items", root.media_count.to_string(), cx));
        if let Some(media) = cover {
            properties = properties
                .child(Self::property(
                    "Dimensions",
                    format!(
                        "{} × {}",
                        media.facts.width.unwrap_or(0),
                        media.facts.height.unwrap_or(0)
                    ),
                    cx,
                ))
                .child(Self::property(
                    "Size",
                    format_size(root.total_size_bytes),
                    cx,
                ))
                .child(Self::property("Type", media.facts.mime.clone(), cx));
        }
        panel.child(self.section("Properties", properties.into_any_element(), cx))
    }
}

impl Render for Inspector {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::stats::rendered(crate::stats::View::Inspector);
        let body = self.body(cx);
        div()
            .relative()
            .size_full()
            .child(body)
            .child(ui::scrollbar(
                "inspector-scrollbar",
                &self.scroll,
                window,
                cx,
            ))
    }
}
