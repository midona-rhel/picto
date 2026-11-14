//! The detail window ("Open in New Window", `DetailWindow.tsx` and
//! `GroupDetailWindow.tsx`, opened by `open_in_new_window` in the Electron
//! main process): a frameless window of its own, sized from the media's
//! shape and locked to it, showing the items that were selected when it
//! opened. Its toolbar slides over the media and hides a second after the
//! pointer last moved. Several can be open at once, one per item.
use super::{
    ActualSize, Boundary, End, FitToWindow, Media, NextItem, PreviousItem, ZoomIn, ZoomOut,
    group::GroupReader, options, record_view, size_of, surface::MediaSurface,
};
use crate::grid::menu as rows;
use crate::session::Session;
use crate::theme::Text as _;
use crate::ui::tooltip;
use crate::{motion, theme, ui};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use picto_library::query::RootSummary;
use picto_library::{RootDetails, RootId, RootKind};
use std::collections::HashMap;
use std::time::Instant;

// The detail window's own actions. Their keys come from the shortcut
// registry (`crate::keymap`, the `DetailWindow` scope), as do the viewer's
// previous, next and zoom actions it shares with the media view.
actions!(detail_window, [CloseWindow, ToggleAlwaysOnTop]);

/// `.toolbar`: 38px tall, 10px in from either side.
const TOOLBAR_HEIGHT: f32 = 38.;
const TOOLBAR_PADDING: f32 = 10.;
/// `.titleName`'s `max-width`.
const TITLE_WIDTH: f32 = 300.;
/// `.zoomRatio`'s `min-width`.
const ZOOM_WIDTH: f32 = 38.;

/// `calcDetailWindowSize`: the largest window of the media's shape within
/// 85% of the display's work area, grown to at least 400×300 for very small
/// or very narrow media. Without a known shape, the whole 85%.
pub fn window_size(work_area: (f32, f32), media: Option<(u32, u32)>) -> (f32, f32) {
    let max_width = (work_area.0 * 0.85).round();
    let max_height = (work_area.1 * 0.85).round();
    let Some((image_width, image_height)) = media.filter(|(w, h)| *w > 0 && *h > 0) else {
        return (max_width, max_height);
    };
    let aspect = image_width as f32 / image_height as f32;
    let mut width = max_width;
    let mut height = (width / aspect).round();
    if height > max_height {
        height = max_height;
        width = (height * aspect).round();
    }
    if width < 400. || height < 300. {
        let grow = (400. / width).max(300. / height);
        width = (width * grow).round();
        height = (height * grow).round();
    }
    (width, height)
}

/// `calcDetailWindowAspectRatio`: the shape the window keeps while resized,
/// when the media's is known.
pub fn aspect_ratio(media: Option<(u32, u32)>) -> Option<f64> {
    media
        .filter(|(w, h)| *w > 0 && *h > 0)
        .map(|(w, h)| w as f64 / h as f64)
}

/// `LightImage`: what the window keeps of each item it was given.
#[derive(Clone, Debug)]
pub struct Entry {
    pub root: RootId,
    pub hash: String,
    pub name: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub collection: bool,
}

impl From<&RootSummary> for Entry {
    fn from(item: &RootSummary) -> Self {
        Self {
            root: item.root_id,
            hash: item.content_hash.clone(),
            name: item.name.clone(),
            width: item.width,
            height: item.height,
            collection: item.kind == RootKind::Collection,
        }
    }
}

impl Entry {
    fn dimensions(&self) -> Option<(u32, u32)> {
        Some((self.width?, self.height?))
    }

    /// The window's label in the Electron main process: one window per
    /// media file or collection.
    fn label(&self) -> String {
        if self.collection {
            format!("detail-group-{}", self.root.0)
        } else {
            format!("detail-{}", &self.hash[..self.hash.len().min(12)])
        }
    }

    /// `titleText`: the name (or the hash's start) and the size.
    fn title(&self) -> String {
        let name = if self.name.is_empty() {
            &self.hash[..self.hash.len().min(12)]
        } else {
            &self.name
        };
        match self.dimensions() {
            Some((width, height)) => format!("{name} ({width}\u{00d7}{height})"),
            None => name.to_owned(),
        }
    }
}

