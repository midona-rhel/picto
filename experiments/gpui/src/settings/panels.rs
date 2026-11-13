//! The Settings window's sections and their rows, in production's order and
//! wording (`Settings.tsx`, `AiTaggingPanel.tsx`), and the pieces they are
//! built from (`Settings.module.css`).
use super::model::{NOTIFICATION_TONES, Threshold};
use super::window::{CloudInfo, SettingsWindow};
use super::{Settings, SettingsModel};
use crate::grid::format::grouped;
use crate::theme::Text as _;
use crate::ui::{self, action_button::Look, choice};
use crate::{motion, theme};
use gpui::*;

/// A section of the window (`PANELS`).
pub struct PanelDef {
    pub id: &'static str,
    pub label: &'static str,
    /// The Tabler glyph, drawn at 1.8 in the navigation and at 2 in search
    /// results.
    pub icon: &'static str,
    /// Terms the settings search matches for this section.
    pub keywords: &'static str,
    pub description: &'static str,
    pub separator_before: bool,
}

const fn panel(
    id: &'static str,
    label: &'static str,
    icon: &'static str,
    keywords: &'static str,
    description: &'static str,
    separator_before: bool,
) -> PanelDef {
    PanelDef {
        id,
        label,
        icon,
        keywords,
        description,
        separator_before,
    }
}

pub const PANELS: [PanelDef; 14] = [
    panel(
        "general",
        "General",
        "settings-2",
        "general appearance theme color light dark gray blue purple zoom tags group namespace prefix compact",
        "Appearance and zoom.",
        false,
    ),
    panel(
        "updates",
        "Updates",
        "refresh",
        "updates software version release notes download install latest",
        "Check for new versions of Picto.",
        false,
    ),
    panel(
        "library",
        "Library",
        "library",
        "library statistics media images video audio files size all inbox trash tags folders smart subscriptions collections",
        "Current library contents and storage.",
        false,
    ),
    panel(
        "sidebar",
        "Sidebar",
        "layout-sidebar",
        "sidebar navigation folder tree guides hierarchy lines",
        "Sidebar and folder-tree presentation.",
        false,
    ),
    panel(
        "controls",
        "Controls",
        "adjustments-horizontal",
        "controls grid layout thumbnails spacing density wide tight sort order name resolution extension label count fit",
        "Default grid controls and item presentation.",
        true,
    ),
    panel(
        "preview",
        "Preview",
        "eye",
        "preview image video audio scaling pixelated zoom transparency autoplay loop visualization visualizer spectrum oscilloscope orbit",
        "Media preview behavior.",
        false,
    ),
    panel(
        "shortcuts",
        "Shortcuts",
        "command",
        "shortcuts keyboard shortcut keybind hotkey binding command key layout qwerty qwertz azerty eu us european preset",
        "Keyboard layout and shortcut bindings.",
        false,
    ),
    panel(
        "notifications",
        "Notifications",
        "bell",
        "notifications alerts popups success information warnings errors",
        "Choose which actions show pop-up notifications.",
        true,
    ),
    panel(
        "autoimport",
        "Auto-Import",
        "folder-down",
        "auto import watched folder watch files inbox recursive subfolders",
        "Control imports from watched folders.",
        false,
    ),
    panel(
        "deleting",
        "Inbox & Deleting",
        "trash",
        "inbox reject rejected trash delete permanently undo restore empty",
        "What rejecting does, and how much a delete can be undone.",
        false,
    ),
    panel(
        "subscriptions",
        "Subscriptions",
        "download",
        "subscriptions defaults schedule daily weekly monthly posts run group multi media",
        "Defaults for new subscriptions and source queries.",
        false,
    ),
    panel(
        "cloud",
        "Cloud",
        "cloud",
        "cloud sync backup restore google drive dropbox offline retention snapshots",
        "Library sync, recovery snapshots, and missing files.",
        false,
    ),
    panel(
        "aitagging",
        "AI Models",
        "box",
        "ai tagging tagger models model download threshold confidence auto tag rating",
        "Local models, confidence thresholds, and auto-tag behavior.",
        true,
    ),
    panel(
        "about",
        "About",
        "info-circle",
        "about picto version build copyright license github website release",
        "Version, project information, and licenses.",
        true,
    ),
];

/// `ALL_SETTINGS`: individual settings the search finds, by section.
const ALL_SETTINGS: [(&str, &str, &str); 14] = [
    (
        "Appearance",
        "theme color light dark gray blue purple zoom",
        "general",
    ),
    (
        "Language",
        "language locale translation english german spanish portuguese french chinese japanese finnish",
        "general",
    ),
    (
        "Keyboard Layout",
        "keyboard layout qwerty qwertz azerty eu us european preset shortcut",
        "shortcuts",
    ),
    (
        "Folder Tree",
        "sidebar folder hierarchy tree guides lines",
        "sidebar",
    ),
    (
        "Grid Defaults",
        "grid layout thumbnail size spacing density wide tight sort name resolution extension label count fit",
        "controls",
    ),
    (
        "Image Preview",
        "image preview scaling smooth pixelated zoom fit actual transparency checkerboard",
        "preview",
    ),
    (
        "Video Preview",
        "video preview autoplay loop playback",
        "preview",
    ),
    (
        "Audio Preview",
        "audio preview visualization spectrum oscilloscope orbit",
        "preview",
    ),
    (
        "Notifications",
        "notifications alerts popups success information warnings errors",
        "notifications",
    ),
    (
        "Rejected items",
        "inbox reject rejected trash delete permanently",
        "deleting",
    ),
    (
        "Undo limit (media items)",
        "undo delete permanently restore trash empty limit",
        "deleting",
    ),
    (
        "Auto-Import",
        "auto import watched folder watch files inbox recursive subfolders",
        "autoimport",
    ),
    (
        "Subscription Defaults",
        "subscriptions schedule daily weekly monthly posts per run group multi media inbox maximum limit capacity",
        "subscriptions",
    ),
    (
        "Cloud Sync and Recovery",
        "cloud sync backup restore google drive dropbox snapshots retention offline",
        "cloud",
    ),
];

/// The sections a search finds (`matchedPanels`): by their label, their
/// keywords, or a setting of theirs whose label or keywords match. The
/// query is already trimmed and lower-cased.
pub fn matched_panels(query: &str) -> Vec<usize> {
    if query.is_empty() {
        return Vec::new();
    }
    PANELS
        .iter()
        .enumerate()
        .filter(|(_, panel)| {
            panel.label.to_lowercase().contains(query)
                || panel.keywords.contains(query)
                || ALL_SETTINGS.iter().any(|(label, keywords, of)| {
                    *of == panel.id
                        && (label.to_lowercase().contains(query) || keywords.contains(query))
                })
        })
        .map(|(index, _)| index)
        .collect()
}

/// `formatBytes`
pub fn format_bytes(value: i64) -> String {
    if value < 1024 {
        return format!("{value} B");
    }
    let units = ["KB", "MB", "GB", "TB"];
    let mut amount = value as f64 / 1024.;
    let mut unit = 0;
    while amount >= 1024. && unit < units.len() - 1 {
        amount /= 1024.;
        unit += 1;
    }
    let digits = if amount >= 10. { 1 } else { 2 };
    format!("{amount:.digits$} {}", units[unit])
}

/// `fmtSize` in the AI Models panel: decimal gigabytes with trailing zeros
/// dropped, else whole megabytes.
pub fn model_size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        let text = format!("{:.2}", bytes as f64 / 1e9);
        let text = text.trim_end_matches('0').trim_end_matches('.');
        format!("{text} GB")
    } else {
        format!("{} MB", (bytes as f64 / 1e6).round())
    }
}

