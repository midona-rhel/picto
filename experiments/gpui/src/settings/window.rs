//! The Settings window (`Settings.tsx`, `openSettingsWindow`): a separate
//! 900×650 window, at least that size, whose own close button replaces the
//! traffic lights. A navigation of sections on the left, the chosen section
//! in a raised panel on the right, and Save & Close and Apply below it.
use super::SettingsModel;
use super::panels::{self, PANELS};
use super::registry::Slot;
use crate::theme::Text as _;
use crate::ui::{self, action_button::Look, field::Look as FieldLook, text_input::TextInput};
use crate::{motion, theme};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use picto_library::LibraryStatistics;
use std::collections::HashMap;

/// The Cloud section's state, read from the library's `cloud_state`.
#[derive(Clone, Debug, Default)]
pub struct CloudInfo {
    pub provider: Option<String>,
    pub root_path: Option<String>,
    pub state: String,
    pub message: String,
    pub last_sync_at: Option<String>,
    pub pending_mutations: i64,
    pub pending_blobs: i64,
    pub missing_blobs: i64,
    /// Daily, weekly and yearly snapshots, and days deleted files are kept.
    pub retention: [i64; 4],
}

pub struct SettingsWindow {
    pub(super) model: Entity<SettingsModel>,
    /// The chosen section, by its place in `PANELS`.
    selected: usize,
    search: Entity<TextInput>,
    pub(super) shortcut_search: Entity<TextInput>,
    nav_scroll: ScrollHandle,
    body_scroll: ScrollHandle,
    pub(super) shortcuts_scroll: ScrollHandle,
    pub(super) statistics: Option<LibraryStatistics>,
    pub(super) cloud: Option<CloudInfo>,
    /// Shortcuts whose new keys another shortcut has: the other's label, and
    /// which conflict this was.
    pub(super) conflicts: HashMap<&'static str, (&'static str, u64)>,
    pub(super) conflict_generation: u64,
    /// The binding field listening for keys, and what it captured.
    pub(super) capturing: Option<(&'static str, Slot, Option<String>)>,
    pub(super) capture_focus: FocusHandle,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

/// Opens the window, or returns `None` if the platform refused one.
pub fn open(model: Entity<SettingsModel>, cx: &mut App) -> Option<WindowHandle<SettingsWindow>> {
    let mut bounds = Bounds::centered(None, size(px(900.), px(650.)), cx);
    bounds.origin = bounds.origin.map(|at| at.floor());
    let handle = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(900.), px(650.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Settings".into()),
                    appears_transparent: true,
                    traffic_light_position: None,
                }),
                // The section list's title and the panel's header move the
                // window; the controls in them do not.
                app_owns_titlebar_drag: true,
                ..Default::default()
            },
            move |window, cx| cx.new(|cx| SettingsWindow::new(model, window, cx)),
        )
        .ok()?;
    let _ = handle.update(cx, |_, window, _| {
        hide_window_buttons(window);
        window.activate_window();
    });
    Some(handle)
}

