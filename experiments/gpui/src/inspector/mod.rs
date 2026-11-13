//! The inspector rail's scrolling body (`Inspector.tsx`): the destination
//! showing when nothing is selected, the selected item, or a summary of
//! several. Layout follows InspectorSkeleton, InspectorField, TagChip and
//! PropertyRow.
mod scope;
mod selection;
mod stack;

use crate::grid::{format, thumbnails::Thumbnails};
use crate::session::{Change, Session};
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{
    self,
    field::Look,
    text_input::{self, TextInput},
};
use gpui::*;
use picto_library::predicate::FilterClause;
use picto_library::query::ItemScope;
use picto_library::{RootDetails, RootId};
use std::collections::HashSet;

/// What the rail describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// A destination that is a screen of its own: nothing.
    Empty,
    /// Nothing selected: the destination showing.
    Scope,
    Item,
    /// Several items selected.
    Selection,
}

/// What the rail should describe (`liveInspectorTargetAtom`): an item open in
/// the viewer, else what the grid has selected, else the grid's destination.
fn wanted(screen: bool, viewer: bool, selected: usize) -> Mode {
    if screen {
        Mode::Empty
    } else if viewer || selected == 1 {
        Mode::Item
    } else if selected > 1 {
        Mode::Selection
    } else {
        Mode::Scope
    }
}

/// Where a click on a star applies its rating.
#[derive(Clone, Copy)]
enum Rate {
    Item(RootId),
    Selection,
}

/// A chip's colour and sort order for a tag namespace (`NS_COLORS` in
/// `TagChip.tsx`, `tagGroupOrder`). General tags come last.
fn tag_style(namespace: &str) -> (u32, u8) {
    match namespace {
        "creator" => (0xff6667, 0),
        "studio" => (0xff6667, 10),
        "series" => (0xc499ff, 1),
        "character" => (0x30d159, 2),
        "person" => (0x30d159, 10),
        "species" => (0x00aaff, 3),
        "rating" => (0xffd60a, 4),
        "system" => (0xff9f0a, 10),
        "" | "general" => (0xbdbec0, 99),
        _ => (0xbdbec0, 10),
    }
}

/// The `showTagPrefixes` setting, at its default: a chip shows the tag
/// without its namespace.
const SHOW_TAG_PREFIXES: bool = false;
/// A folder chip without a colour of its own.
const FOLDER_COLOR: u32 = 0x868e96;
/// The preview fits this square.
const PREVIEW: f32 = 200.;

/// The name a folder chip shows: the folder's sidebar label, or its number
/// when the sidebar does not list it.
fn folder_label(entries: &[crate::backend::Navigation], folder: picto_library::FolderId) -> String {
    entries
        .iter()
        .find(|entry| {
            matches!(entry.scope, ItemScope::FolderTree { folder_id } if folder_id == folder)
        })
        .map_or_else(|| format!("Folder {}", folder.0), |entry| entry.label.clone())
}

fn is_general(tag: &picto_library::TagRecord) -> bool {
    matches!(tag.namespace.as_str(), "" | "general" | "default")
}

/// A tag's full name (`namespace:subtag`, or the subtag alone for a general
/// tag), as the library adds and removes it by.
fn tag_raw(tag: &picto_library::TagRecord) -> String {
    if is_general(tag) {
        tag.subname.clone()
    } else {
        format!("{}:{}", tag.namespace, tag.subname)
    }
}

/// A tag's chip in its namespace's colour (`TagChip`).
fn tag_chip(tag: &picto_library::TagRecord) -> ui::Chip {
    ui::chip(
        ("inspector-tag", tag.tag_id.0 as usize),
        tag.subname.clone(),
        tag_style(&tag.namespace).0,
    )
    .namespace((SHOW_TAG_PREFIXES && !is_general(tag)).then(|| tag.namespace.clone().into()))
}

