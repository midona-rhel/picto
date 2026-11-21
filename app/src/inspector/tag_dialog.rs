//! Draws the Add Tags dialog and holds its state. The T key and Add Tags in
//! the grid's menu open it. It is a dialog in the middle of the window with a
//! search field, a rail with Recent, Selected, All and each tag group, and a
//! list of the library's tags with check boxes. A click only marks a tag to be
//! added or removed. Apply puts the tags marked to be added on the selected
//! items and takes the tags marked to be removed off them. Cancel and Escape
//! discard the marks. The tags that are ticked when the dialog opens are those
//! of the item the inspector shows or last showed.
use crate::i18n::format;
use crate::i18n::t;
use crate::session::Session;
use crate::tags::{Preferences, TagPager, View, record_name as name};
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{
    self,
    action_button::Look,
    check::{self, Mark, Size as CheckSize},
    menu::Return,
    modal::{self, Size},
    rail_item::{Look as RailLook, rail_item},
    selection_modal::{self as shell, Key, ROW},
    text_input::{Kind, TextInput},
};
use gpui::*;
use picto_library::{TagNamespaceRecord, TagRecord};
use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

/// How many tags are loaded at a time.
const PAGE: usize = 100;
/// The height of the dialog's body, which holds the search field, the rail
/// and the list of tags.
const BODY: f32 = 520.;
/// The width of the rail at the left of the dialog's body. `NAME` is the
/// largest width of a name in the rail.
const RAIL: f32 = 220.;
const NAME: f32 = 140.;
/// How long the search waits after the last change of its text before it asks
/// the library.
const SEARCH_DELAY: Duration = Duration::from_millis(150);
/// The next page of tags is loaded when the rows in view come within this many
/// pixels of the last loaded row.
const LOAD_AHEAD: f32 = 200.;
/// How many rows are drawn above and below the visible ones.
const OVERSCAN: usize = 15;
/// The Recent view shows at most this many tags.
const RECENT_SHOWN: usize = 30;

pub enum Event {
    /// The user pressed Apply. `add` holds the tags to put on the selected
    /// items, and `remove` the tags to take off them.
    Apply {
        add: Vec<String>,
        remove: Vec<String>,
    },
}

impl EventEmitter<Event> for TagDialog {}

/// Returns the rows for a view. Selected gives the items' tags, ordered by
/// group and then by name. Recent gives the recently used tags that are among
/// the loaded ones. Every other view gives the loaded page.
pub fn display_tags(
    view: &View,
    tags: &[TagRecord],
    entity: &BTreeSet<String>,
    recent: &[String],
) -> Vec<TagRecord> {
    match view {
        View::Selected => crate::tags::records_by_group(entity, true),
        View::Recent => recent
            .iter()
            .filter_map(|wanted| tags.iter().find(|tag| name(tag) == *wanted).cloned())
            .take(RECENT_SHOWN)
            .collect(),
        View::All | View::Group(_) | View::Starred => tags.to_vec(),
    }
}

/// Returns how many rows the list will have when every page is loaded, as far
/// as that is known. The list is given that height while more pages remain,
/// so that its scrollbar has the size of the whole list.
fn estimated_total(
    view: &View,
    query: &str,
    namespaces: &[TagNamespaceRecord],
    shown: usize,
) -> usize {
    match view {
        View::Selected | View::Recent | View::Starred => shown,
        _ if !query.trim().is_empty() => shown,
        View::Group(group) => namespaces
            .iter()
            .find(|namespace| namespace.name == *group)
            .map_or(shown, |namespace| namespace.tag_count as usize),
        View::All => {
            let total = namespaces.iter().map(|group| group.tag_count).sum::<u64>() as usize;
            if total > 0 { total } else { shown }
        }
    }
}

