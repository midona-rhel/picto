//! Exports items and reads Picto Packs. An item, a selection or a whole
//! destination can be written out in three ways: as its original files
//! (the command `media.export` with the original format, after a folder
//! dialog), converted and resized through the Export dialog, or packed
//! with its metadata into a Picto Pack (the command `picto_pack.export`,
//! after a save dialog). A Picto Pack is read back in with
//! `picto_pack.inspect`, then a dialog that shows what the pack holds, and
//! then `picto_pack.import`.
//!
//! The window has one `Exports` view. The shell draws it over everything
//! else, and other code reaches it through [`get`].
mod view;

use crate::i18n::t;
use crate::session::Session;
use crate::ui::{
    self,
    text_input::{Kind, TextInput},
};
use gpui::*;
use picto_library::selection::SelectionTarget;
use picto_library::{FolderId, SmartFolderId};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::path::PathBuf;

/// The items that a Picto Pack is made from.
#[derive(Clone, Debug, PartialEq)]
pub enum PackSource {
    Items(SelectionTarget),
    Folder(FolderId),
    SmartFolder(SmartFolderId),
}

impl PackSource {
    fn json(&self) -> Value {
        match self {
            PackSource::Items(target) => json!({ "kind": "items", "target": target }),
            PackSource::Folder(id) => json!({ "kind": "folder", "folder_id": id }),
            PackSource::SmartFolder(id) => {
                json!({ "kind": "smart_folder", "smart_folder_id": id })
            }
        }
    }
}

/// A file format that the Export dialog offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Original,
    Jpeg,
    Png,
    Webp,
    Avif,
    /// The original files in one zip archive.
    Zip,
}

impl Format {
    /// Every format, in the order in which the dialog's select lists them.
    pub const ALL: [Format; 6] = [
        Format::Original,
        Format::Jpeg,
        Format::Png,
        Format::Webp,
        Format::Avif,
        Format::Zip,
    ];

    pub fn value(self) -> &'static str {
        match self {
            Format::Original => "original",
            Format::Jpeg => "jpeg",
            Format::Png => "png",
            Format::Webp => "webp",
            Format::Avif => "avif",
            Format::Zip => "zip",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::Original => t("Original Format"),
            Format::Jpeg => t("JPEG"),
            Format::Png => t("PNG"),
            Format::Webp => t("WebP"),
            Format::Avif => t("AVIF"),
            Format::Zip => crate::i18n::en("ZIP Archive"),
        }
    }

    fn from_value(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|format| format.value() == value)
    }

    /// Returns whether the dialog shows the quality slider, which it does
    /// for the lossy formats only.
    pub fn has_quality(self) -> bool {
        !matches!(self, Format::Original | Format::Png | Format::Zip)
    }

    /// Returns whether the dialog offers resizing. A file can be resized
    /// only when it is converted to another format.
    pub fn resizes(self) -> bool {
        !matches!(self, Format::Original | Format::Zip)
    }
}

/// The arguments of the command `media.export`.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub target: SelectionTarget,
    pub output_dir: PathBuf,
    pub format: Format,
    pub quality: u8,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub keep_aspect: bool,
}

impl Request {
    /// Returns the request that writes every file of `target` as it is, in
    /// its original format.
    pub fn originals(target: SelectionTarget, output_dir: PathBuf) -> Self {
        Self {
            target,
            output_dir,
            format: Format::Original,
            quality: 90,
            width: None,
            height: None,
            keep_aspect: true,
        }
    }

    fn json(&self) -> Value {
        json!({
            "target": self.target,
            "output_dir": self.output_dir.to_string_lossy(),
            "format": self.format.value(),
            "quality": self.quality,
            "width": self.width,
            "height": self.height,
            "keep_aspect": self.keep_aspect,
        })
    }
}