/// A preview's size: the media scaled down to fit the square, never up.
fn fitted(dimensions: Option<(u32, u32)>) -> (f32, f32) {
    match dimensions {
        Some((width, height)) if width > 0 && height > 0 => {
            let scale = (PREVIEW / width as f32)
                .min(PREVIEW / height as f32)
                .min(1.);
            (width as f32 * scale, height as f32 * scale)
        }
        _ => (PREVIEW, PREVIEW),
    }
}

fn format_size(bytes: u64) -> String {
    let bytes = bytes as f64;
    if bytes < 1024. {
        format!("{bytes} B")
    } else if bytes < 1048576. {
        format!("{:.1} KB", bytes / 1024.)
    } else if bytes < 1073741824. {
        format!("{:.1} MB", bytes / 1048576.)
    } else {
        format!("{:.2} GB", bytes / 1073741824.)
    }
}

/// `YYYY/MM/DD HH:MM` in local time.
fn format_date(milliseconds: i64) -> Option<String> {
    use chrono::TimeZone;
    let date = chrono::Local.timestamp_millis_opt(milliseconds).single()?;
    Some(date.format("%Y/%m/%d %H:%M").to_string())
}

/// The short type name the Type row shows (`fmtExt`).
fn format_type(mime: &str) -> String {
    let subtype = mime.split_once('/').map_or("", |(_, subtype)| subtype);
    match subtype {
        "jpeg" => "JPG".into(),
        "svg+xml" => "SVG".into(),
        "quicktime" => "MOV".into(),
        "x-matroska" => "MKV".into(),
        "epub+zip" => "EPUB".into(),
        other => other.trim_start_matches("x-").to_uppercase(),
    }
}

