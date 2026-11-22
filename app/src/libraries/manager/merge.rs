//! The Library Manager's "Merge with Another Library…": the dialog in which
//! the user chooses the second library and the library that is kept, and the
//! start of the merge.
//!
//! The merge itself is the application's work (`core/src/library_merge.rs`).
//! It runs in the library that is kept, which has to be the open library. So
//! when the user keeps a library that is not open, this view first opens it
//! in the main window (`host::request`) and then sends the command
//! `library.merge.start`. The merge is a foreground task. The main window
//! shows its progress and its Cancel button in the task dialog
//! (`imports/`), so this view brings the main window forward once the merge
//! has started.
//!
//! The texts of the dialog have no key in the locale files, so they are
//! shown in English in every language.
use super::*;
use crate::paths::Canonical as _;
use std::collections::HashMap;

/// The width of the dialog. At this width its two paragraphs take two lines
/// each.
const WIDTH: f32 = 600.;
/// The greatest height of the list of second libraries: two and a half rows,
/// so that a longer list is seen to scroll.
const LIST_HEIGHT: f32 = 65.;

/// What the merge dialog holds while it is open.
#[derive(Clone)]
pub(super) struct Merge {
    /// The library whose menu opened the dialog.
    first: String,
    /// The second library, which the user chooses in the dialog. It is
    /// chosen from the start when the list has only one other library.
    second: Option<String>,
    /// Whether `first` is the library that is kept. Otherwise `second` is.
    keep_first: bool,
    /// For each library that is not open and was read: the number of its
    /// items, or the reason why it cannot be merged.
    counts: HashMap<String, Result<u64, String>>,
    /// Why the merge did not start.
    error: Option<String>,
    /// Whether the user has pressed Merge and the merge has not started
    /// yet. While it is set, only Cancel reacts.
    starting: bool,
    /// Whether the command that starts the merge has been sent.
    sent: bool,
}

impl Merge {
    /// Returns the library that is kept and the other library, or `None`
    /// until the second library is chosen.
    fn sides(&self) -> Option<(String, String)> {
        let second = self.second.clone()?;
        Some(if self.keep_first {
            (self.first.clone(), second)
        } else {
            (second, self.first.clone())
        })
    }
}

/// Returns whether `path` is the folder of the open library.
fn is_open(path: &str, cx: &App) -> bool {
    let path = Path::new(path);
    host::current(cx).is_some_and(|(root, _)| {
        root == path || path.canonical().is_ok_and(|path| path == root)
    })
}

/// Returns the number of items as the dialog writes it: "1 item" or, for
/// example, "12,408 items".
fn items(count: u64) -> String {
    if count == 1 {
        en("1 item").to_owned()
    } else {
        format!("{} items", crate::i18n::format::grouped(count))
    }
}

impl Manager {
    /// Returns the libraries that `first` can be merged with: every other
    /// library of the list whose folder exists.
    fn merge_partners(&self, first: &str) -> Vec<Row> {
        // The consent question passes a library's path in its canonical
        // form, which can be written differently from the path in the list.
        let canonical = Path::new(first).canonical().ok();
        self.rows
            .iter()
            .filter(|row| {
                row.path != first
                    && !row.missing
                    && (canonical.is_none() || Path::new(&row.path).canonical().ok() != canonical)
            })
            .cloned()
            .collect()
    }

    /// Returns whether the library in `row` can be merged with another one:
    /// its folder exists and the list has another library whose folder
    /// exists.
    pub(super) fn can_merge(&self, row: &Row) -> bool {
        !row.missing && !self.merge_partners(&row.path).is_empty()
    }

    /// Opens the merge dialog for the library at `first`. When the dialog is
    /// already open, which is the case after the user has agreed to the
    /// consent question for one of the two libraries, it stays as it is and
    /// the numbers of items are read again.
    pub(super) fn open_merge(&mut self, first: String, cx: &mut Context<Self>) {
        if let Some(merge) = self.merging.content_mut() {
            merge.error = None;
            let paths = [Some(merge.first.clone()), merge.second.clone()];
            for path in paths.into_iter().flatten() {
                self.count_items(path, cx);
            }
            return;
        }
        let partners = self.merge_partners(&first);
        let second = match partners.as_slice() {
            [only] => Some(only.path.clone()),
            _ => None,
        };
        self.merging.open(
            Merge {
                first: first.clone(),
                second: second.clone(),
                keep_first: true,
                counts: HashMap::new(),
                error: None,
                starting: false,
                sent: false,
            },
            ui::menu::Return::Opener,
            cx,
        );
        for path in [Some(first), second].into_iter().flatten() {
            self.count_items(path, cx);
        }
        // The open library's statistics are read again, because items may
        // have been added since the Library Manager last read them.
        self.read_stats(cx);
    }

