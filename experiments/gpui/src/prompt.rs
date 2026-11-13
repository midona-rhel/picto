//! The system's file, folder and save dialogs, as the Electron main process
//! opens them for the renderer (`picto:dialog:open`, `picto:dialog:save`).
//! Each answers with the path chosen, or `None` when the dialog was
//! cancelled or could not open.
//!
//! The off-screen tool cannot drive a system dialog, so a test can answer
//! the dialogs itself: [`answer_next`] queues answers, and each dialog asked
//! for while one is queued takes it instead of opening. Nothing queues an
//! answer outside the off-screen tool's `answer` line (`src/offscreen.rs`).
use futures::channel::oneshot;
use gpui::{App, PathPromptOptions};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Answers waiting for the next dialogs, oldest first. `None` cancels one.
static ANSWERS: Mutex<VecDeque<Option<PathBuf>>> = Mutex::new(VecDeque::new());

/// Has the next dialog answer `path` (or be cancelled, for `None`) without
/// opening. For the off-screen tool only.
#[cfg(test)]
pub fn answer_next(path: Option<PathBuf>) {
    if let Ok(mut answers) = ANSWERS.lock() {
        answers.push_back(path);
    }
}

fn queued() -> Option<Option<PathBuf>> {
    ANSWERS.lock().ok()?.pop_front()
}

/// What a dialog hands back: the path, or nothing.
pub type Answer = oneshot::Receiver<Option<PathBuf>>;

fn answered(path: Option<PathBuf>) -> Answer {
    let (sender, receiver) = oneshot::channel();
    let _ = sender.send(path);
    receiver
}

/// `properties: ['openDirectory']`: one folder.
pub fn folder(prompt: &str, cx: &App) -> Answer {
    choose(false, true, prompt, cx)
}

/// `properties: ['openFile']`: one file.
pub fn file(prompt: &str, cx: &App) -> Answer {
    choose(true, false, prompt, cx)
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

/// `dialog.showSaveDialog`: where to write a new file, offered as
/// `suggested` in `directory`. A name chosen without `extension` gets it, as
/// the save dialog's file type filter adds it in production.
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

/// `path`, ending in `.extension` (whatever its case) or given it.
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

/// Where a save dialog opens: the user's home folder, else the working one.
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
