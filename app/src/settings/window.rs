//! The Settings window. It is a separate window that opens at 900×650 and
//! cannot be made smaller than that. It draws its own close button, and on
//! macOS it hides the traffic lights. On the left is the list of sections.
//! On the right is a panel with a border and a shadow. The panel holds a
//! header with the section's name and the close button, the chosen section
//! below the header, and the Save & Close and Apply buttons below the
//! section.
use super::SettingsModel;
use super::panels::{self, PANELS};
use super::registry::Slot;
use crate::i18n::t;
use crate::theme::Text as _;
use crate::ui::{
    self,
    action_button::Look,
    field::Look as FieldLook,
    text_input::{Kind, TextInput},
};
use crate::{motion, theme};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use picto_library::LibraryStatistics;
use std::collections::HashMap;

pub struct SettingsWindow {
    pub(super) model: Entity<SettingsModel>,
    /// The chosen section. It is stored as its index in `PANELS`.
    selected: usize,
    search: Entity<TextInput>,
    pub(super) shortcut_search: Entity<TextInput>,
    nav_scroll: ScrollHandle,
    body_scroll: ScrollHandle,
    /// The focus handles that the two scrolling elements track. `nav_focus`
    /// belongs to the list of sections and `body_focus` to the body of the
    /// chosen section. When the Tab key moves keyboard focus to a control
    /// inside one of the two, `ui::focus::follow` uses the handle to find
    /// which one, and scrolls it so that the control is in view.
    nav_focus: FocusHandle,
    body_focus: FocusHandle,
    pub(super) shortcuts_scroll: ScrollHandle,
    pub(super) statistics: Option<LibraryStatistics>,
    /// `cloud` is the cloud sync state of the library, which the session
    /// holds. `cloud_panel` is the state of the Cloud section itself: the
    /// restore points, whether an operation is under way, and the restore
    /// that the user is asked to confirm.
    pub(super) cloud: Option<Entity<crate::cloud::Cloud>>,
    pub(super) cloud_panel: super::panels::CloudPanel,
    /// The state of the AI Models section, whose id is `aitagging`: the
    /// status of the AI model runtime, and the downloads and optimizations
    /// that are under way.
    pub(super) ai: super::panels::AiState,
    /// The shortcuts that the user tried to bind to keys that another
    /// shortcut already has. The key is the shortcut's id. The value is the
    /// label of the other shortcut, and the value that `conflict_generation`
    /// had when the conflict was recorded. For a conflict of a shortcut's
    /// first binding, a timer removes the entry after 1.5 s. The timer
    /// compares that number first, so that it does not remove a newer
    /// conflict of the same shortcut.
    pub(super) conflicts: HashMap<&'static str, (&'static str, u64)>,
    pub(super) conflict_generation: u64,
    /// The binding field that is waiting for the user to press keys, or
    /// `None` when no field is. It holds the shortcut's id, which of the
    /// shortcut's two bindings the field is, and the keys that the field
    /// has captured, as the text it shows. The keys are `None` until the
    /// user has pressed some.
    pub(super) capturing: Option<(&'static str, Slot, Option<String>)>,
    pub(super) capture_focus: FocusHandle,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

/// Opens the Settings window. Returns `None` if the system could not open a
/// window.
pub fn open(model: Entity<SettingsModel>, cx: &mut App) -> Option<WindowHandle<SettingsWindow>> {
    let mut bounds = Bounds::centered(None, size(px(900.), px(650.)), cx);
    bounds.origin = bounds.origin.map(|at| at.floor());
    let handle = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(900.), px(650.))),
                // The user moves the window by dragging the title above the
                // list of sections or the header of the panel. Pressing a
                // control inside either of them does not move the window.
                // i18n: the window's title, which is English in every language
                ..ui::window_chrome::frameless("Settings", None)
            },
            move |window, cx| cx.new(|cx| SettingsWindow::new(model, window, cx)),
        )
        .ok()?;
    let _ = handle.update(cx, |_, window, _| {
        // The Settings window draws its own close button, so on macOS the
        // traffic lights are hidden.
        ui::window_chrome::hide_window_buttons(window);
        ui::window_chrome::activate(window);
    });
    Some(handle)
}