/// `formatCloudDate`: a stored timestamp in the local time, as
/// `toLocaleString` writes it in English.
fn cloud_date(value: Option<&str>) -> String {
    let Some(value) = value else {
        return "Never".into();
    };
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|date| {
            date.with_timezone(&chrono::Local)
                .format("%-m/%-d/%Y, %-I:%M:%S %p")
                .to_string()
        })
        .unwrap_or_else(|_| value.to_owned())
}

/// The AI models production registers (`known_models`), shown before any
/// is downloaded.
struct ModelInfo {
    slug: &'static str,
    short: &'static str,
    label: &'static str,
    dataset: &'static str,
    size: u64,
    ms: f32,
}

const MODELS: [ModelInfo; 4] = [
    ModelInfo {
        slug: "wd14-swinv2-v3",
        short: "WD14 SWN",
        label: "WD14 SwinV2 v3",
        dataset: "Danbooru · general, character, rating",
        size: 467_000_000,
        ms: 17.74,
    },
    ModelInfo {
        slug: "z3d-e621-convnext",
        short: "Z3D",
        label: "E621 ConvNext (Z3D)",
        dataset: "e621 · general, creator, series, character, species",
        size: 390_000_000,
        ms: 14.71,
    },
    ModelInfo {
        slug: "wd14-eva02-large-v3",
        short: "WD14 EVA",
        label: "WD14 EVA02-Large v3",
        dataset: "Danbooru · general, character, rating · Highest accuracy",
        size: 1_260_000_000,
        ms: 48.38,
    },
    ModelInfo {
        slug: "oppai-oracle-v1-1",
        short: "OppaiOracle",
        label: "OppaiOracle V1.1",
        dataset: "Danbooru · wide-vocabulary general tags",
        size: 993_246_982,
        ms: 86.45,
    },
];

const THRESHOLDS: [(Threshold, &str, &str); 6] = [
    (Threshold::General, "General", "general"),
    (Threshold::Character, "Character", "character"),
    (Threshold::Species, "Species", "species"),
    (Threshold::Copyright, "Series", "series"),
    (Threshold::Artist, "Creator", "creator"),
    (Threshold::Rating, "Rating", "rating"),
];

/// `ZOOM_OPTIONS`
const ZOOM: [(&str, &str); 7] = [
    ("75", "75%"),
    ("80", "80%"),
    ("90", "90%"),
    ("100", "100%"),
    ("110", "110%"),
    ("125", "125%"),
    ("150", "150%"),
];

/// `THEMES`: the name, stored value and swatch colour of each theme.
const THEMES: [(&str, &str, Option<u32>); 7] = [
    ("Auto", "auto", None),
    ("Light", "light", Some(0xffffff)),
    ("Light Gray", "lightgray", Some(0xc8cacd)),
    ("Gray", "gray", Some(0x444444)),
    ("Dark", "dark", Some(0x010101)),
    ("Blue", "blue", Some(0x28356e)),
    ("Purple", "purple", Some(0x463275)),
];

// ── Pieces ──────────────────────────────────────────────────────

/// `.settingsBlock`
fn block() -> Div {
    div().w_full().mb(px(12.)).flex().flex_col()
}

/// `.blockContent`
fn card(theme: theme::Theme) -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .p(px(12.))
        .rounded(px(theme::RADIUS_LG))
        .border_1()
        .border_color(theme.border_secondary())
        .bg(theme.contrast(3.))
}

/// `.blockTitle`
fn title(theme: theme::Theme, text: &'static str) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .text(theme::TEXT_MD)
        .line_height(px(26.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.text())
        .child(text)
}

/// `.settingLabel`
fn label(theme: theme::Theme) -> Div {
    div()
        .min_w(px(100.))
        .flex_none()
        .pt(px(2.))
        .text(theme::TEXT_MD)
        .line_height(px(theme::TEXT_MD * 1.3))
        .text_color(theme.text_secondary())
}

/// `.settingControl`
fn control() -> Div {
    div()
        .ml_auto()
        .min_w_0()
        .flex()
        .items_center()
        .justify_end()
        .gap(px(6.))
}

/// `Row`: a label and its control.
fn row(theme: theme::Theme, text: impl Into<SharedString>, child: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .min_h(px(26.))
        .min_w_0()
        .child(label(theme).child(text.into()))
        .child(control().child(child))
}

/// `.rowSep`
fn separator(theme: theme::Theme) -> Div {
    div()
        .h(px(1.))
        .my(px(12.))
        .flex_none()
        .bg(theme.border_secondary())
}

/// `.settingHint`
fn hint(theme: theme::Theme, text: &'static str) -> Div {
    div()
        .mt(px(8.))
        .text(theme::TEXT_SM)
        .line_height(px(20.))
        .text_color(theme.text_secondary())
        .child(text)
}

/// `.staticValue`
fn value(theme: theme::Theme, text: impl Into<SharedString>) -> Div {
    div()
        .ml_auto()
        .min_h(px(26.))
        .flex()
        .items_center()
        .justify_end()
        .text(theme::TEXT_MD)
        .text_color(theme.text())
        .child(text.into())
}

/// `.valueLabel`
fn value_label(theme: theme::Theme, text: String) -> Div {
    div()
        .ml(px(8.))
        .font_family(theme::FONT_INSPECTOR_MONO)
        .text(theme::TEXT_SM)
        .text_color(theme.text_secondary())
        .child(text)
}

/// `.checkboxGrid`: two columns, each 4px short of half, 4px apart.
fn two_columns() -> Div {
    div().w_full().grid().grid_cols(2).gap(px(4.)).pr(px(4.))
}

/// `.sidebarSettingsGrid`: three equal columns 4px apart.
fn three_columns() -> Div {
    div().w_full().grid().grid_cols(3).gap(px(4.))
}

/// A change to the draft, for a control's handler.
fn edit(
    model: &Entity<SettingsModel>,
    change: impl Fn(&mut Settings) + 'static,
) -> impl Fn(&mut Window, &mut App) + 'static {
    let model = model.clone();
    move |_, cx| model.update(cx, |model, cx| model.edit(&change, cx))
}

/// A choice from a select, written into the draft.
fn pick(
    model: &Entity<SettingsModel>,
    change: impl Fn(&mut Settings, &'static str) + 'static,
) -> impl Fn(&'static str, &mut Window, &mut App) + 'static {
    let model = model.clone();
    move |value, _, cx| model.update(cx, |model, cx| model.edit(|draft| change(draft, value), cx))
}

/// A number from a field or slider, written into the draft.
fn number<T: 'static>(
    model: &Entity<SettingsModel>,
    change: impl Fn(&mut Settings, T) + 'static,
) -> impl Fn(T, &mut App) + 'static {
    let model = model.clone();
    move |value, cx| model.update(cx, |model, cx| model.edit(|draft| change(draft, value), cx))
}

