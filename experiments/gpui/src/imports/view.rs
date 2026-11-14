//! What the imports show: the foreground task dialog (`TaskDialog.tsx`),
//! the task bar of tasks sent to the background, and the dialogs an import
//! asks in (`FolderImportModal`, "Import as Collection?").
use super::task::{Phase, StopChoice, Task, Tone};
use super::{Analysis, Dialog, FolderOption, Imports, TaskView, folder_name};
use crate::theme::Text as _;
use crate::ui::action_button::Look as ButtonLook;
use crate::ui::icon_button::Look as IconLook;
use crate::{motion, theme, ui};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use std::time::Instant;

/// `.dialog`: `width: min(440px, calc(100vw - 48px))`, inside a 1px glass
/// border.
const DIALOG_WIDTH: f32 = 440.;
const BORDER: f32 = 1.;
/// `.dialog`'s and `.barRow`'s corners.
const DIALOG_RADIUS: f32 = 12.;
const BAR_RADIUS: f32 = 10.;
/// The body's line height, which text with none of its own inherits from
/// the page (`--mantine-line-height`).
fn line(size: f32) -> Pixels {
    px(size * theme::LINE_HEIGHT_BODY)
}

/// Runs `action` on the imports from a control's handler.
fn on(
    this: &WeakEntity<Imports>,
    action: impl Fn(&mut Imports, &mut Context<Imports>) + 'static,
) -> impl Fn(&mut Window, &mut App) + 'static {
    let this = this.clone();
    move |_, cx| {
        let _ = this.update(cx, |imports, cx| action(imports, cx));
    }
}

fn on_click(
    this: &WeakEntity<Imports>,
    action: impl Fn(&mut Imports, &mut Context<Imports>) + 'static,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let handler = on(this, action);
    move |_, window, cx| handler(window, cx)
}

impl Render for Imports {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The dialog follows the task in front; a new one comes in afresh.
        let focused = self.focused().map(|task| task.task_id);
        if self.view.task != focused {
            if self.view.focused && self.focus.contains_focused(window, cx) {
                window.focus(&self.back, cx);
            }
            self.view = TaskView {
                task: focused,
                shown: focused.map(|_| Instant::now()),
                ..Default::default()
            };
        }
        let mut root = div().absolute().inset_0();
        root = root.children(self.bar(window, cx));
        root = root.children(self.task_dialog(window, cx));
        root = root.children(self.dialog_layer(window, cx));
        root
    }
}

impl Imports {
    // ── The task dialog ─────────────────────────────────────────

