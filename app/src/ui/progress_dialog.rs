//! A glass dialog over a dimmed window, shown while work runs that the user
//! must wait for. It has the keyboard focus and stops every press. It shows a
//! bold message, optionally a detail in the monospaced font, and a 4px
//! progress bar. It has no buttons, and Escape does nothing. The caller
//! removes it when the work is done.
use crate::theme;
use crate::theme::Text as _;
use gpui::*;

#[derive(IntoElement)]
pub struct ProgressDialog {
    id: ElementId,
    message: SharedString,
    detail: Option<SharedString>,
    /// The share of the work that is done, or `None` while it is not known.
    share: Option<f32>,
    focus: FocusHandle,
}

/// Returns the dialog with the message `message`. It keeps the keyboard focus
/// on `focus`.
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

    /// Sets the progress to `done` of `total`. Without this call the bar shows
    /// unknown progress.
    pub fn progress(mut self, done: i64, total: i64) -> Self {
        self.share = Some(super::progress_bar::share(done, total));
        self
    }
}

impl RenderOnce for ProgressDialog {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        // The keyboard focus is kept on the dialog while the dialog is shown,
        // so that no shortcut of the app acts.
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
        let panel = super::modal::dialog_panel(
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "panel".into()),
            theme::WINDOW_RADIUS,
            self.message,
            cx,
        )
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
            // Tab is stopped here, so it does not move the keyboard focus.
            .on_key_down(|event, _, cx| {
                if event.keystroke.key == "tab" {
                    cx.stop_propagation();
                }
            })
            .child(panel);
        super::modal::over_window(layer, super::layer::DIALOG)
    }
}
