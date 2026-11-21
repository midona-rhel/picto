//! The About section of the Settings window. It shows the app's logo, name
//! and version, a card with the button that checks for updates and with the
//! licence, and three links.
use super::version;
use crate::i18n::t;
use crate::settings::pieces::{block, card, row, separator, title, value};
use crate::settings::window::SettingsWindow;
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{self, action_button::Look};
use gpui::*;

/// Draws one of the three links of the About section: a box 54px high with
/// a border, which holds an icon, `text` and the glyph of an external link.
/// While the pointer is on the box, its background is the hover colour and
/// its border is stronger. A click calls `open`.
fn about_link(
    id: &'static str,
    icon: &'static str,
    text: &'static str,
    open: impl Fn(&mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let theme = theme::theme(cx);
    div()
        .id(id)
        .role(Role::Link)
        .aria_label(text)
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
        .on_click(move |_, _, cx| open(cx))
}

impl SettingsWindow {
    pub(super) fn about(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
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
            // The section is as wide as the panel that shows it.
            div()
                .w_full()
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
                                        .child(t("Picto")),
                                )
                                .child(
                                    div()
                                        .mt(px(3.))
                                        .mb(px(7.))
                                        // The line has the line height of
                                        // body text.
                                        .line_height(px(theme::TEXT_MD * 1.55))
                                        .text_color(theme.text_secondary())
                                        .child(t("Personal media library and organization.")),
                                )
                                .child(
                                    // The line of the version is 20.6px
                                    // high, and its text starts 4px below
                                    // the top of the line.
                                    div()
                                        .h(px(20.6))
                                        .pt(px(4.))
                                        .font_family(theme::FONT_INSPECTOR_MONO)
                                        .text(theme::TEXT_SM)
                                        .text_color(theme.text_tertiary())
                                        .child(format!("{}{}{platform}", t("Version "), version())),
                                ),
                        ),
                )
                .child(
                    block().child(
                        card(theme)
                            .child(title(theme, t("Application")))
                            .child(row(
                                theme,
                                t("Software updates"),
                                ui::action_button("about-check-updates", Look::Settings)
                                    .label(t("Check Now"))
                                    .on_click(|_, _, cx| crate::updates::check(cx)),
                            ))
                            .child(separator(theme))
                            .child(row(theme, t("License"), value(theme, t("MIT")))),
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
                            // The key in `en.json` ends with a space, which
                            // is removed here.
                            t("Project on GitHub ").trim_end(),
                            |cx| {
                                crate::desktop::open_link(
                                    "https://github.com/midona-rhel/picto",
                                    cx,
                                )
                            },
                            cx,
                        ))
                        .child(about_link(
                            "release-notes",
                            "refresh",
                            // The key in `en.json` ends with a space, which
                            // is removed here.
                            t("Release notes ").trim_end(),
                            // This link opens the page of the release that
                            // the last check for updates found. When the
                            // check found none, it opens the page that
                            // lists the releases.
                            crate::updates::open_release,
                            cx,
                        ))
                        .child(about_link(
                            "license",
                            "settings-scale",
                            // The key in `en.json` ends with a space, which
                            // is removed here.
                            t("Open-source license ").trim_end(),
                            |cx| {
                                crate::desktop::open_link(
                                    "https://github.com/midona-rhel/picto/blob/main/LICENSE",
                                    cx,
                                )
                            },
                            cx,
                        )),
                )
                .child(
                    div()
                        .mt(px(22.))
                        .flex()
                        .justify_center()
                        .text(theme::TEXT_SM)
                        .line_height(px(theme::TEXT_SM * 1.55))
                        .text_color(theme.text_tertiary())
                        .child(format!("© {year} {}", t("Picto contributors"))),
                )
                .into_any_element(),
        ]
    }
}