/// The open detail windows, by label.
#[derive(Default)]
struct Windows(HashMap<String, WindowHandle<DetailWindow>>);

impl Global for Windows {}

/// `openGridItemInWindow`: opens `entries[start]` in a window of its own,
/// with the other entries to step through, or brings forward the window
/// already showing it. A collection opens alone, as `GroupDetailWindow`.
pub fn open(session: Entity<Session>, mut entries: Vec<Entry>, start: usize, cx: &mut App) {
    let Some(entry) = entries.get(start).cloned() else {
        return;
    };
    let label = entry.label();
    let existing = cx
        .try_global::<Windows>()
        .and_then(|windows| windows.0.get(&label).copied());
    if let Some(handle) = existing
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    if entry.collection {
        entries = vec![entry.clone()];
    }
    let start = if entry.collection { 0 } else { start };
    let shape = (!entry.collection).then(|| entry.dimensions()).flatten();
    let work_area = cx
        .primary_display()
        .map(|display| display.visible_bounds().size)
        .map_or((1440., 900.), |area| {
            (f32::from(area.width), f32::from(area.height))
        });
    let (width, height) = window_size(work_area, shape);
    let mut bounds = Bounds::centered(None, size(px(width), px(height)), cx);
    bounds.origin = bounds.origin.map(|at| at.floor());
    let title = entry.title();
    let Ok(handle) = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            // `frame: false`: no titlebar and no traffic lights; the toolbar
            // moves the window.
            ..ui::window_chrome::frameless(title, None)
        },
        move |window, cx| cx.new(|cx| DetailWindow::new(session, entries, start, window, cx)),
    ) else {
        return;
    };
    let _ = handle.update(cx, |_, window, _| {
        ui::window_chrome::hide_window_buttons(window);
        if let Some(ratio) = aspect_ratio(shape) {
            ui::window_chrome::lock_aspect_ratio(window, ratio);
        }
        window.activate_window();
    });
    cx.default_global::<Windows>().0.insert(label, handle);
}

pub struct DetailWindow {
    session: Entity<Session>,
    entries: Vec<Entry>,
    index: usize,
    /// Opened on a collection (`GroupDetailWindow`): its members in the
    /// reader, and the toolbar has only the pin and close buttons.
    group: bool,
    reader: Entity<GroupReader>,
    /// A member of the collection opened by itself with a double-click
    /// (`viewerIndex`): Escape goes back to the collection, the arrows
    /// step through its members.
    opened: Option<usize>,
    /// The shown entry's details, once they have arrived.
    details: Option<RootDetails>,
    loading: Option<Task<()>>,
    surface: Entity<MediaSurface>,
    focus: FocusHandle,
    on_top: bool,
    /// The toolbar's opacity, and when it hides unless the pointer moves.
    toolbar: motion::Fade,
    hide_at: Option<Instant>,
    boundary: Boundary,
    recorded: Option<RootId>,
    /// The viewer's context menu, and the actions of its rows.
    menu: ui::menu::Host,
    menu_actions: Vec<rows::Action>,
    _subscriptions: Vec<Subscription>,
}

