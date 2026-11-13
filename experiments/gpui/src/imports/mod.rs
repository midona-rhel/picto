//! Importing, as `filesController.ts` and the task runtime do it: files and
//! folders dropped on the window, chosen in the system's dialogs or pasted,
//! sent to the application's import queue (`imports.enqueue`), and the
//! foreground task that follows each import (`TaskDialog.tsx`, `TaskBar`)
//! with its progress, Pause, Stop and the task bar it can be sent to.
//!
//! One `Imports` view serves the window; the shell draws it over everything
//! and anything else reaches it through [`get`].
pub mod plan;
pub mod task;
mod view;

use crate::session::Session;
use crate::ui;
use gpui::*;
use plan::{Batch, Destination, Dropped, Params};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use task::{Failure, Presentation, StopChoice, Task};

/// `taskSettle.ts`: a working task is read again this often.
const WORKING_POLL: Duration = Duration::from_millis(1000);
/// Bursts of `tasks` changes are read once, after this long.
const REFRESH_COALESCE: Duration = Duration::from_millis(150);
/// The action an import task blocks (`IMPORT_ACTION`).
const IMPORT: &str = "import";

/// The dialogs an import may ask in.
enum Dialog {
    /// `FolderImportModal`: how a folder is imported.
    Folder(FolderDialog),
    /// "Import as Collection?" for several files under the `ask` setting.
    Collection { paths: Vec<PathBuf>, params: Params },
}

struct FolderDialog {
    path: PathBuf,
    destination: Destination,
    preserve_structure: bool,
    include_subfolders: bool,
    include_folders_without_media: bool,
    watch: bool,
    /// What `imports.folder.analyze` said of the folder's depth.
    analysis: Analysis,
    /// Counts the analyses asked for, so an old answer is ignored.
    generation: u64,
}

enum Analysis {
    /// Not asked: structure is not preserved.
    None,
    Checking,
    Done(serde_json::Value),
    Failed,
}

/// What the foreground dialog shows of its task beyond the task itself.
#[derive(Default)]
struct TaskView {
    task: Option<i64>,
    /// Stopping: the question is shown; `true` when the import was running
    /// and resumes if the user keeps importing.
    stopping: Option<bool>,
    choice: Option<StopChoice>,
    show_failures: bool,
    failures: Option<Vec<Failure>>,
    /// When the dialog came in, for `dialogIn`.
    shown: Option<Instant>,
    /// The dialog has taken the keyboard.
    focused: bool,
}

pub struct Imports {
    session: Entity<Session>,
    tasks: Vec<Task>,
    presentation: Presentation,
    /// The list is being read; `true` inside when it should be read again.
    reading: Option<bool>,
    coalesce: Option<gpui::Task<()>>,
    poll: Option<gpui::Task<()>>,
    view: TaskView,
    dialog: ui::modal::Host<Dialog>,
    /// Holds the keyboard while the task dialog shows, so that the window's
    /// shortcuts are held back (`useShortcutSuspension`) and Escape reaches it.
    focus: FocusHandle,
    /// Where the keyboard goes back to.
    back: FocusHandle,
    /// The rails' widths, which the task bar keeps clear of.
    pub(crate) rails: (f32, f32),
    _subscriptions: Vec<Subscription>,
}

struct ImportsGlobal(WeakEntity<Imports>);

impl Global for ImportsGlobal {}

/// Import Folder's key, which production's application menu fixes
/// (`Shift+CmdOrCtrl+I`) rather than the shortcut registry.
pub fn bind(cx: &mut App) {
    let keys = if cfg!(target_os = "macos") {
        "cmd-shift-i"
    } else {
        "ctrl-shift-i"
    };
    let context = crate::keymap::Scope::App.predicate();
    cx.bind_keys([KeyBinding::new(
        keys,
        crate::keymap::ImportFolder,
        context.as_deref(),
    )]);
}

