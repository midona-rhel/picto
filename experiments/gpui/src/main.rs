mod backend;
mod demo;
mod document;
mod flash;
mod format;
mod input;
mod inspector;
mod layout;
mod prefs;
mod surface;
mod theme;
mod thumbnails;
mod zoom;
use backend::{Navigation, Reply, Request};
use gpui::*;
use picto_library::query::RootSummary;
use picto_library::{RootDetails, TagRecord};
use std::{
    path::PathBuf,
    sync::mpsc::{Receiver, Sender},
    time::Duration,
};

actions!(
    browser,
    [
        Next,
        Previous,
        Down,
        Up,
        Preview,
        ClosePreview,
        Undo,
        Redo,
        Quit,
        DemoSvg,
        DemoFlash,
        DemoWord
    ]
);
const BG: u32 = theme::BACKGROUND;
const PANEL: u32 = theme::RAIL;
const MUTED: u32 = 0x96999e;
struct Browser {
    surface: Entity<surface::MediaSurface>,
    zoom: Entity<zoom::Zoom>,
    _zoom_observer: Subscription,
    focus: FocusHandle,
    search: Entity<input::TextInput>,
    folder_filter: Entity<input::TextInput>,
    _folder_filter_observer: Subscription,
    name_field: Entity<input::TextInput>,
    notes_field: Entity<input::TextInput>,
    _field_observers: Vec<Subscription>,
    collapsed_sections: std::collections::HashSet<&'static str>,
    _search_observer: Subscription,
    sender: Sender<Request>,
    receiver: Receiver<Reply>,
    nav: Vec<Navigation>,
    tags: std::collections::HashMap<u32, TagRecord>,
    collapsed: std::collections::HashSet<usize>,
    items: Vec<RootSummary>,
    scope: usize,
    selected: usize,
    details: Option<RootDetails>,
    preview: bool,
    menu: Option<Point<Pixels>>,
    demo: Option<(&'static str, &'static str)>,
    fixtures: tempfile::TempDir,
    member: usize,
    tile: f32,
    columns: usize,

