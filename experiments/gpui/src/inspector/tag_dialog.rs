//! The Add Tags dialog (`TagSelectModal.tsx`) that T and the grid menu's
//! Add Tags open: a centred dialog with a search, a rail of Recent,
//! Selected, All and the tag groups, and a list of the library's tags with
//! check boxes. Clicks only mark tags to add or remove; Apply puts them on
//! the selected items, Cancel and Escape forget them. The tags checked at
//! first are those of the item the inspector shows or last showed.
use crate::grid::format;
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
    selection_modal::{self as shell, Key, ROW},
    text_input::{Kind, TextInput},
};
use gpui::*;
use picto_library::{TagNamespaceRecord, TagRecord};
use std::collections::BTreeSet;
use std::time::Duration;

/// `PAGE_SIZE`
const PAGE: usize = 100;
/// `.panelBody`'s height.
const BODY: f32 = 520.;
/// `.sidebar`'s width and `.sidebarName`'s widest.
const RAIL: f32 = 220.;
const NAME: f32 = 140.;
/// The search waits this long after each change before it asks.
const SEARCH_DELAY: Duration = Duration::from_millis(150);
/// More tags load once the list is scrolled this near its end.
const LOAD_AHEAD: f32 = 200.;
/// Rows drawn beyond the visible ones (`overscan`).
const OVERSCAN: usize = 15;
/// The Recent view shows at most this many.
const RECENT_SHOWN: usize = 30;

pub enum Event {
    /// Apply: the tags to put on the selected items and to take off.
    Apply {
        add: Vec<String>,
        remove: Vec<String>,
    },
}

impl EventEmitter<Event> for TagDialog {}

/// `displayTags`: the items' tags by group then name, the recent tags among
/// those loaded, or the loaded page.
pub fn display_tags(
    view: &View,
    tags: &[TagRecord],
    entity: &BTreeSet<String>,
    recent: &[String],
) -> Vec<TagRecord> {
    match view {
        View::Selected => crate::tags::records_by_group(entity),
        View::Recent => recent
            .iter()
            .filter_map(|wanted| tags.iter().find(|tag| name(tag) == *wanted).cloned())
            .take(RECENT_SHOWN)
            .collect(),
        View::All | View::Group(_) | View::Starred => tags.to_vec(),
    }
}

/// `estimatedTotal`: how many rows the list may reach while more pages are
/// left, so its scrollbar is the size of the whole list.
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
    /// `entityTagKeys`: the tags of the item the inspector shows.
    entity: BTreeSet<String>,
    /// `checked` and `unchecked`, in the order they were clicked.
    add: Vec<String>,
    remove: Vec<String>,
    view: View,
    focus_index: Option<usize>,
    /// How many items are selected, for the footer.
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

    /// Opens on the shown item's tags for `selected` items, the search
    /// empty and nothing marked. The tags loaded last stay until the new
    /// ones arrive.
    pub fn open(&mut self, entity: BTreeSet<String>, selected: usize, cx: &mut Context<Self>) {
        self.entity = entity;
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

    // ── Loading ─────────────────────────────────────────────────

    fn load_groups(&mut self, cx: &mut Context<Self>) {
        crate::tags::read_groups(&self.session, cx, |this, namespaces, cx| {
            this.namespaces = namespaces;
            cx.notify();
        });
    }

    /// One request per change of the search or view, once 150 ms pass
    /// without another; an older answer never replaces a newer one.
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

    /// The next page, once the list nears its end.
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

    // ── Choosing ────────────────────────────────────────────────

    fn rows(&self, cx: &App) -> Vec<TagRecord> {
        display_tags(
            &self.view,
            &self.pager.tags,
            &self.entity,
            &self.session.read(cx).recent_tags,
        )
    }

    fn toggle(&mut self, tag: &str, cx: &mut Context<Self>) {
        super::membership::mark(
            &self.entity,
            &mut self.add,
            &mut self.remove,
            tag.to_owned(),
        );
        cx.notify();
    }

    fn show_view(&mut self, view: View, cx: &mut Context<Self>) {
        self.view = view;
        self.request(cx);
    }

    /// `applyTags`
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

    /// The keys typed in the search (`handleKeyDown`).
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
                    self.toggle(&name(tag), cx);
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

    /// `scrollToIndex(focusIdx, { align: 'auto' })`
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

    /// A click on the rail or the list leaves the search, as a click on
    /// something that cannot take the keyboard does in the browser.
    fn leave_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(focus) = self.host.get_mut().map(|dialog| dialog.focus.clone()) {
            window.focus(&focus, cx);
        }
    }
}

// ── Drawing ─────────────────────────────────────────────────────

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
            shell::rail_item(id, label, count, active, dot, Some(NAME), cx).on_click(cx.listener(
                move |this, _, window, cx| {
                    this.leave_search(window, cx);
                    this.show_view(view.clone(), cx);
                },
            ))
        };
        let mut rail = shell::rail(RAIL, cx)
            .child(item(
                "tag-dialog-recent",
                "Recent".into(),
                recent.to_string(),
                View::Recent,
                None,
                cx,
            ))
            .child(item(
                "tag-dialog-selected",
                "Selected".into(),
                self.entity.len().to_string(),
                View::Selected,
                None,
                cx,
            ))
            .child(shell::rail_separator(cx))
            .child(item(
                "tag-dialog-all",
                "All".into(),
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
                shell::rail_item(
                    ("tag-dialog-group", index),
                    label,
                    format::grouped(group.tag_count),
                    active,
                    Some(crate::tags::group_color(&group.name, &preferences)),
                    Some(NAME),
                    cx,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.leave_search(window, cx);
                    this.show_view(view.clone(), cx);
                })),
            );
        }
        rail
    }

    /// `.tagRow`
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
        .child(check::check_box(
            if checked { Mark::Checked } else { Mark::Empty },
            CheckSize::Compact,
        ))
        // `.tagDot`
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
            // `.tagNs`
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
        .on_click(cx.listener(move |this, _, _, cx| this.toggle(&full, cx)))
    }

    fn list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let settings = crate::settings::current(cx).clone();
        let preferences = Preferences::from_settings(&settings);
        let rows = self.rows(cx);
        let query = self.query.trim().to_owned();
        let max = f32::from(self.scroll.max_offset().y);
        let scrolled = -f32::from(self.scroll.offset().y);
        if max - scrolled < LOAD_AHEAD {
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
                    View::Selected => "No tags on this entity",
                    View::Recent => "No recently used tags",
                    _ => "No tags found",
                },
                cx,
            ));
        } else {
            // Only the rows in view, and fifteen either side, are drawn, each
            // at its place in the whole list's height.
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

/// `highlightMatch`: the first match of the search in bold, in the primary
/// colour.
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
                .label("Cancel")
                .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
        );
        if pending > 0 {
            buttons = buttons.child(
                ui::action_button("tag-dialog-apply", Look::Primary)
                    .label(format!("Apply ({pending})"))
                    .on_click(cx.listener(|this, _, _, cx| this.apply(cx))),
            );
        }
        ui::modal("tag-dialog", "Add Tags", presence)
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
