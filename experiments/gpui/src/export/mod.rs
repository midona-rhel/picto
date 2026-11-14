//! Exporting, as `filesController.ts`, `ExportModal.tsx` and
//! `PictoPackModal.tsx` do it: an item, a selection or a whole destination
//! written out as its original files (`media.export` with the original
//! format, after a folder dialog), converted and resized through the Export
//! dialog, or packed with its metadata into a Picto Pack
//! (`picto_pack.export`, after a save dialog); and a Picto Pack read back in
//! (`picto_pack.inspect`, the dialog showing what it holds, then
//! `picto_pack.import`).
//!
//! One `Exports` view serves the window; the shell draws it over everything
//! and anything else reaches it through [`get`].
mod view;

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

/// What a Picto Pack is made from (`PictoPackSource`).
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

/// `ExportFormat`, with the label the dialog's select gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Original,
    Jpeg,
    Png,
    Webp,
    Avif,
}

impl Format {
    /// `FORMAT_OPTIONS`, in their order.
    pub const ALL: [Format; 5] = [
        Format::Original,
        Format::Jpeg,
        Format::Png,
        Format::Webp,
        Format::Avif,
    ];

    pub fn value(self) -> &'static str {
        match self {
            Format::Original => "original",
            Format::Jpeg => "jpeg",
            Format::Png => "png",
            Format::Webp => "webp",
            Format::Avif => "avif",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::Original => "Original Format",
            Format::Jpeg => "JPEG",
            Format::Png => "PNG",
            Format::Webp => "WebP",
            Format::Avif => "AVIF",
        }
    }

    fn from_value(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|format| format.value() == value)
    }

    /// The quality slider shows for the lossy formats only.
    pub fn has_quality(self) -> bool {
        !matches!(self, Format::Original | Format::Png)
    }

    /// Resizing needs the file to be converted.
    pub fn resizes(self) -> bool {
        self != Format::Original
    }
}

/// The arguments of `media.export`, as `exportMedia` sends them.
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
    /// Every file as it is (`chooseAndExportOriginals`).
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

/// How a finished export is reported: how many files were written, and
/// the first reason one was not.
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

/// "1 file", "3 files", with grouped digits.
fn files(count: u64) -> String {
    format!(
        "{} file{}",
        crate::grid::format::grouped(count),
        if count == 1 { "" } else { "s" }
    )
}

/// `safePackName`: a pack's file name from a destination's name, without
/// the characters file systems refuse or trailing dots.
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
        "Picto Pack".into()
    } else {
        cleaned.to_owned()
    }
}

/// What a Picto Pack holds (`PictoPackSummary`).
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

/// How a notification reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Success,
    Warning,
    Error,
}

/// Shows a notification (`showSuccessNotification`, `showWarningNotification`,
/// `showErrorNotification`).
pub(crate) fn notify(level: Level, title: &str, message: &str, cx: &mut App) {
    let (title, message) = (title.to_owned(), message.to_owned());
    match level {
        Level::Success => crate::ui::notify::success(title, message, cx),
        Level::Warning => crate::ui::notify::warning(title, message, cx),
        Level::Error => crate::ui::notify::error(title, message, cx),
    }
}

/// The dialogs export opens.
enum Dialog {
    /// `ExportModal`
    Convert(Convert),
    /// `PictoPackModal`
    Pack(Pack),
}

/// The Export dialog's choices.
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
    /// The pack is being written or read in; the dialog waits for it.
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
    /// Where the keyboard goes back to.
    back: FocusHandle,
    /// Packs the system asked the app to open (`pendingPictoPackPaths`),
    /// shown one at a time as each dialog closes.
    pending_packs: VecDeque<PathBuf>,
    /// A pack is being read for its dialog.
    pack_opening: bool,
}

struct ExportsGlobal(WeakEntity<Exports>);

impl Global for ExportsGlobal {}

/// The window's exports, for the views that start one.
pub fn get(cx: &App) -> Option<Entity<Exports>> {
    cx.try_global::<ExportsGlobal>()?.0.upgrade()
}

/// Keeps only the digits of what was typed (`replace(/\D/g, '')`).
fn digits(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).collect()
}

