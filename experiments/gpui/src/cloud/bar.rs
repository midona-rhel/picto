//! `CloudSyncBar`: while the open library syncs, a 48px glass bar slides up
//! along the bottom of the main window between the rails, with the sync's
//! progress and what it is doing; while a library joined from the cloud
//! restores its media (a blocking sync), a progress dialog holds the window
//! instead.
use super::{Cloud, Status};
use crate::i18n::en;
use crate::theme;
use crate::theme::Text as _;
use crate::{motion, ui};
use gpui::*;
use std::time::Duration;

/// `.root`'s `transition: transform 300ms`.
const SLIDE: Duration = Duration::from_millis(300);
const HEIGHT: f32 = 48.;

/// The detail after the message: bytes while files move, else units.
fn detail(status: &Status) -> Option<String> {
    let total = status.total_units.filter(|total| *total > 0)?;
    Some(if status.phase == "blobs" {
        format!(
            "{} / {}",
            super::bytes(status.completed_units),
            super::bytes(total)
        )
    } else {
        format!("{} / {}", status.completed_units, total)
    })
}

fn message(status: &Status) -> String {
    if status.message.is_empty() {
        // Production writes it without a key.
        en("Working…").to_owned()
    } else {
        status.message.clone()
    }
}

/// The bar, kept clear of the rails (`left`, `right`), and the blocking
/// dialog, which holds the keyboard on `focus`.
pub fn bar(
    cloud: &Entity<Cloud>,
    (left, right): (f32, f32),
    focus: &FocusHandle,
    window: &mut Window,
    cx: &mut App,
) -> Vec<AnyElement> {
    let status = cloud.read(cx).status.clone();
    let active = status
        .as_ref()
        .is_some_and(|status| status.state == "reconciling");
    let blocking = active && status.as_ref().is_some_and(|status| status.blocking);
    let shown = active && !blocking;
    let slide = window.use_keyed_state("cloud-sync-bar", cx, |_, _| motion::Fade::default());
    slide.update(cx, |slide, _| slide.set(shown, SLIDE));
    let (progress, animating) = {
        let slide = slide.read(cx);
        (slide.value(), slide.animating())
    };
    if animating {
        window.request_animation_frame();
    }
    let mut layers = Vec::new();
    let Some(status) = status else {
        return layers;
    };
    let exact = status.total_units.is_some_and(|total| total > 0);
    if blocking {
        let mut dialog = ui::progress_dialog::progress_dialog(
            "cloud-restoring",
            message(&status),
            focus,
        )
        .detail(detail(&status));
        if exact {
            dialog = dialog.progress(status.completed_units, status.total_units.unwrap_or(0));
        }
        layers.push(dialog.into_any_element());
    }
    if progress <= 0. {
        return layers;
    }
    let theme = theme::theme(cx);
    let mut track = ui::progress_bar()
        .height(4.)
        .radius(10.)
        .track(theme.contrast(10.));
    if exact {
        track = track.filled(
            "cloud-sync-bar-fill",
            ui::progress_bar::share(status.completed_units, status.total_units.unwrap_or(0)),
        );
    }
    let label = message(&status);
    let mut text = div()
        .flex_none()
        .max_w(relative(0.5))
        .min_w_0()
        .pl(px(12.))
        .flex()
        .items_end()
        .text(theme::TEXT_MD)
        .whitespace_nowrap()
        .child(div().min_w_0().truncate().child(label.clone()));
    if let Some(detail) = detail(&status) {
        text = text.child(
            div()
                .flex_none()
                .ml(px(6.))
                .font_family(theme::FONT_INSPECTOR_MONO)
                .font_features(theme::tabular_figures())
                .child(detail),
        );
    }
    layers.push(
        div()
            .id("cloud-sync-bar")
            .role(Role::Status)
            .aria_live(gpui::accesskit::Live::Polite)
            .aria_label(label)
            .absolute()
            .left(px(left))
            .right(px(right))
            // `translateY(100%)` to `0`.
            .bottom(px(-HEIGHT * (1. - progress)))
            .h(px(HEIGHT))
            .opacity(if shown || progress > 0. { 1. } else { 0. })
            .pl(px(16.))
            .pr(px(12.))
            .flex()
            .items_center()
            .text_color(theme.text())
            .font_family(theme::FONT_UI)
            .bg(theme.glass())
            // `box-shadow: 0 -1px 0 var(--color-border-secondary)`.
            .border_t_1()
            .border_color(theme.border_secondary())
            .occlude()
            .child(div().flex_1().min_w_0().child(track))
            .child(text)
            .into_any_element(),
    );
    layers
}