    /// Reads how many items the library at `path` holds, for its row in the
    /// dialog. The open library's number comes from its statistics. Another
    /// library is read by the application on a background thread
    /// (`picto_core::library_merge::inspect`), which opens it read-only, and
    /// only when `host::permit` allows this process to use that library.
    fn count_items(&mut self, path: String, cx: &mut Context<Self>) {
        if is_open(&path, cx) {
            return;
        }
        if host::permit(Path::new(&path), cx).is_err() {
            return;
        }
        let request = picto_core::library_merge::MergeRequest {
            path: PathBuf::from(&path),
        };
        let read = cx
            .background_executor()
            .spawn(async move { picto_core::library_merge::inspect(&request) });
        cx.spawn(async move |this, cx| {
            let summary = read.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(merge) = this.merging.content_mut() {
                    merge
                        .counts
                        .insert(path, summary.map(|summary| summary.items));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Returns the number of items of the library at `path`, when it is
    /// known.
    fn item_count(&self, merge: &Merge, path: &str, cx: &App) -> Option<u64> {
        if is_open(path, cx) {
            let stats = self.stats.as_ref()?;
            let count = |key: &str| stats[key].as_u64().unwrap_or(0);
            return Some(count("active_items") + count("inbox_items") + count("trash_items"));
        }
        merge.counts.get(path)?.as_ref().ok().copied()
    }

    /// Shows `error` in the dialog and lets the user change the choices
    /// again.
    fn merge_failed(&mut self, error: String, cx: &mut Context<Self>) {
        if let Some(merge) = self.merging.content_mut() {
            merge.error = Some(error);
            merge.starting = false;
            merge.sent = false;
        }
        cx.notify();
    }

    /// Does the Merge button. Both libraries must pass `host::permit`. For a
    /// library that the user has not yet agreed to, the consent question is
    /// shown, and the user presses Merge again afterwards. Then the merge is
    /// started at once when the kept library is the open one. Otherwise the
    /// kept library is opened first, and [`Self::continue_merge`] starts the
    /// merge when it has opened.
    fn start_merge(&mut self, cx: &mut Context<Self>) {
        let Some((kept, other)) = self.merging.content().and_then(Merge::sides) else {
            return;
        };
        for path in [&kept, &other] {
            match host::permit(Path::new(path), cx) {
                Ok(()) => {}
                Err(host::Denied::Error(error)) => return self.merge_failed(error, cx),
                Err(denied) => return self.denied(denied, Some(Act::Merge), cx),
            }
        }
        if host::host(cx).read(cx).busy() {
            return self.merge_failed(
                en("A library operation is already in progress.").to_owned(),
                cx,
            );
        }
        if let Some(merge) = self.merging.content_mut() {
            merge.error = None;
            merge.starting = true;
            merge.sent = false;
        }
        cx.notify();
        if is_open(&kept, cx) {
            self.send_merge(cx);
        } else {
            self.error = None;
            cx.defer(move |cx| host::request(Path::new(&kept), Origin::Manager, cx));
        }
    }

    /// Called whenever the host has changed. While the dialog waits for the
    /// kept library to open, this starts the merge once that library is
    /// open, and shows the reason when it could not be opened.
    pub(super) fn continue_merge(&mut self, cx: &mut Context<Self>) {
        let Some(merge) = self.merging.content() else {
            return;
        };
        if !merge.starting || merge.sent {
            return;
        }
        let Some((kept, _)) = merge.sides() else {
            return;
        };
        let (busy, failure) = {
            let host = host::host(cx).read(cx);
            (host.busy(), host.failure.clone())
        };
        if busy {
            return;
        }
        if let Some((_, message)) = failure {
            return self.merge_failed(message, cx);
        }
        if is_open(&kept, cx) {
            self.send_merge(cx);
        }
    }

    /// Sends `library.merge.start` to the open library, which is the kept
    /// one. When the merge has started, the dialog closes and the main
    /// window, which shows the merge's progress, comes forward.
    fn send_merge(&mut self, cx: &mut Context<Self>) {
        let Some((_, other)) = self.merging.content().and_then(Merge::sides) else {
            return;
        };
        let Some(application) = host::application(cx) else {
            return self.merge_failed(en("No library is open.").to_owned(), cx);
        };
        if let Some(merge) = self.merging.content_mut() {
            merge.sent = true;
        }
        let answer = application.dispatch(
            "library.merge.start",
            serde_json::json!({ "path": other }),
        );
        cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(Ok(started)) => {
                    this.merging.dismiss(cx);
                    cx.notify();
                    // The main window's task dialog shows the merge, also
                    // when the merge has already finished.
                    if let (Some(task), Some(imports)) =
                        (started["task_id"].as_i64(), crate::imports::get(cx))
                    {
                        imports.update(cx, |imports, cx| imports.show_task(task, cx));
                    }
                    cx.defer(crate::app_menu::bring_forward);
                }
                Ok(Err(error)) => this.merge_failed(error, cx),
                Err(_) => this.merge_failed(
                    en("The library was closed before the merge started.").to_owned(),
                    cx,
                ),
            });
        })
        .detach();
    }

