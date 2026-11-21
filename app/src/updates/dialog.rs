//! The update dialog (`UpdateModal.tsx`): this version, what the last check
//! found or the download under way, and the release's notes, with Check
//! Again and the step that follows (Open Download Page on macOS, Restart
//! and Install elsewhere). Closing it after a new version's first start
//! marks its notes as read.
use super::{Status, VERSION};
use crate::i18n::{en, t};
use crate::theme::{self, Text as _};
use crate::ui::{self, action_button::Look};
use gpui::{prelude::FluentBuilder as _, *};

pub struct UpdateDialog {
    dialog: ui::modal::Host<()>,
    back: FocusHandle,
    notes: ScrollHandle,
    _observe: Subscription,
}

impl UpdateDialog {
    /// The dialog of the main window, open at once when this version's
    /// notes are still to be read.
    pub fn new(back: FocusHandle, cx: &mut Context<Self>) -> Self {
        let updates = super::entity(cx);
        let observe = cx.observe(&updates, |_, _, cx| cx.notify());
        let mut dialog = Self {
            dialog: ui::modal::Host::default(),
            back,
            notes: ScrollHandle::new(),
            _observe: observe,
        };
        if updates.read(cx).state().status == Status::Installed {
            dialog.open(cx);
        }
        dialog
    }

    pub fn open(&mut self, cx: &mut Context<Self>) {
        if !self.dialog.is_open() {
            self.dialog
                .open((), ui::menu::Return::To(self.back.clone()), cx);
        }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        if super::state(cx).status == Status::Installed {
            super::acknowledge_installed(cx);
        }
        self.dialog.dismiss(cx);
    }
}

/// A block of the release notes (`NoteBlock`).
#[derive(Debug, PartialEq)]
enum Block {
    Heading(String),
    Paragraph(String),
    Bullet(String),
}

/// `parseReleaseNotes`, without a leading level-one heading and with bold
/// marks taken out, as `NoteBody` shows them.
fn blocks(text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut levels = Vec::new();
    let mut current: Option<Block> = None;
    let flush = |current: &mut Option<Block>, blocks: &mut Vec<Block>, levels: &mut Vec<usize>| {
        if let Some(block) = current.take() {
            let empty = match &block {
                Block::Heading(text) | Block::Paragraph(text) | Block::Bullet(text) => {
                    text.is_empty()
                }
            };
            if !empty {
                blocks.push(block);
                levels.push(0);
            }
        }
    };
    for raw in text.trim().lines() {
        let line = raw.trim();
        if line.is_empty() {
            flush(&mut current, &mut blocks, &mut levels);
            continue;
        }
        let hashes = line.bytes().take_while(|byte| *byte == b'#').count();
        if (1..=3).contains(&hashes) && line[hashes..].starts_with(char::is_whitespace) {
            let rest = line[hashes..].trim();
            if !rest.is_empty() {
                flush(&mut current, &mut blocks, &mut levels);
                blocks.push(Block::Heading(rest.to_owned()));
                levels.push(hashes);
                continue;
            }
        }
        let bullet = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .map(str::trim_start)
            .filter(|rest| !rest.is_empty());
        if let Some(rest) = bullet {
            flush(&mut current, &mut blocks, &mut levels);
            current = Some(Block::Bullet(rest.to_owned()));
            continue;
        }
        match &mut current {
            Some(Block::Paragraph(text) | Block::Bullet(text) | Block::Heading(text)) => {
                text.push(' ');
                text.push_str(line);
            }
            None => current = Some(Block::Paragraph(line.to_owned())),
        }
    }
    flush(&mut current, &mut blocks, &mut levels);
    if levels.first() == Some(&1) {
        blocks.remove(0);
    }
    blocks
        .into_iter()
        .map(|block| match block {
            Block::Heading(text) => Block::Heading(text.replace("**", "")),
            Block::Paragraph(text) => Block::Paragraph(text.replace("**", "")),
            Block::Bullet(text) => Block::Bullet(text.replace("**", "")),
        })
        .collect()
}

/// `new Date(releaseDate).toLocaleDateString()`.
fn release_date(value: &str) -> Option<String> {
    let date = chrono::DateTime::parse_from_rfc3339(value)
        .ok()?
        .with_timezone(&chrono::Local)
        .date_naive();
    Some(crate::i18n::format::date(date))
}