impl DetailWindow {
    fn new(
        session: Entity<Session>,
        entries: Vec<Entry>,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let surface = cx.new(|cx| {
            let mut surface = MediaSurface::default();
            surface.set_options(options(cx, true, true), cx);
            surface
        });
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let subscriptions = vec![
            // The toolbar shows the zoom and whether an image is there.
            cx.observe(&surface, |_, _, cx| cx.notify()),
            cx.subscribe(
                &crate::settings::model(cx),
                |this, _, _: &crate::settings::Applied, cx| {
                    let options = options(cx, true, true);
                    this.surface
                        .update(cx, |surface, cx| surface.set_options(options, cx));
                },
            ),
            // Focus shows the toolbar for a moment; leaving hides it at once.
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    this.wake(cx);
                } else {
                    this.hide_at = None;
                    this.toolbar.set(false, motion::DETAIL_TOOLBAR_FADE);
                    cx.notify();
                }
            }),
        ];
        let group = entries.get(index).is_some_and(|entry| entry.collection);
        let reader = cx.new(|_| GroupReader::new(session.clone()));
        let opened = cx.subscribe(
            &reader,
            |this, _, event: &super::group::MemberOpened, cx| {
                this.opened = Some(event.0);
                this.boundary.clear();
                this.surface.update(cx, |surface, _| surface.refit());
                this.present(cx);
            },
        );
        let mut subscriptions = subscriptions;
        subscriptions.push(opened);
        let mut this = Self {
            session,
            entries,
            index,
            group,
            reader,
            opened: None,
            details: None,
            loading: None,
            surface,
            focus: focus.clone(),
            on_top: false,
            toolbar: motion::Fade::default(),
            hide_at: None,
            boundary: Boundary::default(),
            recorded: None,
            menu: ui::menu::Host::inline(ui::menu::Return::To(focus.clone())),
            menu_actions: Vec::new(),
            _subscriptions: subscriptions,
        };
        this.load(window, cx);
        this.wake(cx);
        this
    }

    fn entry(&self) -> Option<&Entry> {
        self.entries.get(self.index)
    }

    /// Shows the current entry: its thumbnail at once, its file once its
    /// details arrive. The previous one stays until the thumbnail decodes.
    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.entry().cloned() else {
            return;
        };
        window.set_window_title(&entry.title());
        if self.recorded != Some(entry.root) {
            self.recorded = Some(entry.root);
            record_view(self.session.read(cx), entry.root);
        }
        if !entry.collection {
            let thumbnail =
                crate::grid::thumbnails::existing(self.session.read(cx).thumbnail(&entry.hash));
            let natural = size_of(entry.width, entry.height);
            self.surface.update(cx, |surface, cx| {
                surface.show(entry.hash.clone(), thumbnail, natural, None, cx)
            });
        }
        let root = entry.root;
        let read = self
            .session
            .read(cx)
            .run(move |library| library.details(root));
        self.loading = Some(cx.spawn(async move |this, cx| {
            if let Ok(Ok(details)) = read.await {
                let _ = this.update(cx, |this, cx| {
                    if this.entry().is_some_and(|entry| entry.root == root) {
                        this.details = Some(details);
                        this.present(cx);
                    }
                });
            }
        }));
    }

    fn present(&mut self, cx: &mut Context<Self>) {
        if self.group && self.opened.is_none() {
            if let Some(details) = &self.details {
                self.reader
                    .update(cx, |reader, cx| reader.show(details, cx));
            }
            cx.notify();
            return;
        }
        let member = self.opened.unwrap_or_else(|| self.entry_member());
        let media = self
            .details
            .as_ref()
            .and_then(|details| Media::of(self.session.read(cx), details, member));
        if let Some(media) = media {
            self.surface
                .update(cx, |surface, cx| media.open(surface, cx));
        }
        cx.notify();
    }

    /// `navigate`: the previous or next entry, or the end's notice; with a
    /// member opened by itself, the previous or next member.
    fn step(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(opened) = self.opened {
            let count = self
                .details
                .as_ref()
                .map_or(0, |details| details.media.len());
            let target = opened as isize + delta;
            if target < 0 || target as usize >= count {
                let end = if target < 0 { End::First } else { End::Last };
                self.boundary.show(end, cx, |this| &mut this.boundary);
                return;
            }
            self.boundary.clear();
            self.opened = Some(target as usize);
            self.present(cx);
            return;
        }
        let target = self.index as isize + delta;
        if target < 0 || target as usize >= self.entries.len() {
            let end = if target < 0 { End::First } else { End::Last };
            self.boundary.show(end, cx, |this| &mut this.boundary);
            return;
        }
        self.boundary.clear();
        self.index = target as usize;
        self.load(window, cx);
        cx.notify();
    }

    /// `resetToolbarTimer`: shows the toolbar and hides it a second after
    /// the last call.
    fn wake(&mut self, cx: &mut Context<Self>) {
        self.toolbar.set(true, motion::DETAIL_TOOLBAR_FADE);
        let waiting = self.hide_at.is_some();
        self.hide_at = Some(Instant::now() + motion::DETAIL_TOOLBAR_HIDE);
        if !waiting {
            cx.spawn(async move |this, cx| {
                loop {
                    let Ok(wait) = this.update(cx, |this: &mut Self, cx| {
                        let at = this.hide_at?;
                        let left = at.saturating_duration_since(Instant::now());
                        if left.is_zero() {
                            this.hide_at = None;
                            this.toolbar.set(false, motion::DETAIL_TOOLBAR_FADE);
                            cx.notify();
                            return None;
                        }
                        Some(left)
                    }) else {
                        break;
                    };
                    match wait {
                        Some(left) => cx.background_executor().timer(left).await,
                        None => break,
                    }
                }
            })
            .detach();
        }
        cx.notify();
    }

    fn toggle_on_top(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !ui::window_chrome::can_stay_on_top() {
            return;
        }
        self.on_top = !self.on_top;
        ui::window_chrome::set_always_on_top(window, self.on_top);
        cx.notify();
    }

    fn copy_path(&mut self, cx: &mut Context<Self>) {
        let path = self
            .details
            .as_ref()
            .and_then(|details| details.media.first())
            .map(|media| media.file_path.clone());
        if let Some(path) = path {
            cx.write_to_clipboard(ClipboardItem::new_string(path));
        }
    }

    /// `edit.copy` and the menu's Copy (`copyFileForHash`): the shown file
    /// itself on the clipboard.
    fn copy_file(&mut self, cx: &mut Context<Self>) {
        if let Some((path, ..)) = self.shown_file()
            && let Err(error) = crate::desktop::copy_files(&[path.into()], cx)
        {
            crate::ui::notify::error("Could not copy file", error, cx);
        }
    }

    /// The file of its item the entry names: a collection member opened in
    /// a window of its own is its collection's file of that hash; any
    /// other item shows its first.
    fn entry_member(&self) -> usize {
        let hash = self.entry().map(|entry| entry.hash.as_str());
        self.details
            .as_ref()
            .and_then(|details| {
                details
                    .media
                    .iter()
                    .position(|media| Some(media.facts.content_hash.as_str()) == hash)
            })
            .unwrap_or(0)
    }

    /// The shown file: its path, content hash, type and name.
    fn shown_file(&self) -> Option<(String, String, String, String)> {
        let entry = self.entry()?;
        let media = self
            .details
            .as_ref()?
            .media
            .get(self.opened.unwrap_or_else(|| self.entry_member()))?;
        Some((
            media.file_path.clone(),
            media.facts.content_hash.clone(),
            media.facts.mime.clone(),
            entry.name.clone(),
        ))
    }

    /// `useViewerEntityContextMenu` as the detail window calls it: on the
    /// file alone, with no item to edit, so only the rows that open and
    /// copy the file, search by it and set it as the library cover.
    fn open_menu(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, hash, mime, name)) = self.shown_file() else {
            return;
        };
        let description = rows::Description {
            surface: rows::Surface::Viewer,
            ai_tag_enabled: Some(false),
            single: true,
            single_hash: Some(hash),
            single_kind: Some(rows::Kind::Media),
            single_name: Some(name),
            single_mime: Some(mime),
            handlers: rows::Handlers {
                open_default: true,
                reveal: true,
                open_new_window: true,
                copy_file: true,
                copy_path: true,
                copy_name: true,
                copy_link: true,
                search_by_image: true,
                regenerate: true,
                library_cover: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let playback = rows::Playback {
            flash: self.surface.read(cx).flash_playback(),
            capture: self.surface.read(cx).can_capture(cx),
        };
        let hints = |id: &str| crate::keymap::hint(id, cx).map(String::from);
        let built = rows::viewer_menu(&description, playback, &hints);
        self.menu_actions = rows::actions(&built);
        // The viewer's rows hold no view panels.
        let entries = rows::entries(built, &mut |_| ui::menu::Entry::Separator);
        let menu = cx.new(|cx| ui::menu::Menu::new(entries, position, cx).search(false));
        self.menu
            .open(menu, window, cx, |this, _, key, window, cx| {
                let action = this
                    .menu_actions
                    .iter()
                    .copied()
                    .find(|action| action.key() == key);
                if let Some(action) = action {
                    this.run_menu_action(action, window, cx);
                }
            });
    }

    fn run_menu_action(
        &mut self,
        action: rows::Action,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use rows::Action;
        let Some((path, hash, mime, name)) = self.shown_file() else {
            return;
        };
        match action {
            Action::OpenDefault => crate::desktop::open_default(std::path::Path::new(&path), cx),
            Action::Reveal => crate::desktop::reveal(std::path::Path::new(&path), cx),
            Action::OpenInNewWindow => {
                // One window per file: this one, brought forward.
                if let Some(entry) = self.entry().cloned() {
                    open(self.session.clone(), vec![entry], 0, cx);
                }
            }
            Action::CopyFile => self.copy_file(cx),
            Action::CopyPath => cx.write_to_clipboard(ClipboardItem::new_string(path)),
            Action::CopyName => cx.write_to_clipboard(ClipboardItem::new_string(name)),
            Action::CopyLink => cx.write_to_clipboard(ClipboardItem::new_string(
                crate::shell::media_link(&hash, &mime),
            )),
            Action::FlashPlay | Action::FlashStop | Action::FlashMute => {
                use super::flash::Flash;
                let control: Box<dyn Fn(&mut Flash)> = match action {
                    Action::FlashPlay => Box::new(Flash::toggle_play),
                    Action::FlashStop => Box::new(Flash::stop),
                    _ => Box::new(Flash::toggle_mute),
                };
                self.surface
                    .update(cx, |surface, cx| surface.control_flash(control, cx));
            }
            Action::RegenerateThumbnails => {
                crate::grid::thumbnails::regenerate(&self.session, vec![(hash, mime)], cx);
            }
            Action::SetAsThumbnail => {
                super::capture::set_as_thumbnail(&self.surface, &self.session, hash, cx);
            }
            Action::SetLibraryCover => {
                // Production's detail window has no dialog layer, so its row
                // does nothing there; here the main window's dialog opens
                // on the file, framed at once.
                let Some(entry) = self.entry() else {
                    return;
                };
                let candidate = crate::screens::subscriptions::cover_dialog::Candidate {
                    media_item: i64::from(entry.root.0),
                    size: (entry.width.unwrap_or(1), entry.height.unwrap_or(1)),
                    thumbnail: crate::grid::thumbnails::present(
                        &self.session.read(cx).thumbnail(&hash),
                    ),
                    hash,
                };
                if let Some(main) = crate::app_menu::main_window(cx) {
                    let _ = main.update(cx, move |shell, window, cx| {
                        shell.open_library_cover(Some(candidate), cx);
                        window.activate_window();
                    });
                }
            }
            _ => {
                // TODO: Search by Image, which uploads the file to the
                // engine from a hidden browser window (`runReverseImageSearch`).
            }
        }
    }

    /// `.toolbar`: the title and count on the left; the zoom, actual size,
    /// fit, pin and close on the right, over a dark strip that moves the
    /// window.
    fn toolbar(&self, window: &mut Window, cx: &mut Context<Self>) -> ui::GlassPanel {
        let theme = theme::theme(cx);
        let title = if self.group {
            // `GroupDetailWindow`: the collection's name.
            self.details
                .as_ref()
                .map(|details| details.root.name.clone())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "Group".to_owned())
        } else {
            self.entry().map(Entry::title).unwrap_or_default()
        };
        let mut left = div().flex().items_center().gap(px(8.)).min_w_0().child(
            div()
                .min_w_0()
                .max_w(px(TITLE_WIDTH))
                .truncate()
                .text(theme::TEXT_MD)
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text())
                .child(title),
        );
        if !self.group && self.entries.len() > 1 {
            left = left.child(
                div()
                    .whitespace_nowrap()
                    .text(theme::TEXT_SM)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_tertiary())
                    .child(format!("{} / {}", self.index + 1, self.entries.len())),
            );
        }
        let mut right = div().flex().items_center().gap(px(4.)).flex_none();
        let (zoom, percent) = {
            let surface = self.surface.read(cx);
            (surface.shows_zoom(), surface.percent())
        };
        if !self.group && zoom {
            right = right
                .child(
                    div()
                        .min_w(px(ZOOM_WIDTH))
                        .flex()
                        .justify_end()
                        .text(theme::TEXT_SM)
                        .font_weight(FontWeight::MEDIUM)
                        .font_features(theme::tabular_figures())
                        .text_color(theme.text_secondary())
                        .child(format!("{percent}%")),
                )
                .child(
                    ui::icon_button("detail-actual-size", "toolbar-actual-size")
                        .tip(ui::tip("Actual size").shortcut("view.actualSize", cx))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.surface
                                .update(cx, |surface, cx| surface.actual_size(cx));
                        })),
                )
                .child(
                    ui::icon_button("detail-fit", "toolbar-fit")
                        .tip(ui::tip("Fit to window").shortcut("view.fitWindow", cx))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.surface.update(cx, |surface, cx| surface.fit(cx));
                        })),
                );
        }
        right = right
            // Not offered where the window cannot be kept on top (Linux).
            .children(ui::window_chrome::can_stay_on_top().then(|| {
                // `.icBtnActive` composes `iconBtnFilled`: pinned shows the
                // filled glyph, with no background of its own.
                ui::icon_button("detail-pin", if self.on_top { "pin-filled" } else { "pin" })
                    .tip(
                        ui::tip(if self.on_top {
                            "Unpin"
                        } else {
                            "Always on top"
                        })
                        .shortcut("view.alwaysOnTop", cx),
                    )
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_on_top(window, cx)))
            }))
            .child(tooltip::attach(
                div()
                    .id("detail-close-tip")
                    .child(ui::window_chrome::close_button(
                        "detail-close",
                        |_, window, _| window.remove_window(),
                    )),
                "detail-close-tooltip",
                ui::tip("Close").shortcut("view.closeDetail", cx),
                theme::CONTROL_MD,
                window,
                cx,
            ));
        ui::GlassPanel::new(ui::drag_region("detail-toolbar", window, cx), 0.)
            .backdrop_blur(theme::detail_toolbar_blur())
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(TOOLBAR_HEIGHT))
            .px(px(TOOLBAR_PADDING))
            .flex()
            .items_center()
            .justify_between()
            .bg(rgba(theme::DETAIL_TOOLBAR_BACKGROUND))
            .opacity(self.toolbar.value_with(motion::EASE))
            .child(left)
            .child(right)
    }
}