    /// Draws the merge dialog.
    pub(super) fn merge_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.merging.layer(window, cx)?;
        let (presence, focus) = (dialog.presence, dialog.focus.clone());
        let merge = &dialog.content.clone();
        let theme = theme::theme(cx);
        let partners = self.merge_partners(&merge.first);
        let starting = merge.starting;
        let first_name = crate::libraries::name(&merge.first);

        let mut body = ui::form::stack().gap(px(14.));
        // The reason why the merge did not start is shown first. Without
        // one, the reason why the other library cannot be read is shown.
        let unreadable = merge
            .sides()
            .and_then(|(_, other)| merge.counts.get(&other)?.as_ref().err().cloned());
        if let Some(error) = merge.error.clone().or(unreadable) {
            body = body.child(ui::form::error_banner(theme, error));
        }
        body = body.child(ui::form::help_text(
            theme,
            en("Every item, tag, folder, smart folder and collection of both libraries is put into one library. A file that both libraries hold is kept once. Nothing is deleted from the other library."),
        ));

        // The list of second libraries is shown when there is a choice.
        if partners.len() > 1 {
            let rows = partners.iter().enumerate().map(|(index, row)| {
                let path = row.path.clone();
                let choose = cx.listener(move |this, _: &(), _, cx| {
                    if let Some(merge) = this.merging.content_mut() {
                        merge.second = Some(path.clone());
                        merge.error = None;
                    }
                    this.count_items(path.clone(), cx);
                    cx.notify();
                });
                ui::check::check(
                    ("library-merge-with", index),
                    row.name.clone(),
                    merge.second.as_deref() == Some(row.path.as_str()),
                )
                .radio()
                .disabled(starting)
                .on_toggle(move |window, cx| choose(&(), window, cx))
            });
            body = body.child(ui::form::field(
                &theme,
                format!("Merge “{first_name}” with"),
                div()
                    .id("library-merge-partners")
                    .role(Role::RadioGroup)
                    .aria_label(en("The second library"))
                    .max_h(px(LIST_HEIGHT))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .children(rows),
            ));
        }

        if let Some(second) = merge.second.clone() {
            let kept = [(merge.first.clone(), true), (second, false)]
                .into_iter()
                .enumerate()
                .map(|(index, (path, is_first))| {
                    let keep = cx.listener(move |this, _: &(), _, cx| {
                        if let Some(merge) = this.merging.content_mut() {
                            merge.keep_first = is_first;
                            merge.error = None;
                        }
                        cx.notify();
                    });
                    let mut row = ui::check::check(
                        ("library-merge-keep", index),
                        crate::libraries::name(&path),
                        merge.keep_first == is_first,
                    )
                    .radio()
                    .detail(path.clone())
                    .disabled(starting)
                    .on_toggle(move |window, cx| keep(&(), window, cx));
                    if let Some(count) = self.item_count(merge, &path, cx) {
                        row = row.trailing(items(count));
                    }
                    row
                });
            body = body.child(ui::form::field(
                &theme,
                en("Library to keep"),
                div()
                    .id("library-merge-kept")
                    .role(Role::RadioGroup)
                    .aria_label(en("Library to keep"))
                    .flex()
                    .flex_col()
                    .children(kept),
            ));
        }
        body = body.child(ui::form::help_text(
            theme,
            en("The library that is kept receives the content of the other one. Subscriptions, cloud sync settings, watched folders and application settings of the other library are not carried over. A merge cannot be undone with Undo."),
        ));

        let close = cx.listener(|this: &mut Self, _: &(), _, cx| this.merging.dismiss(cx));
        let cancel = cx.listener(|this: &mut Self, _: &(), _, cx| {
            this.merging.close(cx);
        });
        let accept = cx.listener(|this: &mut Self, _: &(), _, cx| this.start_merge(cx));
        let ready = merge.second.is_some() && !starting;
        Some(
            ui::modal::modal("library-merge", en("Merge Libraries"), presence)
                .width(WIDTH)
                .focus(&focus)
                .on_close(move |window, cx| close(&(), window, cx))
                .child(body)
                .footer(
                    ui::action_button::action_button(
                        "library-merge-cancel",
                        ui::action_button::Look::Standard,
                    )
                    .label(t("Cancel"))
                    .on_click(move |_, window, cx| cancel(&(), window, cx)),
                )
                .footer(
                    ui::action_button::action_button(
                        "library-merge-start",
                        ui::action_button::Look::Primary,
                    )
                    .label(if starting {
                        t("Starting…")
                    } else {
                        en("Merge")
                    })
                    .disabled(!ready)
                    .on_click(move |_, window, cx| accept(&(), window, cx)),
                )
                .into_any_element(),
        )
    }
}
