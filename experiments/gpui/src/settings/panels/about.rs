//! The About section: the version, the update check and the links.
use super::version;
use crate::settings::pieces::{block, card, row, separator, title, value};
use crate::settings::window::SettingsWindow;
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{self, action_button::Look};
use gpui::*;

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
        .on_click(move |_, _, cx| crate::desktop::open_link(url, cx))
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
            // `.aboutPanel`'s 620px limit loses to `.panelContent`'s
            // `max-width: 100%`, which comes later in the stylesheet.
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
                                        .child("Picto"),
                                )
                                .child(
                                    div()
                                        .mt(px(3.))
                                        .mb(px(7.))
                                        // Mantine's body `line-height: 1.55`.
                                        .line_height(px(theme::TEXT_MD * 1.55))
                                        .text_color(theme.text_secondary())
                                        .child("Personal media library and organization."),
                                )
                                .child(
                                    // An inline span on the paragraph's 20.15px
                                    // strut, which its own 18.6px line lowers.
                                    div()
                                        .h(px(20.6))
                                        .pt(px(4.))
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
                        .line_height(px(theme::TEXT_SM * 1.55))
                        .text_color(theme.text_tertiary())
                        .child(format!("© {year} Picto contributors")),
                )
                .into_any_element(),
        ]
    }
}
