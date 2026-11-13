mod backend;
mod demo;
mod flash;
mod format;
mod input;
mod inspector;
mod layout;
mod motion;
mod nav;
mod prefs;
mod reveal;
mod shell;
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
        First,
        Last,
        PageUp,
        PageDown,
        Preview,
        ToggleSidebar,
        ToggleInspector,
        TogglePanels,
        ClosePreview,
        Undo,
        Redo,
        Quit,
        DemoSvg,
        DemoFlash
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
    /// Visited sidebar destinations, for Back and Forward.
    history: Vec<usize>,
    history_index: usize,
    selected: usize,
    details: Option<RootDetails>,
    preview: bool,
    menu: Option<Point<Pixels>>,
    /// A dismissed menu, still on screen while it fades out.
    menu_closing: Option<(Point<Pixels>, std::time::Instant)>,
    demo: Option<(&'static str, &'static str)>,
    fixtures: tempfile::TempDir,
    member: usize,
    tile: f32,
    /// The grid's size at the last paint, for keyboard paging and scrolling.
    grid_size: (f32, f32),

    scroll: ScrollHandle,
    layout: layout::Layout,
    layout_key: Option<(u32, u32, usize, u32, layout::ViewMode, u32)>,
    prefs: prefs::ViewPrefs,
    prefs_scope: Option<usize>,
    reveal: reveal::RevealTracker,
    hover: motion::Fades,
    /// A press on a titlebar drag region that has not yet moved.
    titlebar_press: bool,
    sidebar_collapsed: bool,
    inspector_collapsed: bool,
    /// How far each rail has slid out of view.
    rails: motion::Fades,
    /// The steady clock the reveal timings are measured on.
    clock: std::time::Instant,
    transition: shell::Transition,
    /// The destination shown in the grid and title; trails `scope` while the
    /// previous one fades out.
    displayed: usize,
    /// A destination's first page that arrived before the fade-out finished.
    pending_page: Option<(picto_library::query::RootPage, prefs::ViewPrefs)>,
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
            history: vec![0],
            history_index: 0,
            selected: 0,
            details: None,
            preview: false,
            menu: None,
            menu_closing: None,
            demo: None,
            fixtures: demo::fixtures().expect("create disposable media fixtures"),
            member: 0,
            tile: 200.,
            grid_size: (0., 0.),

            scroll: ScrollHandle::new(),
            layout: layout::Layout::default(),
            layout_key: None,
            prefs: prefs::ViewPrefs::default(),
            prefs_scope: None,
            reveal: reveal::RevealTracker::default(),
            hover: motion::Fades::default(),
            titlebar_press: false,
            sidebar_collapsed: false,
            inspector_collapsed: false,
            rails: motion::Fades::default(),
            clock: std::time::Instant::now(),
            transition: shell::Transition::Idle,
            displayed: 0,
            pending_page: None,
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
        if nav.screen {
            return;
        }
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
            nav.random,
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
                    if !append && matches!(self.transition, shell::Transition::FadingOut(_)) {
                        // The previous grid is still fading out. Hold the
                        // destination until it is gone.
                        self.pending_page = Some((page, prefs));
                    } else {
                        self.apply_page(append, page, prefs, cx);
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
    pub(crate) fn apply_page(
        &mut self,
        append: bool,
        page: picto_library::query::RootPage,
        prefs: prefs::ViewPrefs,
        cx: &mut Context<Self>,
    ) {
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
            self.displayed = self.scope;
            if matches!(self.transition, shell::Transition::Waiting) {
                self.transition = shell::Transition::FadingIn(std::time::Instant::now());
            }
            self.select(0, cx);
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
            cx.notify();
        }
    }
    /// Moves the selection as the Electron grid's keyboard navigation does and
    /// scrolls the new selection into view.
    fn move_selection(&mut self, action: nav::Move, cx: &mut Context<Self>) {
        let (width, height) = self.grid_size;
        let action = match action {
            // With the preview open only the item changes, one at a time.
            nav::Move::Left | nav::Move::Right => action,
            _ if self.preview => return,
            _ => action,
        };
        let Some(target) = nav::target(
            action,
            self.selected,
            self.items.len(),
            nav::column_count(width, self.tile, self.prefs.gap),
            nav::visible_rows(height, self.tile, self.prefs.gap),
        ) else {
            return;
        };
        self.select(target, cx);
        if let Some(tile) = self.layout.tiles.get(target) {
            let top = -f32::from(self.scroll.offset().y);
            if let Some(offset) = nav::scroll_into_view(tile, top, height, self.prefs.gap) {
                self.scroll.set_offset(point(px(0.), px(-offset)));
            }
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
            .occlude()
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
    fn tile_element(&self, index: usize, now: f64, cx: &mut Context<Self>) -> Stateful<Div> {
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
        // The image fades in over its dominant colour, then that colour fades
        // away beneath it, which matters for images with transparency.
        let placeholder = item
            .palette
            .first()
            .map(|color| format::lab_to_rgb(color.l, color.a, color.b))
            .unwrap_or(theme::PLACEHOLDER);
        let (image_alpha, placeholder_alpha) =
            reveal::alphas(self.reveal.progress(item.root_id.0, now));
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
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(px(theme::TILE_RADIUS))
                        .bg(rgb(placeholder))
                        .opacity(placeholder_alpha),
                )
                .child(
                    img(self.thumbnail(&item.content_hash))
                        .image_cache(&self.thumbnails)
                        .size_full()
                        .rounded(px(theme::TILE_RADIUS))
                        .object_fit(ObjectFit::Cover)
                        .opacity(image_alpha),
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
    /// Dismisses the context menu, leaving it to fade out. Returns whether one
    /// was open.
    fn close_menu(&mut self) -> bool {
        match self.menu.take() {
            Some(position) => {
                self.menu_closing = Some((position, std::time::Instant::now()));
                true
            }
            None => false,
        }
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
        self.close_menu();
        self.update_surface(cx);
        cx.notify();
    }
}
impl Render for Browser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.advance_transition(cx);
        let fade = self.transition.opacity();
        let now = self.clock.elapsed().as_secs_f64() * 1000.;
        if !self.preview {
            self.surface.update(cx, |surface, _| surface.stop());
        }
        // The rails give up their width at once; only their own slide is animated.
        let left = if self.sidebar_collapsed {
            0.
        } else {
            theme::SIDEBAR_WIDTH
        };
        let right = if self.inspector_collapsed {
            0.
        } else {
            theme::INSPECTOR_WIDTH
        };
        let width = (f32::from(window.viewport_size().width) - left - right).max(100.);
        self.grid_size = (width, f32::from(window.viewport_size().height) - 80.);
        self.surface
            .update(cx, |surface, _| surface.chrome_width = left + right + 24.);
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
        let sidebar_hidden = self.rails.value(("sidebar", 0));
        let inspector_hidden = self.rails.value(("inspector", 0));
        let sidebar = (sidebar_hidden < 1.).then(|| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(-theme::SIDEBAR_WIDTH * sidebar_hidden))
                .child(self.sidebar(avatar, cx))
        });
        let mut toolbar = self.toolbar(cx);
        if self.extra_filter.is_some() {
            toolbar = toolbar.child(self.button("clear-filter", "Clear filter").on_click(
                cx.listener(|this, _, _, cx| {
                    this.extra_filter = None;
                    this.query(false, cx);
                }),
            ));
        }
        let mut center = div()
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(left))
            .right(px(right))
            .flex()
            .flex_col();
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
                        .child(self.button("previous", "Previous").on_click(
                            cx.listener(|this, _, _, cx| this.move_selection(nav::Move::Left, cx)),
                        ))
                        .child(self.button("next", "Next").on_click(
                            cx.listener(|this, _, _, cx| this.move_selection(nav::Move::Right, cx)),
                        )),
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
                    .opacity(fade)
                    .text_color(rgb(MUTED))
                    .child(match self.nav.get(self.displayed) {
                        Some(entry) if entry.screen => {
                            format!("{} is not part of this prototype yet", entry.label)
                        }
                        _ if self.loading => "Opening library...".into(),
                        _ => "No items match this filter".to_owned(),
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
                let visible = self
                    .layout
                    .visible(top - reveal::MARGIN, top + viewport + reveal::MARGIN);
                let decoded = {
                    let thumbnails = self.thumbnails.read(cx);
                    visible
                        .iter()
                        .map(|&index| {
                            let item = &self.items[index];
                            let thumbnail = self.thumbnail(&item.content_hash);
                            (
                                item.root_id.0,
                                thumbnails.is_loaded(&Resource::Path(thumbnail.into())),
                            )
                        })
                        .collect::<Vec<_>>()
                };
                self.reveal.update(&decoded, now);
                for index in visible {
                    content = content.child(self.tile_element(index, now, cx));
                }
                div()
                    .id("media-grid")
                    .flex_1()
                    .min_h_0()
                    .opacity(fade)
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
        // The inspector rail spans the window's full height, titlebar strip
        // included, and slides as one piece.
        let inspector = (inspector_hidden < 1.).then(|| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .right(px(-theme::INSPECTOR_WIDTH * inspector_hidden))
                .w(px(theme::INSPECTOR_WIDTH))
                .flex()
                .flex_col()
                .bg(rgb(PANEL))
                .border_l_1()
                .border_color(rgb(0x303236))
                .child(
                    self.drag_region(div().id("inspector-titlebar"), cx)
                        .h(px(theme::TITLEBAR_HEIGHT))
                        .flex_none(),
                )
                .child(self.inspector(cx))
        });
        let titlebar = div()
            .h(px(theme::TITLEBAR_HEIGHT))
            .flex_none()
            .flex()
            .child(self.titlebar_left(cx))
            .child(toolbar)
            .child(
                // The inspector's share of the titlebar stays reserved, and
                // draggable, while the inspector is hidden.
                self.drag_region(div().id("titlebar-right"), cx)
                    .w(px(theme::INSPECTOR_WIDTH))
                    .flex_none()
                    .h_full(),
            );
        let body = div()
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .child(center)
            .children(sidebar);
        let mut root = div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
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
            .on_action(
                cx.listener(|this, _: &Next, _, cx| this.move_selection(nav::Move::Right, cx)),
            )
            .on_action(
                cx.listener(|this, _: &Previous, _, cx| this.move_selection(nav::Move::Left, cx)),
            )
            .on_action(
                cx.listener(|this, _: &Down, _, cx| this.move_selection(nav::Move::Down, cx)),
            )
            .on_action(cx.listener(|this, _: &Up, _, cx| this.move_selection(nav::Move::Up, cx)))
            .on_action(
                cx.listener(|this, _: &First, _, cx| this.move_selection(nav::Move::First, cx)),
            )
            .on_action(
                cx.listener(|this, _: &Last, _, cx| this.move_selection(nav::Move::Last, cx)),
            )
            .on_action(
                cx.listener(|this, _: &PageUp, _, cx| this.move_selection(nav::Move::PageUp, cx)),
            )
            .on_action(
                cx.listener(|this, _: &PageDown, _, cx| {
                    this.move_selection(nav::Move::PageDown, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &Preview, _, cx| {
                this.preview = !this.preview;
                this.update_surface(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ClosePreview, _, cx| {
                if !this.close_menu() {
                    this.preview = false;
                }
                cx.notify();
            }))
            .on_action(
                cx.listener(|this, _: &ToggleSidebar, _, cx| this.toggle_rails(true, false, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ToggleInspector, _, cx| this.toggle_rails(false, true, cx)),
            )
            .on_action(
                cx.listener(|this, _: &TogglePanels, _, cx| this.toggle_rails(true, true, cx)),
            )
            .child(titlebar)
            .child(body)
            .children(inspector);
        // `menuOut`: a dismissed menu fades over 80 ms and takes no more input.
        let closing = self.menu_closing.and_then(|(position, started)| {
            let progress = motion::progress(started.elapsed(), motion::MENU_OUT);
            (progress < 1.).then(|| (position, 1. - motion::EASE_IN.ease(progress)))
        });
        if closing.is_none() {
            self.menu_closing = None;
        }
        let interactive = self.menu.is_some();
        if let Some((position, opacity)) = self.menu.map(|position| (position, 1.)).or(closing) {
            // The floating glass surface of `surfaces.css`, without its blur.
            let mut menu = div()
                .absolute()
                .left(position.x)
                .top(position.y)
                .w(px(270.))
                .p(px(4.))
                .gap(px(2.))
                .rounded(px(8.))
                .border_1()
                .border_color(rgba(0xffffff26))
                .bg(rgba(theme::GLASS))
                .shadow(vec![
                    BoxShadow::new(px(0.), px(12.), rgba(0x0000004d).into()).blur_radius(px(24.)),
                    BoxShadow::new(px(0.), px(0.), rgba(0x00000080).into()).spread_radius(px(1.)),
                ])
                .opacity(opacity)
                .flex()
                .flex_col();
            for (index, label) in ["Quick Look", "Copy Name", "Copy File Path"]
                .into_iter()
                .enumerate()
            {
                menu = menu.child(
                    div()
                        .id(("context-action", index))
                        .h(px(26.))
                        .px(px(6.))
                        .flex()
                        .items_center()
                        .rounded(px(6.))
                        .hover(|s| s.bg(rgba(0xf8f9fb14)))
                        .child(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            // A menu that is fading out no longer acts.
                            if !this.close_menu() {
                                return;
                            }
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
            if interactive {
                root = root.child(
                    div()
                        .id("menu-backdrop")
                        .absolute()
                        .inset_0()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.close_menu();
                                cx.notify();
                            }),
                        ),
                );
            }
            root = root.child(menu);
        }
        // Keep painting while a fade or a thumbnail reveal is in motion.
        if self.transition.animating()
            || self.reveal.active(now)
            || self.menu_closing.is_some()
            || self.hover.animating()
            || self.rails.animating()
        {
            window.request_animation_frame();
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
                // Arrows and WASD, Home/End and paging: `shortcuts.ts` grid.*.
                KeyBinding::new("right", Next, Some("Browser && !TextInput")),
                KeyBinding::new("d", Next, Some("Browser && !TextInput")),
                KeyBinding::new("left", Previous, Some("Browser && !TextInput")),
                KeyBinding::new("a", Previous, Some("Browser && !TextInput")),
                KeyBinding::new("down", Down, Some("Browser && !TextInput")),
                KeyBinding::new("s", Down, Some("Browser && !TextInput")),
                KeyBinding::new("up", Up, Some("Browser && !TextInput")),
                KeyBinding::new("w", Up, Some("Browser && !TextInput")),
                KeyBinding::new("home", First, Some("Browser && !TextInput")),
                KeyBinding::new("end", Last, Some("Browser && !TextInput")),
                KeyBinding::new("pageup", PageUp, Some("Browser && !TextInput")),
                KeyBinding::new("pagedown", PageDown, Some("Browser && !TextInput")),
                // `shortcuts.ts` view.toggleSidebar / toggleInspector / toggleBothPanels.
                KeyBinding::new("cmd-alt-1", ToggleSidebar, Some("Browser")),
                KeyBinding::new("cmd-alt-2", ToggleInspector, Some("Browser")),
                KeyBinding::new("tab", TogglePanels, Some("Browser && !TextInput")),
                KeyBinding::new("space", Preview, Some("Browser && !TextInput")),
                KeyBinding::new("enter", Preview, Some("Browser && !TextInput")),
                KeyBinding::new("escape", ClosePreview, Some("Browser")),
                KeyBinding::new("cmd-q", Quit, None),
                KeyBinding::new("cmd-1", DemoSvg, Some("Browser")),
                KeyBinding::new("cmd-2", DemoFlash, Some("Browser")),
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
                    }),
                    // The toolbar sits in the titlebar. AppKit would drag the
                    // window from anywhere in that zone, controls included, so
                    // the drag regions are declared by the shell instead.
                    app_owns_titlebar_drag: true,
                    ..Default::default()
                },
                move |window, cx| cx.new(|cx| Browser::new(database, blobs, window, cx)),
            )
            .unwrap();
            cx.activate(true);
        });
}
