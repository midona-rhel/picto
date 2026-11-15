//! `ProgressDialog.tsx`: a glass dialog over a dimmed window that holds the
//! keyboard and every press while work the user must wait for runs, with a
//! bold message, an optional mono detail, and a 4px progress bar. It has no
//! buttons and Escape does nothing: it goes when the work is done.
use crate::theme;
use crate::theme::Text as _;
use gpui::*;

#[derive(IntoElement)]
pub struct ProgressDialog {
    id: ElementId,
    message: SharedString,
    detail: Option<SharedString>,
    /// `done / total`, or none while indeterminate.
    share: Option<f32>,
    focus: FocusHandle,
}

/// The dialog saying `message`, holding the keyboard on `focus`.
pub fn progress_dialog(
    id: impl Into<ElementId>,
    message: impl Into<SharedString>,
    focus: &FocusHandle,
) -> ProgressDialog {
    ProgressDialog {
        id: id.into(),
        message: message.into(),
        detail: None,
        share: None,
        focus: focus.clone(),
    }
}

impl ProgressDialog {
    pub fn detail(mut self, detail: Option<String>) -> Self {
        self.detail = detail.map(Into::into);
        self
    }

    /// Known progress; without it the bar sweeps.
    pub fn progress(mut self, done: i64, total: i64) -> Self {
        self.share = Some(super::progress_bar::share(done, total));
        self
    }
}

impl RenderOnce for ProgressDialog {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        // `useShortcutSuspension` and the dialog's own focus: the keyboard
        // stays here while it is up.
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }
        let mut bar = super::progress_bar()
            .height(4.)
            .radius(20.)
            .track(theme.contrast(10.));
        if let Some(share) = self.share {
            bar = bar.filled(
                ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "fill".into()),
                share,
            );
        }
        let mut message = div()
            .flex()
            .items_end()
            .max_w(px(560.))
            .min_w_0()
            .overflow_hidden()
            .text(theme::TEXT_MD)
            .font_weight(FontWeight::BOLD)
            .whitespace_nowrap()
            .child(div().min_w_0().truncate().child(self.message.clone()));
        if let Some(detail) = self.detail {
            message = message.child(
                div()
                    .flex_none()
                    .ml(px(6.))
                    .text_color(theme.text_secondary())
                    .font_family(theme::FONT_INSPECTOR_MONO)
                    .font_weight(FontWeight::NORMAL)
                    .font_features(theme::tabular_figures())
                    .child(detail),
            );
        }
        let panel = super::glass_panel(
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "panel".into()),
            10.,
            cx,
        )
        .role(Role::Dialog)
        .aria_label(self.message)
        .aria_modal(true)
        .min_w(px(360.))
        .px(px(24.))
        .py(px(20.))
        .text_color(theme.text())
        .font_family(theme::FONT_UI)
        .child(
            div()
                .id("progress-dialog-content")
                .role(Role::Status)
                .aria_live(gpui::accesskit::Live::Polite)
                .flex()
                .flex_col()
                .items_center()
                .gap(px(12.))
                .child(message)
                .child(div().w_full().child(bar)),
        );
        let layer = super::modal::backdrop(self.id, 1., Some(&self.focus), window)
            // Tab goes nowhere (`event.key === 'Tab'`).
            .on_key_down(|event, _, cx| {
                if event.keystroke.key == "tab" {
                    cx.stop_propagation();
                }
            })
            .child(panel);
        super::modal::over_window(layer, super::layer::DIALOG)
    }
}