pub struct Inspector {
    session: Entity<Session>,
    thumbnails: Entity<Thumbnails>,
    name: Entity<TextInput>,
    notes: Entity<TextInput>,
    collapsed: HashSet<&'static str>,
    /// The star under the pointer, which previews that rating.
    hovered_star: Option<usize>,
    scroll: ScrollHandle,
    /// The item the rail describes, or last described.
    item: Option<RootDetails>,
    /// What the rail shows. A new state takes its place only once that
    /// state's data is there, so the rail never goes blank in between.
    shown: Mode,
    scope: scope::ScopeState,
    selection: selection::SelectionState,
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
        let scope = scope::ScopeState::new(window, cx);
        let selection = selection::SelectionState::new(window, cx);
        let subscriptions = vec![
            cx.subscribe(&name, |this, _, event: &text_input::Committed, cx| {
                this.session
                    .read(cx)
                    .rename(RootId(event.0 as u32), event.1.clone());
            }),
            cx.subscribe(&notes, |this, _, event: &text_input::Committed, cx| {
                this.session
                    .read(cx)
                    .set_notes(RootId(event.0 as u32), event.1.clone());
            }),
            cx.subscribe(&scope.name, |this, _, event: &text_input::Committed, cx| {
                this.commit_scope(event.0, Some(event.1.clone()), None, cx);
            }),
            cx.subscribe(
                &scope.notes,
                |this, _, event: &text_input::Committed, cx| {
                    this.commit_scope(event.0, None, Some(event.1.clone()), cx);
                },
            ),
            cx.subscribe(
                &selection.notes,
                |this, _, event: &text_input::Committed, cx| {
                    this.commit_selection_notes(event.0, event.1.clone(), cx);
                },
            ),
            cx.subscribe(&session, |this, _, change: &Change, cx| match change {
                Change::Selection => {
                    // A pending edit belongs to what it was typed for.
                    for field in [&this.name, &this.notes, &this.selection.notes] {
                        field.update(cx, |field, cx| field.commit(cx));
                    }
                    this.sync_item(false, cx);
                    this.selection_changed(cx);
                    cx.notify();
                }
                Change::Details => {
                    this.sync_item(true, cx);
                    cx.notify();
                }
                Change::Viewer => {
                    this.sync_item(false, cx);
                    this.ensure_summary(cx);
                    cx.notify();
                }
                Change::Navigation => {
                    this.refresh_scope(cx);
                    cx.notify();
                }
                Change::Results => this.refresh_scope(cx),
            }),
        ];
        Self {
            session,
            thumbnails,
            name,
            notes,
            collapsed: HashSet::new(),
            hovered_star: None,
            scroll: ScrollHandle::new(),
            item: None,
            shown: Mode::Empty,
            scope,
            selection,
            _subscriptions: subscriptions,
        }
    }

    fn wanted(&self, cx: &App) -> Mode {
        let session = self.session.read(cx);
        if session.demo.is_some() {
            return Mode::Item;
        }
        wanted(
            session.screen().is_some(),
            session.viewer,
            session.selection.len(),
        )
    }

    /// Takes the selected item's details once they are there and the rail
    /// is to describe it. `edited` takes them even when they are for the
    /// item already shown, after an edit.
    fn sync_item(&mut self, edited: bool, cx: &mut Context<Self>) {
        if self.wanted(cx) != Mode::Item {
            return;
        }
        let Some(details) = self.session.read(cx).selected_details() else {
            return;
        };
        let id = details.root.root_id;
        if !edited
            && self
                .item
                .as_ref()
                .is_some_and(|item| item.root.root_id == id && item.revision == details.revision)
        {
            return;
        }
        let details = details.clone();
        let key = id.0 as u64;
        self.name.update(cx, |field, cx| {
            field.set_field(key, details.root.name.clone(), cx)
        });
        self.notes.update(cx, |field, cx| {
            field.set_field(key, details.root.notes.clone().unwrap_or_default(), cx)
        });
        self.item = Some(details);
    }

    /// What to draw: the state wanted once its data is there, else the one
    /// shown so far, else (`None`) the wanted state's skeleton.
    fn decide(&mut self, cx: &mut Context<Self>) -> (Mode, bool) {
        let wanted = self.wanted(cx);
        let ready = {
            let session = self.session.read(cx);
            match wanted {
                Mode::Empty => true,
                Mode::Scope => self.scope.ready(session),
                Mode::Item => session.demo.is_some() || session.selected_details().is_some(),
                Mode::Selection => self.selection.ready(),
            }
        };
        if ready {
            if wanted == Mode::Item {
                self.sync_item(false, cx);
            }
            self.shown = wanted;
            return (wanted, true);
        }
        let held = match self.shown {
            Mode::Empty => false,
            Mode::Scope => self.scope.view.is_some(),
            Mode::Item => self.item.is_some(),
            Mode::Selection => self.selection.shown.is_some(),
        };
        if held {
            (self.shown, true)
        } else {
            (wanted, false)
        }
    }

    /// Puts the caret in the name field, as Rename does.
    pub fn edit_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = self.name.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
        self.name.update(cx, |name, cx| name.select_everything(cx));
    }

    fn property(label: &'static str, value: String, cx: &App) -> Div {
        div()
            .h(px(20.))
            .flex()
            .items_center()
            .gap(px(12.))
            .whitespace_nowrap()
            .text(theme::TEXT_CAPTION)
            .child(div().w(px(88.)).flex_none().child(label))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_right()
                    .truncate()
                    .font_family(theme::FONT_INSPECTOR_MONO)
                    .font_features(theme::tabular_figures())
                    .text_color(theme::theme(cx).contrast(80.))
                    .child(value),
            )
    }

    fn section(
        &self,
        title: &'static str,
        content: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        ui::panel_section(title, title)
            .expanded(!self.collapsed.contains(title))
            .content(content)
            .on_toggle(cx.listener(move |this, _, _, cx| {
                if !this.collapsed.remove(title) {
                    this.collapsed.insert(title);
                }
                cx.notify();
            }))
    }

    /// The small "+" that ends a row of chips (`.tagAddBtn`).
    fn add_button(
        id: &'static str,
        tip: ui::tooltip::Tip,
        window: &mut Window,
        cx: &mut App,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        // TODO: open the tag or folder picker.
        let button = div()
            .id(id)
            .group(id)
            .w(px(26.))
            .h(px(theme::CONTROL_SM))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(theme::RADIUS_MD))
            .cursor_pointer()
            .child(
                ui::icon("inspector-add", 15., theme.text())
                    .opacity(0.5)
                    .group_hover(id, |style| style.opacity(1.)),
            );
        ui::tooltip::attach(button, (id, 1usize), tip, theme::CONTROL_SM, window, cx)
    }

    /// The Folders section's content: the chips and a "+" after them, or one
    /// wide "Add to Folder" button when there are none.
    fn folders_row(chips: Div, empty: bool, window: &mut Window, cx: &mut App) -> AnyElement {
        if empty {
            // TODO: open the folder picker.
            ui::tooltip::attach(
                div().id("add-to-folder").child(ui::action_row(
                    "add-to-folder-action",
                    "inspector-add",
                    "Add to Folder",
                )),
                "add-to-folder-tip",
                ui::tip("Add to Folder").keys(&["F"]),
                theme::CONTROL_SM,
                window,
                cx,
            )
            .into_any_element()
        } else {
            chips
                .child(Self::add_button(
                    "add-folder",
                    ui::tip("Add to Folder").keys(&["F"]),
                    window,
                    cx,
                ))
                .into_any_element()
        }
    }

    /// The Tags section's content: the chips and a "+" after them, or one
    /// wide "Add Tags" button when there are none.
    fn tags_row(chips: Div, empty: bool, window: &mut Window, cx: &mut App) -> AnyElement {
        if empty {
            // TODO: open the tag picker.
            ui::tooltip::attach(
                div().id("add-tags").child(ui::action_row(
                    "add-tags-action",
                    "inspector-add",
                    "Add Tags",
                )),
                "add-tags-tip",
                ui::tip("Add Tags").keys(&["T"]),
                theme::CONTROL_SM,
                window,
                cx,
            )
            .into_any_element()
        } else {
            chips
                .child(Self::add_button(
                    "add-tag",
                    ui::tip("Add Tags").keys(&["T"]),
                    window,
                    cx,
                ))
                .into_any_element()
        }
    }

    /// The status line under the sections (`.status`): a quiet note, or an
    /// error with its icon.
    fn status(message: impl Into<SharedString>, error: bool, cx: &App) -> Div {
        let theme = theme::theme(cx);
        let status = div()
            .mt(px(10.))
            .min_h(px(20.))
            .flex()
            .items_center()
            .gap(px(6.));
        if error {
            status
                .child(ui::icon("alert-circle", 16., theme.negative()))
                .child(
                    div()
                        .text(theme::TEXT_XS)
                        .text_color(theme.negative())
                        .child(message.into()),
                )
        } else {
            status.child(
                div()
                    .text(theme::TEXT_SM)
                    .text_color(theme.text_tertiary())
                    .child(message.into()),
            )
        }
    }

    /// The cover in its fitted frame, with the format badge
    /// (`InspectorPreview`).
    fn preview(
        &self,
        thumbnail: std::path::PathBuf,
        dimensions: Option<(u32, u32)>,
        label: AnyElement,
    ) -> Div {
        let (width, height) = fitted(dimensions);
        div()
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
                    .rounded(px(theme::RADIUS_MD))
                    .overflow_hidden()
                    .shadow(vec![
                        BoxShadow::new(px(0.), px(0.), rgba(0x0000001a).into()).blur_radius(px(5.)),
                    ])
                    .child(
                        img(thumbnail)
                            .image_cache(&self.thumbnails)
                            .size_full()
                            .rounded(px(theme::RADIUS_MD))
                            .object_fit(ObjectFit::Cover),
                    )
                    // The glass edge: a 1px ring over the image.
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .rounded(px(theme::RADIUS_MD))
                            .border_1()
                            .border_color(rgba(0x0000001a)),
                    )
                    .child(label),
            )
    }

    /// The colour pill (`ColorPalette`). It keeps its height when there are
    /// no colours, so the fields below never move.
    fn palette(&self, colors: &[picto_library::LabColor], cx: &mut Context<Self>) -> Div {
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
            let color = color.clone();
            let group = SharedString::from(format!("swatch-{index}"));
            swatches = swatches.child(
                div()
                    .id(("palette", index))
                    .group(group.clone())
                    .relative()
                    .size(px(16.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    // The hover ring sits one pixel outside the swatch's cell.
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
                            .bg(rgb(format::lab_to_rgb(color.l, color.a, color.b)))
                            .border_1()
                            .border_color(theme.contrast(10.)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let clause = FilterClause::Color {
                            color: color.clone(),
                            delta_e: 20.,
                        };
                        this.session
                            .update(cx, |session, cx| session.add_filter(clause, cx));
                    })),
            );
        }
        div()
            .h(px(theme::CONTROL_SM))
            .mb(px(8.))
            .flex()
            .justify_center()
            .child(swatches)
    }

    /// The source field (`InspectorField` in its URL form): the host, the
    /// rest of the address dimmed, and the button that manages the list.
    /// Without addresses to show (`unavailable`) it holds a dash and no button.
    fn source(urls: Option<&[String]>, cx: &App) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let unavailable = urls.is_none();
        let urls = urls.unwrap_or_default();
        let first = urls.first().cloned();
        let value = match first.as_deref().map(url::Url::parse) {
            _ if unavailable => div().child("—"),
            Some(Ok(address)) => {
                let host = address.host_str().unwrap_or_default();
                let host = host.strip_prefix("www.").unwrap_or(host).to_owned();
                let mut rest = address.path().to_owned();
                if let Some(query) = address.query() {
                    rest = format!("{rest}?{query}");
                }
                if let Some(fragment) = address.fragment() {
                    rest = format!("{rest}#{fragment}");
                }
                div()
                    .flex()
                    .min_w_0()
                    .child(div().flex_none().child(host))
                    .child(div().min_w_0().truncate().opacity(0.5).child(rest))
            }
            Some(Err(_)) => div().min_w_0().truncate().children(first.clone()),
            None => div().text_color(theme.text_tertiary()).child("http://"),
        };
        let mut field = div()
            .id("source-url")
            .h(px(theme::CONTROL_LG))
            .mb(px(8.))
            .rounded(px(theme::RADIUS_MD))
            .border_1()
            .border_color(theme.border_secondary())
            .bg(theme.control_surface())
            .overflow_hidden()
            .flex()
            .items_center()
            .text(theme::TEXT_MD)
            .line_height(px(18.))
            .whitespace_nowrap()
            .child(
                div()
                    .id("open-source")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .cursor(if unavailable {
                        CursorStyle::Arrow
                    } else {
                        CursorStyle::PointingHand
                    })
                    .child(value)
                    .on_click(move |_, _, cx| {
                        if let Some(address) = &first {
                            cx.open_url(address);
                        }
                    }),
            );
        if urls.len() > 1 {
            field = field.child(
                div()
                    .h_full()
                    .px(px(6.))
                    .flex()
                    .items_center()
                    .opacity(0.5)
                    .child(format!("+{}", urls.len() - 1)),
            );
        }
        if unavailable {
            return field;
        }
        // TODO: this opens the list of source URLs for editing.
        field.child(
            div()
                .id("manage-sources")
                .group("manage-sources")
                .w(px(31.))
                .h_full()
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .border_l_1()
                .border_color(theme.border_secondary())
                .cursor_pointer()
                .hover(move |style| style.bg(theme.control_surface()))
                .child(
                    ui::icon("link", 14., theme.text())
                        .opacity(0.5)
                        .group_hover("manage-sources", |style| style.opacity(1.)),
                ),
        )
    }

    /// The rating row (`StarRating`): the stars up to the one under the
    /// pointer preview that rating, and a click on the current rating clears it.
    fn rating(&self, target: Rate, current: usize, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let shown = self.hovered_star.unwrap_or(current);
        let mut row = div()
            .h(px(20.))
            .flex()
            .items_center()
            .text(theme::TEXT_CAPTION)
            .child(div().flex_1().child("Rating"));
        for value in 1..=5 {
            let star = if value <= shown {
                ui::icon("star-filled", 12., rgb(theme::RATING))
            } else {
                ui::icon("star", 12., theme.text_secondary()).opacity(0.25)
            };
            row = row.child(
                div()
                    .id(("rating", value))
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
                            Rate::Item(id) => this.session.read(cx).rate(id, next),
                            Rate::Selection => this.rate_selection(next, cx),
                        }
                    })),
            );
        }
        row
    }
}