/// The window's imports, for the views that start one.
pub fn get(cx: &App) -> Option<Entity<Imports>> {
    cx.try_global::<ImportsGlobal>()?.0.upgrade()
}

/// Why an import cannot start now (`taskGate('import').reason`), for a
/// disabled row's tooltip; `None` when one can.
pub fn blocked(cx: &App) -> Option<String> {
    let imports = get(cx)?;
    task::gate(&imports.read(cx).tasks, IMPORT).map(|(_, reason)| reason)
}

impl Imports {
    pub fn new(session: Entity<Session>, back: FocusHandle, cx: &mut Context<Self>) -> Self {
        cx.set_global(ImportsGlobal(cx.entity().downgrade()));
        let subscriptions = vec![cx.subscribe(
            &session,
            |this, _, event: &crate::application::Event, cx| {
                if let crate::application::Event::Changed(changed) = event
                    && crate::application::touches(changed, "tasks")
                {
                    this.refresh_soon(cx);
                }
            },
        )];
        let mut imports = Self {
            session,
            tasks: Vec::new(),
            presentation: Presentation::default(),
            reading: None,
            coalesce: None,
            poll: None,
            view: TaskView::default(),
            dialog: ui::modal::Host::default(),
            focus: cx.focus_handle(),
            back,
            rails: (0., 0.),
            _subscriptions: subscriptions,
        };
        // A task left open when the app last closed takes the foreground.
        imports.refresh(cx);
        imports
    }

    // ── Starting an import ──────────────────────────────────────

    /// Files or folders dropped on the window (`onDragDropEvent`'s drop).
    pub fn drop_paths(
        &mut self,
        paths: Vec<PathBuf>,
        destination: Destination,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match plan::classify(paths, |path| {
            std::fs::metadata(path).ok().map(|meta| meta.is_dir())
        }) {
            Dropped::Nothing => {}
            Dropped::PackMixed => {
                self.report(
                    "Could not import Picto Pack",
                    "Drop one Picto Pack at a time without other files.",
                    cx,
                );
            }
            Dropped::Pack(path) => {
                // TODO: inspect the pack (`picto_pack.inspect`) and open the
                // Picto Pack import dialog for it.
                self.report(
                    "Could not import Picto Pack",
                    &format!(
                        "Picto Pack import is not built yet: {}",
                        path.to_string_lossy()
                    ),
                    cx,
                );
            }
            Dropped::Folder(path) => {
                if self.allow(cx) {
                    self.open_folder_dialog(path, destination, window, cx);
                }
            }
            Dropped::Files(paths) => self.request(paths, Params::to(destination), window, cx),
        }
    }