    fn task_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let task = self.focused()?.clone();
        let theme = theme::theme(cx);
        let this = cx.entity().downgrade();
        if !self.view.focused {
            self.view.focused = true;
            window.focus(&self.focus, cx);
        }
        let shown = self
            .view
            .shown
            .map(|since| motion::progress(since.elapsed(), motion::TASK_DIALOG_IN))
            .unwrap_or(1.);
        if shown < 1. {
            window.request_animation_frame();
        }
        let eased = motion::EASE_SCALE_IN.ease(shown);
        let viewport = window.viewport_size();
        let width = DIALOG_WIDTH.min(f32::from(viewport.width) - 48.);
        let (body, foot) = if task.open() && self.view.stopping.is_some() {
            self.stop_view(&task, &this, cx)
        } else {
            self.task_view(&task, &this, window, cx)
        };
        let panel = ui::glass_panel("task-dialog", DIALOG_RADIUS, cx)
            .w(px(width))
            .overflow_hidden()
            .opacity(eased)
            .p(px(BORDER))
            .flex()
            .flex_col()
            .text_color(theme.text())
            .font_family(theme::FONT_UI)
            .text_size(px(theme::TEXT_MD))
            .line_height(line(theme::TEXT_MD))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(body)
            .child(foot);
        let open = task.open();
        let id = task.task_id;
        let escape = this.clone();
        let layer = ui::modal::backdrop("task-backdrop", eased, Some(&self.focus), window)
            // Escape sends a working import to the task bar.
            .on_action(move |_: &ui::modal::Close, _, cx| {
                if open {
                    let _ = escape.update(cx, |imports, cx| imports.minimize(id, cx));
                }
            })
            .child(panel);
        Some(ui::modal::over_window(layer, crate::ui::layer::DIALOG).into_any_element())
    }

    /// `TaskView`: the progress, the counts and the controls.
    fn task_view(
        &mut self,
        task: &Task,
        this: &WeakEntity<Imports>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (AnyElement, AnyElement) {
        let theme = theme::theme(cx);
        let phase = task.phase();
        let ended = !task.open();
        let id = task.task_id;
        let mut buttons = div()
            .flex()
            .flex_none()
            .gap(px(2.))
            .mt(px(-6.))
            .mr(px(-10.));
        if !ended {
            buttons = buttons.child(
                ui::icon_button("task-minimize", "task-minus")
                    .look(IconLook::Task)
                    .tip(ui::tip(crate::i18n::t("Minimize")))
                    .on_click(on_click(this, move |imports, cx| imports.minimize(id, cx))),
            );
        }
        buttons = buttons.child(
            ui::icon_button("task-close", "notify-close")
                .look(IconLook::Task)
                .tip(ui::tip(crate::i18n::t("Close")))
                .on_click(on_click(this, move |imports, cx| {
                    if ended {
                        imports.dismiss(id, cx);
                    } else {
                        imports.ask_to_stop(cx);
                    }
                })),
        );
        let warning = task.error.is_some();
        let head = div()
            .flex()
            .items_start()
            .gap(px(10.))
            .min_w_0()
            .child(titles(task.heading(), task.subtitle(), warning, &theme))
            .child(buttons);
        let (left, right) = task.counts();
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .pt(px(18.))
            .px(px(20.))
            .pb(px(16.))
            .child(head)
            .child(progress(task, 6., 4., window, cx))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .gap(px(12.))
                    .text_size(px(theme::TEXT_SM))
                    .line_height(line(theme::TEXT_SM))
                    .text_color(theme.text_secondary())
                    .font_features(theme::tabular_figures())
                    .child(div().text_color(theme.text()).child(left))
                    .children(right.map(|right| div().child(right))),
            );
        if task.existing > 0 || task.failed > 0 {
            body = body.child(
                div()
                    .flex()
                    .gap(px(12.))
                    .text_size(px(11.5))
                    .line_height(line(11.5))
                    .text_color(theme.text_tertiary())
                    .font_features(theme::tabular_figures())
                    .children((task.existing > 0).then(|| {
                        div().child(crate::tf!(
                            "{value0} already in library",
                            crate::grid::format::grouped(task.existing)
                        ))
                    }))
                    .children((task.failed > 0).then(|| {
                        div()
                            .text_color(rgb(theme::TASK_WARNING_GLYPH))
                            .child(crate::tf!(
                                "{value0} failed",
                                crate::grid::format::grouped(task.failed)
                            ))
                    })),
            );
        }
        if self.view.show_failures && task.failed > 0 {
            body = body.children(self.failures(&theme));
        }
        let mut foot = foot(&theme);
        if ended && task.failed > 0 {
            foot = foot.child(
                foot_button("task-failures", ButtonLook::Ghost)
                    .padding(8.)
                    .icon("task-alert-triangle", 14.)
                    .label(if self.view.show_failures {
                        crate::i18n::t("Hide failed").to_string()
                    } else {
                        crate::tf!(
                            "Show {value0} failed",
                            crate::grid::format::grouped(task.failed)
                        )
                    })
                    .on_click(on_click(this, |imports, cx| imports.toggle_failures(cx))),
            );
        }
        foot = foot.child(div().flex_1());
        if ended {
            foot = foot.child(
                foot_button("task-done", ButtonLook::Primary)
                    .label(crate::i18n::t("Done"))
                    .on_click(on_click(this, move |imports, cx| imports.dismiss(id, cx))),
            );
        } else if phase != Phase::Removing {
            foot = foot.child(if phase == Phase::Paused {
                foot_button("task-resume", ButtonLook::Primary)
                    .icon("task-player-play", 14.)
                    .label(crate::i18n::t("Resume"))
                    .on_click(on_click(this, move |imports, cx| imports.resume(id, cx)))
            } else {
                foot_button("task-pause", ButtonLook::Standard)
                    .icon("task-player-pause", 14.)
                    .label(crate::i18n::t("Pause"))
                    .on_click(on_click(this, move |imports, cx| imports.pause(id, cx)))
            });
            foot = foot.child(
                foot_button("task-stop", ButtonLook::Standard)
                    .icon("task-player-stop", 14.)
                    .label(crate::i18n::t("Stop…"))
                    .on_click(on_click(this, |imports, cx| imports.ask_to_stop(cx))),
            );
        }
        (body.into_any_element(), foot.into_any_element())
    }

    /// `Failures`: the files that could not be imported, name and reason.
    fn failures(&self, theme: &theme::Theme) -> Option<Stateful<Div>> {
        let failures = self.view.failures.as_ref()?;
        Some(
            div()
                .id("task-failures-list")
                .max_h(px(140.))
                .overflow_y_scroll()
                .px(px(8.))
                .py(px(6.))
                .rounded(px(theme::RADIUS_MD))
                .bg(theme.control_surface())
                .text_size(px(11.5))
                .line_height(line(11.5))
                .children(failures.iter().map(|failure| {
                    let name = std::path::Path::new(&failure.path)
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| failure.path.clone());
                    div()
                        .flex()
                        .gap(px(8.))
                        .py(px(2.))
                        .min_w_0()
                        .child(
                            div()
                                .flex_none()
                                .max_w(relative(0.45))
                                .truncate()
                                .child(name),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_color(theme.text_tertiary())
                                .child(failure.error.clone()),
                        )
                })),
        )
    }

    /// `StopView`: what becomes of what the import added.
    fn stop_view(
        &mut self,
        task: &Task,
        this: &WeakEntity<Imports>,
        cx: &mut Context<Self>,
    ) -> (AnyElement, AnyElement) {
        let theme = theme::theme(cx);
        let choices = task.stop_choices();
        let chosen = self.view.choice.unwrap_or(StopChoice::Keep);
        let head = div()
            .flex()
            .items_start()
            .gap(px(10.))
            .min_w_0()
            .child(titles(
                crate::tf!("Stop importing “{value0}”?", task.title),
                crate::tf!(
                    "{value0} of {value1} files are done so far",
                    crate::grid::format::grouped(task.done),
                    crate::grid::format::grouped(task.total)
                ),
                false,
                &theme,
            ));
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .pt(px(18.))
            .px(px(20.))
            .pb(px(16.))
            .child(head);
        if choices.len() > 1 {
            body = body.child(div().flex().flex_col().gap(px(6.)).mt(px(2.)).children(
                choices.into_iter().map(|(choice, title, detail)| {
                    let selected = choice == chosen;
                    let danger = choice == StopChoice::Remove;
                    let (border, fill) = match (selected, danger) {
                        (true, true) => (
                            rgba(theme::TASK_DANGER_BORDER),
                            rgba(theme::TASK_DANGER_FILL),
                        ),
                        (true, false) => (theme.primary_at(60.), theme.primary_at(10.)),
                        _ => (theme.border(), theme.surface_hover()),
                    };
                    let mark = if danger {
                        rgb(theme::TASK_DANGER_MARK)
                    } else {
                        theme.primary()
                    };
                    let hover = theme.surface_active();
                    div()
                        .id(match choice {
                            StopChoice::Keep => "stop-keep",
                            StopChoice::Remove => "stop-remove",
                        })
                        .flex()
                        .items_start()
                        .gap(px(10.))
                        .pt(px(9.))
                        .pb(px(9.))
                        .pr(px(10.))
                        .pl(px(12.))
                        .rounded(px(theme::RADIUS_LG))
                        .border_1()
                        .border_color(border)
                        .bg(fill)
                        .cursor_pointer()
                        .when(!selected, |row| row.hover(move |style| style.bg(hover)))
                        .on_click(on_click(this, move |imports, cx| {
                            imports.view.choice = Some(choice);
                            cx.notify();
                        }))
                        .child(
                            div()
                                .flex()
                                .flex_1()
                                .flex_col()
                                .gap(px(2.))
                                .min_w_0()
                                .child(
                                    div()
                                        .text_size(px(12.5))
                                        .line_height(line(12.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(title),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.5))
                                        .line_height(px(11.5 * 1.4))
                                        .text_color(theme.text_secondary())
                                        .child(detail),
                                ),
                        )
                        .child(
                            div()
                                .flex_none()
                                .self_center()
                                .size(px(16.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .children(
                                    selected.then(|| ui::icon("task-choice-check", 14., mark)),
                                ),
                        )
                }),
            ));
        } else if let Some((_, _, detail)) = choices.into_iter().next() {
            body = body.child(
                div()
                    .text_size(px(theme::TEXT_SM))
                    .line_height(px(theme::TEXT_SM * 1.45))
                    .text_color(theme.text_secondary())
                    .child(detail),
            );
        }
        let remove = chosen == StopChoice::Remove;
        let foot = foot(&theme)
            .child(div().flex_1())
            .child(
                foot_button("stop-cancel", ButtonLook::Standard)
                    .label(crate::i18n::t("Keep importing"))
                    .on_click(on_click(this, |imports, cx| imports.keep_importing(cx))),
            )
            .child(
                foot_button(
                    "stop-confirm",
                    if remove {
                        ButtonLook::Danger
                    } else {
                        ButtonLook::Primary
                    },
                )
                .label(if remove {
                    crate::i18n::t("Stop and remove")
                } else {
                    crate::i18n::t("Stop import")
                })
                .on_click(on_click(this, |imports, cx| imports.stop(cx))),
            );
        (body.into_any_element(), foot.into_any_element())
    }

    // ── The task bar ────────────────────────────────────────────

    /// `TaskBar`: one row per task sent to the background, above the
    /// bottom edge between the rails.
    fn bar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Div> {
        let shown: Vec<Task> = self.presentation.in_bar(&self.tasks).cloned().collect();
        if shown.is_empty() {
            return None;
        }
        let theme = theme::theme(cx);
        let this = cx.entity().downgrade();
        let (left, right) = self.rails;
        Some(
            div()
                .absolute()
                .left(px(left + 12.))
                .right(px(right + 12.))
                .bottom(px(12.))
                .flex()
                .flex_col()
                .gap(px(6.))
                .children(shown.into_iter().map(|task| {
                    let id = task.task_id;
                    let phase = task.phase();
                    let control = match phase {
                        Phase::Importing | Phase::Finding => Some(
                            ui::icon_button(("task-bar-pause", id as u64), "task-player-pause")
                                .look(IconLook::Task)
                                .on_click(on_click(&this, move |imports, cx| {
                                    imports.pause(id, cx)
                                })),
                        ),
                        Phase::Paused => Some(
                            ui::icon_button(("task-bar-resume", id as u64), "task-player-play")
                                .look(IconLook::Task)
                                .on_click(on_click(&this, move |imports, cx| {
                                    imports.resume(id, cx)
                                })),
                        ),
                        _ => None,
                    };
                    let panel = ui::glass_panel(("task-bar", id as u64), BAR_RADIUS, cx)
                        .relative()
                        .h(px(40.))
                        // 8px and 6px inside the 1px glass edge.
                        .pl(px(9.))
                        .pr(px(7.))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .text_color(theme.text())
                        .font_family(theme::FONT_UI)
                        .text_size(px(theme::TEXT_SM))
                        .line_height(line(theme::TEXT_SM))
                        .occlude()
                        .child(kind(&task, 24., 6., &theme))
                        .child(
                            div()
                                .id(("task-bar-title", id as u64))
                                .max_w(relative(0.4))
                                .min_w_0()
                                .truncate()
                                .cursor_pointer()
                                .on_click(on_click(&this, move |imports, cx| imports.show(id, cx)))
                                .child(task.heading()),
                        )
                        .child(div().flex_1().child(progress(&task, 4., 0., window, cx)))
                        .child(
                            div()
                                .flex_none()
                                .whitespace_nowrap()
                                .text_color(theme.text_secondary())
                                .font_features(theme::tabular_figures())
                                .child(task.bar_detail()),
                        )
                        .children(control)
                        .child(
                            ui::icon_button(("task-bar-show", id as u64), "task-chevron-up")
                                .look(IconLook::Task)
                                .tip(ui::tip(crate::i18n::t("Show")).above())
                                .on_click(on_click(&this, move |imports, cx| imports.show(id, cx))),
                        );
                    panel
                })),
        )
    }

    // ── The import dialogs ──────────────────────────────────────

    fn dialog_layer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let blocked = super::task::gate(&self.tasks, super::IMPORT).map(|(_, reason)| reason);
        let layer = self.dialog.layer(window, cx)?;
        let theme = theme::theme(cx);
        let this = cx.entity().downgrade();
        let close = on(&this, |imports, cx| imports.dialog.dismiss(cx));
        Some(match &layer.content {
            Dialog::Collection { paths, .. } => ui::confirm::confirm(
                "import-collection",
                crate::i18n::t("Import as Collection?"),
                format!(
                    "Do you wish to import these {} media items as a collection?",
                    paths.len()
                ),
                crate::i18n::t("Yes"),
                layer.presence,
            )
            .cancel_label(crate::i18n::t("No"))
            .focus(&layer.focus)
            .on_close(close)
            .on_cancel(on(&this, |imports, cx| {
                imports.answer_collection(false, cx)
            }))
            .on_confirm(on(&this, |imports, cx| imports.answer_collection(true, cx)))
            .into_any_element(),
            Dialog::Folder(folder) => {
                let name = folder_name(&folder.path);
                let label = format!("{}{name}", crate::i18n::t("Import "));
                let bold = label.len() - name.len()..label.len();
                let analyzing = matches!(folder.analysis, Analysis::Checking);
                let failed = matches!(folder.analysis, Analysis::Failed);
                let message = match &folder.analysis {
                    Analysis::Done(analysis) => super::plan::consolidation_message(analysis),
                    _ => None,
                };
                let row = |label: &'static str, id: &'static str, on: bool, option, disabled| {
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .child(
                            div()
                                .text(theme::TEXT_MD)
                                .line_height(px(theme::LINE_HEIGHT_MD))
                                .text_color(theme.text())
                                .child(label),
                        )
                        .child(
                            ui::toggle(id, on)
                                .disabled(disabled)
                                .on_toggle(self::on(&this, move |imports, cx| {
                                    imports.toggle_folder_option(option, cx)
                                })),
                        )
                };
                let help = |text: &'static str| {
                    div()
                        .text(theme::TEXT_SM)
                        .line_height(px(18.))
                        .text_color(theme.text_secondary())
                        .child(text)
                };
                ui::modal("import-folder", crate::i18n::t("Import Folder"), layer.presence)
                    .size(ui::modal::Size::Small)
                    .focus(&layer.focus)
                    .on_close(close)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(16.))
                            .child(
                                div()
                                    .text(theme::TEXT_SM)
                                    .line_height(px(18.))
                                    .text_color(theme.text_secondary())
                                    .child(StyledText::new(label).with_highlights([(
                                        bold,
                                        HighlightStyle {
                                            font_weight: Some(FontWeight::BOLD),
                                            ..Default::default()
                                        },
                                    )])),
                            )
                            .children(analyzing.then(|| help(crate::i18n::t("Checking the folder structure..."))))
                            .children(message.map(|message| ui::form::warning_box(theme, message)))
                            .children(failed.then(|| {
                                ui::form::warning_box(
                                    theme,
                                    crate::i18n::t("Picto could not inspect this folder. Choose it again or check that it is still available."),
                                )
                            }))
                            .child(row(
                                crate::i18n::t("Preserve folder structure"),
                                "import-preserve",
                                folder.preserve_structure,
                                FolderOption::Preserve,
                                false,
                            ))
                            .child(row(
                                crate::i18n::t("Include subfolders"),
                                "import-subfolders",
                                folder.include_subfolders,
                                FolderOption::Subfolders,
                                false,
                            ))
                            .child(row(
                                crate::i18n::t("Include folders without media"),
                                "import-without-media",
                                folder.include_folders_without_media,
                                FolderOption::WithoutMedia,
                                !folder.preserve_structure,
                            ))
                            .child(row(
                                crate::i18n::t("Watch this folder"),
                                "import-watch",
                                folder.watch,
                                FolderOption::Watch,
                                false,
                            ))
                            .child(help(
                                crate::i18n::t("Automatically import new media added to this folder. Watching uses the same subfolder setting above."),
                            ))
                            .child(help(
                                crate::i18n::t("Folders whose subtree contains no supported media are skipped by default."),
                            )),
                    )
                    .footer(
                        ui::action_button("import-folder-cancel", ButtonLook::Standard)
                            .label(crate::i18n::t("Cancel"))
                            .on_click(on_click(&this, |imports, cx| imports.dialog.dismiss(cx))),
                    )
                    .footer({
                        let mut button = ui::action_button("import-folder-go", ButtonLook::Primary)
                            .label(crate::i18n::t("Import"))
                            .disabled(analyzing || failed || blocked.is_some());
                        if let Some(reason) = blocked {
                            button = button.tip(ui::tip(reason));
                        }
                        button.on_click(on_click(&this, |imports, cx| imports.import_folder(cx)))
                    })
                    .into_any_element()
            }
        })
    }
}