/// Returns how a finished export is reported. The first string is the
/// notification's title, which says how many files were written. The second
/// is present when files were skipped: it gives their number and the first
/// error.
fn export_outcome(result: &Value) -> (String, Option<String>) {
    let count = |key: &str| result[key].as_u64().unwrap_or(0);
    let exported = count("exported");
    let skipped = count("skipped");
    let title = format!("Exported {}", files(exported));
    let problem = (skipped > 0).then(|| {
        let first = result["errors"][0].as_str().unwrap_or_default();
        format!("{} could not be exported. {first}", files(skipped))
            .trim_end()
            .to_owned()
    });
    (title, problem)
}

/// Returns the count with the word "file" or "files", for example "1 file"
/// or "1,200 files". The report of an export is in English in every
/// language.
fn files(count: u64) -> String {
    format!(
        "{} file{}",
        crate::i18n::format::grouped(count),
        // i18n: the report of an export is in English in every language
        if count == 1 { "" } else { "s" }
    )
}

/// Returns the file name for a pack that is named after a destination.
/// Each run of characters that file systems refuse becomes one hyphen, and
/// dots at the end are removed. The name is "Picto Pack" when nothing is
/// left.
pub fn safe_pack_name(value: &str) -> String {
    let mut cleaned = String::new();
    let mut in_run = false;
    for character in value.trim().chars() {
        if matches!(
            character,
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
        ) {
            if !in_run {
                cleaned.push('-');
            }
            in_run = true;
        } else {
            cleaned.push(character);
            in_run = false;
        }
    }
    let cleaned = cleaned.trim_end_matches('.');
    if cleaned.is_empty() {
        // i18n: a file's name, which is English in every language
        "Picto Pack".into()
    } else {
        cleaned.to_owned()
    }
}

/// What a Picto Pack holds, as the commands `picto_pack.inspect` and
/// `picto_pack.export` report it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub name: String,
    pub root_count: u64,
    pub media_count: u64,
    pub folder_count: u64,
    pub smart_folder_count: u64,
    pub total_bytes: u64,
}

impl Summary {
    fn read(value: &Value) -> Self {
        let count = |key: &str| value[key].as_u64().unwrap_or(0);
        Self {
            name: value["name"].as_str().unwrap_or_default().to_owned(),
            root_count: count("root_count"),
            media_count: count("media_count"),
            folder_count: count("folder_count"),
            smart_folder_count: count("smart_folder_count"),
            total_bytes: count("total_bytes"),
        }
    }
}

/// Whether a notification reports a success, a warning or an error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Success,
    Warning,
    Error,
}

/// Shows a notification of the given level.
pub(crate) fn notify(level: Level, title: &str, message: &str, cx: &mut App) {
    let (title, message) = (title.to_owned(), message.to_owned());
    match level {
        Level::Success => crate::ui::notify::success(title, message, cx),
        Level::Warning => crate::ui::notify::warning(title, message, cx),
        Level::Error => crate::ui::notify::error(title, message, cx),
    }
}

/// A dialog that this view opens.
enum Dialog {
    /// The Export dialog, which converts and resizes files.
    Convert(Convert),
    /// The Picto Pack dialog, for a pack that is written or read in.
    Pack(Pack),
}

/// The choices that the user has made in the Export dialog.
struct Convert {
    target: SelectionTarget,
    count: u64,
    output_dir: Option<PathBuf>,
    format: Format,
    quality: u8,
    width: Entity<TextInput>,
    height: Entity<TextInput>,
    keep_aspect: bool,
    _typed: [Subscription; 2],
}

struct Pack {
    mode: PackMode,
    /// True while the pack is being written or read in. The dialog cannot be
    /// closed during that time.
    busy: bool,
}

enum PackMode {
    Export {
        source: PackSource,
        count: u64,
        name: String,
    },
    Import {
        path: PathBuf,
        summary: Summary,
    },
}

pub struct Exports {
    session: Entity<Session>,
    dialog: ui::modal::Host<Dialog>,
    /// The element that gets the keyboard focus when a dialog closes.
    back: FocusHandle,
    /// The packs that the system asked the app to open. They are shown one
    /// at a time, each after the dialog before it has closed.
    pending_packs: VecDeque<PathBuf>,
    /// True while a pack is being read so that its dialog can be shown.
    pack_opening: bool,
}