    /// `chooseAndImportFiles`: the system's file chooser, then the files as
    /// a drop of them would import.
    pub fn choose_files(
        &mut self,
        destination: Destination,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.allow(cx) {
            return;
        }
        // TODO: limit the chooser to the media types (`MEDIA_IMPORT_FILTERS`);
        // GPUI's path prompt takes no file types.
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Import files".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.request(paths, Params::to(destination), window, cx)
            });
        })
        .detach();
    }

    /// `chooseAndImportFolder`: the system's folder chooser, then the folder
    /// dialog.
    pub fn choose_folder(
        &mut self,
        destination: Destination,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.allow(cx) {
            return;
        }
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Import folder".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.open_folder_dialog(path, destination, window, cx)
            });
        })
        .detach();
    }

    /// `pasteImport`: files copied in Finder, or a copied picture written
    /// to a temporary file that the import deletes once it has it.
    pub fn paste(&mut self, destination: Destination, window: &mut Window, cx: &mut Context<Self>) {
        if !self.allow(cx) {
            return;
        }
        let pasted = clipboard_import(cx);
        let (paths, temporary) = match pasted {
            Some(Pasted::Paths(paths)) => (paths, false),
            Some(Pasted::Picture(image)) => match write_picture(&image) {
                Ok(path) => (vec![path], true),
                Err(error) => {
                    self.report("Could not paste import", &error.to_string(), cx);
                    return;
                }
            },
            None => {
                self.report(
                    "Could not paste import",
                    "The clipboard does not contain importable files or an image.",
                    cx,
                );
                return;
            }
        };
        let params = Params {
            delete_after_ingest: temporary,
            ..Params::to(destination)
        };
        if paths.len() > 1 {
            self.request(paths, params, window, cx);
        } else {
            self.enqueue(paths, params, cx);
        }
    }

    /// `allowTaskAction('import')`: while an import is open another cannot
    /// start; the one in the way comes forward, and the reason is given.
    fn allow(&mut self, cx: &mut Context<Self>) -> bool {
        let Some((blocking, _)) = task::gate(&self.tasks, IMPORT) else {
            return true;
        };
        let (id, title) = (blocking.task_id, blocking.title.clone());
        self.show(id, cx);
        self.report(
            "An import is already running",
            &format!("Let “{title}” finish or stop it before starting another."),
            cx,
        );
        false
    }

    /// `requestMediaImport`: one batch, through the multi-file setting.
    fn request(
        &mut self,
        paths: Vec<PathBuf>,
        params: Params,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.allow(cx) {
            return;
        }
        let behavior = crate::settings::current(cx).multi_file_import_behavior;
        match plan::batch(paths.len(), params.clone(), behavior) {
            Batch::Now(params) => self.enqueue(paths, params, cx),
            Batch::Ask => {
                self.dialog.open(
                    Dialog::Collection { paths, params },
                    ui::menu::Return::To(self.back.clone()),
                    cx,
                );
            }
        }
    }

    /// `addMedia`: starts the import; its task takes the foreground.
    fn enqueue(&mut self, paths: Vec<PathBuf>, params: Params, cx: &mut Context<Self>) {
        self.enqueue_as("Could not import media", paths, params, cx);
    }

    /// [`Imports::enqueue`], a failure titled `failure` ("Could not import
    /// folder" for the folder dialog).
    fn enqueue_as(
        &mut self,
        failure: &'static str,
        paths: Vec<PathBuf>,
        params: Params,
        cx: &mut Context<Self>,
    ) {
        let args = params.enqueue(&paths);
        let answer = self.session.read(cx).dispatch("imports.enqueue", args);
        let folder = params.destination.folder;
        cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(Ok(started)) => {
                    if let Some(id) = started["task_id"].as_i64() {
                        this.show(id, cx);
                    }
                    if let Some(folder) = folder {
                        this.session
                            .update(cx, |session, _| session.record_folder_use(folder));
                    }
                    this.refresh(cx);
                }
                Ok(Err(error)) => this.report(failure, &error, cx),
                Err(_) => {}
            });
        })
        .detach();
    }

    /// Errors production shows as a notification (`showErrorNotification`).
    fn report(&self, title: &str, message: &str, cx: &mut App) {
        crate::ui::notify::error(title.to_owned(), message.to_owned(), cx);
    }

    // ── The folder dialog ───────────────────────────────────────

    fn open_folder_dialog(
        &mut self,
        path: PathBuf,
        destination: Destination,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dialog.open(
            Dialog::Folder(FolderDialog {
                path,
                destination,
                preserve_structure: true,
                include_subfolders: true,
                include_folders_without_media: false,
                watch: false,
                analysis: Analysis::None,
                generation: 0,
            }),
            ui::menu::Return::To(self.back.clone()),
            cx,
        );
        self.analyze(cx);
    }

    /// `analyzeFolderTree`, again whenever structure or subfolders change.
    fn analyze(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::Folder(folder)) = self.dialog.content_mut() else {
            return;
        };
        folder.generation += 1;
        if !folder.preserve_structure {
            folder.analysis = Analysis::None;
            cx.notify();
            return;
        }
        folder.analysis = Analysis::Checking;
        let generation = folder.generation;
        let args = serde_json::json!({
            "path": folder.path.to_string_lossy(),
            "destination_folder_id": folder.destination.folder.map(|folder| folder.0),
            "include_subfolders": folder.include_subfolders,
            "include_source_root": true,
        });
        let answer = self
            .session
            .read(cx)
            .dispatch("imports.folder.analyze", args);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(Dialog::Folder(folder)) = this.dialog.content_mut()
                    && folder.generation == generation
                {
                    folder.analysis = match result {
                        Ok(Ok(analysis)) => Analysis::Done(analysis),
                        _ => Analysis::Failed,
                    };
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// A switch in the folder dialog. Watching needs the structure kept.
    fn toggle_folder_option(&mut self, option: FolderOption, cx: &mut Context<Self>) {
        let Some(Dialog::Folder(folder)) = self.dialog.content_mut() else {
            return;
        };
        let reanalyze = match option {
            FolderOption::Preserve => {
                folder.preserve_structure = !folder.preserve_structure;
                true
            }
            FolderOption::Subfolders => {
                folder.include_subfolders = !folder.include_subfolders;
                true
            }
            FolderOption::WithoutMedia => {
                folder.include_folders_without_media = !folder.include_folders_without_media;
                false
            }
            FolderOption::Watch => {
                folder.watch = !folder.watch;
                if folder.watch && !folder.preserve_structure {
                    folder.preserve_structure = true;
                    true
                } else {
                    false
                }
            }
        };
        if reanalyze {
            self.analyze(cx);
        }
        cx.notify();
    }

    /// The folder dialog's Import.
    fn import_folder(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::Folder(folder)) = self.dialog.content() else {
            return;
        };
        let params = Params {
            preserve_structure: folder.preserve_structure,
            include_subfolders: Some(folder.include_subfolders),
            include_folders_without_media: folder.include_folders_without_media,
            watch_source_folder: folder.watch,
            ..Params::to(folder.destination)
        };
        let path = folder.path.clone();
        self.dialog.dismiss(cx);
        if self.allow(cx) {
            self.enqueue_as("Could not import folder", vec![path], params, cx);
        }
    }

    /// "Import as Collection?": Yes groups the files, No imports them apart.
    fn answer_collection(&mut self, group: bool, cx: &mut Context<Self>) {
        let Some(Dialog::Collection { paths, params }) = self.dialog.content() else {
            return;
        };
        let (paths, params) = (
            paths.clone(),
            Params {
                group_files: Some(group),
                ..params.clone()
            },
        );
        self.dialog.dismiss(cx);
        if self.allow(cx) {
            self.enqueue(paths, params, cx);
        }
    }

    // ── Tasks ───────────────────────────────────────────────────

    /// `showTask`
    fn show(&mut self, task: i64, cx: &mut Context<Self>) {
        self.presentation.show(task);
        cx.notify();
    }

    /// Reads the list once a burst of changes has passed.
    fn refresh_soon(&mut self, cx: &mut Context<Self>) {
        if self.coalesce.is_some() {
            return;
        }
        self.coalesce = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(REFRESH_COALESCE).await;
            let _ = this.update(cx, |this, cx| {
                this.coalesce = None;
                this.refresh(cx);
            });
        }));
    }

    /// `tasks.list`, and every second while a task works.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        if let Some(again) = &mut self.reading {
            *again = true;
            return;
        }
        self.reading = Some(false);
        let answer = self
            .session
            .read(cx)
            .dispatch("tasks.list", serde_json::json!({}));
        cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| {
                let again = this.reading.take() == Some(true);
                match result {
                    Ok(Ok(list)) => this.apply(list, cx),
                    Ok(Err(error)) => eprintln!("picto: task refresh failed: {error}"),
                    Err(_) => {}
                }
                if again {
                    this.refresh(cx);
                }
            });
        })
        .detach();
    }

    /// `applyTasks`: the list from a read or a control's answer.
    fn apply(&mut self, list: serde_json::Value, cx: &mut Context<Self>) {
        let Some(tasks) = Task::read_list(&list) else {
            eprintln!("picto: unreadable task list: {list}");
            return;
        };
        self.presentation.apply(&tasks);
        if tasks != self.tasks {
            self.tasks = tasks;
            cx.notify();
        }
        // Production's dialog drops the Stop question once the task is over.
        if let Some(task) = self.focused()
            && !task.open()
        {
            self.view.stopping = None;
        }
        self.poll = if self.tasks.iter().any(Task::working) {
            Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(WORKING_POLL).await;
                let _ = this.update(cx, |this, cx| this.refresh(cx));
            }))
        } else {
            None
        };
    }

    fn focused(&self) -> Option<&Task> {
        let id = self.presentation.focused?;
        self.tasks.iter().find(|task| task.task_id == id)
    }

    /// Sends a task control (`tasks.pause`, `tasks.resume`, `tasks.stop`,
    /// `tasks.dismiss`) and takes the list it answers with.
    fn control(
        &mut self,
        command: &'static str,
        args: serde_json::Value,
        failure: &'static str,
        cx: &mut Context<Self>,
    ) {
        let answer = self.session.read(cx).dispatch(command, args);
        cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(Ok(list)) => this.apply(list, cx),
                Ok(Err(error)) => this.report(failure, &error, cx),
                Err(_) => {}
            });
        })
        .detach();
    }

    fn pause(&mut self, task: i64, cx: &mut Context<Self>) {
        self.control(
            "tasks.pause",
            serde_json::json!({ "task_id": task }),
            "Could not pause the import",
            cx,
        );
    }

    fn resume(&mut self, task: i64, cx: &mut Context<Self>) {
        self.control(
            "tasks.resume",
            serde_json::json!({ "task_id": task }),
            "Could not resume the import",
            cx,
        );
    }

    fn dismiss(&mut self, task: i64, cx: &mut Context<Self>) {
        self.control(
            "tasks.dismiss",
            serde_json::json!({ "task_id": task }),
            "Could not close the task",
            cx,
        );
    }

    /// `minimizeTask`: the task goes on in the task bar.
    fn minimize(&mut self, task: i64, cx: &mut Context<Self>) {
        self.presentation.minimize(task);
        cx.notify();
    }

    /// Close or Stop… on an open task: asks what becomes of what it
    /// imported, pausing it meanwhile.
    fn ask_to_stop(&mut self, cx: &mut Context<Self>) {
        let Some(task) = self.focused() else {
            return;
        };
        if task.phase() == task::Phase::Removing {
            return;
        }
        let (id, running) = (task.task_id, task.state == "running");
        self.view.stopping = Some(running);
        self.view.choice = Some(StopChoice::Keep);
        if running {
            self.pause(id, cx);
        }
        cx.notify();
    }

    /// Keep importing: back to the progress, resumed if it was running.
    fn keep_importing(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.presentation.focused else {
            return;
        };
        if self.view.stopping.take() == Some(true) {
            self.resume(id, cx);
        }
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.presentation.focused else {
            return;
        };
        let undo = self.view.choice == Some(StopChoice::Remove);
        self.view.stopping = None;
        self.control(
            "tasks.stop",
            serde_json::json!({ "task_id": id, "undo": undo }),
            "Could not stop the import",
            cx,
        );
        cx.notify();
    }

    /// Show N failed / Hide failed, reading the failures once.
    fn toggle_failures(&mut self, cx: &mut Context<Self>) {
        self.view.show_failures = !self.view.show_failures;
        cx.notify();
        let Some(id) = self.presentation.focused else {
            return;
        };
        if !self.view.show_failures || self.view.failures.is_some() {
            return;
        }
        let answer = self
            .session
            .read(cx)
            .dispatch("tasks.failures", serde_json::json!({ "task_id": id }));
        cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| {
                if this.view.task == Some(id) {
                    this.view.failures = Some(match result {
                        Ok(Ok(list)) => Failure::read_list(&list),
                        _ => Vec::new(),
                    });
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

#[derive(Clone, Copy)]
enum FolderOption {
    Preserve,
    Subfolders,
    WithoutMedia,
    Watch,
}

/// What the clipboard offers to import.
enum Pasted {
    Paths(Vec<PathBuf>),
    Picture(Image),
}

/// `clipboardFilePaths` and `readImage`: files copied in Finder (or paths
/// copied as text that exist), else a copied picture.
fn clipboard_import(cx: &App) -> Option<Pasted> {
    let item = cx.read_from_clipboard()?;
    let mut paths = Vec::new();
    let mut picture = None;
    for entry in item.entries() {
        match entry {
            ClipboardEntry::ExternalPaths(external) => {
                paths.extend(external.paths().iter().cloned())
            }
            ClipboardEntry::String(text) => paths.extend(
                text.text()
                    .lines()
                    .filter_map(|line| path_from_text(line.trim())),
            ),
            ClipboardEntry::Image(image) => picture = picture.or(Some(image.clone())),
        }
    }
    let mut seen = std::collections::HashSet::new();
    paths.retain(|path| path.exists() && seen.insert(path.clone()));
    if !paths.is_empty() {
        return Some(Pasted::Paths(paths));
    }
    picture.map(Pasted::Picture)
}

/// An absolute path or a `file:` address on a line of copied text.
fn path_from_text(line: &str) -> Option<PathBuf> {
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let path = match line.strip_prefix("file://") {
        Some(rest) => {
            let decoded = percent_decode(rest);
            // `file:///C:/…`: on Windows the slash before the drive letter
            // is the address's, not the path's.
            let local = decoded
                .strip_prefix('/')
                .filter(|rest| cfg!(windows) && rest.as_bytes().get(1) == Some(&b':'))
                .unwrap_or(&decoded);
            PathBuf::from(local)
        }
        None => PathBuf::from(line),
    };
    path.is_absolute().then_some(path)
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(byte) = text
                .get(index + 1..index + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether the clipboard holds something Paste Import can take
/// (`hasClipboardImport`).
pub fn has_clipboard_import(cx: &App) -> bool {
    clipboard_import(cx).is_some()
}

/// Writes a copied picture where production writes it
/// (`<temp>/picto-clipboard-imports/`), in its own format.
fn write_picture(image: &Image) -> std::io::Result<PathBuf> {
    let directory = std::env::temp_dir().join("picto-clipboard-imports");
    std::fs::create_dir_all(&directory)?;
    let extension = match image.format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpg",
        ImageFormat::Webp => "webp",
        ImageFormat::Gif => "gif",
        ImageFormat::Svg => "svg",
        ImageFormat::Bmp => "bmp",
        ImageFormat::Tiff => "tiff",
        _ => "png",
    };
    let path = directory.join(format!("{}-{}.{extension}", crate::now_ms(), image.id));
    std::fs::write(&path, &image.bytes)?;
    Ok(path)
}

/// The folder an import of `path` is named after, as the dialog shows it.
fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "folder".into())
}

#[cfg(test)]
mod tests {
    use super::{folder_name, path_from_text};
    use std::path::{Path, PathBuf};

    #[test]
    fn copied_text_counts_only_absolute_paths_and_file_addresses() {
        // What an absolute path looks like differs by platform.
        let (plain, address) = if cfg!(windows) {
            (r"C:\a\b c.png", "file:///C:/a/b%20c.png")
        } else {
            ("/a/b c.png", "file:///a/b%20c.png")
        };
        assert_eq!(path_from_text(plain), Some(PathBuf::from(plain)));
        assert!(path_from_text(address).is_some_and(|path| path.is_absolute()));
        assert_eq!(path_from_text("relative.png"), None);
        assert_eq!(path_from_text("# comment"), None);
        assert_eq!(path_from_text(""), None);
    }

    #[test]
    fn a_folder_is_named_by_its_last_component() {
        assert_eq!(folder_name(Path::new("/x/Holiday/")), "Holiday");
        assert_eq!(folder_name(Path::new("/")), "folder");
    }
}