impl SettingsWindow {
    fn new(model: Entity<SettingsModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| TextInput::new(Kind::Form, cx).placeholder("Search..."));
        let shortcut_search =
            cx.new(|cx| TextInput::new(Kind::Form, cx).placeholder("Search shortcuts…"));
        let capture_focus = cx.focus_handle();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let updates = crate::updates::entity(cx);
        let subscriptions = vec![
            cx.observe(&model, |_, _, cx| cx.notify()),
            cx.observe(&search, |_, _, cx| cx.notify()),
            cx.observe(&shortcut_search, |_, _, cx| cx.notify()),
            // The Updates section is drawn again when the state of the check
            // for updates or of the download changes.
            cx.observe(&updates, |_, _, cx| cx.notify()),
            // A binding field stops waiting for keys when it loses keyboard
            // focus.
            cx.on_blur(&capture_focus, window, |this, _, cx| {
                this.capturing = None;
                cx.notify();
            }),
        ];
        // The data of every section is read when the window opens, so that
        // a section is not shown empty when the user chooses it.
        let session = model.read(cx).session.clone();
        let read = session
            .read(cx)
            .run(|library| library.library_statistics().ok());
        cx.spawn(async move |this, cx| {
            if let Ok(statistics) = read.await {
                let _ = this.update(cx, |this, cx| {
                    this.statistics = statistics;
                    cx.notify();
                });
            }
        })
        .detach();
        // The cloud sync state is the entity that the shell created and the
        // session holds. The window observes it and draws again when it
        // changes. The restore points are read by this window: once when it
        // opens, and again each time the library reports a change that
        // names the resource `cloud`.
        let cloud = session.read(cx).cloud.clone();
        let mut subscriptions = subscriptions;
        if let Some(cloud) = &cloud {
            subscriptions.push(cx.observe(cloud, |_, _, cx| cx.notify()));
            subscriptions.push(cx.subscribe(
                &session,
                |this, _, event: &crate::application::Event, cx| {
                    if let crate::application::Event::Changed(changed) = event
                        && crate::application::touches(changed, "cloud")
                    {
                        this.refresh_restore_points(cx);
                    }
                },
            ));
        }
        cx.defer_in(window, |this, _, cx| this.refresh_restore_points(cx));
        // The status of the AI model runtime is read when the window opens
        // too. Reading it does not load a model: it checks which models'
        // files are in the models' folder.
        cx.defer_in(window, |this, _, cx| this.refresh_ai(cx));
        Self {
            model,
            selected: 0,
            search,
            shortcut_search,
            nav_scroll: ScrollHandle::new(),
            body_scroll: ScrollHandle::new(),
            nav_focus: cx.focus_handle(),
            body_focus: cx.focus_handle(),
            shortcuts_scroll: ScrollHandle::new(),
            statistics: None,
            cloud,
            cloud_panel: Default::default(),
            ai: Default::default(),
            conflicts: HashMap::new(),
            conflict_generation: 0,
            capturing: None,
            capture_focus,
            focus,
            _subscriptions: subscriptions,
        }
    }

    fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        self.selected = index;
        self.search.update(cx, |search, cx| search.clear(cx));
        // All the sections share one scrolling body, and its scroll offset
        // is kept when the section changes. If the new section is shorter
        // than the offset, it is shown scrolled to its end.
        cx.notify();
    }

    /// Shows the section with the id `id`, or does nothing when no section
    /// has that id.
    pub(super) fn show_panel(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(index) = PANELS.iter().position(|panel| panel.id == id) {
            self.choose(index, cx);
        }
    }

    /// Discards the draft and closes the window. Closing also puts back the
    /// saved theme and the saved choice for the frosted material of the
    /// sidebar and the inspector, because a change to either of them is
    /// shown at once while the window is open, before it is saved.
    pub(super) fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.model.update(cx, |model, _| model.begin());
        let saved = &self.model.read(cx).saved.app;
        let (scheme, rail_material) = (saved.color_scheme.clone(), saved.rail_material());
        theme::show(&scheme, cx);
        ui::window_chrome::want_rail_material(rail_material, cx);
        window.remove_window();
    }

    /// Saves the draft and then, if `close` is `true` and the save
    /// succeeded, closes the window.
    fn save(&mut self, close: bool, window: &mut Window, cx: &mut Context<Self>) {
        let saved = self.model.update(cx, |model, cx| model.save(cx));
        cx.spawn_in(window, async move |this, cx| {
            if saved.await && close {
                let _ = this.update_in(cx, |_, window, _| window.remove_window());
            }
        })
        .detach();
    }

    /// Draws the row of one section in the list of sections. `index` is the
    /// section's index in `PANELS`, and `active` is whether it is the chosen
    /// section. When the pointer moves onto the row, the hover colour fades
    /// in behind it over 100 ms, and it fades out when the pointer leaves.
    /// The row of the chosen section has the active colour behind it
    /// instead, with no fade.
    fn nav_item(
        &self,
        index: usize,
        active: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = theme::theme(cx);
        let panel = &PANELS[index];
        let (hover, set_hover) = motion::hover_fade(
            ("settings-nav-hover", index),
            motion::ROW_HOVER,
            motion::EASE,
            window,
            cx,
        );
        let fill = if active {
            theme.surface_active()
        } else {
            theme::blend(
                gpui::transparent_black().into(),
                theme.surface_hover(),
                hover,
            )
        };
        let id = ElementId::from(("settings-nav", index));
        let focus = ui::focus::control(&id, window, cx);
        // Assistive technology is told that each section's row is a tab
        // and that the row of the chosen section is the selected tab.
        let item = div()
            .id(id)
            .role(Role::Tab)
            .aria_label(t(panel.label))
            .aria_selected(active)
            .h(px(32.))
            .flex_none()
            .px(px(6.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded(px(theme::RADIUS_MD))
            .bg(fill)
            .text(theme::TEXT_MD)
            .line_height(px(20.))
            .text_color(theme.text())
            .cursor_pointer()
            .child(ui::icon(nav_icon(panel.icon), 18., theme.text()))
            .child(t(panel.label))
            .on_hover(set_hover)
            .on_click(cx.listener(move |this, _, _, cx| this.choose(index, cx)));
        // Enter and Space choose the section while the row has keyboard
        // focus. The row draws no focus ring.
        focus.attach(
            item,
            ui::focus::Kind::Button,
            cx.listener(move |this, _, _, cx| this.choose(index, cx)),
        )
    }

    /// Draws one section that the search found, as a card with the
    /// section's icon, label and description. Clicking the card chooses the
    /// section. `first` is whether the card is the first in the list. Every
    /// other card has 8px of space above it.
    fn result(
        &self,
        index: usize,
        first: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = theme::theme(cx);
        let panel = &PANELS[index];
        let id = ElementId::from(("settings-result", index));
        let focus = ui::focus::control(&id, window, cx);
        // The focus ring is a line 1px thick in the theme's focus border
        // colour, 2px outside the card's edge.
        let ring = focus.ring(
            ui::focus::Ring {
                width: 1.,
                offset: 2.,
                color: theme.border_focus(),
            },
            theme::RADIUS_LG,
            1.,
        );
        let card = div()
            .id(id)
            .role(Role::Button)
            .aria_label(format!("{} {}", t(panel.label), t(panel.description)))
            .relative()
            .children(ring)
            .w_full()
            .when(!first, |card| card.mt(px(8.)))
            .flex()
            .items_start()
            .gap(px(10.))
            .py(px(10.))
            .px(px(12.))
            .rounded(px(theme::RADIUS_LG))
            .border_1()
            .border_color(theme.border_secondary())
            .bg(theme.footer_surface())
            .hover(move |style| style.bg(theme.surface_hover()))
            .text_color(theme.text())
            .cursor_pointer()
            .child(ui::icon(result_icon(panel.icon), 18., theme.text()))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text(theme::TEXT_MD)
                            .line_height(px(theme::TEXT_MD * 1.55))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t(panel.label)),
                    )
                    .child(
                        div()
                            .text(theme::TEXT_SM)
                            .line_height(px(theme::TEXT_SM * 1.4))
                            .text_color(theme.text_secondary())
                            .child(t(panel.description)),
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.choose(index, cx)));
        focus.attach(
            card,
            ui::focus::Kind::Button,
            cx.listener(move |this, _, _, cx| this.choose(index, cx)),
        )
    }
}

