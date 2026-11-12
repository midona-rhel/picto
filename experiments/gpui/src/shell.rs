//! The sidebar and the grid toolbar, following `Sidebar.tsx` and
//! `GridToolbar.tsx`: the same rows in the same order, the same section
//! headers, and Picto's own toolbar glyphs.
use crate::{Browser, MUTED, PANEL, format, motion, theme};
use gpui::*;
use picto_library::query::ItemScope;
use std::time::Instant;

/// Changing destination, as `WorkspaceSurface.tsx` stages it: the current grid
/// fades out, the surface stays hidden until the destination is ready, then
/// the destination fades in.
#[derive(Clone, Copy)]
pub(crate) enum Transition {
    Idle,
    FadingOut(Instant),
    Waiting,
    FadingIn(Instant),
}

impl Transition {
    /// Opacity of the grid surface and the scope title.
    pub(crate) fn opacity(&self) -> f32 {
        let eased = |started: &Instant| {
            motion::EASE_DEFAULT.ease(motion::progress(started.elapsed(), motion::SCOPE_FADE))
        };
        match self {
            Transition::Idle => 1.,
            Transition::FadingOut(started) => 1. - eased(started),
            Transition::Waiting => 0.,
            Transition::FadingIn(started) => eased(started),
        }
    }