impl Inspector {
    /// `.scrollContent`: 16px all round, 48 below, with the scrollbar's
    /// gutter reserved on the right.
    fn scroller(&self) -> Stateful<Div> {
        div()
            .id("inspector")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .pl(px(16.))
            .pr(px(16. + ui::scrollbar::GUTTER))
            .pt(px(16.))
            .pb(px(48.))
            .font_family(theme::FONT_INSPECTOR)
    }

    /// `.contentStack`: a flex column as tall as its content, so the fields'
    /// and sections' margins add up instead of collapsing.
    fn panel() -> Div {
        div().w_full().flex().flex_col()
    }

    /// The scrolling column of fields and sections for the selected item.
    fn body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let scroller = self.scroller();
        let mut panel = Self::panel();
        let (demo, details, entries, tags, cover_thumbnail) = {
            let session = self.session.read(cx);
            let details = self.item.clone();
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
        // The preview stays decoded while the grid scrolls past it.
        let pinned = cover_thumbnail
            .clone()
            .map(|path| Resource::Path(path.into()));
        self.thumbnails.update(cx, |thumbnails, _| {
            thumbnails.pin("inspector-preview", pinned)
        });
        if let Some((name, mime)) = demo {
            return scroller.child(
                panel
                    .child(Self::property("Example", name.to_owned(), cx))
                    .child(Self::property("Type", mime.to_owned(), cx)),
            );
        }
        let Some(details) = details else {
            return scroller.child(panel);
        };
        let root = &details.root;
        let id = root.root_id;
        let cover = details
            .media
            .iter()
            .find(|media| media.media_id == root.cover_media_id)
            .or(details.media.first());
        if let (Some(media), Some(thumbnail)) = (cover, cover_thumbnail) {
            // `.formatBadge`: opaque white on the grid's badge surface. A
            // collection shows the group glyph instead of a format.
            let label = if root.kind == picto_library::RootKind::Collection {
                ui::badge(0.)
                    .top(px(5.))
                    .left(px(5.))
                    .w(px(18.))
                    .justify_center()
                    .child(ui::icon("group", 14., white()))
                    .into_any_element()
            } else {
                ui::badge(3.)
                    .top(px(5.))
                    .left(px(5.))
                    .font_family(theme::FONT_UI)
                    .text_color(white())
                    .child(format::format_label(&media.facts.mime))
                    .into_any_element()
            };
            panel = panel.child(self.preview(
                thumbnail,
                media.facts.width.zip(media.facts.height),
                label,
            ));
        }
        let palette = cover.map_or(&[][..], |media| &media.facts.palette[..]);
        panel = panel
            .child(self.palette(palette, cx))
            .child(
                div()
                    .mb(px(8.))
                    .child(ui::field("inspector-name", &self.name, Look::Inspector)),
            )
            .child(div().mb(px(8.)).child(ui::field(
                "inspector-notes",
                &self.notes,
                Look::Inspector,
            )))
            .child(Self::source(Some(&root.source_urls), cx));

        // Folders: a chip each, then "+"; or one wide button when there are none.
        let mut folders = div().flex().flex_wrap().gap(px(4.));
        for folder in &details.folder_ids {
            let folder = *folder;
            let label = folder_label(&entries, folder);
            folders = folders.child(
                ui::chip(("inspector-folder", folder.0 as usize), label, FOLDER_COLOR).on_remove(
                    cx.listener(move |this, _, _, cx| {
                        this.session.read(cx).remove_from_folder(id, folder);
                    }),
                ),
            );
        }
        let folders = Self::folders_row(folders, details.folder_ids.is_empty(), window, cx);
        panel = panel.child(self.section("Folders", Some(folders), cx));

        let mut chips = div().flex().flex_wrap().gap(px(4.));
        let mut tags = tags;
        tags.sort_by_key(|tag| (tag_style(&tag.namespace).1, tag.subname.to_lowercase()));
        let tagged = !tags.is_empty();
        for tag in tags {
            let raw = tag_raw(&tag);
            chips = chips.child(tag_chip(&tag).on_remove(cx.listener(move |this, _, _, cx| {
                this.session.read(cx).remove_tag(id, raw.clone());
            })));
        }
        let tags = Self::tags_row(chips, !tagged, window, cx);
        panel = panel.child(self.section("Tags", Some(tags), cx));

        // `normalizedCoreProperties`: a row with nothing to say is left out.
        let mut properties = div()
            .child(self.rating(Rate::Item(id), details.rating.bitmap_key() as usize, cx))
            .child(Self::property(
                "Items",
                format::grouped(root.media_count as u64),
                cx,
            ));
        if let Some((width, height)) = cover
            .and_then(|media| media.facts.width.zip(media.facts.height))
            .filter(|(width, height)| *width > 0 && *height > 0)
        {
            properties = properties.child(Self::property(
                "Dimensions",
                format!("{width} × {height}"),
                cx,
            ));
        }
        properties = properties.child(Self::property(
            "Size",
            format_size(root.total_size_bytes),
            cx,
        ));
        let mut types = details
            .media
            .iter()
            .map(|media| media.facts.mime.as_str())
            .collect::<Vec<_>>();
        types.sort_unstable();
        types.dedup();
        if let Some(kind) = match types[..] {
            [] => None,
            [one] => Some(format_type(one)),
            _ => Some("Mixed".into()),
        } {
            properties = properties.child(Self::property("Type", kind, cx));
        }
        if let Some(duration) = cover
            .and_then(|media| media.facts.duration_ms)
            .filter(|duration| *duration > 0)
        {
            properties = properties.child(Self::property(
                "Duration",
                format::format_duration(duration),
                cx,
            ));
        }
        for (label, at) in [
            ("Date Imported", Some(root.imported_at_ms)),
            ("Date Created", root.captured_at_ms),
            ("Date Modified", Some(root.modified_at_ms)),
        ] {
            if let Some(date) = at.and_then(format_date) {
                properties = properties.child(Self::property(label, date, cx));
            }
        }
        // TODO: Export opens the export dialog for this item.
        scroller.child(
            panel
                .child(self.section("Properties", Some(properties.into_any_element()), cx))
                .child(
                    div().mt(px(12.)).child(
                        ui::action_row("export", "inspector-export", "Export")
                            .glyph(16.)
                            .height(theme::CONTROL_MD),
                    ),
                ),
        )
    }
}

