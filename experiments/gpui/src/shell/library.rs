//! The library switcher's popover (`LibrarySwitcherPopover.tsx`): a search
//! header, the libraries the app knows with their covers, names and folders,
//! the open one checked, and the library actions at the foot.
//!
//! The list is the Electron app's own: `config.json` in its application-data
//! folder, which it keeps as `libraryHistory` and `pinnedLibraries`. This
//! view only reads that file and the covers beside each library; it writes
//! the cropped covers into the session's scratch folder and nowhere else.
use crate::grid::thumbnails::Thumbnails;
use crate::i18n::t;
use crate::session::Session;
use crate::theme::Text as _;
use crate::ui::popover::{self, Opening, Placement, Side};
use crate::ui::text_input::{Kind, TextInput};
use crate::{theme, ui};
use gpui::*;
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// `LibrarySwitcherPopover`'s placement: 8px under the button, kept 8px
/// inside the window.
const PLACEMENT: Placement = Placement::new(Side::Below, 8., 8.);
/// 300px wide, and placed as if 422px tall, so its top is never lower than
/// 430px above the window's bottom (`viewport.height - 430`).
const SIZE: Size<f32> = Size {
    width: 300.,
    height: 422.,
};
const MAX_HEIGHT: f32 = 420.;
/// `.list`'s padding, `.item`'s padding and the avatar's size and gap.
const LIST_PADDING: f32 = 8.;
const ROW_PADDING: f32 = 8.;
const AVATAR: f32 = 36.;
const GAP: f32 = 8.;
const CHECK: f32 = 14.;

/// A library the app has opened before.
#[derive(Clone, Debug, PartialEq)]
pub struct Known {
    pub path: String,
    pub name: String,
    /// The folders above it, joined with `/` as the Electron row shows them.
    pub dir: String,
    pub current: bool,
    pub cover: Option<PathBuf>,
}

/// A library's name and the folders above it, as the Electron row splits
/// its path: on either separator, empty parts dropped, `.library` removed.
pub fn split(path: &str) -> (String, String) {
    let mut parts = path
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let base = parts.pop().unwrap_or(path);
    let name = base.strip_suffix(".library").unwrap_or(base);
    (name.to_owned(), parts.join("/"))
}

/// The rows in the Electron order: pinned libraries, then the history
/// without them. A library in the history whose folder is gone is left out.
/// The open library heads the list when the history does not have it, as
/// opening a library puts it there.
pub fn order(
    pinned: &[String],
    history: &[String],
    exists: impl Fn(&str) -> bool,
    is_current: impl Fn(&str) -> bool,
    current: Option<&str>,
) -> Vec<Known> {
    let mut paths = pinned.to_vec();
    paths.extend(
        history
            .iter()
            .filter(|path| !pinned.contains(path))
            .cloned(),
    );
    let mut rows = paths
        .into_iter()
        .filter(|path| !history.contains(path) || exists(path))
        .map(|path| {
            let (name, dir) = split(&path);
            Known {
                current: is_current(&path),
                path,
                name,
                dir,
                cover: None,
            }
        })
        .collect::<Vec<_>>();
    if let Some(current) = current
        && !rows.iter().any(|row| row.current)
    {
        let (name, dir) = split(current);
        rows.insert(
            0,
            Known {
                path: current.to_owned(),
                name,
                dir,
                current: true,
                cover: None,
            },
        );
    }
    rows
}

/// The widths the row's flex layout gives the name and the folder: the
/// name keeps its own width up to 70% of the room, less the gap, since the
/// folder (`flex: 1`) never goes below 30%; the folder takes the rest.
pub fn info_widths(name: f32, available: f32) -> (f32, f32) {
    let name = name.min(0.7 * available - GAP).max(0.);
    (name, (available - GAP - name).max(0.))
}