/// `.themeSwatch`: a 20px swatch whose outline lightens under the pointer
/// and is full when chosen, over 100 ms, with its name above it while the
/// pointer is on it (a CSS tooltip, without the usual delay).
fn swatch(
    index: usize,
    name: &'static str,
    color: Option<u32>,
    active: bool,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    let theme = theme::theme(cx);
    let id = ElementId::Name(format!("theme-swatch-{index}").into());
    let hover = window.use_keyed_state(id.clone(), cx, |_, _| motion::Fade::default());
    let tip = window.use_keyed_state(
        ElementId::Name(format!("theme-swatch-tip-{index}").into()),
        cx,
        |_, _| motion::Fade::default(),
    );
    if hover.read(cx).animating() || tip.read(cx).animating() {
        window.request_animation_frame();
    }
    let lit = hover.read(cx).value_with(motion::EASE);
    let shown = tip.read(cx).value_with(motion::EASE);
    let outline = if active {
        theme.text()
    } else {
        theme::blend(
            gpui::transparent_black().into(),
            theme.text_secondary(),
            lit,
        )
    };
    let fill = match color {
        Some(color) => div().bg(rgb(color)),
        // `linear-gradient(135deg, #f0f0f0 50%, #1a1a1e 50%)`
        None => div().bg(linear_gradient(
            135.,
            linear_color_stop(rgb(0xf0f0f0), 0.5),
            linear_color_stop(rgb(0x1a1a1e), 0.5001),
        )),
    };
    // The 1.5px outline sits 1.5px out from the border.
    let mut ring = div()
        .absolute()
        .top(px(-3.))
        .left(px(-3.))
        .size(px(26.))
        .rounded(px(9.))
        .border_color(outline);
    ring.style().border_widths = EdgesRefinement {
        top: Some(px(1.5).into()),
        right: Some(px(1.5).into()),
        bottom: Some(px(1.5).into()),
        left: Some(px(1.5).into()),
    };
    let mut cell = div()
        .id(id)
        .relative()
        .size(px(20.))
        .flex_none()
        .cursor_pointer()
        .child(
            fill.absolute()
                .inset_0()
                .rounded(px(theme::RADIUS_MD))
                .border_1()
                .border_color(theme.border_secondary()),
        )
        .child(ring)
        .on_hover(move |hovered, _, cx| {
            hover.update(cx, |hover, cx| {
                hover.set(*hovered, motion::ROW_HOVER);
                cx.notify();
            });
            tip.update(cx, |tip, cx| {
                tip.set(*hovered, motion::TOOLTIP_FADE);
                cx.notify();
            });
        })
        .on_click(move |_, window, cx| on_click(window, cx));
    if shown > 0. {
        // Centred 6px above the swatch, growing from 95% as it fades in.
        let bubble = div()
            .relative()
            .h(px(24.))
            .px(px(6.))
            .flex()
            .items_center()
            .rounded(px(theme::RADIUS_SM))
            .bg(theme.glass())
            .border_1()
            .border_color(theme.border_secondary())
            .shadow(vec![
                BoxShadow::new(px(0.), px(2.), rgba(0x0000004d).into()).blur_radius(px(8.)),
            ])
            .text(theme::TEXT_SM)
            .line_height(px(24.))
            .font_weight(FontWeight::NORMAL)
            .text_color(theme.text())
            .whitespace_nowrap()
            .opacity(shown)
            .child(
                div()
                    .absolute()
                    .top(px(-1.))
                    .left_0()
                    .right_0()
                    .h(px(1.))
                    .bg(theme.contrast(15.)),
            )
            .child(name);
        cell = cell.child(div().absolute().top(px(-6.)).left(relative(0.5)).child(
            deferred(anchored().anchor(Anchor::BottomCenter).child(bubble)).with_priority(3),
        ));
    }
    cell
}

/// `.searchResult`-like bordered link of the About panel (`.aboutLinks
/// button`): it lightens and its border strengthens under the pointer.
fn about_link(
    id: &'static str,
    icon: &'static str,
    text: &'static str,
    url: &'static str,
    cx: &App,
) -> impl IntoElement {
    let theme = theme::theme(cx);
    div()
        .id(id)
        .min_w_0()
        .h(px(54.))
        .px(px(10.))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .rounded(px(theme::RADIUS_MD))
        .border_1()
        .border_color(theme.border_secondary())
        .bg(theme.control_surface())
        .text_color(theme.text())
        .cursor_pointer()
        .hover(move |style| style.bg(theme.surface_hover()).border_color(theme.border()))
        .child(ui::icon(icon, 16., theme.text()))
        .child(text)
        .child(ui::icon(
            "settings-external-link",
            13.,
            theme.text_tertiary(),
        ))
        .on_click(move |_, _, cx| cx.open_url(url))
}

fn copy(
    theme: theme::Theme,
    name: impl Into<SharedString>,
    description: impl Into<SharedString>,
) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .child(
            div()
                .text(theme::TEXT_MD)
                .text_color(theme.text())
                .child(name.into()),
        )
        .child(
            div()
                .mt(px(2.))
                .text(theme::TEXT_SM)
                .line_height(px(theme::TEXT_SM * 1.4))
                .text_color(theme.text_tertiary())
                .child(description.into()),
        )
}

/// The installed version, from the app's `package.json`.
fn version() -> String {
    serde_json::from_str::<serde_json::Value>(include_str!("../../../../package.json"))
        .ok()
        .and_then(|package| package.get("version")?.as_str().map(str::to_owned))
        .unwrap_or_default()
}

impl SettingsWindow {
    /// The rows of the section with this id.
    pub(super) fn panel(
        &self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        match id {
            "general" => self.general(window, cx),
            "updates" => self.updates(cx),
            "library" => self.library(cx),
            "sidebar" => self.sidebar(cx),
            "controls" => self.controls(cx),
            "preview" => self.preview(cx),
            "notifications" => self.notifications(cx),
            "autoimport" => self.auto_import(cx),
            "deleting" => self.deleting(cx),
            "subscriptions" => self.subscriptions(cx),
            "cloud" => self.cloud(cx),
            "aitagging" => self.ai_models(window, cx),
            "about" => self.about(cx),
            _ => Vec::new(),
        }
    }

    fn general(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let model = &self.model;
        let draft = &model.read(cx).draft;
        let active = draft.app.color_scheme.clone();
        let zoom = draft
            .app
            .zoom_factor
            .map(|factor| (factor * 100.).round() as i64)
            .filter(|percent| [75, 80, 90, 100, 110, 125, 150].contains(percent))
            .unwrap_or(100);
        let (locale, counts, prefixes) = (
            draft.locale,
            draft.app.show_sidebar_counts,
            draft.app.show_tag_prefixes,
        );
        let mut swatches = div().flex().flex_wrap().justify_end().gap(px(8.));
        for (index, &(name, css, color)) in THEMES.iter().enumerate() {
            // TODO: preview and apply the theme; only Dark exists here yet.
            swatches = swatches.child(swatch(
                index,
                name,
                color,
                active == css,
                edit(model, move |draft| draft.app.color_scheme = css.into()),
                window,
                cx,
            ));
        }
        vec![
            block()
                .child(
                    card(theme)
                        .child(title(theme, "Appearance"))
                        .child(row(theme, "Theme", swatches))
                        .child(separator(theme))
                        .child(row(
                            theme,
                            "Zoom Level",
                            // TODO: scale the main window's content by this factor.
                            ui::select(
                                "zoom",
                                zoom.to_string(),
                                ZOOM.iter()
                                    .map(|&(value, label)| choice(value, label))
                                    .collect(),
                            )
                            .on_change(pick(model, |draft, value| {
                                draft.app.zoom_factor =
                                    value.parse::<f64>().ok().map(|percent| percent / 100.)
                            })),
                        )),
                )
                .into_any_element(),
            block()
                .child(
                    card(theme)
                        .child(title(theme, "Interface"))
                        .child(row(
                            theme,
                            "Language",
                            // TODO: translate the interface; the strings are English only.
                            ui::select(
                                "language",
                                locale,
                                vec![
                                    choice("en", "English"),
                                    choice("de", "Deutsch"),
                                    choice("es", "Español"),
                                    choice("pt", "Português"),
                                    choice("fr", "Français"),
                                    choice("zh-CN", "简体中文"),
                                    choice("ja", "日本語"),
                                    choice("fi", "Suomi"),
                                ],
                            )
                            .width(220.)
                            .on_change(pick(model, |draft, value| draft.locale = value)),
                        ))
                        .child(separator(theme))
                        .child(
                            ui::check("sidebar-counts", "Show item counts in the sidebar", counts)
                                .on_toggle(edit(model, |draft| {
                                    draft.app.show_sidebar_counts = !draft.app.show_sidebar_counts
                                })),
                        ),
                )
                .into_any_element(),
            block()
                .child(
                    card(theme)
                        .child(title(theme, "Tags"))
                        .child(
                            ui::check("hide-prefixes", "Hide group prefixes", !prefixes).on_toggle(
                                edit(model, |draft| {
                                    draft.app.show_tag_prefixes = !draft.app.show_tag_prefixes
                                }),
                            ),
                        )
                        .child(hint(
                            theme,
                            "Show shorter tag names without changing their underlying group.",
                        )),
                )
                .into_any_element(),
        ]
    }