impl Inspector {
    /// `UnavailableInspectorSkeleton`: the rows a state will have, with
    /// dashes for values, while there is nothing to show yet.
    fn skeleton(&mut self, mode: Mode, cx: &mut Context<Self>) -> Stateful<Div> {
        let error = self.session.read(cx).error.clone();
        let mut panel = Self::panel()
            .child(div().h(px(PREVIEW)).mb(px(8.)))
            .child(self.palette(&[], cx))
            .child(div().mb(px(8.)).child(ui::field::read_only("—", cx)))
            .child(div().mb(px(8.)).child(ui::field::read_only("—", cx)));
        if mode != Mode::Scope {
            panel = panel.child(Self::source(None, cx));
        }
        panel = panel.child(self.section("Properties", Some(div().into_any_element()), cx));
        // An item's or a selection's details are on their way; a
        // destination's skeleton says nothing unless something failed.
        let status = match error {
            Some(error) => Some(Self::status(error, true, cx)),
            None if mode != Mode::Scope => Some(Self::status("Loading...", false, cx)),
            None => None,
        };
        self.scroller().child(panel.children(status))
    }
}

impl Render for Inspector {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::stats::rendered(crate::stats::View::Inspector);
        let (mode, ready) = self.decide(cx);
        // A state that is not drawn gives back the thumbnails it kept.
        if (mode, ready) != (Mode::Item, true) {
            self.thumbnails.update(cx, |thumbnails, _| {
                thumbnails.pin("inspector-preview", None)
            });
        }
        if (mode, ready) != (Mode::Scope, true) {
            self.release_collage(cx);
        }
        if (mode, ready) != (Mode::Selection, true) {
            self.release_stack(cx);
        }
        // Before the first destination has loaded there is nothing to
        // describe, and the Electron rail is empty too.
        let launching = self.session.read(cx).results_epoch == 0;
        let body = match (mode, ready) {
            (Mode::Empty, _) => self.scroller(),
            (Mode::Scope, false) if launching => self.scroller(),
            (Mode::Item, true) => self.body(window, cx),
            (Mode::Scope, true) => self.scope_body(cx),
            (Mode::Selection, true) => self.selection_body(window, cx),
            (mode, false) => self.skeleton(mode, cx),
        };
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

#[cfg(test)]
mod tests {
    use super::{Mode, fitted, format_size, format_type, tag_style, wanted};