/// Where the Electron app keeps `config.json` (`app.getPath('appData')`
/// joined with `picto`).
fn config_path() -> Option<PathBuf> {
    Some(
        crate::state::config_folder()?
            .join("picto")
            .join("config.json"),
    )
}

/// Reads the known libraries and prepares their covers. Blocking: run it
/// off the thread that draws.
fn read(current: Option<PathBuf>, current_cover: Option<PathBuf>, scratch: PathBuf) -> Vec<Known> {
    let config = config_path()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    let list = |key: &str| {
        config
            .as_ref()
            .and_then(|config| config.get(key))
            .and_then(serde_json::Value::as_array)
            .map(|paths| {
                paths
                    .iter()
                    .filter_map(|path| path.as_str().map(str::to_owned))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let canonical = |path: &Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    let current_canonical = current.as_deref().map(canonical);
    let is_current = |path: &str| {
        current_canonical
            .as_ref()
            .is_some_and(|current| canonical(Path::new(path)) == *current)
    };
    let current_text = current
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned());
    let mut rows = order(
        &list("pinnedLibraries"),
        &list("libraryHistory"),
        |path| Path::new(path).exists(),
        is_current,
        current_text.as_deref(),
    );
    for row in &mut rows {
        row.cover = if row.current {
            current_cover.clone()
        } else {
            // Each library's cover is cropped once per session, into a file
            // named after its path.
            let mut hasher = std::hash::DefaultHasher::new();
            std::hash::Hash::hash(&row.path, &mut hasher);
            let target = scratch.join(format!(
                "known-library-cover-{:016x}.png",
                std::hash::Hasher::finish(&hasher)
            ));
            if target.is_file() {
                Some(target)
            } else {
                crate::cover::prepare_into(Path::new(&row.path), &target)
            }
        };
    }
    rows
}

/// The popover asks its owner to repaint the switcher's open state.
pub struct Toggled;

pub struct LibraryPopover {
    session: Entity<Session>,
    thumbnails: Entity<Thumbnails>,
    search: Entity<TextInput>,
    focus: FocusHandle,
    /// The switcher button's bounds as last painted.
    control: Rc<Cell<Bounds<Pixels>>>,
    opening: Option<Opening>,
    libraries: Vec<Known>,
    /// Counts reads, so an older one never replaces a newer one.
    reads: u64,
    /// The row under the pointer, whose avatar takes the hover ring.
    hovered: Option<usize>,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Toggled> for LibraryPopover {}

impl LibraryPopover {
    pub fn new(
        session: Entity<Session>,
        thumbnails: Entity<Thumbnails>,
        control: Rc<Cell<Bounds<Pixels>>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let search =
            cx.new(|cx| TextInput::new(Kind::Popover, cx).placeholder("Search libraries…"));
        let subscriptions = vec![cx.observe(&search, |_, _, cx| cx.notify())];
        let mut this = Self {
            session,
            thumbnails,
            search,
            focus: cx.focus_handle(),
            control,
            opening: None,
            libraries: Vec::new(),
            reads: 0,
            hovered: None,
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        };
        // Read once now, so the first opening already has its rows.
        this.read(cx);
        this
    }

    pub fn is_open(&self) -> bool {
        self.opening.is_some()
    }

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_open() {
            self.close(window, cx);
        } else {
            self.open(window, cx);
        }
    }

    fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.opening = Some(Opening::new(window, cx));
        self.hovered = None;
        self.search.update(cx, |search, cx| search.clear(cx));
        self.scroll.set_offset(point(px(0.), px(0.)));
        // `autoFocus` on the search field.
        let field = self.search.focus_handle(cx);
        window.focus(&field, cx);
        // The rows read before stay until this read replaces them.
        self.read(cx);
        cx.emit(Toggled);
        cx.notify();
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(opening) = self.opening.take() {
            opening.finish(&self.focus, window, cx);
            cx.emit(Toggled);
            cx.notify();
        }
    }

    fn read(&mut self, cx: &mut Context<Self>) {
        self.reads += 1;
        let reads = self.reads;
        let (current, cover, scratch) = {
            let session = self.session.read(cx);
            (
                session.library_root().map(Path::to_path_buf),
                session.library_cover.clone(),
                session.scratch().to_path_buf(),
            )
        };
        let task = cx
            .background_executor()
            .spawn(async move { read(current, cover, scratch) });
        cx.spawn(async move |this, cx| {
            let libraries = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.reads != reads {
                    return;
                }
                // Start decoding the other libraries' covers now, so they
                // are there when the rows first show. The open library's
                // cover is already decoded for the switcher.
                for library in libraries.iter().filter(|library| !library.current) {
                    if let Some(cover) = &library.cover {
                        cx.fetch_asset::<ImgResourceLoader>(&Resource::Path(cover.clone().into()));
                    }
                }
                this.libraries = libraries;
                cx.notify();
            });
        })
        .detach();
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let field = self.search.focus_handle(cx);
        div()
            .relative()
            .h(px(48.))
            .flex_none()
            .px(px(12.))
            .flex()
            .items_center()
            .border_b_1()
            .border_color(theme.border_secondary())
            .child(
                div()
                    .absolute()
                    .left(px(17.))
                    .top_0()
                    .bottom_0()
                    .flex()
                    .items_center()
                    .child(ui::icon("library-search", 16., theme.text_tertiary())),
            )
            .child(
                // `width: calc(100% - 24px)` beside the 24px close button.
                div()
                    .h(px(48.))
                    .flex_1()
                    .min_w_0()
                    .pl(px(28.))
                    .pr(px(10.))
                    .cursor(CursorStyle::IBeam)
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        window.focus(&field, cx)
                    })
                    .child(self.search.clone()),
            )
            // `.closeButton` is a `<button>`.
            .child(ui::focus::stop(
                "close-library-switcher",
                div()
                    .id("close-library-switcher")
                    .size(px(24.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(theme::RADIUS_SM))
                    .cursor_pointer()
                    .group("close-library-switcher")
                    .hover(move |style| style.bg(theme.surface_hover()))
                    .child(
                        ui::icon("x", 14., theme.text_tertiary())
                            .group_hover("close-library-switcher", move |style| {
                                style.text_color(theme.text())
                            }),
                    )
                    .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
                cx.listener(|this, _, window, cx| this.close(window, cx)),
            ))
    }

    /// A known library chosen: the popover closes.
    fn pick(&mut self, current: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !current {
            // TODO: switch the app to this library, as
            // `picto.library.switch` does: close this library's
            // session and open that one's database and blobs.
        }
        self.close(window, cx);
    }

    /// One known library (`.item`).
    fn row(
        &self,
        index: usize,
        library: &Known,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let measure = |text: &str, size: f32, window: &mut Window| {
            ui::text_width(text, font(theme::FONT_UI), size, window)
        };
        // The name and folder share what the avatar and check leave.
        let available = width
            - 2. * LIST_PADDING
            - ui::scrollbar::GUTTER
            - 2. * ROW_PADDING
            - AVATAR
            - GAP
            - if library.current { GAP + CHECK } else { 0. };
        let name_width = measure(&library.name, theme::TEXT_MD, window);
        let dir_width = measure(&library.dir, theme::TEXT_SM, window);
        let (name_room, dir_room) = info_widths(name_width, available);
        // Only text that overflows is given the ellipsis, so neither is
        // measured wider than it is.
        let cut = |label: Div, room: f32, natural: f32| {
            let label = label.w(px(room)).whitespace_nowrap();
            if natural > room {
                label.truncate()
            } else {
                label
            }
        };
        let current = library.current;
        let row = div()
            .id(("known-library", index))
            .w_full()
            .min_h(px(48.))
            .px(px(ROW_PADDING))
            .flex()
            .items_center()
            .gap(px(GAP))
            .rounded(px(theme::RADIUS_MD))
            .text_color(theme.text())
            .cursor_pointer()
            .hover(move |style| style.bg(theme.surface_hover()))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.hovered = Some(index);
                } else if this.hovered == Some(index) {
                    this.hovered = None;
                }
                cx.notify();
            }))
            .on_click(cx.listener(move |this, _, window, cx| this.pick(current, window, cx)))
            .child({
                let avatar = ui::library_avatar(
                    ("known-library-avatar", index),
                    library.cover.clone(),
                    AVATAR,
                )
                .highlighted(current)
                .hovered(self.hovered == Some(index));
                if current {
                    avatar.cache(&self.thumbnails)
                } else {
                    avatar
                }
            })
            .child(
                // `.itemInfo`: name and folder on one baseline. The 13px
                // name's baseline is a pixel lower in its line than the 12px
                // folder's.
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_start()
                    .gap(px(GAP))
                    .child(cut(
                        div().text(theme::TEXT_MD).child(library.name.clone()),
                        name_room,
                        name_width,
                    ))
                    .child(cut(
                        div()
                            .mt(px(1.))
                            .text(theme::TEXT_SM)
                            .text_color(theme.text_tertiary())
                            .child(library.dir.clone()),
                        dir_room,
                        dir_width,
                    )),
            )
            .children(current.then(|| {
                div()
                    .flex()
                    .flex_none()
                    .child(ui::icon("check", CHECK, theme.text()))
            }));
        // `.item` is a `<button>`: Enter and Space choose it.
        let press = cx.listener(move |this, _, window, cx| this.pick(current, window, cx));
        ui::focus::button(
            row,
            &ElementId::from(("known-library", index)),
            window,
            cx,
            press,
        )
    }

    /// The actions at the foot (`.functionItem`).
    fn action(
        &self,
        id: &'static str,
        icon: &'static str,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        div()
            .id(id)
            .w_full()
            .h(px(theme::CONTROL_LG))
            .flex_none()
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(theme::RADIUS_MD))
            .text_color(theme.text_tertiary())
            .cursor_pointer()
            .group(id)
            .hover(move |style| style.bg(theme.surface_hover()).text_color(theme.text()))
            .child(
                ui::icon(icon, 14., theme.text_tertiary())
                    .group_hover(id, move |style| style.text_color(theme.text())),
            )
            .child(label)
    }
}