    scroll: ScrollHandle,
    layout: layout::Layout,
    layout_key: Option<(u32, u32, usize, u32, layout::ViewMode, u32)>,
    prefs: prefs::ViewPrefs,
    prefs_scope: Option<usize>,
    thumbnails: Entity<thumbnails::Thumbnails>,
    generation: u64,
    search_text: String,
    extra_filter: Option<picto_library::predicate::FilterExpr>,
    library_cover: Option<PathBuf>,
    loading: bool,
    error: Option<String>,
    total: u64,
    cursor: Option<String>,
    blobs: PathBuf,
}
impl Browser {
    fn new(database: PathBuf, blobs: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (sender, receiver) = backend::start(database);
        let search = cx.new(input::TextInput::new);
        let folder_filter = cx.new(|cx| {
            let mut field = input::TextInput::new(cx);
            field.set_placeholder("Filter");
            field
        });
        let folder_filter_observer = cx.observe(&folder_filter, |_, _, cx| cx.notify());
        let name_field = cx.new(|cx| input::TextInput::field("Name", window, cx));
        let notes_field = cx.new(|cx| input::TextInput::field("Notes", window, cx));
        let field_observers = vec![
            cx.subscribe(&name_field, |this, _, event: &input::Committed, _| {
                let _ = this.sender.send(Request::Name(event.0, event.1.clone()));
            }),
            cx.subscribe(&notes_field, |this, _, event: &input::Committed, _| {
                let _ = this.sender.send(Request::Notes(event.0, event.1.clone()));
            }),
        ];
        let observer = cx.observe(&search, |this, search, cx| {
            let text = search.read(cx).value();
            if text != this.search_text {
                this.search_text = text;
                this.query(false, cx);
            }
        });
        cx.spawn(async move |entity, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(30))
                    .await;
                if entity
                    .update(cx, |this: &mut Self, cx| this.receive(cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let surface = cx.new(|_| surface::MediaSurface::default());
        let zoom = cx.new(|_| zoom::Zoom::new());
        let zoom_observer = cx.subscribe(&zoom, |this, _, event: &zoom::Changed, cx| {
            this.tile = event.0;
            cx.notify();
        });
        let library_cover = blobs.parent().and_then(|root| {
            ["jpg", "png"]
                .into_iter()
                .map(|ext| root.join(format!(".picto-library-cover.{ext}")))
                .find(|p| p.is_file())
        });
        Self {
            zoom,
            _zoom_observer: zoom_observer,
            surface,
            focus,
            search,
            folder_filter,
            _folder_filter_observer: folder_filter_observer,
            name_field,
            notes_field,
            _field_observers: field_observers,
            collapsed_sections: Default::default(),
            _search_observer: observer,
            sender,
            receiver,
            nav: vec![],
            tags: Default::default(),
            collapsed: Default::default(),
            items: vec![],
            scope: 0,
            selected: 0,
            details: None,
            preview: false,
            menu: None,
            demo: None,
            fixtures: demo::fixtures().expect("create disposable media fixtures"),
            member: 0,
            tile: 200.,
            columns: 4,

            scroll: ScrollHandle::new(),
            layout: layout::Layout::default(),
            layout_key: None,
            prefs: prefs::ViewPrefs::default(),
            prefs_scope: None,
            thumbnails: cx.new(thumbnails::Thumbnails::new),
            generation: 0,
            search_text: String::new(),
            extra_filter: None,
            library_cover,
            loading: true,
            error: None,
            total: 0,
            cursor: None,
            blobs,
        }
    }
    fn query(&mut self, append: bool, cx: &mut Context<Self>) {
        let Some(nav) = self.nav.get(self.scope) else {
            return;
        };
        self.generation += 1;
        self.loading = true;
        self.error = None;
        let _ = self.sender.send(Request::Query(
            self.generation,
            nav.scope.clone(),
            picto_library::predicate::FilterExpr::All(vec![
                backend::text_filter(&self.search_text),
                self.extra_filter.clone().unwrap_or_default(),
            ]),
            if append { self.cursor.clone() } else { None },
        ));
        cx.notify();
    }
    fn receive(&mut self, cx: &mut Context<Self>) {
        while let Ok(reply) = self.receiver.try_recv() {
            match reply {
                Reply::Ready(nav, tags) => {
                    self.nav = nav;
                    self.tags = tags.into_iter().map(|tag| (tag.tag_id.0, tag)).collect();
                    self.query(false, cx);
                }
                Reply::Page(generation, append, page, prefs) if generation == self.generation => {
                    // A scope opens at its saved zoom. Searching inside it keeps
                    // whatever the zoom control was moved to since.
                    if self.prefs_scope != Some(self.scope) {
                        self.prefs_scope = Some(self.scope);
                        self.tile = prefs.target_size;
                        self.zoom
                            .update(cx, |zoom, cx| zoom.show(prefs.target_size, cx));
                    }
                    self.prefs = prefs;
                    self.total = page.total;
                    self.cursor = page.next_cursor;
                    self.loading = false;
                    if append {
                        self.items.extend(page.items);
                    } else {
                        self.items = page.items;
                        self.selected = 0;
                        self.details = None;
                        self.scroll.set_offset(point(px(0.), px(0.)));
                        self.layout_key = None;
                        self.select(0, cx);
                    }
                }
                Reply::Details(id, details)
                    if self
                        .items
                        .get(self.selected)
                        .is_some_and(|item| item.root_id == id) =>
                {
                    self.name_field.update(cx, |field, cx| {
                        field.set_field(id, details.root.name.clone(), cx)
                    });
                    self.notes_field.update(cx, |field, cx| {
                        field.set_field(id, details.root.notes.clone().unwrap_or_default(), cx)
                    });
                    if let Some(item) = self.items.get_mut(self.selected) {
                        item.name = details.root.name.clone();
                    }
                    self.details = Some(details);
                    self.update_surface(cx);
                }
                Reply::Error(error) => {
                    self.error = Some(error);
                    self.loading = false;
                }
                _ => {}
            }
            cx.notify();
        }
    }
    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        self.demo = None;
        self.name_field.update(cx, |field, cx| field.commit(cx));
        self.notes_field.update(cx, |field, cx| field.commit(cx));
        if let Some(item) = self.items.get(index) {
            let key = item.content_hash.clone();
            let thumbnail = self.thumbnail(&key);
            self.surface
                .update(cx, |surface, cx| surface.show(key, thumbnail, None, cx));
            self.selected = index;
            self.member = 0;
            self.details = None;
            let _ = self.sender.send(Request::Details(item.root_id));
            if let Some(tile) = self.layout.tiles.get(index) {
                let top = -f32::from(self.scroll.offset().y);
                if tile.y < top || tile.y > top + 600. {
                    self.scroll.set_offset(point(px(0.), px(-tile.y)));
                }
            }
            cx.notify();
        }
    }
    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        if !self.items.is_empty() {
            self.select(
                self.selected
                    .saturating_add_signed(delta)
                    .min(self.items.len() - 1),
                cx,
            );
        }
    }
    fn thumbnail(&self, hash: &str) -> PathBuf {
        if hash.len() < 4 {
            return PathBuf::new();
        }
        self.blobs
            .join("t")
            .join(&hash[..2])
            .join(&hash[2..4])
            .join(format!("{hash}.jpg"))
    }
    fn button(&self, id: &'static str, label: impl Into<SharedString>) -> Stateful<Div> {
        div()
            .id(id)
            .px_3()
            .py_1()
            .rounded_md()
            .bg(rgb(0x303236))
            .hover(|s| s.bg(rgb(0x41444a)))
            .cursor_pointer()
            .child(label.into())
    }
    /// One grid tile, drawn to the rules of the Electron canvas grid
    /// (`drawBase.ts`): the media rect, its hairline ring, badges inside the
    /// rect, the selection stroke outside it, and optional text rows below.
    fn tile_element(&self, index: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        let tile = self.layout.tiles[index];
        let item = &self.items[index];
        let prefs = &self.prefs;
        let selected = index == self.selected;
        let image_height = tile.h - prefs.text_height();
        let timed = item.mime.starts_with("video/") || item.mime.starts_with("audio/");
        // Letterboxed media keeps its own rect inside the tile; badges and the
        // selection stroke follow that rect, not the tile.
        let (x, y, w, h) = if !prefs.fit_cover || timed {
            let aspect = match (item.width, item.height) {
                (Some(w), Some(h)) if w > 0 && h > 0 => w as f32 / h as f32,
                _ => 1.,
            };
            let scale = (tile.w / aspect).min(image_height);
            let (w, h) = (aspect * scale, scale);
            ((tile.w - w) / 2., (image_height - h) / 2., w, h)
        } else {
            (0., 0., tile.w, image_height)
        };
        let mut cell = div()
            .id(index)
            .absolute()
            .left(px(tile.x))
            .top(px(tile.y))
            .w(px(tile.w))
            .h(px(tile.h));
        if selected {
            cell = cell.child(
                div()
                    .absolute()
                    .left(px(x - layout::SELECTION_EXTENT))
                    .top(px(y - layout::SELECTION_EXTENT))
                    .w(px(w + 2. * layout::SELECTION_EXTENT))
                    .h(px(h + 2. * layout::SELECTION_EXTENT))
                    .rounded(px(theme::TILE_RADIUS + layout::SELECTION_EXTENT))
                    .border_2()
                    .border_color(rgb(theme::ACCENT)),
            );
        }
        cell = cell.child(
            div()
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(w))
                .h(px(h))
                .rounded(px(theme::TILE_RADIUS))
                .bg(rgb(theme::PLACEHOLDER))
                .child(
                    img(self.thumbnail(&item.content_hash))
                        .image_cache(&self.thumbnails)
                        .size_full()
                        .rounded(px(theme::TILE_RADIUS))
                        .object_fit(ObjectFit::Cover),
                )
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(px(theme::TILE_RADIUS))
                        .border_1()
                        .border_color(rgba(theme::HAIRLINE)),
                ),
        );
        let collection = item.kind == picto_library::RootKind::Collection;
        if prefs.show_extension_label {
            let label = format::format_label(&item.mime);
            if collection {
                cell = cell.child(
                    Self::badge(0.)
                        .w(px(18.))
                        .justify_center()
                        .left(px(x + 5.))
                        .top(px(y + 5.))
                        .child(
                            svg()
                                .path("folders")
                                .size(px(12.))
                                .text_color(rgba(theme::BADGE_TEXT)),
                        ),
                );
            } else if !label.is_empty() {
                cell = cell.child(
                    Self::badge(4.)
                        .left(px(x + 5.))
                        .top(px(y + 5.))
                        .child(label),
                );
            }
        }
        let animated = item.mime == "image/gif" && item.frame_count.unwrap_or(0) > 1;
        if let Some(duration) = item.duration_ms.filter(|ms| *ms > 0 && (timed || animated)) {
            cell = cell.child(
                Self::badge(2.)
                    .right(px(tile.w - x - w + 5.))
                    .top(px(y + 5.))
                    .child(format::format_duration(duration)),
            );
        }
        if prefs.show_item_count && collection {
            cell = cell.child(
                Self::badge(4.)
                    .left(px(x + 5.))
                    .top(px(y + h - 18. - 5.))
                    .child(item.media_count.to_string()),
            );
        }
        if prefs.show_name {
            cell = cell.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(image_height + 7.))
                    .h(px(15.))
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .max_w_full()
                            .px(px(3.))
                            .rounded(px(2.))
                            .bg(if selected {
                                rgb(theme::ACCENT)
                            } else {
                                rgba(0x00000000)
                            })
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .line_height(px(15.))
                            .child(item.name.clone()),
                    ),
            );
        }
        if prefs.show_resolution
            && let (Some(width), Some(height)) = (item.width, item.height)
        {
            cell = cell.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(image_height
                        + prefs.show_name as u8 as f32 * layout::TEXT_ROW_HEIGHT
                        + 5.))
                    .h(px(15.))
                    .flex()
                    .justify_center()
                    .text_size(px(11.))
                    .line_height(px(15.))
                    .text_color(rgba(theme::TEXT_TERTIARY))
                    .child(format!("{width} × {height}")),
            );
        }
        cell.on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
            this.select(index, cx);
            if event.click_count() == 2 {
                this.preview = true;
                this.update_surface(cx);
            }
            window.focus(&this.focus, cx);
            cx.notify();
        }))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                this.select(index, cx);
                let size = window.viewport_size();
                this.menu = Some(point(
                    event.position.x.min(size.width - px(278.)),
                    event.position.y.min(size.height - px(120.)),
                ));
                window.focus(&this.focus, cx);
                cx.notify();
            }),
        )
    }
    /// The grid's translucent label chip (`drawBadge`).
    fn badge(padding: f32) -> Div {
        div()
            .absolute()
            .h(px(18.))
            .px(px(padding))
            .flex()
            .items_center()
            .rounded(px(theme::TILE_RADIUS))
            .bg(rgba(0x00000080))
            .border_1()
            .border_color(rgba(0x00000033))
            .text_size(px(10.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgba(theme::BADGE_TEXT))
    }
    fn update_surface(&mut self, cx: &mut Context<Self>) {
        if !self.preview {
            return;
        }
        if let Some((name, mime)) = self.demo {
            let path = self.fixtures.path().join(name);
            self.surface.update(cx, |surface, cx| {
                surface.open(name.to_owned(), PathBuf::new(), path, mime, cx)
            });
            return;
        }
        if let Some(media) = self.details.as_ref().and_then(|d| d.media.get(self.member)) {
            let key = media.facts.content_hash.clone();
            let thumb = self.thumbnail(&key);
            self.surface.update(cx, |surface, cx| {
                surface.open(
                    key,
                    thumb,
                    PathBuf::from(&media.file_path),
                    &media.facts.mime,
                    cx,
                )
            });
        }
    }
    fn show_demo(&mut self, name: &'static str, mime: &'static str, cx: &mut Context<Self>) {
        self.demo = Some((name, mime));
        self.preview = true;
        self.menu = None;
        self.update_surface(cx);
        cx.notify();
    }
}
impl Render for Browser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.preview {
            self.surface.update(cx, |surface, _| surface.stop());
        }
        let width = (f32::from(window.viewport_size().width) - 580.).max(100.);
        let text_height = self.prefs.text_height();
        let key = Some((
            width as u32,
            self.tile as u32,
            self.items.len(),
            self.prefs.gap as u32,
            self.prefs.mode,
            text_height as u32,
        ));
        if key != self.layout_key {
            let aspects = self
                .items
                .iter()
                .map(|i| i.width.unwrap_or(1) as f32 / i.height.unwrap_or(1).max(1) as f32)
                .collect::<Vec<_>>();
            self.layout = layout::Layout::compute(
                &aspects,
                width,
                self.tile,
                self.prefs.gap,
                self.prefs.mode,
                text_height,
            );
            self.layout_key = key;
            self.columns = self.layout.column_count.max(1);
        }
        let selected = self.items.get(self.selected).cloned();
        let name = self
            .demo
            .map(|(name, _)| name.to_owned())
            .unwrap_or_else(|| {
                selected
                    .as_ref()
                    .map(|s| s.name.clone())
                    .unwrap_or_else(|| "No selection".into())
            });
        let avatar = if let Some(path) = &self.library_cover {
            img(path.clone())
                .image_cache(&self.thumbnails)
                .size(px(39.))
                .rounded_full()
                .object_fit(ObjectFit::Cover)
                .into_any_element()
        } else {
            svg()
                .path("books")
                .size(px(28.))
                .text_color(rgb(theme::TEXT))
                .into_any_element()
        };
        let mut sidebar = div()
            .id("sidebar")
            .w(px(300.))
            .flex_none()
            .h_full()
            .bg(rgb(PANEL))
            .border_r_1()
            .border_color(rgb(0x353638))
            .flex()
            .flex_col()
            .child(div().h(px(48.)).flex_none())
            .child(
                div()
                    .h(px(48.))
                    .mt(px(4.))
                    .mb(px(8.))
                    .pl(px(20.5))
                    .pr(px(27.))
                    .flex()
                    .items_center()
                    .text_size(px(14.))
                    .gap(px(6.))
                    .child(
                        div()
                            .size(px(39.))
                            .rounded_full()
                            .overflow_hidden()
                            .bg(rgb(0x303236))
                            .border_1()
                            .border_color(rgb(0x48494c))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(avatar),
                    )
                    .child("Main"),
            );
        let mut nav = div()
            .id("navigation")
            .flex_1()
            .overflow_y_scroll()
            .pr(px(8.));
        let filter = self.folder_filter.read(cx).value().to_lowercase();
        for (i, entry) in self.nav.iter().enumerate() {
            if !filter.is_empty() && !entry.label.to_lowercase().contains(&filter) {
                continue;
            }
            let mut parent = entry.parent;
            let mut hidden = false;
            while let Some(index) = parent {
                if self.collapsed.contains(&index) {
                    hidden = true;
                    break;
                }
                parent = self.nav[index].parent;
            }
            if hidden {
                continue;
            }
            let section = match entry.scope {
                picto_library::query::ItemScope::FolderTree { .. } => Some("Folders"),
                picto_library::query::ItemScope::SmartFolder { .. } => Some("Smart Folders"),
                _ => None,
            };
            let previous_section = i.checked_sub(1).and_then(|p| match self.nav[p].scope {
                picto_library::query::ItemScope::FolderTree { .. } => Some("Folders"),
                picto_library::query::ItemScope::SmartFolder { .. } => Some("Smart Folders"),
                _ => None,
            });
            if section.is_some() && section != previous_section {
                nav = nav.child(
                    div()
                        .pl(px(20.))
                        .mt(px(8.))
                        .mb(px(2.))
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .text_size(px(12.))
                        .text_color(rgb(MUTED))
                        .child(section.unwrap()),
                );
            }
            let has_children = self.nav.iter().any(|n| n.parent == Some(i));
            let mut disclosure = div()
                .id(("disclosure", i))
                .absolute()
                .left(px(2. + entry.depth as f32 * 20.))
                .w(px(14.))
                .h(px(theme::ROW_HEIGHT))
                .flex_none()
                .flex()
                .items_center()
                .justify_center();
            if has_children {
                disclosure = disclosure
                    .rounded_sm()
                    .hover(|s| s.bg(rgb(0x45474b)))
                    .child(theme::icon(if self.collapsed.contains(&i) {
                        "chevron-right"
                    } else {
                        "chevron-down"
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if !this.collapsed.remove(&i) {
                            this.collapsed.insert(i);
                        }
                        cx.notify();
                    }));
            }

            let icon = match entry.scope {
                picto_library::query::ItemScope::All => "photo",
                picto_library::query::ItemScope::Inbox => "inbox",
                picto_library::query::ItemScope::Trash => "trash",
                picto_library::query::ItemScope::Untagged => "bookmark-question",
                picto_library::query::ItemScope::Uncategorized => "folder-question",
                picto_library::query::ItemScope::RecentlyViewed => "clock",
                picto_library::query::ItemScope::SmartFolder { .. } if entry.parent.is_none() => {
                    "layout-grid"
                }
                _ => "folder",
            };

            nav = nav.child(
                div()
                    .id(i)
                    .relative()
                    .group("sidebar-row")
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(theme::ROW_HEIGHT))
                    .pl(px(16. + entry.depth as f32 * 20.))
                    .pr(px(2.))
                    .rounded_md()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.scope = i;
                        this.preview = false;
                        this.query(false, cx);
                        window.focus(&this.focus, cx);
                    }))
                    .child(
                        div()
                            .absolute()
                            .left(px(16. + entry.depth as f32 * 20.))
                            .right(px(2.))
                            .top_0()
                            .bottom(px(1.))
                            .rounded(px(6.))
                            .bg(if self.scope == i {
                                rgba(0xffffff14)
                            } else {
                                rgba(0x00000000)
                            })
                            .group_hover("sidebar-row", |s| s.bg(rgba(0xffffff0d))),
                    )
                    .child(disclosure)
                    .child(
                        div()
                            .w(px(26.))
                            .h(px(27.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(theme::icon(icon)),
                    )
                    .child(
                        div().flex_1().overflow_hidden().text_ellipsis().child(
                            entry
                                .label
                                .strip_prefix("Folder / ")
                                .or_else(|| entry.label.strip_prefix("Smart / "))
                                .unwrap_or(&entry.label)
                                .to_owned(),
                        ),
                    )
                    .child(
                        div()
                            .px(px(5.))
                            .text_size(px(11.))
                            .font_family("SF Mono")
                            .min_w(px(36.))
                            .text_right()
                            .text_color(rgb(if self.scope == i { theme::TEXT } else { MUTED }))
                            .child(format::grouped(entry.count)),
                    ),
            );
        }
        sidebar = sidebar.child(nav).child(
            div().h(px(38.)).px(px(10.)).pb(px(10.)).flex_none().child(
                div()
                    .h(px(28.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(0x303236))
                    .overflow_hidden()
                    .child(self.folder_filter.clone()),
            ),
        );
        let mut toolbar = div()
            .h(px(48.))
            .flex_none()
            .px_4()
            .flex()
            .items_center()
            .gap_3()
            .border_b_1()
            .border_color(rgb(0x303236))
            .child(
                div().w(px(120.)).overflow_hidden().text_ellipsis().child(
                    self.nav
                        .get(self.scope)
                        .map(|n| n.label.clone())
                        .unwrap_or_else(|| "Opening library...".into()),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .justify_center()
                    .child(self.zoom.clone()),
            )
            .child(
                div()
                    .w(px(148.))
                    .h(px(24.))
                    .rounded(px(5.))
                    .border_1()
                    .border_color(rgb(0x303135))
                    .overflow_hidden()
                    .child(self.search.clone()),
            );
        if self.extra_filter.is_some() {
            toolbar = toolbar.child(self.button("clear-filter", "Clear filter").on_click(
                cx.listener(|this, _, _, cx| {
                    this.extra_filter = None;
                    this.query(false, cx);
                }),
            ));
        }
        let mut center = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(toolbar);
        if self.preview {
            let count = if self.demo.is_some() {
                1
            } else {
                self.details.as_ref().map(|d| d.media.len()).unwrap_or(1)
            };
            center = center
                .child(
                    div()
                        .h(px(44.))
                        .px_4()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(self.button("close", "Back to grid").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.preview = false;
                                cx.notify();
                            },
                        )))
                        .child(
                            div()
                                .flex_1()
                                .overflow_hidden()
                                .text_ellipsis()
                                .child(name.clone()),
                        )
                        .child(
                            self.button("previous", "Previous")
                                .on_click(cx.listener(|this, _, _, cx| this.step(-1, cx))),
                        )
                        .child(
                            self.button("next", "Next")
                                .on_click(cx.listener(|this, _, _, cx| this.step(1, cx))),
                        ),
                )
                .child(div().flex_1().min_h_0().p_3().child(self.surface.clone()));
            if count > 1 {
                center = center.child(
                    div()
                        .h(px(48.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .child(self.button("member-back", "‹").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.member = this.member.saturating_sub(1);
                                this.update_surface(cx);
                                cx.notify();
                            },
                        )))
                        .child(format!("{} / {} in collection", self.member + 1, count))
                        .child(self.button("member-next", "›").on_click(cx.listener(
                            move |this, _, _, cx| {
                                this.member = (this.member + 1).min(count - 1);
                                this.update_surface(cx);
                                cx.notify();
                            },
                        ))),
                );
            }
        } else if self.items.is_empty() {
            center = center.child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(MUTED))
                    .child(if self.loading {
                        "Opening library..."
                    } else {
                        "No items match this filter"
                    }),
            );
        } else {
            center = center.child({
                let top = -f32::from(self.scroll.offset().y);
                let viewport = f32::from(window.viewport_size().height) - 80.;
                let height = self.layout.total_height;
                if top + viewport + 1000. >= height && self.cursor.is_some() && !self.loading {
                    self.query(true, cx);
                }
                let mut content = div().relative().w_full().h(px(height));
                for index in self.layout.visible(top - 500., top + viewport + 500.) {
                    content = content.child(self.tile_element(index, cx));
                }
                div()
                    .id("media-grid")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .on_scroll(cx.listener(|_, _, _, cx| cx.notify()))
                    .child(content)
            });
        }

        center = center.child(
            div()
                .h(px(28.))
                .flex_none()
                .px_4()
                .flex()
                .items_center()
                .text_size(px(11.))
                .text_color(rgb(MUTED))
                .child(
                    self.error
                        .clone()
                        .unwrap_or_else(|| format!("{} items", format::grouped(self.total))),
                ),
        );
        let inspector = self.inspector(cx);
        let mut root = div()
            .relative()
            .size_full()
            .flex()
            .bg(rgb(BG))
            .text_color(rgb(theme::TEXT))
            .font_family("SF Pro Text")
            .text_size(px(13.))
            .track_focus(&self.focus)
            .key_context("Browser")
            .on_action(cx.listener(|this, _: &DemoSvg, _, cx| {
                this.show_demo("vector.svg", "image/svg+xml", cx)
            }))
            .on_action(cx.listener(|this, _: &DemoFlash, _, cx| {
                this.show_demo("interactive.swf", "application/x-shockwave-flash", cx)
            }))
            .on_action(cx.listener(|this, _: &DemoWord, _, cx| {
                this.show_demo(
                    "document.docx",
                    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                    cx,
                )
            }))
            .on_action(cx.listener(|this, _: &Undo, _, _| {
                if let Some(item) = this.items.get(this.selected) {
                    let _ = this.sender.send(Request::Undo(item.root_id));
                }
            }))
            .on_action(cx.listener(|this, _: &Redo, _, _| {
                if let Some(item) = this.items.get(this.selected) {
                    let _ = this.sender.send(Request::Redo(item.root_id));
                }
            }))
            .on_action(cx.listener(|this, _: &Next, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &Previous, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &Down, _, cx| this.step(this.columns as isize, cx)))
            .on_action(cx.listener(|this, _: &Up, _, cx| this.step(-(this.columns as isize), cx)))
            .on_action(cx.listener(|this, _: &Preview, _, cx| {
                this.preview = !this.preview;
                this.update_surface(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ClosePreview, _, cx| {
                if this.menu.take().is_none() {
                    this.preview = false;
                }
                cx.notify();
            }))
            .child(sidebar)
            .child(center)
            .child(inspector);
        if let Some(position) = self.menu {
            let mut menu = div()
                .absolute()
                .left(position.x)
                .top(position.y)
                .w(px(270.))
                .p_1()
                .rounded(px(8.))
                .border_1()
                .border_color(rgb(0x47484b))
                .bg(rgb(0x292a2d))
                .flex()
                .flex_col();
            for (index, label) in ["Quick Look", "Copy Name", "Copy File Path"]
                .into_iter()
                .enumerate()
            {
                menu = menu.child(
                    div()
                        .id(("context-action", index))
                        .h(px(28.))
                        .px(px(6.))
                        .flex()
                        .items_center()
                        .rounded(px(5.))
                        .hover(|s| s.bg(rgb(0x424348)))
                        .child(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.menu = None;
                            match index {
                                0 => {
                                    this.preview = true;
                                    this.update_surface(cx);
                                }
                                1 => {
                                    if let Some(item) = this.items.get(this.selected) {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            item.name.clone(),
                                        ));
                                    }
                                }
                                _ => {
                                    if let Some(media) =
                                        this.details.as_ref().and_then(|d| d.media.get(this.member))
                                    {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            media.file_path.clone(),
                                        ));
                                    }
                                }
                            }
                            cx.notify();
                        })),
                );
            }
            root = root
                .child(
                    div()
                        .id("menu-backdrop")
                        .absolute()
                        .inset_0()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.menu = None;
                                cx.notify();
                            }),
                        ),
                )
                .child(menu);
        }
        root
    }
}
fn main() {
    let mut args = std::env::args().skip(1);
    let database = PathBuf::from(args.next().expect("Pass a snapshot database from run.sh"));
    let blobs = PathBuf::from(args.next().expect("Pass the source blobs directory"));
    assert!(
        database.parent().unwrap().join(".gpui-snapshot").is_file(),
        "Only a marked disposable snapshot is accepted"
    );
    gpui_platform::application()
        .with_assets(theme::Assets)
        .run(move |cx: &mut App| {
            cx.text_system()
                .add_fonts(vec![
                    std::borrow::Cow::Borrowed(include_bytes!("../assets/Roboto.ttf")),
                    std::borrow::Cow::Borrowed(include_bytes!("../assets/FiraMono.ttf")),
                ])
                .expect("load bundled inspector font");
            input::bind(cx);
            cx.bind_keys([
                KeyBinding::new("cmd-z", Undo, Some("Browser && !TextInput")),
                KeyBinding::new("cmd-shift-z", Redo, Some("Browser && !TextInput")),
                KeyBinding::new("right", Next, Some("Browser && !TextInput")),
                KeyBinding::new("left", Previous, Some("Browser && !TextInput")),
                KeyBinding::new("down", Down, Some("Browser && !TextInput")),
                KeyBinding::new("up", Up, Some("Browser && !TextInput")),
                KeyBinding::new("space", Preview, Some("Browser && !TextInput")),
                KeyBinding::new("enter", Preview, Some("Browser && !TextInput")),
                KeyBinding::new("escape", ClosePreview, Some("Browser")),
                KeyBinding::new("cmd-q", Quit, None),
                KeyBinding::new("cmd-1", DemoSvg, Some("Browser")),
                KeyBinding::new("cmd-2", DemoFlash, Some("Browser")),
                KeyBinding::new("cmd-3", DemoWord, Some("Browser")),
            ]);
            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.set_menus(vec![
                Menu {
                    name: "Picto GPUI".into(),
                    items: vec![MenuItem::action("Quit", Quit)],
                    disabled: false,
                },
                Menu {
                    name: "Media Examples".into(),
                    items: vec![
                        MenuItem::action("SVG", DemoSvg),
                        MenuItem::action("Flash (Ruffle)", DemoFlash),
                        MenuItem::action("Word", DemoWord),
                    ],
                    disabled: false,
                },
            ]);
            let bounds = Bounds::centered(None, size(px(1320.), px(850.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(1000.), px(600.))),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Picto GPUI · Library snapshot".into()),
                        appears_transparent: true,
                        traffic_light_position: Some(point(px(12.), px(12.))),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                move |window, cx| cx.new(|cx| Browser::new(database, blobs, window, cx)),
            )
            .unwrap();
            cx.activate(true);
        });
}