impl Render for DetailWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        if self.boundary.animating() || self.toolbar.animating() {
            window.request_animation_frame();
        }
        let mut pane = div()
            .id("detail-window")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .overflow_hidden()
            .on_action(cx.listener(|this, _: &PreviousItem, window, cx| this.step(-1, window, cx)))
            .on_action(cx.listener(|this, _: &NextItem, window, cx| this.step(1, window, cx)))
            .on_action(cx.listener(|this, _: &FitToWindow, _, cx| {
                this.surface.update(cx, |surface, cx| surface.fit(cx));
            }))
            .on_action(cx.listener(|this, _: &ActualSize, _, cx| {
                this.surface
                    .update(cx, |surface, cx| surface.actual_size(cx));
            }))
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| {
                this.surface
                    .update(cx, |surface, cx| surface.step_zoom(true, cx));
            }))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| {
                this.surface
                    .update(cx, |surface, cx| surface.step_zoom(false, cx));
            }))
            .map(|pane| super::document::handle_actions(pane, &self.surface))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    // TODO: a collection's members have the tile menu on the
                    // collection (`openMemberMenu`), and the space between
                    // them the collection's (`openEmptyMenu`).
                    if !this.group || this.opened.is_some() {
                        this.open_menu(event.position, window, cx);
                    }
                }),
            )
            // Escape closes a member opened by itself before the window.
            .on_action(cx.listener(|this, _: &CloseWindow, _, cx| {
                if this.opened.take().is_some() {
                    this.boundary.clear();
                    this.present(cx);
                } else {
                    cx.propagate();
                }
            }))
            // TODO: the per-image zoom memory (`zoomCache`).
            .child(if self.group && self.opened.is_none() {
                self.reader.clone().into_any_element()
            } else {
                self.surface.clone().into_any_element()
            })
            .child(self.boundary.notice(End::First, cx))
            .child(self.boundary.notice(End::Last, cx));
        // Hidden, the toolbar takes no pointer (`pointer-events: none`).
        if self.toolbar.on() || self.toolbar.animating() {
            pane = pane.child(self.toolbar(window, cx));
        }
        let media = self.surface.read(cx).player().is_some();
        let surface = self.surface.downgrade();
        let mut context = super::video::key_context("DetailWindow", media);
        // A document's pages take the scrolling keys.
        if self.surface.read(cx).shows_pages() {
            context.add(super::document::PAGES_CONTEXT);
        }
        let window_root = div().key_context(context);
        super::video::on_actions(window_root, move |cx| {
            surface
                .upgrade()
                .and_then(|surface| surface.read(cx).player())
        })
        .size_full()
        .bg(theme.background())
        .text_color(theme.text())
        .font_family(theme::FONT_UI)
        .text(theme::TEXT_MD)
        .line_height(px(theme::LINE_HEIGHT))
        .on_mouse_move(cx.listener(|this, _: &MouseMoveEvent, _, cx| this.wake(cx)))
        .on_action(cx.listener(|_, _: &CloseWindow, window, _| window.remove_window()))
        .on_action(
            cx.listener(|this, _: &ToggleAlwaysOnTop, window, cx| this.toggle_on_top(window, cx)),
        )
        .on_action(cx.listener(|this, _: &crate::keymap::CopyFilePath, _, cx| this.copy_path(cx)))
        .on_action(cx.listener(|this, _: &crate::keymap::CopyFiles, _, cx| this.copy_file(cx)))
        .relative()
        .child(pane)
        .children(ui::window_chrome::resize_edges(window))
        .children(self.menu.layer(window, cx))
        // A video player that has the window to itself, over everything.
        .children(super::video::fullscreen_layer(window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::{aspect_ratio, window_size};

    const WORK_AREA: (f32, f32) = (1512., 944.);

    #[test]
    fn a_window_takes_the_media_shape_within_85_percent() {
        // 85% of the work area is 1285 × 802.
        assert_eq!(window_size(WORK_AREA, None), (1285., 802.));
        // Wide media: limited by the width.
        assert_eq!(window_size(WORK_AREA, Some((4000, 1000))), (1285., 321.));
        // Tall media: limited by the height.
        assert_eq!(window_size(WORK_AREA, Some((1000, 2000))), (401., 802.));
        // Unknown or empty dimensions: the whole 85%.
        assert_eq!(window_size(WORK_AREA, Some((0, 100))), (1285., 802.));
    }

    #[test]
    fn a_window_is_grown_to_at_least_400_by_300() {
        // Very wide media: 1285 × 129 grows until it is 300 tall.
        assert_eq!(window_size(WORK_AREA, Some((10000, 1000))), (2988., 300.));
        // Very tall media: 80 × 802 grows five times, to 400 wide.
        assert_eq!(window_size(WORK_AREA, Some((100, 1000))), (400., 4010.));
    }

    #[test]
    fn the_aspect_ratio_is_kept_only_when_known() {
        assert_eq!(aspect_ratio(Some((1600, 900))), Some(1600. / 900.));
        assert_eq!(aspect_ratio(Some((0, 900))), None);
        assert_eq!(aspect_ratio(None), None);
    }
}