    #[test]
    fn the_rail_describes_the_viewer_then_the_selection_then_the_destination() {
        assert_eq!(wanted(true, false, 0), Mode::Empty);
        assert_eq!(wanted(false, true, 3), Mode::Item);
        assert_eq!(wanted(false, false, 1), Mode::Item);
        assert_eq!(wanted(false, false, 2), Mode::Selection);
        assert_eq!(wanted(false, false, 0), Mode::Scope);
    }

    #[test]
    fn previews_shrink_to_the_square_but_never_grow() {
        assert_eq!(fitted(Some((400, 200))), (200., 100.));
        assert_eq!(fitted(Some((100, 50))), (100., 50.));
        assert_eq!(fitted(None), (200., 200.));
    }

    #[test]
    fn sizes_roll_over_to_the_next_unit() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(5 * 1048576), "5.0 MB");
        assert_eq!(format_size(3 * 1073741824 / 2), "1.50 GB");
    }

    #[test]
    fn the_type_row_shows_a_short_name() {
        assert_eq!(format_type("image/jpeg"), "JPG");
        assert_eq!(format_type("video/quicktime"), "MOV");
        assert_eq!(format_type("video/x-matroska"), "MKV");
        assert_eq!(format_type("image/svg+xml"), "SVG");
        assert_eq!(
            format_type("application/x-shockwave-flash"),
            "SHOCKWAVE-FLASH"
        );
        assert_eq!(format_type("image/png"), "PNG");
    }

    #[test]
    fn general_tags_sort_after_every_named_group() {
        let order = |namespace| tag_style(namespace).1;
        assert!(order("creator") < order("series"));
        assert!(order("rating") < order("somethingelse"));
        assert!(order("somethingelse") < order("general"));
        assert_eq!(order(""), order("general"));
    }
}