struct ExportsGlobal(WeakEntity<Exports>);

impl Global for ExportsGlobal {}

/// Returns the window's `Exports` view, for the views that start an export.
pub fn get(cx: &App) -> Option<Entity<Exports>> {
    cx.try_global::<ExportsGlobal>()?.0.upgrade()
}

/// Returns `text` with every character that is not a digit removed.
fn digits(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).collect()
}

/// Returns the number in a width or height field, or `None` when the field
/// is empty or the number does not fit in a `u32`.
fn dimension(input: &Entity<TextInput>, cx: &App) -> Option<u32> {
    let text = input.read(cx).value();
    if text.is_empty() {
        return None;
    }
    // The field holds digits only, so parsing fails only for a number that
    // is too large for `u32`. Such a number is treated as no size.
    text.parse::<u32>().ok()
}

impl Exports {
    pub fn new(session: Entity<Session>, back: FocusHandle, cx: &mut Context<Self>) -> Self {
        cx.set_global(ExportsGlobal(cx.entity().downgrade()));
        Self {
            session,
            dialog: ui::modal::Host::default(),
            back,
            pending_packs: VecDeque::new(),
            pack_opening: false,
        }
    }

    // The functions below export files in their original format.

    /// Asks for a folder and then writes every file of `target` into it as
    /// it is.
    pub fn export_originals(&mut self, target: SelectionTarget, cx: &mut Context<Self>) {
        let chosen = crate::prompt::folder(t("Export originals"), cx);
        cx.spawn(async move |this, cx| {
            let Ok(Some(folder)) = chosen.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.export(
                    Request::originals(target, folder),
                    t("Could not export originals"),
                    cx,
                )
            });
        })
        .detach();
    }

    /// Sends the export to the application and shows its outcome as a
    /// notification. `failure` is the notification's title when the export
    /// fails.
    fn export(&mut self, request: Request, failure: &'static str, cx: &mut Context<Self>) {
        let answer = self
            .session
            .read(cx)
            .dispatch("media.export", request.json());
        cx.spawn(async move |this, cx| {
            let result = answer.await;
            // Only one notification shows at a time, so the skipped files are
            // reported in the same notification as the number of written files.
            let _ = this.update(cx, |_, cx| match result {
                Ok(Ok(result)) => match export_outcome(&result) {
                    (title, Some(problem)) => notify(Level::Warning, &title, &problem, cx),
                    (title, None) => notify(Level::Success, &title, "", cx),
                },
                Ok(Err(error)) => notify(Level::Error, failure, &error, cx),
                Err(_) => {}
            });
        })
        .detach();
    }

    // The functions below serve the Export dialog.

    /// Opens the Export dialog for `count` items. The original format is
    /// selected at first.
    pub fn export_as(&mut self, target: SelectionTarget, count: u64, cx: &mut Context<Self>) {
        let width = cx.new(|cx| {
            TextInput::new(Kind::Form, cx)
                .placeholder("Width")
                .submits()
        });
        let height = cx.new(|cx| {
            TextInput::new(Kind::Form, cx)
                .placeholder("Height")
                .submits()
        });
        let typed = |input: &Entity<TextInput>, cx: &mut Context<Self>| {
            cx.observe(input, |_, input, cx| {
                let text = input.read(cx).value();
                let kept = digits(&text);
                if kept != text {
                    input.update(cx, |input, cx| {
                        input.set_text(kept, cx);
                        input.caret_at_end(cx);
                    });
                }
            })
        };
        let subscriptions = [typed(&width, cx), typed(&height, cx)];
        self.dialog.open(
            Dialog::Convert(Convert {
                target,
                count,
                output_dir: None,
                format: Format::Original,
                quality: 90,
                width,
                height,
                keep_aspect: true,
                _typed: subscriptions,
            }),
            ui::menu::Return::To(self.back.clone()),
            cx,
        );
    }

    /// Asks for the folder that the files go to, for the dialog's Browse
    /// button.
    fn browse(&mut self, cx: &mut Context<Self>) {
        let chosen = crate::prompt::folder(t("Choose export folder"), cx);
        cx.spawn(async move |this, cx| {
            let Ok(Some(folder)) = chosen.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if let Some(Dialog::Convert(convert)) = this.dialog.content_mut() {
                    convert.output_dir = Some(folder);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn set_format(&mut self, value: &str, cx: &mut Context<Self>) {
        if let (Some(Dialog::Convert(convert)), Some(format)) =
            (self.dialog.content_mut(), Format::from_value(value))
        {
            convert.format = format;
            cx.notify();
        }
    }

    fn set_quality(&mut self, quality: f64, cx: &mut Context<Self>) {
        if let Some(Dialog::Convert(convert)) = self.dialog.content_mut() {
            convert.quality = quality.round().clamp(1., 100.) as u8;
            cx.notify();
        }
    }

    fn toggle_aspect(&mut self, cx: &mut Context<Self>) {
        if let Some(Dialog::Convert(convert)) = self.dialog.content_mut() {
            convert.keep_aspect = !convert.keep_aspect;
            cx.notify();
        }
    }

    /// Closes the Export dialog and writes the files, for the dialog's
    /// Export button.
    fn convert(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::Convert(convert)) = self.dialog.content() else {
            return;
        };
        let Some(output_dir) = convert.output_dir.clone() else {
            return;
        };
        let request = Request {
            target: convert.target.clone(),
            output_dir,
            format: convert.format,
            quality: convert.quality,
            width: dimension(&convert.width, cx),
            height: dimension(&convert.height, cx),
            keep_aspect: convert.keep_aspect,
        };
        self.dialog.dismiss(cx);
        self.drain_packs(cx);
        self.export(request, "Could not export files", cx);
    }

    // The functions below write and read Picto Packs.

    /// Opens the dialog that says what a pack keeps. The save dialog follows
    /// when the user confirms.
    pub fn export_pack(
        &mut self,
        source: PackSource,
        count: u64,
        name: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.dialog.open(
            Dialog::Pack(Pack {
                mode: PackMode::Export {
                    source,
                    count,
                    // i18n: a file's name, which is English in every language
                    name: name.unwrap_or_else(|| "Picto Pack".into()),
                },
                busy: false,
            }),
            ui::menu::Return::To(self.back.clone()),
            cx,
        );
    }

    /// Asks for a pack file and then opens the pack's dialog. The caller has
    /// checked that an import may start.
    pub fn choose_pack(&mut self, cx: &mut Context<Self>) {
        // TODO: limit the dialog to `.picto-pack` files. GPUI's path prompt
        // takes no file types.
        let chosen = crate::prompt::file(t("Import Picto Pack"), cx);
        cx.spawn(async move |this, cx| {
            let Ok(Some(path)) = chosen.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.open_pack(path, cx));
        })
        .detach();
    }

    /// Reads what the pack at `path` holds and shows it in the pack dialog.
    /// The caller has checked that an import may start.
    pub fn open_pack(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let answer = self.session.read(cx).dispatch(
            "picto_pack.inspect",
            json!({ "path": path.to_string_lossy() }),
        );
        self.pack_opening = true;
        cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| {
                this.pack_opening = false;
                if !matches!(result, Ok(Ok(_))) {
                    this.drain_packs(cx);
                }
            });
            let _ = this.update(cx, |this, cx| match result {
                Ok(Ok(summary)) => {
                    let back = ui::menu::Return::To(this.back.clone());
                    this.dialog.open(
                        Dialog::Pack(Pack {
                            mode: PackMode::Import {
                                path,
                                summary: Summary::read(&summary),
                            },
                            busy: false,
                        }),
                        back,
                        cx,
                    );
                }
                Ok(Err(error)) => {
                    notify(Level::Error, t("Could not import Picto Pack"), &error, cx)
                }
                Err(_) => {}
            });
        })
        .detach();
    }

    /// Runs the pack dialog's primary button. For an export the button is
    /// "Choose Destination...": it opens the save dialog and then writes the
    /// pack. For an import the button is "Import": it reads the pack into the
    /// library.
    fn run_pack(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::Pack(pack)) = self.dialog.content_mut() else {
            return;
        };
        if pack.busy {
            return;
        }
        match &pack.mode {
            PackMode::Export { source, name, .. } => {
                let source = source.clone();
                pack.busy = true;
                let chosen = crate::prompt::save(
                    &crate::prompt::home(),
                    &format!("{}.picto-pack", safe_pack_name(name)),
                    "picto-pack",
                    cx,
                );
                cx.notify();
                cx.spawn(async move |this, cx| {
                    let path = chosen.await.ok().flatten();
                    let _ = this.update(cx, |this, cx| match path {
                        Some(path) => this.write_pack(source, path, cx),
                        None => this.settle_pack(cx),
                    });
                })
                .detach();
            }
            PackMode::Import { path, .. } => {
                let path = path.clone();
                // Check again that an import may start, because another import
                // may have started while the dialog was open.
                let allowed = crate::imports::get(cx)
                    .is_none_or(|imports| imports.update(cx, |imports, cx| imports.allow(cx)));
                if !allowed {
                    return;
                }
                if let Some(Dialog::Pack(pack)) = self.dialog.content_mut() {
                    pack.busy = true;
                }
                cx.notify();
                let answer = self.session.read(cx).dispatch(
                    "picto_pack.import",
                    json!({ "path": path.to_string_lossy() }),
                );
                cx.spawn(async move |this, cx| {
                    let result = answer.await;
                    let _ = this.update(cx, |this, cx| match result {
                        Ok(Ok(result)) => {
                            notify(
                                Level::Success,
                                t("Picto Pack imported"),
                                &crate::tf!(
                                    "Imported {value0} items and {value1} media files.",
                                    result["imported_roots"].as_u64().unwrap_or(0),
                                    result["imported_media"].as_u64().unwrap_or(0)
                                ),
                                cx,
                            );
                            this.close(cx);
                        }
                        Ok(Err(error)) => {
                            notify(Level::Error, t("Could not import Picto Pack"), &error, cx);
                            this.settle_pack(cx);
                        }
                        Err(_) => this.settle_pack(cx),
                    });
                })
                .detach();
            }
        }
    }

    fn write_pack(&mut self, source: PackSource, path: PathBuf, cx: &mut Context<Self>) {
        let answer = self.session.read(cx).dispatch(
            "picto_pack.export",
            json!({ "source": source.json(), "output_path": path.to_string_lossy() }),
        );
        cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(Ok(result)) => {
                    let summary = Summary::read(&result["summary"]);
                    notify(
                        Level::Success,
                        t("Picto Pack exported"),
                        &crate::tf!(
                            "Exported {value0} items and {value1} media files.",
                            summary.root_count,
                            summary.media_count
                        ),
                        cx,
                    );
                    this.close(cx);
                }
                Ok(Err(error)) => {
                    notify(Level::Error, t("Could not export Picto Pack"), &error, cx);
                    this.settle_pack(cx);
                }
                Err(_) => this.settle_pack(cx),
            });
        })
        .detach();
    }

    /// Lets the pack dialog take input again after the save dialog was
    /// cancelled or the work failed.
    fn settle_pack(&mut self, cx: &mut Context<Self>) {
        if let Some(Dialog::Pack(pack)) = self.dialog.content_mut() {
            pack.busy = false;
            cx.notify();
        }
    }

    /// Starts the dialog's fade-out, for Escape, the close button and a click
    /// on the backdrop. A pack dialog that is writing or reading a pack stays
    /// open.
    fn dismiss(&mut self, cx: &mut Context<Self>) {
        if let Some(Dialog::Pack(Pack { busy: true, .. })) = self.dialog.content() {
            return;
        }
        self.dialog.dismiss(cx);
        self.drain_packs(cx);
    }

    /// Removes the dialog at once, for Cancel and for a dialog whose work is
    /// done. Then the next pack that the system asked to open is shown.
    fn close(&mut self, cx: &mut Context<Self>) {
        self.dialog.close(cx);
        self.drain_packs(cx);
    }

    /// Queues a pack that was opened from the file manager or passed by a
    /// second launch. Its dialog is shown once no dialog of this view is open.
    pub fn queue_pack(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !self.pending_packs.contains(&path) {
            self.pending_packs.push_back(path);
        }
        self.drain_packs(cx);
    }

    /// Opens the next queued pack when no dialog of this view is open and no
    /// pack is being read. Only one dialog of this view shows at a time, so
    /// opening a pack while the Export dialog is open would close that dialog.
    /// A pack that cannot be opened is reported, and the next one is opened.
    fn drain_packs(&mut self, cx: &mut Context<Self>) {
        let showing = self
            .dialog
            .get_mut()
            .is_some_and(|dialog| !dialog.presence.closing());
        if self.pack_opening || showing {
            return;
        }
        // A pack cannot be opened while an import runs. `allow` reports that
        // to the user, and the pack is removed from the queue.
        while let Some(path) = self.pending_packs.pop_front() {
            let allowed = crate::imports::get(cx)
                .is_none_or(|imports| imports.update(cx, |imports, cx| imports.allow(cx)));
            if allowed {
                self.open_pack(path, cx);
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PackSource, Request, export_outcome, safe_pack_name};
    use picto_library::selection::SelectionTarget;
    use picto_library::{FolderId, RootId, SmartFolderId};
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn pack_names_drop_what_file_systems_refuse() {
        assert_eq!(safe_pack_name("  Summer: 2024/25 "), "Summer- 2024-25");
        assert_eq!(safe_pack_name("a<>b"), "a-b");
        assert_eq!(safe_pack_name("Notes..."), "Notes");
        assert_eq!(safe_pack_name("..."), "Picto Pack");
        assert_eq!(safe_pack_name(""), "Picto Pack");
    }

    #[test]
    fn requests_and_sources_are_sent_as_the_application_reads_them() {
        let target = SelectionTarget::Explicit {
            root_ids: vec![RootId(4)],
        };
        let request = Request::originals(target.clone(), PathBuf::from("/out"));
        assert_eq!(
            request.json(),
            json!({
                "target": { "kind": "explicit", "root_ids": [4] },
                "output_dir": "/out",
                "format": "original",
                "quality": 90,
                "width": null,
                "height": null,
                "keep_aspect": true,
            })
        );
        assert_eq!(
            PackSource::Folder(FolderId(7)).json(),
            json!({ "kind": "folder", "folder_id": 7 })
        );
        assert_eq!(
            PackSource::SmartFolder(SmartFolderId(2)).json(),
            json!({ "kind": "smart_folder", "smart_folder_id": 2 })
        );
        assert_eq!(
            PackSource::Items(target).json(),
            json!({ "kind": "items", "target": { "kind": "explicit", "root_ids": [4] } })
        );
    }

    #[test]
    fn an_export_reports_what_it_wrote_and_what_it_could_not() {
        let (title, problem) =
            export_outcome(&json!({ "exported": 1, "skipped": 0, "errors": [] }));
        assert_eq!(title, "Exported 1 file");
        assert_eq!(problem, None);
        let (title, problem) = export_outcome(
            &json!({ "exported": 1200, "skipped": 2, "errors": ["Cannot convert non-image media ab"] }),
        );
        assert_eq!(title, "Exported 1,200 files");
        assert_eq!(
            problem.as_deref(),
            Some("2 files could not be exported. Cannot convert non-image media ab")
        );
    }
}