impl Render for UpdateDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = div().absolute().inset_0();
        let Some(layer) = self.dialog.layer(window, cx) else {
            return root;
        };
        let (presence, focus) = (layer.presence, layer.focus.clone());
        let theme = theme::theme(cx);
        let state = super::state(cx);
        let this = cx.entity().downgrade();
        let title = match &state.version {
            Some(version) => crate::tf!("Picto {value0}", version),
            None => t("Software Update").to_owned(),
        };

        let paragraph = |text: String| div().child(text);
        let mut summary = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .text_color(theme.text_secondary())
            .child(
                div()
                    .text(theme::TEXT_SM)
                    .text_color(theme.text_tertiary())
                    .child(format!("{}{VERSION}", t("Current version "))),
            );
        let version = state.version.clone().unwrap_or_default();
        summary = match state.status {
            Status::Checking => summary.child(paragraph(t("Checking for the latest version…").into())),
            Status::Current => summary.child(paragraph(t("Picto is up to date.").into())),
            Status::Unavailable => summary.child(paragraph(state.error.clone().unwrap_or_default())),
            Status::Error => summary.child(
                div()
                    .text_color(rgb(theme::DANGER))
                    .child(state.error.clone().unwrap_or_default()),
            ),
            Status::Downloading => {
                let percent = state.progress.map(|progress| progress.percent);
                summary
                    .child(paragraph(format!(
                        "{}{version}… {}",
                        t("Downloading Picto "),
                        percent.map(|percent| format!("{}%", percent.round())).unwrap_or_default()
                    )))
                    .child(
                        ui::progress_bar::progress_bar()
                            .height(4.)
                            .filled("update-download", (percent.unwrap_or(2.) / 100.) as f32),
                    )
            }
            Status::Downloaded => summary.child(paragraph(
                t("The update is ready. Picto will close before the installer starts.").into(),
            )),
            Status::Installed => summary.child(paragraph(
                t("Picto was updated successfully. Here’s what changed.").into(),
            )),
            Status::Available if state.mac() => summary.child(paragraph(
                t("A new version is available. Download it from the release page to update this Mac.").into(),
            )),
            Status::Available | Status::Idle => summary,
        };

        let release = state.version.as_ref().map(|version| {
            let name = state
                .release_name
                .clone()
                .unwrap_or_else(|| format!("{} {version}", en("What’s new in")));
            let notes = blocks(&state.release_notes);
            let body = if notes.is_empty() {
                div()
                    .p(px(14.))
                    .text_color(theme.text_tertiary())
                    .child(t("No release notes were provided."))
                    .into_any_element()
            } else {
                div()
                    .relative()
                    .child(
                        div()
                            .id("update-notes")
                            .max_h(px(310.))
                            .overflow_y_scroll()
                            .track_scroll(&self.notes)
                            .pt(px(16.))
                            .pb(px(20.))
                            .pl(px(18.))
                            .pr(px(18.))
                            .line_height(px(theme::TEXT_MD * 1.5))
                            .children(notes.into_iter().enumerate().map(|(index, block)| {
                                match block {
                                    Block::Heading(text) => div()
                                        .when(index > 0, |heading| heading.mt(px(20.)))
                                        .mb(px(8.))
                                        .text(theme::TEXT_MD)
                                        .line_height(px(theme::TEXT_MD * 1.35))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(theme.text())
                                        .child(text),
                                    Block::Paragraph(text) => div().mb(px(10.)).child(text),
                                    Block::Bullet(text) => div()
                                        .mb(px(9.))
                                        .flex()
                                        .items_start()
                                        .gap(px(8.))
                                        .child(
                                            div()
                                                .w(px(14.))
                                                .flex_none()
                                                .flex()
                                                .justify_center()
                                                .text_color(theme.text_tertiary())
                                                .child("•"),
                                        )
                                        .child(div().flex_1().min_w_0().child(text)),
                                }
                            })),
                    )
                    .child(ui::scrollbar(
                        "update-notes-scrollbar",
                        &self.notes,
                        window,
                        cx,
                    ))
                    .into_any_element()
            };
            div()
                .mt(px(16.))
                .border_1()
                .border_color(theme.border_secondary())
                .rounded(px(theme::RADIUS_MD))
                .overflow_hidden()
                .bg(theme.control_surface())
                .text_color(theme.text_secondary())
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .justify_between()
                        .gap(px(16.))
                        .px(px(18.))
                        .py(px(13.))
                        .text_color(theme.text())
                        .border_b_1()
                        .border_color(theme.border_secondary())
                        .child(div().font_weight(FontWeight::BOLD).child(name))
                        .children(state.release_date.as_deref().and_then(release_date).map(
                            |date| {
                                div()
                                    .flex_none()
                                    .text(theme::TEXT_SM)
                                    .text_color(theme.text_tertiary())
                                    .child(date)
                            },
                        )),
                )
                .child(body)
        });

        let close = {
            let this = this.clone();
            move |_: &mut Window, cx: &mut App| {
                let _ = this.update(cx, |dialog, cx| dialog.close(cx));
            }
        };
        let mut modal = ui::modal::modal("update-dialog", title, presence)
            .focus(&focus)
            .on_close(close.clone())
            .child(summary);
        if let Some(release) = release {
            modal = modal.child(release);
        }
        if state.status == Status::Installed {
            modal = modal.footer(
                ui::action_button("update-done", Look::Primary)
                    .label(t("Done"))
                    .on_click(move |_, window, cx| close(window, cx)),
            );
        } else {
            modal = modal.footer(
                ui::action_button("update-check", Look::Standard)
                    .icon("refresh", 15.)
                    .label(t("Check Again"))
                    .disabled(state.busy())
                    .on_click(|_, _, cx| super::check(cx)),
            );
            // `actionLabel`, which production's dialog leaves in English.
            let label = if state.mac() {
                "Open Download Page" // i18n: production keeps it in English
            } else {
                "Restart and Install" // i18n: production keeps it in English
            };
            let next = (state.status == Status::Downloaded
                || (state.status == Status::Available && state.mac()))
            .then_some(label);
            if let Some(label) = next {
                modal = modal.footer(
                    ui::action_button("update-install", Look::Primary)
                        .icon("download", 15.)
                        .label(label)
                        .on_click(|_, _, cx| super::install(cx)),
                );
            }
        }
        root.child(modal)
    }
}