    fn updates(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let description = if cfg!(target_os = "macos") {
            "On macOS, Picto opens the release download because this build is not signed for automatic installation."
        } else {
            "Updates download in the background and install after Picto closes."
        };
        vec![
            block()
                .child(
                    card(theme)
                        .child(title(theme, "Software Update"))
                        .child(row(theme, "Installed version", div().child(version())))
                        .child(separator(theme))
                        .child(row(
                            theme,
                            "Check for a newer version of Picto.",
                            div().flex().gap(px(8.)).child(
                                // TODO: check the release feed and offer the download.
                                ui::action_button("check-updates", Look::Settings)
                                    .label("Check Now"),
                            ),
                        ))
                        .child(
                            div()
                                .mt(px(12.))
                                .text(theme::TEXT_SM)
                                .line_height(px(18.))
                                .text_color(theme.text_tertiary())
                                .child(description),
                        ),
                )
                .into_any_element(),
        ]
    }

    fn library(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let Some(stats) = &self.statistics else {
            return Vec::new();
        };
        let count = |value: i64| grouped(value.max(0) as u64);
        let section = |name: &'static str, rows: Vec<(&'static str, String)>| {
            let mut content = card(theme).child(title(theme, name));
            for (index, (label, shown)) in rows.into_iter().enumerate() {
                if index > 0 {
                    content = content.child(separator(theme));
                }
                content = content.child(row(theme, label, value(theme, shown)));
            }
            block().child(content).into_any_element()
        };
        vec![
            section(
                "Overview",
                vec![
                    (
                        "Library items",
                        count(stats.active_items + stats.inbox_items + stats.trash_items),
                    ),
                    ("Media assets", count(stats.media_assets)),
                    ("Size", format_bytes(stats.original_bytes)),
                ],
            ),
            section(
                "Library Items",
                vec![
                    ("All", count(stats.active_items)),
                    ("Inbox", count(stats.inbox_items)),
                    ("Trash", count(stats.trash_items)),
                    ("Standalone", count(stats.standalone_items)),
                    ("Groups", count(stats.collections)),
                ],
            ),
            section(
                "Media and Storage",
                vec![
                    ("Images", count(stats.image_assets)),
                    ("Videos", count(stats.video_assets)),
                    ("Audio", count(stats.audio_assets)),
                    ("Other media", count(stats.other_assets)),
                    ("Physical files", count(stats.physical_files)),
                ],
            ),
            section(
                "Organization",
                vec![
                    ("Tags", count(stats.tags)),
                    ("Folders", count(stats.folders)),
                    ("Smart folders", count(stats.smart_folders)),
                    ("Subscriptions", count(stats.subscriptions)),
                ],
            ),
        ]
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let model = &self.model;
        let app = &model.read(cx).draft.app;
        let rename = app.sidebar_double_click_action == "rename";
        let guides = app.show_tree_guides;
        let shown = |key: &str| app.sidebar_shows(key);
        let toggled = |index: usize, name: &'static str, key: &'static str, on: bool| {
            let check = ui::check(("sidebar-shows", index), name, on);
            if ["system:active", "system:trash"].contains(&key) {
                check.disabled(true)
            } else {
                check.on_toggle(edit(model, move |draft| {
                    if let Some(on) = draft.app.sidebar_visibility(key) {
                        *on = !*on;
                    }
                }))
            }
        };
        let mut rows = three_columns();
        for (index, (name, key)) in [
            ("All", "system:active"),
            ("Uncategorized", "system:uncategorized"),
            ("Untagged", "system:untagged"),
            ("Inbox", "system:inbox"),
            ("Random", "system:random"),
            ("Tag Manager", "system:tag_manager"),
            ("Recently Viewed", "system:recent_viewed"),
            ("Subscriptions", "system:subscriptions"),
            ("Duplicates", "system:duplicates"),
            ("Trash", "system:trash"),
        ]
        .into_iter()
        .enumerate()
        {
            rows = rows.child(toggled(index, name, key, shown(key)));
        }
        let mut section_rows = three_columns();
        for (index, (name, key)) in [
            ("Quick Access", "section:quick_access"),
            ("Smart Folders", "section:smart_folders"),
            ("Folders", "section:folders"),
        ]
        .into_iter()
        .enumerate()
        {
            section_rows = section_rows.child(toggled(10 + index, name, key, shown(key)));
        }
        vec![
            block()
                .child(
                    card(theme).child(title(theme, "Double Click")).child(
                        three_columns()
                            .child(
                                ui::check("double-click-rename", "Rename", rename)
                                    .radio()
                                    .on_toggle(edit(model, |draft| {
                                        draft.app.sidebar_double_click_action = "rename"
                                    })),
                            )
                            .child(
                                ui::check("double-click-collapse", "Expand/Collapse", !rename)
                                    .radio()
                                    .on_toggle(edit(model, |draft| {
                                        draft.app.sidebar_double_click_action = "collapse"
                                    })),
                            ),
                    ),
                )
                .into_any_element(),
            block()
                .child(
                    card(theme)
                        .child(title(theme, "Show these items in the sidebar:"))
                        .child(rows)
                        .child(separator(theme))
                        .child(section_rows),
                )
                .into_any_element(),
            block()
                .child(
                    card(theme).child(title(theme, "Folder Tree")).child(
                        ui::check("tree-guides", "Show hierarchy guides", guides)
                            .on_toggle(edit(model, |draft| {
                                draft.app.show_tree_guides = !draft.app.show_tree_guides
                            })),
                    ),
                )
                .into_any_element(),
        ]
    }

    fn preview(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let model = &self.model;
        let draft = &model.read(cx).draft;
        let app = &draft.app;
        // TODO: the viewer's scaling, default zoom, transparency grid, and
        // video and audio playback follow these once they exist here.
        vec![
            block()
                .child(
                    card(theme)
                        .child(title(theme, "Image"))
                        .child(row(
                            theme,
                            "Scaling",
                            ui::select(
                                "image-rendering",
                                app.image_rendering,
                                vec![choice("smooth", "Smooth"), choice("pixelated", "Pixelated")],
                            )
                            .on_change(pick(model, |draft, value| {
                                draft.app.image_rendering = value
                            })),
                        ))
                        .child(separator(theme))
                        .child(row(
                            theme,
                            "Default zoom",
                            ui::select(
                                "image-zoom",
                                app.image_default_zoom,
                                vec![
                                    choice("fit", "Fit to window"),
                                    choice("actual", "Actual size"),
                                ],
                            )
                            .on_change(pick(model, |draft, value| {
                                draft.app.image_default_zoom = value
                            })),
                        ))
                        .child(separator(theme))
                        .child(
                            ui::check(
                                "transparency",
                                "Show transparency grid",
                                app.show_transparency_grid,
                            )
                            .on_toggle(edit(model, |draft| {
                                draft.app.show_transparency_grid = !draft.app.show_transparency_grid
                            })),
                        ),
                )
                .into_any_element(),
            block()
                .child(
                    card(theme).child(title(theme, "Video")).child(
                        two_columns()
                            .child(
                                ui::check("autoplay", "Autoplay videos", app.video_auto_play)
                                    .on_toggle(edit(model, |draft| {
                                        draft.app.video_auto_play = !draft.app.video_auto_play
                                    })),
                            )
                            .child(ui::check("loop", "Loop videos", app.video_loop).on_toggle(
                                edit(model, |draft| draft.app.video_loop = !draft.app.video_loop),
                            )),
                    ),
                )
                .into_any_element(),
            block()
                .child(
                    card(theme).child(title(theme, "Audio")).child(row(
                        theme,
                        "Visualization",
                        ui::select(
                            "audio-visualization",
                            draft.audio_visualization,
                            vec![
                                choice("none", "None"),
                                choice("spectrum", "Spectrum"),
                                choice("oscilloscope", "Oscilloscope"),
                                choice("orbit", "Orbit"),
                            ],
                        )
                        .on_change(pick(model, |draft, value| {
                            draft.audio_visualization = value
                        })),
                    )),
                )
                .into_any_element(),
        ]
    }

    fn controls(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let model = &self.model;
        let state = model.read(cx);
        let (app, grid) = (&state.draft.app, &state.draft.grid);
        let resetting = state.reset_views;
        const WIDTH: f32 = 220.;
        // TODO: the grid's wheel, double-click, middle-click and Space
        // actions follow these; today they are fixed at the defaults.
        let mouse = {
            let mut content = card(theme).child(title(theme, "Mouse")).child(row(
                theme,
                "Mouse wheel",
                ui::select(
                    "wheel",
                    app.grid_wheel_action,
                    vec![
                        choice("scroll", "Scroll grid"),
                        choice("zoom", "Adjust thumbnail size"),
                    ],
                )
                .width(WIDTH)
                .on_change(pick(model, |draft, value| {
                    draft.app.grid_wheel_action = value
                })),
            ));
            if cfg!(target_os = "macos") {
                content = content.child(separator(theme)).child(row(
                    theme,
                    "Media view gestures",
                    ui::select(
                        "gestures",
                        if app.viewer_trackpad_gestures {
                            "trackpad"
                        } else {
                            "wheel_zoom"
                        },
                        vec![
                            choice("wheel_zoom", "Wheel zoom"),
                            choice("trackpad", "Trackpad pan + pinch zoom"),
                        ],
                    )
                    .width(WIDTH)
                    .on_change(pick(model, |draft, value| {
                        draft.app.viewer_trackpad_gestures = value == "trackpad"
                    })),
                ));
            }
            content
                .child(separator(theme))
                .child(row(
                    theme,
                    "Double-click",
                    ui::select(
                        "double-click",
                        app.grid_double_click_action,
                        vec![
                            choice("detail", "Open Media View"),
                            choice("external", "Open in default app"),
                        ],
                    )
                    .width(WIDTH)
                    .on_change(pick(model, |draft, value| {
                        draft.app.grid_double_click_action = value
                    })),
                ))
                .child(separator(theme))
                .child(row(
                    theme,
                    "Middle-click",
                    ui::select(
                        "middle-click",
                        app.grid_middle_click_action,
                        vec![
                            choice("new_window", "Open in new window"),
                            choice("none", "Do nothing"),
                        ],
                    )
                    .width(WIDTH)
                    .on_change(pick(model, |draft, value| {
                        draft.app.grid_middle_click_action = value
                    })),
                ))
        };
        let size = grid.target_size.unwrap_or(220.);
        let fit = grid.thumbnail_fit.as_deref() == Some("cover");
        let flag = |value: Option<bool>, fallback: bool| value.unwrap_or(fallback);
        let labels: [(&str, bool, fn(&mut super::model::GridDefaults)); 6] = [
            ("Fit thumbnails", fit, |grid| {
                grid.thumbnail_fit = Some(
                    if grid.thumbnail_fit.as_deref() == Some("cover") {
                        "contain"
                    } else {
                        "cover"
                    }
                    .into(),
                )
            }),
            ("Show name", flag(grid.show_name, true), |grid| {
                grid.show_name = Some(!grid.show_name.unwrap_or(true))
            }),
            (
                "Show resolution",
                flag(grid.show_resolution, false),
                |grid| grid.show_resolution = Some(!grid.show_resolution.unwrap_or(false)),
            ),
            ("Show extension", flag(grid.show_extension, false), |grid| {
                grid.show_extension = Some(!grid.show_extension.unwrap_or(false))
            }),
            ("Show label", flag(grid.show_label, false), |grid| {
                grid.show_label = Some(!grid.show_label.unwrap_or(false))
            }),
            (
                "Show item count",
                flag(grid.show_item_count, true),
                |grid| grid.show_item_count = Some(!grid.show_item_count.unwrap_or(true)),
            ),
        ];
        let mut checks = two_columns();
        for (index, (name, on, toggle)) in labels.into_iter().enumerate() {
            checks = checks.child(
                ui::check(("grid-label", index), name, on)
                    .on_toggle(edit(model, move |draft| toggle(&mut draft.grid))),
            );
        }
        let half = |name: &'static str, child: AnyElement| {
            div()
                .flex_1()
                .min_w_0()
                .min_h(px(26.))
                .flex()
                .items_center()
                .child(label(theme).child(name))
                .child(control().child(child))
        };
        let grid_defaults = card(theme)
            .child(title(theme, "Grid Defaults"))
            .child(row(
                theme,
                "Default layout",
                ui::select(
                    "layout",
                    SharedString::from(grid.view_mode.clone().unwrap_or_else(|| "waterfall".into())),
                    vec![
                        choice("waterfall", "Waterfall").icon("select-layout-board"),
                        choice("grid", "Grid").icon("select-border-all"),
                        choice("justified", "Justified").rotated_icon("select-layout-board"),
                    ],
                )
                .on_change(pick(model, |draft, value| draft.grid.view_mode = Some(value.into()))),
            ))
            .child(separator(theme))
            .child(row(
                theme,
                "Thumbnail size",
                div()
                    .flex()
                    .items_center()
                    .child(
                        ui::slider("thumbnail-size", size, 100., 600., 50.).on_change(number(
                            model,
                            |draft, value: f64| draft.grid.target_size = Some(value),
                        )),
                    )
                    .child(value_label(theme, format!("{}px", size.round() as i64))),
            ))
            .child(separator(theme))
            .child(row(
                theme,
                "Grid spacing",
                ui::select(
                    "spacing",
                    app.grid_spacing,
                    vec![choice("wide", "Wide"), choice("tight", "Tight")],
                )
                .on_change(pick(model, |draft, value| draft.app.grid_spacing = value)),
            ))
            .child(separator(theme))
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .child(half(
                        "Sort by",
                        ui::select(
                            "sort-field",
                            SharedString::from(
                                grid.sort_field.clone().unwrap_or_else(|| "imported_at".into()),
                            ),
                            vec![
                                choice("imported_at", "Date Added"),
                                choice("captured_at", "Date Created"),
                                choice("name", "Name"),
                                choice("rating", "Rating"),
                                choice("total_size", "File Size"),
                                choice("random", "Random"),
                            ],
                        )
                        .on_change(pick(model, |draft, value| {
                            draft.grid.sort_field = Some(value.into())
                        }))
                        .into_any_element(),
                    ))
                    .child(
                        div()
                            .w(px(1.))
                            .h(px(26.))
                            .mx(px(12.))
                            .flex_none()
                            .self_center()
                            .bg(theme.border_secondary()),
                    )
                    .child(half(
                        "Order",
                        ui::select(
                            "sort-order",
                            SharedString::from(
                                grid.sort_order.clone().unwrap_or_else(|| "descending".into()),
                            ),
                            vec![
                                choice("ascending", "Ascending").icon("select-sort-ascending"),
                                choice("descending", "Descending").icon("select-sort-descending"),
                            ],
                        )
                        .on_change(pick(model, |draft, value| {
                            draft.grid.sort_order = Some(value.into())
                        }))
                        .into_any_element(),
                    )),
            )
            .child(separator(theme))
            .child(checks)
            .child(separator(theme))
            .child(row(
                theme,
                "View overrides",
                ui::action_button("reset-views", Look::Settings).label("Reset all views")
                    .disabled(resetting)
                    .on_click({
                        let model = model.clone();
                        move |_, _, cx| {
                            model.update(cx, |model, cx| {
                                model.reset_views = true;
                                cx.notify();
                            })
                        }
                    }),
            ))
            .child(hint(
                theme,
                "Clears saved layout, sorting, and display choices so every view inherits these defaults. Inbox resets to oldest items first.",
            ));
        vec![
            block().child(mouse).into_any_element(),
            block()
                .child(
                    card(theme).child(title(theme, "Keyboard")).child(row(
                        theme,
                        "Space key",
                        ui::select(
                            "space-key",
                            app.space_key_action,
                            vec![
                                choice("quick_look", "Quick Look"),
                                choice("scroll", "Scroll page"),
                            ],
                        )
                        .width(WIDTH)
                        .on_change(pick(model, |draft, value| {
                            draft.app.space_key_action = value
                        })),
                    )),
                )
                .into_any_element(),
            block().child(grid_defaults).into_any_element(),
        ]
    }

    fn notifications(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let model = &self.model;
        let app = &model.read(cx).draft.app;
        let enabled = app.notification_popups_enabled;
        // TODO: pop-up notifications; this prototype shows none yet.
        let mut content = card(theme).child(
            title(theme, "Pop-ups").child(div().ml_auto().flex().items_center().child(
                ui::toggle("popups", enabled).on_toggle(edit(model, |draft| {
                    draft.app.notification_popups_enabled = !draft.app.notification_popups_enabled
                })),
            )),
        );
        if enabled {
            let names = [
                ("success", "Successful actions"),
                ("info", "Information"),
                ("warning", "Warnings"),
                ("error", "Errors"),
            ];
            let mut grid = two_columns();
            for (tone_index, (tone, name)) in names.into_iter().enumerate() {
                let tone = NOTIFICATION_TONES
                    .iter()
                    .copied()
                    .find(|known| *known == tone)
                    .unwrap_or("info");
                grid = grid.child(
                    ui::check(
                        ("tone", tone_index),
                        name,
                        app.notification_popup_tones.contains(&tone),
                    )
                    .on_toggle(edit(model, move |draft| {
                        draft.app.toggle_notification_tone(tone)
                    })),
                );
            }
            content = content.child(grid);
        }
        vec![block().child(content).into_any_element()]
    }

    fn auto_import(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let model = &self.model;
        let app = &model.read(cx).draft.app;
        // TODO: watched-folder imports and the multiple-file prompt.
        vec![
            block()
                .child(
                    card(theme)
                        .child(
                            title(theme, "Auto-Import").child(
                                div().ml_auto().flex().items_center().child(
                                    ui::toggle("auto-import", app.auto_import_enabled).on_toggle(
                                        edit(model, |draft| {
                                            draft.app.auto_import_enabled = !draft.app.auto_import_enabled
                                        }),
                                    ),
                                ),
                            ),
                        )
                        .child(hint(
                            theme,
                            "Import new files into Inbox from watched folders configured in the folder context menu.",
                        )),
                )
                .into_any_element(),
            block()
                .child(
                    card(theme)
                        .child(title(theme, "Manual Imports"))
                        .child(row(
                            theme,
                            "Multiple files",
                            ui::select(
                                "multi-file",
                                app.multi_file_import_behavior,
                                vec![
                                    choice("ask", "Ask every time"),
                                    choice("group", "Group as collection"),
                                    choice("separate", "Keep separate"),
                                ],
                            )
                            .on_change(pick(model, |draft, value| {
                                draft.app.multi_file_import_behavior = value
                            })),
                        ))
                        .child(hint(
                            theme,
                            "Choose whether batches become one collection, stay separate, or ask each time.",
                        )),
                )
                .into_any_element(),
        ]
    }

    fn deleting(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let model = &self.model;
        let app = &model.read(cx).draft.app;
        // TODO: rejecting from the Inbox, and the undo limit on deletes.
        vec![
            block()
                .child(
                    card(theme)
                        .child(title(theme, "Rejecting"))
                        .child(row(
                            theme,
                            "Rejected items",
                            ui::select(
                                "reject-action",
                                app.reject_action,
                                vec![choice("trash", "Move to Trash"), choice("delete", "Delete permanently")],
                            )
                            .on_change(pick(model, |draft, value| draft.app.reject_action = value)),
                        ))
                        .child(hint(
                            theme,
                            "Deleting skips Trash. Only deletes within the undo limit below can be undone.",
                        )),
                )
                .into_any_element(),
            block()
                .child(
                    card(theme)
                        .child(title(theme, "Undoing deletes"))
                        .child(row(
                            theme,
                            "Undo limit (media items)",
                            ui::number_input("undo-limit", app.undo_delete_limit, 0, 100_000).on_commit(
                                number(model, |draft, value: i64| draft.app.undo_delete_limit = value),
                            ),
                        ))
                        .child(hint(
                            theme,
                            "Counts media items, including collection members. Set to 0 to disable delete undo. Deletes within the limit can be undone until Picto closes. Larger deletes ask for confirmation and cannot be undone.",
                        )),
                )
                .into_any_element(),
        ]
    }

    fn subscriptions(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let model = &self.model;
        let app = &model.read(cx).draft.app;
        // TODO: subscriptions read these defaults once they can be created here.
        vec![
            block()
                .child(
                    card(theme)
                        .child(title(theme, "Defaults for New Subscriptions"))
                        .child(row(
                            theme,
                            "Schedule",
                            ui::select(
                                "schedule",
                                app.subscription_default_schedule,
                                vec![
                                    choice("manual", "Manual"),
                                    choice("daily", "Daily"),
                                    choice("weekly", "Weekly"),
                                    choice("monthly", "Monthly"),
                                ],
                            )
                            .on_change(pick(model, |draft, value| {
                                draft.app.subscription_default_schedule = value
                            })),
                        ))
                        .child(separator(theme))
                        .child(row(
                            theme,
                            "Posts per run",
                            ui::number_input("posts-per-run", app.subscription_default_posts_per_run, 1, 10_000)
                                .on_commit(number(model, |draft, value: i64| {
                                    draft.app.subscription_default_posts_per_run = value
                                })),
                        ))
                        .child(separator(theme))
                        .child(
                            ui::check("group-posts", "Group multi-media posts", app.subscription_default_group_posts)
                                .on_toggle(edit(model, |draft| {
                                    draft.app.subscription_default_group_posts =
                                        !draft.app.subscription_default_group_posts
                                })),
                        ),
                )
                .into_any_element(),
            block()
                .child(
                    card(theme)
                        .child(title(theme, "Inbox Capacity"))
                        .child(row(
                            theme,
                            "Maximum Inbox items",
                            ui::number_input("inbox-limit", app.subscription_inbox_item_limit, 1, 1_000_000)
                                .on_commit(number(model, |draft, value: i64| {
                                    draft.app.subscription_inbox_item_limit = value
                                })),
                        ))
                        .child(hint(
                            theme,
                            "Subscription work waits when the Inbox reaches this many top-level items.",
                        )),
                )
                .into_any_element(),
        ]
    }

    fn cloud(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let Some(info) = &self.cloud else {
            return Vec::new();
        };
        let configured = info.provider.is_some();
        let provider = match info.provider.as_deref() {
            Some("google_drive") => "Google Drive",
            Some("dropbox") => "Dropbox",
            _ => "Not configured",
        };
        let state = if info.message.is_empty() {
            info.state.clone()
        } else {
            info.message.clone()
        };
        let mut sync = card(theme)
            .child(title(theme, "Library Sync"))
            .child(row(theme, "Provider", value(theme, provider)))
            .child(separator(theme))
            .child(row(
                theme,
                "Folder",
                value(
                    theme,
                    info.root_path
                        .clone()
                        .unwrap_or_else(|| "Choose a cloud folder in Libraries".into()),
                ),
            ))
            .child(separator(theme))
            .child(row(theme, "State", value(theme, state)))
            .child(separator(theme))
            .child(row(
                theme,
                "Last sync",
                value(theme, cloud_date(info.last_sync_at.as_deref())),
            ))
            .child(separator(theme))
            .child(
                // TODO: sync, pause and snapshot through the cloud service.
                div()
                    .mt(px(10.))
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(
                        ui::action_button("cloud-sync", Look::Settings)
                            .label("Sync now")
                            .disabled(!configured),
                    )
                    .child(
                        ui::action_button("cloud-pause", Look::Settings)
                            .label(if info.state == "paused" {
                                "Resume"
                            } else {
                                "Pause"
                            })
                            .disabled(!configured),
                    )
                    .child(
                        ui::action_button("cloud-snapshot", Look::Settings)
                            .label("Create snapshot")
                            .disabled(!configured),
                    ),
            );
        if !configured {
            sync = sync.child(hint(
                theme,
                "Cloud sync is enabled per library from the Libraries window. Picto uses the Google Drive or Dropbox desktop folder already installed on this device.",
            ));
        }
        let retention = info.retention;
        let entity = cx.entity();
        let keep = move |change: fn(&mut [i64; 4], i64)| {
            let entity = entity.clone();
            move |value: i64, cx: &mut App| {
                entity.update(cx, |this, cx| {
                    if let Some(cloud) = &mut this.cloud {
                        change(&mut cloud.retention, value);
                        cx.notify();
                    }
                })
            }
        };
        // TODO: write retention through `cloud.retention.update`; until then
        // a change lasts while the window is open.
        let retention_card = card(theme)
            .child(title(theme, "Recovery Retention"))
            .child(row(
                theme,
                "Daily snapshots",
                ui::number_input("retention-daily", retention[0], 2, 365)
                    .on_commit(keep(|retention, value| retention[0] = value)),
            ))
            .child(separator(theme))
            .child(row(
                theme,
                "Weekly snapshots",
                ui::number_input("retention-weekly", retention[1], 0, 260)
                    .on_commit(keep(|retention, value| retention[1] = value)),
            ))
            .child(separator(theme))
            .child(row(
                theme,
                "Yearly snapshots",
                ui::number_input("retention-yearly", retention[2], 0, 100)
                    .on_commit(keep(|retention, value| retention[2] = value)),
            ))
            .child(separator(theme))
            .child(row(
                theme,
                "Keep deleted files",
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        ui::number_input("retention-deleted", retention[3], 0, 3650)
                            .on_commit(keep(|retention, value| retention[3] = value)),
                    )
                    .child(value(theme, "days")),
            ))
            .child(hint(
                theme,
                "Recently deleted cloud files can be restored immediately during this period.",
            ));
        vec![
            block().child(sync).into_any_element(),
            block()
                .child(
                    card(theme)
                        .child(title(theme, "Pending Work"))
                        .child(row(
                            theme,
                            "Mutations",
                            value(theme, info.pending_mutations.to_string()),
                        ))
                        .child(separator(theme))
                        .child(row(
                            theme,
                            "Files",
                            value(theme, info.pending_blobs.to_string()),
                        ))
                        .child(separator(theme))
                        .child(row(
                            theme,
                            "Unavailable files",
                            value(theme, info.missing_blobs.to_string()),
                        )),
                )
                .into_any_element(),
            block().child(retention_card).into_any_element(),
            block()
                .child(
                    card(theme)
                        .child(title(theme, "Restore History"))
                        // TODO: list the recovery snapshots of a configured
                        // provider (`cloud.restore.list`) with their Restore
                        // buttons. An unconfigured library has none.
                        .child(hint(theme, "No recovery snapshots are available yet.")),
                )
                .into_any_element(),
        ]
    }

    fn ai_models(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let model = &self.model;
        let app = model.read(cx).draft.app.clone();
        // TODO: read the model runtime's status (which models are downloaded
        // and optimized, the storage they take) and download, optimize and
        // delete through it. Until then every model shows as not downloaded.
        let mut models = card(theme).child(title(theme, "Models"));
        for (index, info) in MODELS.iter().enumerate() {
            if index > 0 {
                models = models.child(separator(theme));
            }
            let metadata = format!(
                "{} · {} · ≈{} ms/image",
                info.dataset,
                model_size(info.size),
                info.ms.round() as i64
            );
            let name = ui::tooltip::attach(
                div()
                    .id(("model-name", index))
                    .flex_basis(px(92.))
                    .flex_shrink(1.)
                    .min_w_0()
                    .text(theme::TEXT_MD)
                    .text_color(theme.text())
                    .truncate()
                    .child(info.short),
                ("model-name-tip", index),
                ui::tip(info.label),
                theme::TEXT_MD + 2.,
                window,
                cx,
            );
            let meta = ui::tooltip::attach(
                div()
                    .id(("model-meta", index))
                    .flex_1()
                    .min_w_0()
                    .text(theme::TEXT_SM)
                    .text_color(theme.text_tertiary())
                    .truncate()
                    .child(metadata.clone()),
                ("model-meta-tip", index),
                ui::tip(metadata),
                theme::TEXT_SM + 2.,
                window,
                cx,
            );
            models = models.child(
                div()
                    .relative()
                    .h(px(32.))
                    .pb(px(2.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_baseline()
                            .gap(px(8.))
                            .child(name)
                            .child(meta),
                    )
                    .child(
                        div()
                            .w(px(286.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(10.))
                            .child(
                                ui::action_button(("model-download", index), Look::Standard)
                                    .label("Download")
                                    .width(92.)
                                    .padding(0.),
                            ),
                    ),
            );
        }
        let models =
            models
                .child(separator(theme))
                .child(row(theme, "Model storage", value(theme, "—")));

        let mut confidence = card(theme).child(title(theme, "Confidence"));
        for (index, (key, name, namespace)) in THRESHOLDS.iter().enumerate() {
            if index > 0 {
                confidence = confidence.child(separator(theme));
            }
            let percent = (app.threshold(*key) * 100.).round();
            let key = *key;
            confidence = confidence.child(
                div()
                    .flex()
                    .items_center()
                    .min_h(px(26.))
                    .child(
                        label(theme)
                            .flex()
                            .items_center()
                            .gap(px(7.))
                            .child(
                                div()
                                    .size(px(8.))
                                    .flex_none()
                                    .rounded_full()
                                    .bg(rgb(crate::inspector::tag_style(namespace).0))
                                    .opacity(0.75),
                            )
                            .child(*name),
                    )
                    .child(
                        control()
                            .child(
                                ui::slider(("threshold", index), percent, 5., 95., 1.).on_change(
                                    number(model, move |draft, value: f64| {
                                        draft.app.set_threshold(key, value / 100.)
                                    }),
                                ),
                            )
                            .child(
                                value_label(theme, format!("{}%", percent as i64))
                                    .w(px(36.))
                                    .flex()
                                    .justify_end(),
                            ),
                    ),
            );
        }
        let confidence = confidence.child(hint(
            theme,
            "Tags below these confidence levels are hidden by default. You can adjust the cutoff for an individual run.",
        ));

        let auto = app.ai_tagger_auto_on_import;
        let downloaded = false;
        let mut behavior = card(theme).child(title(theme, "Behavior")).child(
            div()
                .flex()
                .items_center()
                .min_h(px(26.))
                .child(copy(
                    theme,
                    "Auto-tag new imports",
                    "Run selected models after import and apply tags above the confidence levels.",
                ))
                .child(
                    control().child(
                        ui::toggle("auto-tag", auto)
                            .disabled(!auto && !downloaded)
                            .on_toggle(edit(model, |draft| {
                                draft.app.ai_tagger_auto_on_import =
                                    !draft.app.ai_tagger_auto_on_import
                            })),
                    ),
                ),
        );
        // Each downloaded model's own switch shows while auto-tagging is on.
        for info in MODELS.iter().filter(|_| auto && downloaded) {
            behavior = behavior.child(separator(theme)).child(
                div()
                    .flex()
                    .items_center()
                    .min_h(px(26.))
                    .child(copy(theme, info.short, info.dataset))
                    .child(control().child(ui::toggle(info.slug, app.runs_model(info.slug)))),
            );
        }
        let behavior = behavior.child(separator(theme)).child(
            div()
                .flex()
                .items_center()
                .min_h(px(26.))
                .child(copy(
                    theme,
                    "Write rating tags",
                    "Store general, sensitive, questionable, or explicit as a rating tag.",
                ))
                .child(control().child(
                    ui::toggle("write-rating", app.ai_tagger_write_rating).on_toggle(edit(
                        model,
                        |draft| {
                            draft.app.ai_tagger_write_rating = !draft.app.ai_tagger_write_rating
                        },
                    )),
                )),
        );
        vec![
            block()
                .child(models)
                .child(hint(
                    theme,
                    "Selected models run locally one after another. Warm single-image reference on an Apple M5 Pro; actual speed varies by device and batch size. Picto never uploads media for AI tagging.",
                ))
                .into_any_element(),
            block().child(confidence).into_any_element(),
            block().child(behavior).into_any_element(),
        ]
    }

    fn about(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let platform = if cfg!(target_os = "macos") {
            " · macOS"
        } else if cfg!(target_os = "windows") {
            " · Windows"
        } else {
            " · Linux"
        };
        let year = chrono::Datelike::year(&chrono::Local::now());
        vec![
            div()
                .w_full()
                .max_w(px(620.))
                .mx_auto()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(18.))
                        .pt(px(18.))
                        .px(px(4.))
                        .pb(px(24.))
                        .child(img("picto-logo").size(px(72.)).flex_none())
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(28.))
                                        .line_height(px(32.))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(theme.text())
                                        .child("Picto"),
                                )
                                .child(
                                    div()
                                        .mt(px(3.))
                                        .mb(px(7.))
                                        .text_color(theme.text_secondary())
                                        .child("Personal media library and organization."),
                                )
                                .child(
                                    div()
                                        .font_family(theme::FONT_INSPECTOR_MONO)
                                        .text(theme::TEXT_SM)
                                        .text_color(theme.text_tertiary())
                                        .child(format!("Version {}{platform}", version())),
                                ),
                        ),
                )
                .child(
                    block().child(
                        card(theme)
                            .child(title(theme, "Application"))
                            .child(row(
                                theme,
                                "Software updates",
                                // TODO: check the release feed.
                                ui::action_button("about-check-updates", Look::Settings)
                                    .label("Check Now"),
                            ))
                            .child(separator(theme))
                            .child(row(theme, "License", value(theme, "MIT"))),
                    ),
                )
                .child(
                    div()
                        .grid()
                        .grid_cols(3)
                        .gap(px(8.))
                        .child(about_link(
                            "github",
                            "settings-brand-github",
                            "Project on GitHub",
                            "https://github.com/midona-rhel/picto",
                            cx,
                        ))
                        .child(about_link(
                            "release-notes",
                            "settings-panel-refresh",
                            "Release notes",
                            "https://github.com/midona-rhel/picto/releases",
                            cx,
                        ))
                        .child(about_link(
                            "license",
                            "settings-scale",
                            "Open-source license",
                            "https://github.com/midona-rhel/picto/blob/main/LICENSE",
                            cx,
                        )),
                )
                .child(
                    div()
                        .mt(px(22.))
                        .flex()
                        .justify_center()
                        .text(theme::TEXT_SM)
                        .text_color(theme.text_tertiary())
                        .child(format!("© {year} Picto contributors")),
                )
                .into_any_element(),
        ]
    }
}

