//! Opens the system's dialogs for choosing a file, choosing a folder and
//! saving a file. Each function returns a receiver that gets the path the
//! user chose, or `None` when the dialog was cancelled or could not be
//! opened.
//!
//! The off-screen tool cannot operate a system dialog, so a test can answer
//! the dialogs itself. [`answer_next`] puts an answer in a queue. While the
//! queue holds an answer, the next dialog takes that answer and does not
//! open. Only the `answer` line of the off-screen tool (`src/offscreen.rs`)
//! puts answers in the queue, and only a test build has the queue.
use futures::channel::oneshot;
use gpui::{App, PathPromptOptions};
use std::path::{Path, PathBuf};

/// Holds the answers for the next dialogs, oldest first. `None` is the answer
/// of a cancelled dialog.
#[cfg(test)]
static ANSWERS: std::sync::Mutex<std::collections::VecDeque<Option<PathBuf>>> =
    std::sync::Mutex::new(std::collections::VecDeque::new());

/// Puts `path` in the queue as the answer of the next dialog, which then does
/// not open. With `None` that dialog answers as a cancelled dialog does. Only
/// the off-screen tool calls this.
#[cfg(test)]
pub fn answer_next(path: Option<PathBuf>) {
    if let Ok(mut answers) = ANSWERS.lock() {
        answers.push_back(path);
    }
}

#[cfg(test)]
fn queued() -> Option<Option<PathBuf>> {
    ANSWERS.lock().ok()?.pop_front()
}

#[cfg(not(test))]
fn queued() -> Option<Option<PathBuf>> {
    None
}

/// The receiver that a dialog function returns. It gets the path the user
/// chose, or `None` when the user chose nothing.
pub type Answer = oneshot::Receiver<Option<PathBuf>>;

fn answered(path: Option<PathBuf>) -> Answer {
    let (sender, receiver) = oneshot::channel();
    let _ = sender.send(path);
    receiver
}

/// Asks the user to choose one folder.
pub fn folder(prompt: &str, cx: &App) -> Answer {
    choose(false, true, prompt, cx)
}

/// Asks the user to choose one file.
pub fn file(prompt: &str, cx: &App) -> Answer {
    choose(true, false, prompt, cx)
}

/// Asks the user to choose one file or one folder. Open Library… uses this,
/// because on macOS a `.library` is a package, which the dialog offers as a
/// file, and elsewhere it is a folder.
pub fn file_or_folder(prompt: &str, cx: &App) -> Answer {
    choose(true, true, prompt, cx)
}

fn choose(files: bool, directories: bool, prompt: &str, cx: &App) -> Answer {
    if let Some(path) = queued() {
        return answered(path);
    }
    let chosen = cx.prompt_for_paths(PathPromptOptions {
        files,
        directories,
        multiple: false,
        prompt: Some(prompt.to_owned().into()),
    });
    let (sender, receiver) = oneshot::channel();
    cx.background_executor()
        .spawn(async move {
            let path = match chosen.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                _ => None,
            };
            let _ = sender.send(path);
        })
        .detach();
    receiver
}

/// Asks the user where to write a new file. The dialog opens in `directory`
/// with `suggested` as the file's name. When the name the user chose does not
/// end in `extension`, the extension is added to it.
pub fn save(directory: &Path, suggested: &str, extension: &str, cx: &App) -> Answer {
    let extension = extension.to_owned();
    if let Some(path) = queued() {
        return answered(path.map(|path| with_extension(path, &extension)));
    }
    let chosen = cx.prompt_for_new_path(directory, Some(suggested));
    let (sender, receiver) = oneshot::channel();
    cx.background_executor()
        .spawn(async move {
            let path = match chosen.await {
                Ok(Ok(Some(path))) => Some(with_extension(path, &extension)),
                _ => None,
            };
            let _ = sender.send(path);
        })
        .detach();
    receiver
}

/// Returns `path` unchanged when it ends in `.extension`, in upper or lower
/// case, and with `.extension` added to it otherwise.
fn with_extension(path: PathBuf, extension: &str) -> PathBuf {
    let has = path
        .extension()
        .is_some_and(|ending| ending.to_string_lossy().eq_ignore_ascii_case(extension));
    if has {
        return path;
    }
    let mut name = path.into_os_string();
    name.push(".");
    name.push(extension);
    PathBuf::from(name)
}

/// Returns the folder a save dialog opens in: the user's home folder, or the
/// working folder when the home folder is not known.
pub fn home() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::with_extension;
    use std::path::PathBuf;

    #[test]
    fn a_saved_name_keeps_or_gets_its_extension() {
        assert_eq!(
            with_extension(PathBuf::from("/x/Set.picto-pack"), "picto-pack"),
            PathBuf::from("/x/Set.picto-pack")
        );
        assert_eq!(
            with_extension(PathBuf::from("/x/Set.PICTO-PACK"), "picto-pack"),
            PathBuf::from("/x/Set.PICTO-PACK")
        );
        assert_eq!(
            with_extension(PathBuf::from("/x/Set"), "picto-pack"),
            PathBuf::from("/x/Set.picto-pack")
        );
        assert_eq!(
            with_extension(PathBuf::from("/x/Set.v2"), "picto-pack"),
            PathBuf::from("/x/Set.v2.picto-pack")
        );
    }
}