/// Returns the name of the icon that the list of sections draws for a
/// section whose `PanelDef::icon` is `name`. These icons are drawn with a
/// stroke width of 1.8. Any name that is not listed, which includes
/// `info-circle`, gives `settings-nav-info-circle`.
fn nav_icon(name: &str) -> &'static str {
    match name {
        "settings-2" => "settings-nav-settings-2",
        "refresh" => "settings-nav-refresh",
        "library" => "settings-nav-library",
        "layout-sidebar" => "settings-nav-layout-sidebar",
        "adjustments-horizontal" => "settings-nav-adjustments-horizontal",
        "eye" => "settings-nav-eye",
        "command" => "settings-nav-command",
        "bell" => "settings-nav-bell",
        "folder-down" => "settings-nav-folder-down",
        "trash" => "settings-nav-trash",
        "download" => "settings-nav-download",
        "cloud" => "settings-nav-cloud",
        "box" => "settings-nav-box",
        _ => "settings-nav-info-circle",
    }
}

/// Returns the name of the icon that a search result draws for a section
/// whose `PanelDef::icon` is `name`. These icons are drawn with Tabler's
/// own stroke width of 2. Any name that is not listed, which includes
/// `info-circle`, gives `settings-panel-info-circle`.
fn result_icon(name: &str) -> &'static str {
    match name {
        "settings-2" => "settings-panel-settings-2",
        "refresh" => "refresh",
        "library" => "settings-panel-library",
        "layout-sidebar" => "settings-panel-layout-sidebar",
        "adjustments-horizontal" => "settings-panel-adjustments-horizontal",
        "eye" => "settings-panel-eye",
        "command" => "settings-panel-command",
        "bell" => "settings-panel-bell",
        "folder-down" => "settings-panel-folder-down",
        "trash" => "trash-2",
        "download" => "settings-panel-download",
        "cloud" => "cloud-hero",
        "box" => "settings-panel-box",
        _ => "settings-panel-info-circle",
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let typed = self.search.read(cx).value();
        let query = typed.trim().to_lowercase();
        let searching = !query.is_empty();
        let matched = panels::matched_panels(&query);
        let (saving, error) = {
            let model = self.model.read(cx);
            (model.saving, model.error.clone())
        };

        // The list of sections on the left of the window. While the user is
        // searching, it lists only the sections that the search found.
        let mut items = div().flex().flex_col().gap(px(2.));
        let listed: Vec<usize> = if searching {
            matched.clone()
        } else {
            (0..PANELS.len()).collect()
        };
        for index in listed {
            if PANELS[index].separator_before {
                items = items.child(
                    div()
                        .h(px(1.))
                        .my(px(2.))
                        .flex_none()
                        .bg(theme.border_secondary()),
                );
            }
            let active = index == self.selected && !searching;
            items = items.child(self.nav_item(index, active, window, cx));
        }
        let sidebar = div()
            .relative()
            // The width is 12px of padding on the left and 200px for the
            // content. The scrollbar's gutter on the right is part of the
            // 200px.
            .w(px(212.))
            .flex_none()
            .h_full()
            .child(
                div()
                    .id("settings-sections")
                    .role(Role::TabList)
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.nav_scroll)
                    .track_focus(&self.nav_focus)
                    .pt(px(8.))
                    .pl(px(12.))
                    .pr(px(ui::scrollbar::GUTTER))
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .child(
                                ui::drag_region("settings-title", window, cx)
                                    .h(px(theme::TITLEBAR_HEIGHT))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .text(theme::TEXT_LG)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.text())
                                    .child(t("Preferences")),
                            )
                            .child(
                                div().mb(px(8.)).flex_none().child(
                                    ui::field(
                                        "library-search",
                                        &self.search,
                                        FieldLook::SettingsSearch,
                                    )
                                    .icon("library-search", 14., 10., theme.text_tertiary())
                                    .clearable(),
                                ),
                            )
                            .child(items),
                    ),
            )
            .child(ui::scrollbar(
                "settings-sections-scrollbar",
                &self.nav_scroll,
                window,
                cx,
            ));

        ui::focus::follow(&self.nav_scroll, &self.nav_focus, window, cx);
        ui::focus::follow(&self.body_scroll, &self.body_focus, window, cx);

        // The panel on the right of the window. It shows the chosen section
        // or, while the user is searching, the sections that the search
        // found.
        let panel = &PANELS[self.selected];
        let heading: SharedString = if searching {
            crate::tf!("Search results for \"{value0}\"", typed).into()
        } else {
            t(panel.label).into()
        };
        let header = ui::drag_region("settings-header", window, cx)
            .min_h(px(theme::TITLEBAR_HEIGHT))
            .flex_none()
            .pl(px(16.))
            .pr(px(12.))
            .flex()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text(theme::TEXT_LG)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(heading),
            )
            .child(ui::window_chrome::close_button(
                "settings-close",
                cx.listener(|this, _, window, cx| this.close(window, cx)),
            ));
        let body = if !searching && panel.id == "shortcuts" {
            div()
                .flex_1()
                .min_h_0()
                .pl(px(16.))
                .pr(px(16. + ui::scrollbar::GUTTER))
                .pb(px(16.))
                .child(self.shortcuts_panel(window, cx))
                .into_any_element()
        } else {
            let content: Vec<AnyElement> = if !searching {
                self.panel(panel.id, window, cx)
            } else if matched.is_empty() {
                vec![
                    div()
                        .p(px(32.))
                        .flex()
                        .justify_center()
                        .text(theme::TEXT_MD)
                        .text_color(theme.text_tertiary())
                        .child(format!("No settings match \"{typed}\""))
                        .into_any_element(),
                ]
            } else {
                let mut group = div().mb(px(20.)).flex().flex_col().child(
                    div()
                        .mb(px(8.))
                        .text(theme::TEXT_SM)
                        // The heading has the line height of body text,
                        // which is 1.55 times the font size.
                        .line_height(px(theme::TEXT_SM * 1.55))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text_secondary())
                        // The heading is shown in capital letters.
                        .child(t("Categories").to_uppercase()),
                );
                for (at, index) in matched.iter().enumerate() {
                    group = group.child(self.result(*index, at == 0, window, cx));
                }
                vec![group.into_any_element()]
            };
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .child(
                    div()
                        .id("settings-body")
                        .role(Role::TabPanel)
                        .aria_label(t(PANELS[self.selected].label))
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&self.body_scroll)
                        .track_focus(&self.body_focus)
                        .child(
                            div()
                                .w_full()
                                .flex()
                                .flex_col()
                                .pl(px(16.))
                                .pr(px(16. + ui::scrollbar::GUTTER))
                                .pb(px(16.))
                                .children(content),
                        ),
                )
                .child(ui::scrollbar(
                    "settings-body-scrollbar",
                    &self.body_scroll,
                    window,
                    cx,
                ))
                .into_any_element()
        };
        let footer = div()
            .min_h(px(60.))
            .flex_none()
            .px(px(12.))
            .flex()
            .items_center()
            .justify_end()
            .gap(px(8.))
            .border_t_1()
            .border_color(theme.border_secondary())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text(theme::TEXT_SM)
                    .text_color(theme.negative())
                    .children(error),
            )
            .child(
                ui::action_button("settings-save", Look::SettingsPrimary)
                    .label(if saving {
                        t("Saving…")
                    } else {
                        t("Save & Close")
                    })
                    .disabled(saving)
                    .tip(ui::tip(t("Save settings and close Preferences")))
                    .on_click(cx.listener(|this, _, window, cx| this.save(true, window, cx))),
            )
            .child(
                ui::action_button("settings-apply", Look::Settings)
                    .label(t("Apply"))
                    .disabled(saving)
                    .tip(ui::tip(t("Save settings and keep Preferences open")))
                    .on_click(cx.listener(|this, _, window, cx| this.save(false, window, cx))),
            );
        let content = div()
            .flex_1()
            .min_w_0()
            .my(px(8.))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(theme::RADIUS_LG))
            .border_1()
            .border_color(theme.contrast(5.))
            // The fill is the theme's contrast colour at 3% laid over the
            // window's background, as one opaque colour
            // (`Theme::rail_raised`).
            .bg(theme.rail_raised())
            .shadow(vec![
                BoxShadow::new(px(0.), px(0.), rgba(theme::BLACK_15).into()).spread_radius(px(1.)),
                BoxShadow::new(px(0.), px(2.), rgba(theme::BLACK_10).into()).blur_radius(px(8.)),
            ])
            .child(header)
            .child(body)
            .child(footer);

        // Tab and Shift-Tab move keyboard focus through the window's
        // controls. Tab on the last control moves it to the first, and
        // Shift-Tab on the first control moves it to the last.
        let root = ui::focus::scope(div(), &self.focus)
            .relative()
            .size_full()
            .flex()
            .gap(px(12.))
            .pr(px(8.))
            .overflow_hidden()
            // The window's background is the theme's background colour
            // mixed with 3% of its contrast colour (`Theme::rail`).
            .bg(theme.rail())
            .text_color(theme.text())
            .font_family(theme::FONT_UI)
            .text(theme::TEXT_MD)
            .line_height(px(theme::LINE_HEIGHT))
            .track_focus(&self.focus)
            .key_context("Settings") // i18n: a key context
            .child(sidebar)
            .child(content);
        root.children(self.restore_question(window, cx))
            .children(ui::window_chrome::resize_edges(window))
            .children(ui::window_chrome::rim(window, cx))
    }
}