/// Settings draws its own close button, so the traffic lights are hidden
/// (`setWindowButtonVisibility(false)`). GPUI has no option for this, so it
/// asks AppKit directly.
#[cfg(target_os = "macos")]
fn hide_window_buttons(window: &Window) {
    use std::ffi::{c_char, c_void};
    use wgpu::rwh::{HasWindowHandle, RawWindowHandle};
    #[link(name = "objc", kind = "dylib")]
    unsafe extern "C" {
        fn sel_registerName(name: *const c_char) -> *const c_void;
        fn objc_msgSend();
    }
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return;
    };
    // SAFETY: `ns_view` is the live content view of this window; `window`,
    // `standardWindowButton:` and `setHidden:` are AppKit methods with these
    // signatures, called on the main thread.
    unsafe {
        let send_id: extern "C" fn(*mut c_void, *const c_void) -> *mut c_void =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let send_button: extern "C" fn(*mut c_void, *const c_void, usize) -> *mut c_void =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let send_bool: extern "C" fn(*mut c_void, *const c_void, bool) =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let ns_window = send_id(
            appkit.ns_view.as_ptr(),
            sel_registerName(c"window".as_ptr()),
        );
        if ns_window.is_null() {
            return;
        }
        // Close, miniaturize and zoom.
        for kind in 0..3 {
            let button = send_button(
                ns_window,
                sel_registerName(c"standardWindowButton:".as_ptr()),
                kind,
            );
            if !button.is_null() {
                send_bool(button, sel_registerName(c"setHidden:".as_ptr()), true);
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn hide_window_buttons(_: &Window) {}

impl SettingsWindow {
    fn new(model: Entity<SettingsModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| TextInput::form("Search...", cx));
        let shortcut_search = cx.new(|cx| TextInput::form("Search shortcuts…", cx));
        let capture_focus = cx.focus_handle();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let subscriptions = vec![
            cx.observe(&model, |_, _, cx| cx.notify()),
            cx.observe(&search, |_, _, cx| cx.notify()),
            cx.observe(&shortcut_search, |_, _, cx| cx.notify()),
            // A binding field stops listening when the keyboard leaves it.
            cx.on_blur(&capture_focus, window, |this, _, cx| {
                this.capturing = None;
                cx.notify();
            }),
        ];
        // Every section's data is read when the window opens, as production
        // preloads it, so choosing a section never shows it empty.
        let session = model.read(cx).session.clone();
        let read = session.read(cx).run(|library| {
            (
                library.library_statistics().ok(),
                panels::cloud_info(library),
            )
        });
        cx.spawn(async move |this, cx| {
            if let Ok((statistics, cloud)) = read.await {
                let _ = this.update(cx, |this, cx| {
                    this.statistics = statistics;
                    this.cloud = Some(cloud);
                    cx.notify();
                });
            }
        })
        .detach();
        Self {
            model,
            selected: 0,
            search,
            shortcut_search,
            nav_scroll: ScrollHandle::new(),
            body_scroll: ScrollHandle::new(),
            shortcuts_scroll: ScrollHandle::new(),
            statistics: None,
            cloud: None,
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
        self.body_scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    /// Discards the draft and closes the window (`handleClose`).
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.model.update(cx, |model, _| model.begin());
        window.remove_window();
    }

    /// Saves the draft, then closes the window if asked (`handleSave`).
    fn save(&mut self, close: bool, window: &mut Window, cx: &mut Context<Self>) {
        let saved = self.model.update(cx, |model, cx| model.save(cx));
        cx.spawn_in(window, async move |this, cx| {
            if saved.await && close {
                let _ = this.update_in(cx, |_, window, _| window.remove_window());
            }
        })
        .detach();
    }

    /// A section in the navigation (`.navItem`): its highlight fades in over
    /// 100 ms under the pointer, and the chosen one is solid.
    fn nav_item(
        &self,
        index: usize,
        active: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = theme::theme(cx);
        let panel = &PANELS[index];
        let hover = window.use_keyed_state(("settings-nav-hover", index), cx, |_, _| {
            motion::Fade::default()
        });
        if hover.read(cx).animating() {
            window.request_animation_frame();
        }
        let fill = if active {
            theme.surface_active()
        } else {
            theme::blend(
                gpui::transparent_black().into(),
                theme.surface_hover(),
                hover.read(cx).value_with(motion::EASE),
            )
        };
        div()
            .id(("settings-nav", index))
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
            .child(panel.label)
            .on_hover(move |hovered, _, cx| {
                hover.update(cx, |hover, cx| {
                    hover.set(*hovered, motion::ROW_HOVER);
                    cx.notify();
                })
            })
            .on_click(cx.listener(move |this, _, _, cx| this.choose(index, cx)))
    }

    /// A section found by the search (`.searchResult`).
    fn result(&self, index: usize, first: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let panel = &PANELS[index];
        div()
            .id(("settings-result", index))
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
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(panel.label),
                    )
                    .child(
                        div()
                            .text(theme::TEXT_SM)
                            .line_height(px(theme::TEXT_SM * 1.4))
                            .text_color(theme.text_secondary())
                            .child(panel.description),
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.choose(index, cx)))
    }
}

/// The navigation's glyphs, drawn at stroke 1.8.
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

/// The search results' glyphs, at Tabler's own stroke of 2.
fn result_icon(name: &str) -> &'static str {
    match name {
        "settings-2" => "settings-panel-settings-2",
        "refresh" => "settings-panel-refresh",
        "library" => "settings-panel-library",
        "layout-sidebar" => "settings-panel-layout-sidebar",
        "adjustments-horizontal" => "settings-panel-adjustments-horizontal",
        "eye" => "settings-panel-eye",
        "command" => "settings-panel-command",
        "bell" => "settings-panel-bell",
        "folder-down" => "settings-panel-folder-down",
        "trash" => "settings-panel-trash",
        "download" => "settings-panel-download",
        "cloud" => "settings-panel-cloud",
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

        // ── The section list ──
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
            // 12px of padding, 200px of content less the scrollbar's gutter.
            .w(px(212.))
            .flex_none()
            .h_full()
            .child(
                div()
                    .id("settings-sections")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.nav_scroll)
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
                                    .child("Preferences"),
                            )
                            .child(
                                div().mb(px(8.)).flex_none().child(
                                    ui::field(
                                        "settings-search",
                                        &self.search,
                                        FieldLook::SettingsSearch,
                                    )
                                    .icon(
                                        "settings-search",
                                        14.,
                                        10.,
                                        theme.text_tertiary(),
                                    ),
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

        // ── The chosen section, or what the search found ──
        let panel = &PANELS[self.selected];
        let heading: SharedString = if searching {
            format!("Search results for \"{typed}\"").into()
        } else {
            panel.label.into()
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
                window,
                cx,
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
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text_secondary())
                        .child("CATEGORIES"),
                );
                for (at, index) in matched.iter().enumerate() {
                    group = group.child(self.result(*index, at == 0, cx));
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
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&self.body_scroll)
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
                    .label(if saving { "Saving…" } else { "Save & Close" })
                    .disabled(saving)
                    .tip(ui::tip("Save settings and close Preferences"))
                    .on_click(cx.listener(|this, _, window, cx| this.save(true, window, cx))),
            )
            .child(
                ui::action_button("settings-apply", Look::Settings)
                    .label("Apply")
                    .disabled(saving)
                    .tip(ui::tip("Save settings and keep Preferences open"))
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
            .bg(theme.contrast(3.))
            .shadow(vec![
                BoxShadow::new(px(0.), px(0.), rgba(0x00000026).into()).spread_radius(px(1.)),
                BoxShadow::new(px(0.), px(2.), rgba(0x0000001a).into()).blur_radius(px(8.)),
            ])
            .child(header)
            .child(body)
            .child(footer);

        let mut root = div()
            .relative()
            .size_full()
            .flex()
            .gap(px(12.))
            .pr(px(8.))
            .overflow_hidden()
            // `--settings-shell-bg`: 3% contrast over the background.
            .bg(theme.rail())
            .text_color(theme.text())
            .font_family(theme::FONT_UI)
            .text(theme::TEXT_MD)
            .line_height(px(theme::LINE_HEIGHT))
            .track_focus(&self.focus)
            .key_context("Settings")
            .child(sidebar)
            .child(content);
        if cfg!(target_os = "macos") {
            root = root.child(ui::window_chrome::rim());
        }
        root
    }
}