    pub(crate) fn animating(&self) -> bool {
        matches!(self, Transition::FadingOut(_) | Transition::FadingIn(_))
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Section {
    Folders,
    SmartFolders,
}

fn section_of(scope: &ItemScope, screen: bool) -> Option<Section> {
    match scope {
        _ if screen => None,
        ItemScope::Folder { .. } | ItemScope::FolderTree { .. } => Some(Section::Folders),
        ItemScope::SmartFolder { .. } => Some(Section::SmartFolders),
        _ => None,
    }
}

fn section_header(label: &'static str, count: usize) -> Div {
    let mut header = div()
        .pl(px(16.))
        .mt(px(8.))
        .mb(px(2.))
        .h(px(24.))
        .flex()
        .items_center()
        .text_size(px(12.))
        .text_color(rgb(MUTED))
        .child(label);
    if count > 0 {
        header = header.child(format!("({count})"));
    }
    header
}

impl Browser {
    /// Opens a sidebar destination and records it for Back and Forward.
    pub(crate) fn navigate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.get(self.history_index) != Some(&index) {
            self.history.truncate(self.history_index + 1);
            self.history.push(index);
            self.history_index = self.history.len() - 1;
        }
        self.open(index, window, cx);
    }

    fn step_history(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.history_index.checked_add_signed(delta) else {
            return;
        };
        if let Some(&index) = self.history.get(target) {
            self.history_index = target;
            self.open(index, window, cx);
        }
    }

    fn open(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.scope = index;
        self.preview = false;
        self.pending_page = None;
        let screen = self.nav.get(index).is_some_and(|entry| entry.screen);
        self.transition = match self.transition {
            // Further clicks while the surface is fading out or already hidden
            // only retarget where it lands.
            Transition::FadingOut(_) | Transition::Waiting => self.transition,
            // Turning around mid fade-in continues from the current opacity.
            Transition::FadingIn(started) => {
                let shown = motion::progress(started.elapsed(), motion::SCOPE_FADE);
                Transition::FadingOut(Instant::now() - motion::SCOPE_FADE.mul_f32(1. - shown))
            }
            Transition::Idle => Transition::FadingOut(Instant::now()),
        };
        if screen {
            // Not a grid: nothing to fetch, and any reply still on its way for
            // the previous scope no longer applies.
            self.generation += 1;
            self.loading = false;
            if matches!(self.transition, Transition::Waiting) {
                self.commit_screen();
            }
            cx.notify();
        } else {
            self.query(false, cx);
        }
        window.focus(&self.focus, cx);
    }

    /// Shows a destination that is a screen, not a grid, on the hidden surface.
    fn commit_screen(&mut self) {
        self.items.clear();
        self.selected = 0;
        self.details = None;
        self.total = 0;
        self.cursor = None;
        self.layout_key = None;
        self.displayed = self.scope;
        self.transition = Transition::FadingIn(Instant::now());
    }

    /// Moves a scope change along once its current stage has run its time.
    pub(crate) fn advance_transition(&mut self, cx: &mut Context<Self>) {
        match self.transition {
            Transition::FadingOut(started) if started.elapsed() >= motion::SCOPE_FADE => {
                self.transition = Transition::Waiting;
                if self.nav.get(self.scope).is_some_and(|entry| entry.screen) {
                    self.commit_screen();
                } else if let Some((page, prefs)) = self.pending_page.take() {
                    self.apply_page(false, page, prefs, cx);
                }
            }
            Transition::FadingIn(started) if started.elapsed() >= motion::SCOPE_FADE => {
                self.transition = Transition::Idle;
            }
            _ => {}
        }
    }

    pub(crate) fn sidebar(&self, avatar: AnyElement, cx: &mut Context<Self>) -> Stateful<Div> {
        let mut nav = div()
            .id("navigation")
            .flex_1()
            .overflow_y_scroll()
            .pr(px(8.));
        let filter = self.folder_filter.read(cx).value().to_lowercase();
        let count_of = |section| {
            self.nav
                .iter()
                .filter(|entry| section_of(&entry.scope, entry.screen) == Some(section))
                .count()
        };
        let mut shown = [false; 2];
        for (i, entry) in self.nav.iter().enumerate() {
            let section = section_of(&entry.scope, entry.screen);
            // Both headers always show, in order, even above an empty section.
            for (slot, header, label) in [
                (0, Section::Folders, "Folders"),
                (1, Section::SmartFolders, "Smart Folders"),
            ] {
                let due = match section {
                    Some(current) => current == header || (slot == 0 && !shown[0]),
                    None => false,
                };
                if due && !shown[slot] {
                    shown[slot] = true;
                    nav = nav.child(section_header(label, count_of(header)));
                }
            }
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
            let icon = entry.icon.unwrap_or(match entry.scope {
                ItemScope::SmartFolder { .. } if entry.parent.is_none() => "layout-grid",
                _ => "folder",
            });
            let active = self.scope == i;
            let mut row = div()
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
                .on_click(cx.listener(move |this, _, window, cx| this.navigate(i, window, cx)))
                .child(
                    div()
                        .absolute()
                        .left(px(16. + entry.depth as f32 * 20.))
                        .right(px(2.))
                        .top_0()
                        .bottom(px(1.))
                        .rounded(px(6.))
                        .bg(if active {
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
                    div()
                        .flex_1()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(entry.label.clone()),
                );
            if let Some(count) = entry.count {
                row = row.child(
                    div()
                        .px(px(5.))
                        .text_size(px(11.))
                        .font_family("SF Mono")
                        .min_w(px(36.))
                        .text_right()
                        .text_color(rgb(if active { theme::TEXT } else { MUTED }))
                        .child(format::grouped(count)),
                );
            }
            nav = nav.child(row);
        }
        for (slot, header, label) in [
            (0, Section::Folders, "Folders"),
            (1, Section::SmartFolders, "Smart Folders"),
        ] {
            if !shown[slot] {
                nav = nav.child(section_header(label, count_of(header)));
            }
        }
        div()
            .id("sidebar")
            .w(px(300.))
            .flex_none()
            .h_full()
            .bg(rgb(PANEL))
            .border_r_1()
            .border_color(rgb(0x353638))
            .flex()
            .flex_col()
            .child(
                // The titlebar strip: traffic lights on the left, these on the right.
                div()
                    .h(px(48.))
                    .flex_none()
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(2.))
                    .child(theme::icon_button("open-settings", "settings"))
                    .child(theme::icon_button("toggle-sidebar", "toolbar-panel")),
            )
            .child(
                div()
                    .h(px(48.))
                    .mt(px(4.))
                    .mb(px(8.))
                    .pl(px(20.5))
                    .pr(px(14.))
                    .flex()
                    .items_center()
                    .text_size(px(14.))
                    .gap(px(6.))
                    .child(
                        div()
                            .size(px(39.))
                            .flex_none()
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
                    .child(div().flex_1().child("Main"))
                    .child(svg().path("selector").size(px(14.)).text_color(rgb(MUTED))),
            )
            .child(nav)
            .child(
                div().h(px(38.)).px(px(10.)).pb(px(10.)).flex_none().child(
                    div()
                        .h(px(28.))
                        .rounded(px(6.))
                        .border_1()
                        .border_color(rgb(0x303236))
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .child(
                            div().pl(px(8.)).flex_none().child(
                                svg()
                                    .path("toolbar-filter")
                                    .size(px(14.))
                                    .text_color(rgb(MUTED)),
                            ),
                        )
                        .child(div().flex_1().h_full().child(self.folder_filter.clone())),
                ),
            )
    }

    pub(crate) fn toolbar(&self, cx: &mut Context<Self>) -> Div {
        let can_go = |delta: isize| {
            self.history_index
                .checked_add_signed(delta)
                .is_some_and(|target| target < self.history.len())
        };
        let history = |id, icon, delta: isize, cx: &mut Context<Self>| {
            theme::icon_button(id, icon)
                .opacity(if can_go(delta) { 1. } else { 0.35 })
                .on_click(
                    cx.listener(move |this, _, window, cx| this.step_history(delta, window, cx)),
                )
        };
        div()
            .h(px(48.))
            .flex_none()
            .px(px(12.))
            .flex()
            .items_center()
            .gap(px(2.))
            .border_b_1()
            .border_color(rgb(0x303236))
            .child(history("history-back", "history-back", -1, cx))
            .child(history("history-forward", "history-forward", 1, cx))
            .child(
                div()
                    .ml(px(10.))
                    .w(px(160.))
                    .overflow_hidden()
                    .text_ellipsis()
                    .opacity(self.transition.opacity())
                    .child(
                        self.nav
                            .get(self.displayed)
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
            .child(theme::icon_button("view-options", "toolbar-layout"))
            .child(theme::icon_button("filter-library", "toolbar-filter"))
            .child(
                div()
                    .ml(px(4.))
                    .w(px(160.))
                    .h(px(24.))
                    .rounded(px(5.))
                    .border_1()
                    .border_color(rgb(0x303135))
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .pl(px(7.))
                            .flex_none()
                            .child(svg().path("search").size(px(14.)).text_color(rgb(MUTED))),
                    )
                    .child(div().flex_1().h_full().child(self.search.clone())),
            )
    }
}