pub struct TagDialog {
    session: Entity<Session>,
    search: Entity<TextInput>,
    host: modal::Host<()>,
    query: String,
    pager: TagPager,
    namespaces: Vec<TagNamespaceRecord>,
    /// The names of the tags of the item the inspector shows.
    entity: BTreeSet<String>,
    /// How many items have each tag that the Selected view can list: the
    /// tags of the item the inspector shows and the tags clicked in this
    /// dialog. The Selected view builds its rows from tag names alone, and
    /// takes each row's count from here.
    counts: HashMap<String, u64>,
    /// The tags ticked in this dialog, in the order they were clicked.
    /// `remove` holds the tags unticked in this dialog, in the same order.
    add: Vec<String>,
    remove: Vec<String>,
    view: View,
    focus_index: Option<usize>,
    /// How many items are selected. The footer shows it.
    selected: usize,
    scroll: ScrollHandle,
    search_timer: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl TagDialog {
    pub fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| TextInput::new(Kind::Menu, cx).placeholder("Search tags..."));
        let subscriptions = vec![cx.observe(&search, |this, search, cx| {
            let query = search.read(cx).value();
            if query != this.query {
                this.query = query;
                this.request(cx);
            }
        })];
        Self {
            session,
            search,
            host: modal::Host::default(),
            query: String::new(),
            pager: TagPager::new(PAGE),
            namespaces: Vec::new(),
            entity: BTreeSet::new(),
            counts: HashMap::new(),
            add: Vec::new(),
            remove: Vec::new(),
            view: View::All,
            focus_index: None,
            selected: 0,
            scroll: ScrollHandle::new(),
            search_timer: None,
            _subscriptions: subscriptions,
        }
    }

    /// Opens the dialog for `selected` items, with `tags`, the tags of the
    /// shown item, ticked, an empty search field and nothing marked. The tags
    /// that were loaded last stay in the list until the new ones arrive.
    pub fn open(&mut self, tags: &[TagRecord], selected: usize, cx: &mut Context<Self>) {
        self.entity = tags.iter().map(name).collect();
        self.counts = tags
            .iter()
            .map(|tag| (name(tag), tag.active_count))
            .collect();
        self.selected = selected;
        self.query.clear();
        self.search.update(cx, |search, cx| search.clear(cx));
        self.add.clear();
        self.remove.clear();
        self.view = View::All;
        self.focus_index = None;
        let field = self.search.read(cx).focus_handle(cx);
        self.host.open((), Return::Opener, cx).focus_first(field);
        self.load_groups(cx);
        self.request(cx);
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.search_timer = None;
        self.host.dismiss(cx);
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.search_timer = None;
        self.host.close(cx);
    }

    // The functions below load the tag groups and the pages of tags.

    fn load_groups(&mut self, cx: &mut Context<Self>) {
        crate::tags::read_groups(&self.session, cx, |this, namespaces, cx| {
            this.namespaces = namespaces;
            cx.notify();
        });
    }

    /// Sends one request for each change of the search text or the view, 150
    /// ms after the last change. An answer to an older request never replaces
    /// the answer to a newer one.
    fn request(&mut self, cx: &mut Context<Self>) {
        if !self.host.is_open() {
            return;
        }
        let page = self.pager.begin(self.query.trim(), self.view.group());
        let delay = Some(SEARCH_DELAY);
        self.search_timer = Some(crate::tags::fetch_page(
            &self.session,
            page,
            delay,
            cx,
            |this, page, answer, cx| {
                match answer {
                    Ok((tags, next)) => {
                        if !this.pager.first(&page, tags, next) {
                            return;
                        }
                        this.focus_index = None;
                    }
                    Err(error) => eprintln!("add tags: {error}"),
                }
                cx.notify();
            },
        ));
        cx.notify();
    }

    /// Loads the next page of tags.
    fn load_more(&mut self, cx: &mut Context<Self>) {
        let Some(page) = self.pager.next() else {
            return;
        };
        crate::tags::fetch_page(&self.session, page, None, cx, |this, page, answer, cx| {
            if this.pager.more(&page, answer) {
                cx.notify();
            }
        })
        .detach();
    }

    // The functions below handle clicks and keys in the dialog.

    /// Returns the rows of the list for the view that is shown. The Selected
    /// view lists the tags the items have and the tags ticked in this dialog,
    /// so that it shows everything that is ticked now. A tag that was
    /// unticked here stays in that list with an empty box, so that it can be
    /// ticked again.
    fn rows(&self, cx: &App) -> Vec<TagRecord> {
        let selected: BTreeSet<String>;
        let entity = if self.view == View::Selected && !self.add.is_empty() {
            selected = self.entity.iter().chain(&self.add).cloned().collect();
            &selected
        } else {
            &self.entity
        };
        let mut rows = display_tags(
            &self.view,
            &self.pager.tags,
            entity,
            &self.session.read(cx).recent_tags,
        );
        if self.view == View::Selected {
            for row in &mut rows {
                if let Some(count) = self.counts.get(&name(row)) {
                    row.active_count = *count;
                }
            }
        }
        rows
    }

    /// Ticks or unticks `tag`, and remembers how many items have it, so that
    /// the Selected view can show that number beside it.
    fn toggle(&mut self, tag: &TagRecord, cx: &mut Context<Self>) {
        let full = name(tag);
        self.counts.insert(full.clone(), tag.active_count);
        super::membership::mark(&self.entity, &mut self.add, &mut self.remove, full);
        cx.notify();
    }

    fn show_view(&mut self, view: View, cx: &mut Context<Self>) {
        self.view = view;
        self.request(cx);
    }

    /// Emits `Event::Apply` with the marked tags and closes the dialog. It
    /// does nothing when no tag is marked.
    fn apply(&mut self, cx: &mut Context<Self>) {
        if self.add.is_empty() && self.remove.is_empty() {
            return;
        }
        cx.emit(Event::Apply {
            add: std::mem::take(&mut self.add),
            remove: std::mem::take(&mut self.remove),
        });
        self.close(cx);
    }

    /// Handles the keys that the search field reports. Down and Up move the
    /// keyboard's row in the list. Enter ticks or unticks that row, or the
    /// first row if the keyboard is on none. Tab shows the next view of the
    /// rail. Escape closes the dialog and discards the marks.
    fn key(&mut self, key: Key, cx: &mut Context<Self>) {
        let rows = self.rows(cx);
        match key {
            Key::Down => {
                let last = rows.len() as isize - 1;
                let at = self.focus_index.map_or(-1, |at| at as isize);
                self.focus_index = usize::try_from((at + 1).min(last)).ok();
                self.reveal_focus();
            }
            Key::Up => {
                self.focus_index = self.focus_index.and_then(|at| at.checked_sub(1));
                self.reveal_focus();
            }
            Key::Confirm => {
                if let Some(tag) = rows.get(self.focus_index.unwrap_or(0)) {
                    self.toggle(tag, cx);
                }
            }
            Key::Next => {
                let groups = crate::tags::rail_groups(&self.namespaces, true)
                    .into_iter()
                    .map(|group| group.name)
                    .collect::<Vec<_>>();
                let next = crate::tags::next_view(&self.view, &groups, &View::Recent);
                self.show_view(next, cx);
            }
            Key::Dismiss => self.dismiss(cx),
        }
        cx.notify();
    }

    /// Scrolls the list just far enough to show the row the keyboard is on.
    fn reveal_focus(&mut self) {
        let Some(at) = self.focus_index else { return };
        let top = at as f32 * ROW;
        let viewport = f32::from(self.scroll.bounds().size.height);
        let scrolled = -f32::from(self.scroll.offset().y);
        let next = if top < scrolled {
            top
        } else if top + ROW > scrolled + viewport {
            top + ROW - viewport
        } else {
            return;
        };
        self.scroll.set_offset(point(px(0.), px(-next.max(0.))));
    }

    /// Moves the keyboard focus from the search field to the dialog. A click
    /// on the rail or on the list calls this.
    fn leave_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(focus) = self.host.get_mut().map(|dialog| dialog.focus.clone()) {
            window.focus(&focus, cx);
        }
    }
}