/// The dialog's title and subtitle (`.titles`): the title on one line, the
/// subtitle under it, wrapping only when it is a warning.
fn titles(title: String, subtitle: String, warning: bool, theme: &theme::Theme) -> Div {
    let subtitle = div()
        .mt(px(2.))
        .text_size(px(theme::TEXT_SM))
        .line_height(line(theme::TEXT_SM))
        .child(subtitle);
    div()
        .flex_1()
        .min_w_0()
        .child(
            div()
                .truncate()
                .text_size(px(theme::TEXT_LG))
                .line_height(line(theme::TEXT_LG))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(if warning {
            subtitle.text_color(rgb(theme::TASK_WARNING_GLYPH))
        } else {
            subtitle.truncate().text_color(theme.text_secondary())
        })
}

/// `.foot`: the footer strip of buttons.
fn foot(theme: &theme::Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .pt(px(10.))
        .pb(px(10.))
        .pr(px(12.))
        .pl(px(14.))
        .bg(theme.footer_surface())
        .border_t_1()
        .border_color(theme.border_secondary())
}

/// `.foot button`: 28px, 14px either side, 12.5px text.
fn foot_button(id: &'static str, look: ButtonLook) -> ui::action_button::ActionButton {
    ui::action_button(id, look)
        .height(28.)
        .padding(14.)
        .text_size(12.5)
}

/// `TaskIcon` (`.kind`): the task's glyph on its tone's tile.
fn kind(task: &Task, size: f32, radius: f32, theme: &theme::Theme) -> Div {
    let (glyph, tile) = match task.tone() {
        Tone::Working => (rgb(theme::TASK_WORKING_GLYPH), theme.primary_at(20.)),
        Tone::Paused => (theme.text_secondary(), theme.contrast(8.)),
        Tone::Done => (rgb(theme::TASK_DONE_GLYPH), rgba(theme::TASK_DONE_TILE)),
        Tone::Warning => (
            rgb(theme::TASK_WARNING_GLYPH),
            rgba(theme::TASK_WARNING_TILE),
        ),
    };
    div()
        .flex_none()
        .size(px(size))
        .rounded(px(radius))
        .flex()
        .items_center()
        .justify_center()
        .bg(tile)
        .child(ui::icon(task.glyph(), 17., glyph))
}

/// The progress track (`.track`, `.fill`): a share that moves over 250 ms,
/// or a 30% segment sweeping across while the share is unknown.
fn progress(task: &Task, height: f32, top: f32, window: &mut Window, cx: &mut App) -> Div {
    let theme = theme::theme(cx);
    let fill_color = match task.tone() {
        Tone::Working => theme.primary(),
        Tone::Paused | Tone::Warning => theme.contrast(35.),
        Tone::Done => rgb(theme::TASK_DONE_FILL),
    };
    let fill = if task.indeterminate() {
        window.request_animation_frame();
        div()
            .absolute()
            .top_0()
            .h_full()
            .w(relative(0.3))
            .left(relative(sweep(motion::loop_clock())))
    } else {
        // Where the fill was, where it goes, and since when.
        let state = window.use_keyed_state(
            ElementId::NamedInteger("task-fill".into(), task.task_id as u64),
            cx,
            |_, _| (0f32, 0f32, Instant::now()),
        );
        let target = task.share();
        let (from, to, since) = *state.read(cx);
        let now_at = from
            + (to - from)
                * motion::EASE_DEFAULT
                    .ease(motion::progress(since.elapsed(), motion::TASK_PROGRESS));
        if (target - to).abs() > f32::EPSILON {
            state.update(cx, |state, _| *state = (now_at, target, Instant::now()));
        }
        let (from, to, since) = *state.read(cx);
        let along = motion::progress(since.elapsed(), motion::TASK_PROGRESS);
        if along < 1. {
            window.request_animation_frame();
        }
        let share = from + (to - from) * motion::EASE_DEFAULT.ease(along);
        div().h_full().w(relative(share))
    };
    div()
        .relative()
        .w_full()
        .h(px(height))
        .mt(px(top))
        .overflow_hidden()
        .rounded(px(6.))
        .bg(theme.contrast(10.))
        .child(fill.rounded(px(6.)).bg(fill_color))
}

/// `@keyframes indeterminate`: the segment's left edge, as a share of the
/// track, moving from `translateX(-110%)` to `translateX(340%)` of its own
/// width on `ease-in-out`, over and over.
fn sweep(elapsed: std::time::Duration) -> f32 {
    let period = motion::TASK_PROGRESS_SWEEP.as_secs_f32();
    let along = (elapsed.as_secs_f32() % period) / period;
    0.3 * (-1.1 + 4.5 * motion::EASE_IN_OUT.ease(along))
}

#[cfg(test)]
mod tests {
    use super::sweep;
    use std::time::Duration;

    #[test]
    fn the_unknown_progress_segment_sweeps_from_before_the_track_to_past_it() {
        assert!((sweep(Duration::ZERO) - 0.3 * -1.1).abs() < 1e-4);
        assert!((sweep(Duration::from_millis(1099)) - 0.3 * 3.4).abs() < 1e-2);
    }
}