impl Render for LibraryPopover {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(opening) = &self.opening else {
            return div().into_any_element();
        };
        let theme = theme::theme(cx);
        let width = popover::width(SIZE.width, PLACEMENT.margin, window);
        let query = self.search.read(cx).value();
        let names = self
            .libraries
            .iter()
            .map(|library| library.name.as_str())
            .collect::<Vec<_>>();
        let shown = crate::text::rank_matches(
            names.into_iter().enumerate().collect(),
            &query,
            |(_, name)| name,
        )
        .into_iter()
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
        let mut rows = div().flex().flex_col();
        if shown.is_empty() {
            rows = rows.child(
                // `.empty`: 12px on the popover's 1.55 line (18.6px).
                div()
                    .p(px(16.))
                    .text(theme::TEXT_SM)
                    .line_height(px(18.6))
                    .text_color(theme.text_tertiary())
                    .flex()
                    .justify_center()
                    .child(t("No Result")),
            );
        }
        for index in shown {
            let library = &self.libraries[index];
            rows = rows.child(self.row(index, library, width, window, cx));
        }
        let list = div()
            .relative()
            .flex_auto()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("known-libraries")
                    .flex_auto()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .pt(px(LIST_PADDING))
                    .pb(px(LIST_PADDING))
                    .pl(px(LIST_PADDING))
                    // `scrollbar-gutter: stable` keeps the bar's room.
                    .pr(px(LIST_PADDING + ui::scrollbar::GUTTER))
                    .child(rows),
            )
            .child(ui::scrollbar(
                "known-libraries-scrollbar",
                &self.scroll,
                window,
                cx,
            ));
        let functions = div()
            .flex_none()
            .p(px(8.))
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(theme.border_secondary())
            .bg(theme.control_surface_half())
            // The content clips to a rectangle; the tint rounds its own
            // corners to the panel's.
            .rounded_b(px(theme::RADIUS_LG))
            .child(
                // TODO: open the Library Manager on its form for a new
                // library (`open_library_manager`).
                ui::focus::stop(
                    "create-library",
                    self.action("create-library", "plus", t("Create Library…"), cx)
                        .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
                    cx.listener(|this, _, window, cx| this.close(window, cx)),
                ),
            )
            .child(
                // TODO: choose a library folder and switch to it, as
                // `picto.library.open` does.
                ui::focus::stop(
                    "open-library",
                    self.action(
                        "open-library",
                        "library-folder-open",
                        t("Open Library…"),
                        cx,
                    )
                    .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
                    cx.listener(|this, _, window, cx| this.close(window, cx)),
                ),
            )
            .child(
                // TODO: open the Library Manager (`open_library_manager`).
                ui::focus::stop(
                    "library-manager",
                    self.action("library-manager", "adjustments", t("Library Manager…"), cx)
                        .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
                    cx.listener(|this, _, window, cx| this.close(window, cx)),
                ),
            );
        let this = cx.entity().downgrade();
        popover::popover(
            "library-switcher-popover",
            opening,
            &self.focus,
            self.control.get(),
            PLACEMENT,
            SIZE,
            MAX_HEIGHT,
            move |window, cx| {
                let _ = this.update(cx, |this, cx| this.close(window, cx));
            },
        )
        .child(self.header(cx))
        .child(list)
        .child(functions)
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{info_widths, order, split};

    #[test]
    fn a_path_splits_into_name_and_folders() {
        assert_eq!(
            split("/Users/someone/Pictures/Main.library"),
            ("Main".into(), "Users/someone/Pictures".into())
        );
        assert_eq!(
            split("C:\\Media\\Art.library\\"),
            ("Art".into(), "C:/Media".into())
        );
        assert_eq!(split("/Loose"), ("Loose".into(), String::new()));
    }

    #[test]
    fn pinned_libraries_come_first_and_missing_ones_drop_out() {
        let pinned = vec!["/b/Pinned.library".to_owned(), "/a/One.library".to_owned()];
        let history = vec![
            "/a/One.library".to_owned(),
            "/a/Gone.library".to_owned(),
            "/a/Two.library".to_owned(),
        ];
        let rows = order(
            &pinned,
            &history,
            |path| !path.contains("Gone"),
            |path| path == "/a/Two.library",
            Some("/a/Two.library"),
        );
        let names = rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>();
        // A pinned library not in the history is not checked for existence.
        assert_eq!(names, ["Pinned", "One", "Two"]);
        assert_eq!(
            rows.iter().map(|row| row.current).collect::<Vec<_>>(),
            [false, false, true]
        );
    }

    #[test]
    fn the_open_library_is_listed_when_the_history_lacks_it() {
        let rows = order(&[], &[], |_| true, |_| false, Some("/x/Main.library"));
        assert_eq!(rows.len(), 1);
        assert!(rows[0].current);
        assert_eq!((rows[0].name.as_str(), rows[0].dir.as_str()), ("Main", "x"));
        assert!(order(&[], &[], |_| true, |_| false, None).is_empty());
    }

    #[test]
    fn the_name_keeps_its_width_until_the_folder_reaches_thirty_percent() {
        // Short name: its own width, the folder takes the rest.
        assert_eq!(info_widths(40., 200.), (40., 152.));
        // Long name: stops where the folder would go under 30%.
        assert_eq!(info_widths(500., 200.), (132., 60.));
    }
}