// The functions below draw the dialog.

impl TagDialog {
    fn rail(&self, cx: &mut Context<Self>) -> Div {
        let preferences = Preferences::from_settings(&crate::settings::current(cx).clone());
        let recent = self.session.read(cx).recent_tags.len();
        let total: u64 = self.namespaces.iter().map(|group| group.tag_count).sum();
        let item = |id: &'static str,
                    label: SharedString,
                    count: String,
                    view: View,
                    dot: Option<u32>,
                    cx: &mut Context<Self>| {
            let active = self.view == view;
            rail_item(id, RailLook::SelectionModal, label, count)
                .active(active)
                .dot(dot)
                .name_width(NAME)
                .build(cx)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.leave_search(window, cx);
                    this.show_view(view.clone(), cx);
                }))
        };
        let mut rail = shell::rail(RAIL, cx)
            .child(item(
                "tag-dialog-recent",
                t("Recent").into(),
                recent.to_string(),
                View::Recent,
                None,
                cx,
            ))
            .child(item(
                "tag-dialog-selected",
                t("Selected").into(),
                // The number of tags that are ticked now: those the items
                // have, less those unticked here, plus those ticked here.
                (self.entity.len() + self.add.len() - self.remove.len()).to_string(),
                View::Selected,
                None,
                cx,
            ))
            .child(shell::rail_separator(cx))
            .child(item(
                "tag-dialog-all",
                t("All").into(),
                format::grouped(total),
                View::All,
                None,
                cx,
            ));
        let groups = crate::tags::rail_groups(&self.namespaces, true);
        if !groups.is_empty() {
            rail = rail.child(shell::rail_separator(cx));
        }
        for (index, group) in groups.into_iter().enumerate() {
            let view = View::Group(group.name.clone());
            let active = self.view == view;
            let label = if group.name.is_empty() {
                "general".to_owned()
            } else {
                group.name.clone()
            };
            rail = rail.child(
                rail_item(
                    ("tag-dialog-group", index),
                    RailLook::SelectionModal,
                    label,
                    format::grouped(group.tag_count),
                )
                .active(active)
                .dot(Some(crate::tags::group_color(&group.name, &preferences)))
                .name_width(NAME)
                .build(cx)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.leave_search(window, cx);
                    this.show_view(view.clone(), cx);
                })),
            );
        }
        rail
    }

    /// Draws the row of one tag in the list.
    fn row(
        &self,
        index: usize,
        tag: &TagRecord,
        preferences: &Preferences,
        query: &str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let full = name(tag);
        let on_items = self.entity.contains(&full);
        let checked = (on_items && !self.remove.contains(&full)) || self.add.contains(&full);
        let group = SharedString::from(format!("tag-dialog-row-{index}"));
        let namespace = tag.namespace.to_lowercase();
        let mut row = shell::item_row(
            ("tag-dialog-row", index),
            group.clone(),
            self.focus_index == Some(index),
            cx,
        )
        // For assistive technology the row is a check box whose name is the
        // tag's name. It is ticked while the items will have the tag.
        .role(Role::CheckBox)
        .aria_label(full.clone())
        .aria_toggled(crate::ui::a11y::toggled(checked))
        .child(check::check_box(
            if checked { Mark::Checked } else { Mark::Empty },
            CheckSize::Compact,
        ))
        // The dot is in the colour of the tag's group.
        .child(
            div()
                .size(px(10.))
                .flex_none()
                .rounded_full()
                .bg(rgb(crate::tags::group_color(&tag.namespace, preferences)))
                .opacity(0.6)
                .group_hover(group.clone(), |style| style.opacity(1.)),
        );
        if !namespace.is_empty() && namespace != "general" {
            // The name of the tag's group is drawn before the tag's own name.
            row = row.child(
                div()
                    .flex_none()
                    .text(theme::TEXT_CAPTION)
                    .line_height(px(theme::LINE_HEIGHT_MD))
                    .text_color(theme.text_tertiary())
                    .opacity(0.7)
                    .group_hover(group.clone(), |style| style.opacity(1.))
                    .child(format!("{}:", tag.namespace)),
            );
        }
        let clicked = tag.clone();
        let label = if tag.subname.is_empty() {
            full.clone()
        } else {
            tag.subname.clone()
        };
        row.child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(highlight(label, query, cx)),
        )
        .child(
            shell::badge(group, false, cx)
                .ml_auto()
                .min_w(px(32.))
                .flex()
                .justify_end()
                .child(format::grouped(tag.active_count)),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| this.leave_search(window, cx)),
        )
        .on_click(cx.listener(move |this, _, _, cx| this.toggle(&clicked, cx)))
    }

    fn list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let settings = crate::settings::current(cx).clone();
        let preferences = Preferences::from_settings(&settings);
        let rows = self.rows(cx);
        let query = self.query.trim().to_owned();
        // The next page is loaded when the rows in view come within
        // `LOAD_AHEAD` pixels of the last row that is loaded. The list's
        // height is that of the estimated total, so that its scrollbar has
        // the size of the whole list; the distance to the end of that
        // height says nothing about how many rows are loaded.
        let scrolled = -f32::from(self.scroll.offset().y);
        let in_view = f32::from(self.scroll.bounds().size.height).max(BODY);
        if scrolled + in_view > rows.len() as f32 * ROW - LOAD_AHEAD {
            self.load_more(cx);
        }
        let count = if self.pager.cursor.is_some() {
            rows.len().max(estimated_total(
                &self.view,
                &self.query,
                &self.namespaces,
                rows.len(),
            ))
        } else {
            rows.len()
        };
        let mut scroller = div()
            .id("tag-dialog-list")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .pr(px(ui::scrollbar::GUTTER))
            .on_scroll(cx.listener(|_, _, _, cx| cx.notify()));
        if rows.is_empty() {
            scroller = scroller.child(shell::empty_state(
                match self.view {
                    View::Selected => t("No tags on this entity"),
                    View::Recent => t("No recently used tags"),
                    _ => t("No tags found"),
                },
                cx,
            ));
        } else {
            // Only the rows in view, and fifteen above and below them, are
            // drawn. Each is placed at its position within the height of the
            // whole list.
            let viewport = f32::from(self.scroll.bounds().size.height).max(BODY);
            let first = ((scrolled / ROW).floor().max(0.) as usize).saturating_sub(OVERSCAN);
            let last = (((scrolled + viewport) / ROW).ceil() as usize + OVERSCAN).min(rows.len());
            let mut inner = div().relative().w_full().h(px(count as f32 * ROW));
            for index in first..last.max(first) {
                inner = inner.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px(index as f32 * ROW))
                        .flex()
                        .flex_col()
                        .child(self.row(index, &rows[index], &preferences, &query, cx)),
                );
            }
            scroller = scroller.child(inner);
        }
        div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(scroller)
            .child(ui::scrollbar(
                "tag-dialog-scrollbar",
                &self.scroll,
                window,
                cx,
            ))
    }
}