/// A dimension field's value, if it holds a whole number.
fn dimension(input: &Entity<TextInput>, cx: &App) -> Option<u32> {
    let text = input.read(cx).value();
    if text.is_empty() {
        return None;
    }
    // Production's `parseInt` of the digits; a number past what `u32`
    // holds is no size anyone exports at.
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

    // ── Export Originals ────────────────────────────────────────

    /// `chooseAndExportOriginals`: the folder dialog, then every file as
    /// it is.
    pub fn export_originals(&mut self, target: SelectionTarget, cx: &mut Context<Self>) {
        let chosen = crate::prompt::folder("Export originals", cx);
        cx.spawn(async move |this, cx| {
            let Ok(Some(folder)) = chosen.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.export(
                    Request::originals(target, folder),
                    "Could not export originals",
                    cx,
                )
            });
        })
        .detach();
    }

    /// `exportMedia`, and its outcome as a notification.
    fn export(&mut self, request: Request, failure: &'static str, cx: &mut Context<Self>) {
        let answer = self
            .session
            .read(cx)
            .dispatch("media.export", request.json());
        cx.spawn(async move |this, cx| {
            let result = answer.await;
            // One notification shows at a time, so files that were skipped
            // are told with the count rather than after it.
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

    // ── Export As ───────────────────────────────────────────────

    /// `ExportModal`, for `count` items, starting from the original format.
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

    /// Browse: the folder the files go to.
    fn browse(&mut self, cx: &mut Context<Self>) {
        let chosen = crate::prompt::folder("Choose export folder", cx);
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

    /// The Export dialog's Export: the dialog closes and the files are
    /// written.
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

    // ── Picto Packs ─────────────────────────────────────────────

    /// `requestPictoPackExport`: the dialog that says what a pack keeps,
    /// before the save dialog.
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
                    name: name.unwrap_or_else(|| "Picto Pack".into()),
                },
                busy: false,
            }),
            ui::menu::Return::To(self.back.clone()),
            cx,
        );
    }

    /// `chooseAndImportPictoPack`: the file dialog, then the pack's dialog.
    /// The caller has checked that an import may start.
    pub fn choose_pack(&mut self, cx: &mut Context<Self>) {
        // TODO: limit the dialog to `.picto-pack` files; GPUI's path
        // prompt takes no file types.
        let chosen = crate::prompt::file("Import Picto Pack", cx);
        cx.spawn(async move |this, cx| {
            let Ok(Some(path)) = chosen.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.open_pack(path, cx));
        })
        .detach();
    }

    /// `openPictoPackImport`: reads what the pack holds and shows it. The
    /// caller has checked that an import may start.
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
                Ok(Err(error)) => notify(Level::Error, "Could not import Picto Pack", &error, cx),
                Err(_) => {}
            });
        })
        .detach();
    }

    /// The pack dialog's primary button: Choose Destination… then the save
    /// dialog and the export, or Import.
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
                // `allowTaskAction('import')`, again now that it starts.
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
                                "Picto Pack imported",
                                &format!(
                                    "Imported {} items and {} media files.",
                                    result["imported_roots"].as_u64().unwrap_or(0),
                                    result["imported_media"].as_u64().unwrap_or(0)
                                ),
                                cx,
                            );
                            this.close(cx);
                        }
                        Ok(Err(error)) => {
                            notify(Level::Error, "Could not import Picto Pack", &error, cx);
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
                        "Picto Pack exported",
                        &format!(
                            "Exported {} items and {} media files.",
                            summary.root_count, summary.media_count
                        ),
                        cx,
                    );
                    this.close(cx);
                }
                Ok(Err(error)) => {
                    notify(Level::Error, "Could not export Picto Pack", &error, cx);
                    this.settle_pack(cx);
                }
                Err(_) => this.settle_pack(cx),
            });
        })
        .detach();
    }

    /// The pack dialog takes input again after a cancel or a failure.
    fn settle_pack(&mut self, cx: &mut Context<Self>) {
        if let Some(Dialog::Pack(pack)) = self.dialog.content_mut() {
            pack.busy = false;
            cx.notify();
        }
    }

    /// Cancel, Escape, the close button or the backdrop: a pack dialog at
    /// work cannot be closed.
    fn dismiss(&mut self, cx: &mut Context<Self>) {
        if let Some(Dialog::Pack(Pack { busy: true, .. })) = self.dialog.content() {
            return;
        }
        self.dialog.dismiss(cx);
        self.drain_packs(cx);
    }

    /// A dialog whose work is done, or Cancel: it goes at once, and the
    /// next pack the system asked to open comes up.
    fn close(&mut self, cx: &mut Context<Self>) {
        self.dialog.close(cx);
        self.drain_packs(cx);
    }

    /// `enqueuePictoPack`: a pack opened from the file manager or a second
    /// launch, shown once no dialog of this view is up.
    pub fn queue_pack(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !self.pending_packs.contains(&path) {
            self.pending_packs.push_back(path);
        }
        self.drain_packs(cx);
    }

    /// `startAssociatedFilesRuntime`'s drain: the next queued pack is opened
    /// while no dialog is up (production waits for the pack dialog only;
    /// here the Export dialog is waited for too, since one dialog of this
    /// view shows at a time and opening the pack would close it). A pack
    /// that cannot be opened is reported and the next one comes up.
    fn drain_packs(&mut self, cx: &mut Context<Self>) {
        let showing = self
            .dialog
            .get_mut()
            .is_some_and(|dialog| !dialog.presence.closing());
        if self.pack_opening || showing {
            return;
        }
        // `openPictoPackImport`'s `allowTaskAction('import')`: a pack
        // refused while an import runs is let go, as production's claim is.
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
    use super::{Format, PackSource, Request, digits, export_outcome, safe_pack_name};
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
    fn dimension_fields_keep_only_digits() {
        assert_eq!(digits("12a3 px"), "123");
        assert_eq!(digits("-4.5"), "45");
        assert_eq!(digits(""), "");
    }

    #[test]
    fn the_dialog_shows_quality_and_resizing_only_where_they_apply() {
        assert!(!Format::Original.has_quality() && !Format::Original.resizes());
        assert!(!Format::Png.has_quality() && Format::Png.resizes());
        for format in [Format::Jpeg, Format::Webp, Format::Avif] {
            assert!(format.has_quality() && format.resizes());
        }
        assert_eq!(
            Format::ALL.map(Format::label),
            ["Original Format", "JPEG", "PNG", "WebP", "AVIF"]
        );
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