/// The cloud state shown in the Cloud section.
pub fn cloud_info(library: &picto_library::Library) -> CloudInfo {
    let read = library.auxiliary_read(
        picto_library::database::WorkPriority::VisibleRead,
        |connection| {
            let status = connection.query_row(
                "SELECT state, message, last_sync_at,
                        (SELECT COUNT(*) FROM cloud_journal WHERE expanded_at_ms IS NULL),
                        (SELECT COUNT(*) FROM cloud_blob_state
                         WHERE state IN ('queued', 'downloading')
                            OR (state = 'available' AND remote_present = 0)),
                        (SELECT COUNT(*) FROM cloud_blob_state
                         WHERE state IN ('missing_remote', 'corrupt')),
                        provider, remote_root, retention_json
                 FROM cloud_state WHERE singleton = 1",
                [],
                |row| {
                    let retention: String = row.get(8)?;
                    let retention = serde_json::from_str::<serde_json::Value>(&retention)
                        .unwrap_or(serde_json::Value::Null);
                    let days = |key: &str, fallback: i64| {
                        retention
                            .get(key)
                            .and_then(serde_json::Value::as_i64)
                            .unwrap_or(fallback)
                    };
                    Ok(CloudInfo {
                        state: row.get(0)?,
                        message: row.get(1)?,
                        last_sync_at: row.get(2)?,
                        pending_mutations: row.get(3)?,
                        pending_blobs: row.get(4)?,
                        missing_blobs: row.get(5)?,
                        provider: row.get(6)?,
                        root_path: row.get(7)?,
                        retention: [
                            days("daily", 30),
                            days("weekly", 26),
                            days("yearly", 5),
                            days("deleted_blobs_days", 7),
                        ],
                    })
                },
            )?;
            Ok(status)
        },
    );
    read.unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{PANELS, format_bytes, matched_panels, model_size};

    #[test]
    fn search_finds_sections_by_label_keyword_or_setting() {
        let ids = |query: &str| {
            matched_panels(query)
                .into_iter()
                .map(|index| PANELS[index].id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids("shortcuts"), ["shortcuts"]);
        // "checkerboard" is only a keyword of the Image Preview setting.
        assert_eq!(ids("checkerboard"), ["preview"]);
        assert!(ids("undo").contains(&"deleting"));
        assert_eq!(ids("trash"), ["library", "deleting"]);
        assert!(ids("").is_empty());
        assert!(ids("zzz").is_empty());
    }

    #[test]
    fn sizes_are_written_as_production_writes_them() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(15 * 1024 * 1024), "15.0 MB");
        assert_eq!(model_size(1_260_000_000), "1.26 GB");
        assert_eq!(model_size(2_000_000_000), "2 GB");
        assert_eq!(model_size(993_246_982), "993 MB");
    }
}