/// Returns `label` as text in which the first match of the search text is
/// bold and in the primary colour.
fn highlight(label: String, query: &str, cx: &App) -> StyledText {
    let theme = theme::theme(cx);
    let range = crate::text::match_range(&label, query);
    StyledText::new(label).with_highlights(range.map(|range| {
        (
            range,
            HighlightStyle {
                font_weight: Some(FontWeight::BOLD),
                color: Some(theme.primary().into()),
                ..Default::default()
            },
        )
    }))
}

impl Render for TagDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(layer) = self.host.layer(window, cx) else {
            return div().into_any_element();
        };
        let (presence, focus) = (layer.presence, layer.focus.clone());
        let pending = self.add.len() + self.remove.len();
        let summary = shell::summary(self.add.len(), self.remove.len(), self.selected);
        let entity = cx.entity().downgrade();
        let keys = entity.clone();
        let search = shell::search_bar(
            &self.search,
            true,
            move |key, _, cx| {
                let _ = keys.update(cx, |this, cx| this.key(key, cx));
            },
            cx,
        );
        let body = div().h(px(BODY)).flex().flex_col().child(search).child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .child(self.rail(cx))
                .child(self.list(window, cx)),
        );
        let mut buttons = div().flex().items_center().gap(px(6.)).child(
            ui::action_button("tag-dialog-cancel", Look::Standard)
                .label(t("Cancel"))
                .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
        );
        if pending > 0 {
            buttons = buttons.child(
                ui::action_button("tag-dialog-apply", Look::Primary)
                    .label(format!("{}{pending})", t("Apply (")))
                    .on_click(cx.listener(|this, _, _, cx| this.apply(cx))),
            );
        }
        ui::modal("tag-dialog", t("Add Tags"), presence)
            .size(Size::Large)
            .flush()
            .focus(&focus)
            .on_close(move |_, cx| {
                let _ = entity.update(cx, |this, cx| this.dismiss(cx));
            })
            .child(body)
            .footer(shell::footer_left(summary, true, cx))
            .footer(buttons)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{View, display_tags};
    use crate::tags::record_name as name;
    use picto_library::{TagId, TagNamespaceId, TagRecord};
    use std::collections::BTreeSet;

    fn tag(id: u32, namespace: &str, subname: &str) -> TagRecord {
        TagRecord {
            tag_id: TagId(id),
            namespace_id: TagNamespaceId(0),
            namespace: namespace.into(),
            subname: subname.into(),
            active_count: 0,
            assignment_count: 0,
        }
    }

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn the_views_list_the_items_tags_the_recent_ones_loaded_or_the_page() {
        let tags = [tag(1, "general", "b"), tag(2, "series", "a")];
        let entity = set(&["zeta", "series:x", "alpha"]);
        let rows = display_tags(&View::Selected, &tags, &entity, &[]);
        assert_eq!(
            rows.iter().map(name).collect::<Vec<_>>(),
            ["alpha", "zeta", "series:x"]
        );
        let recent = ["series:a".to_owned(), "gone".to_owned(), "b".to_owned()];
        let rows = display_tags(&View::Recent, &tags, &entity, &recent);
        assert_eq!(rows.iter().map(name).collect::<Vec<_>>(), ["series:a", "b"]);
        assert_eq!(display_tags(&View::All, &tags, &entity, &[]).len(), 2);
    }
}
