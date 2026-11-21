//! The cover of the open library (`LibraryCoverDialogHost`). The user chooses
//! it from the library's own pictures in the cover dialog that a
//! subscription's cover also uses (`MediaCoverDialog`), and frames it there.
//! "Set as Library Cover" on an item opens the dialog with that item already
//! in the frame. Saving does what `setLibraryMeta` does to the library's
//! folder: the item's thumbnail is copied next to the database as
//! `.picto-library-cover.<ext>`, and the frame is written to
//! `.picto-library-cover.json`. The sidebar and the list of libraries then
//! show the new cover.
use crate::grid::thumbnails::{self, Thumbnails};
use crate::i18n::{en, t};
use crate::session::{Change, Session};
use crate::ui;
use crate::ui::cover_dialog::{Candidate, Cover, CoverHost, Target, add_page, cover_layer};
use gpui::*;
use picto_library::RootKind;
use picto_library::query::{ItemScope, PageRequest, RootQuery, RootSummary};
use std::path::{Path, PathBuf};

const INSTRUCTIONS: &str =
    en("Select a media item from this library, then adjust its position and zoom.");
const EMPTY: &str = en("This library has no media available for a cover.");
/// `PAGE_SIZE` in `libraryAppearance.ts`.
const PAGE: usize = 200;
/// The extensions the cover file can have. The file is next to the library's
/// database.
const COVER_EXTENSIONS: [&str; 2] = ["jpg", "png"];

/// An item of the library as a candidate for the dialog. `None` for an item
/// that is not a single media file or has no file.
pub fn candidate(item: &RootSummary, session: &Session) -> Option<Candidate> {
    (item.kind == RootKind::Media && !item.content_hash.is_empty()).then(|| Candidate {
        media_item: i64::from(item.root_id.0),
        hash: item.content_hash.clone(),
        size: (item.width.unwrap_or(1), item.height.unwrap_or(1)),
        thumbnail: thumbnails::present(&session.thumbnail(&item.content_hash)),
    })
}

pub struct LibraryCover {
    session: Entity<Session>,
    /// The cache that the candidates' thumbnails are decoded into. The grid's
    /// cache is not used for them.
    thumbnails: Entity<Thumbnails>,
    dialog: ui::modal::Host<Cover>,
    /// What had the keyboard focus before the dialog opened. The focus goes
    /// back there when the dialog closes.
    back: FocusHandle,
    saving: bool,
}

impl LibraryCover {
    pub fn new(session: Entity<Session>, back: FocusHandle, cx: &mut Context<Self>) -> Self {
        Self {
            session,
            thumbnails: cx.new(Thumbnails::new),
            dialog: ui::modal::Host::default(),
            back,
            saving: false,
        }
    }

    /// `openCurrentLibraryCoverPicker`: opens the dialog. When `initial` is
    /// given, that picture is in the frame at once, and the library's pictures
    /// load in the meantime.
    pub fn open(&mut self, initial: Option<Candidate>, cx: &mut Context<Self>) {
        if self.session.read(cx).library_root().is_none() {
            ui::notify::error(
                t("Could not set library cover"),
                en("No library is open."),
                cx,
            );
            return;
        }
        self.saving = false;
        self.dialog.open(
            Cover::new(Target::Library, initial, INSTRUCTIONS, EMPTY),
            ui::menu::Return::To(self.back.clone()),
            cx,
        );
        self.load(None, cx);
    }

    /// `loadLibraryCoverCandidates`: loads one page of the library's media
    /// (the All destination), most recently imported first.
    fn load(&mut self, cursor: Option<String>, cx: &mut Context<Self>) {
        let more = cursor.is_some();
        let answer = self.session.read(cx).run(move |library| {
            library.query(
                &RootQuery {
                    scope: ItemScope::All,
                    view: Default::default(),
                },
                &PageRequest {
                    limit: PAGE,
                    cursor,
                },
            )
        });
        let session = self.session.clone();
        cx.spawn(async move |this, cx| {
            let page = answer
                .await
                .map_err(|_| "The library has stopped".to_owned())
                .and_then(|page| page.map_err(|error| error.to_string()));
            let _ = this.update(cx, |this, cx| {
                let candidates = page.map(|page| {
                    let session = session.read(cx);
                    let candidates: Vec<Candidate> = page
                        .items
                        .iter()
                        .filter_map(|item| candidate(item, session))
                        .collect();
                    (candidates, page.next_cursor)
                });
                let Some(cover) = this.dialog.content_mut() else {
                    return;
                };
                let again = match candidates {
                    Ok((candidates, next)) => {
                        add_page(cover, candidates, next.map(serde_json::Value::String))
                    }
                    Err(error) => {
                        cover.loading = false;
                        cover.loading_more = false;
                        // The same sentence as the subscription's dialog,
                        // which the Electron app also uses here.
                        let title = if more {
                            t("Unable to load more subscription media")
                        } else {
                            cover.candidates = Default::default();
                            t("Unable to load subscription media")
                        };
                        ui::notify::error(title, error, cx);
                        None
                    }
                };
                if let Some(serde_json::Value::String(cursor)) = again {
                    this.load(Some(cursor), cx);
                }
                cx.notify();
            });
        })
        .detach();
    }
}

/// `setLibraryMeta` for a picture. Copies the thumbnail of `hash` next to the
/// database as the cover, removing a cover with the other extension, and
/// writes the frame next to it. Only the library that is open in the window is
/// written to (`libraries::writable`), and each file is written only inside
/// that library's folder (`libraries::confine`).
fn write_cover(
    library: &Path,
    blobs_thumbnail: &Path,
    hash: &str,
    crop: crate::cover::Crop,
) -> Result<(), String> {
    let source = thumbnails::present(blobs_thumbnail)
        .ok_or("The selected library cover thumbnail is not available")?;
    let extension = source
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("jpg")
        .to_owned();
    let stamp = format!(
        "{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_millis())
    );
    let replace = |destination: PathBuf, write: &dyn Fn(&Path) -> std::io::Result<()>| {
        let destination = crate::libraries::confine(library, &destination)?;
        let mut temporary = destination.clone().into_os_string();
        temporary.push(format!(".{stamp}.tmp"));
        let temporary = PathBuf::from(temporary);
        let written = write(&temporary).and_then(|_| std::fs::rename(&temporary, &destination));
        if written.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        written.map_err(|error| error.to_string())
    };
    replace(
        library.join(format!(".picto-library-cover.{extension}")),
        &|temporary| std::fs::copy(&source, temporary).map(|_| ()),
    )?;
    for other in COVER_EXTENSIONS.iter().filter(|other| **other != extension) {
        if let Ok(other) = crate::libraries::confine(
            library,
            &library.join(format!(".picto-library-cover.{other}")),
        ) {
            let _ = std::fs::remove_file(other);
        }
    }
    let frame = cover_frame(hash, crop);
    replace(library.join(".picto-library-cover.json"), &|temporary| {
        std::fs::write(temporary, &frame)
    })
}

/// `writeMaterializedCoverMetadata`: the frame in the form the Electron app
/// writes it.
fn cover_frame(hash: &str, crop: crate::cover::Crop) -> String {
    let number = |value: f32| {
        if value.fract() == 0. {
            serde_json::json!(value as i64)
        } else {
            serde_json::json!(value)
        }
    };
    serde_json::to_string_pretty(&serde_json::json!({
        "imageHash": hash,
        "imageFocusX": number(crop.focus_x),
        "imageFocusY": number(crop.focus_y),
        "imageZoomPercent": number(crop.zoom),
    }))
    .unwrap_or_default()
}

impl CoverHost for LibraryCover {
    fn cover(&mut self) -> Option<&mut Cover> {
        self.dialog.content_mut()
    }

    fn cover_busy(&self) -> bool {
        self.saving
    }

    fn cover_thumbnails(&self) -> Entity<Thumbnails> {
        self.thumbnails.clone()
    }

    fn cover_blobs(&self, cx: &App) -> std::path::PathBuf {
        self.session.read(cx).blobs().to_path_buf()
    }

    fn load_more_candidates(&mut self, cx: &mut Context<Self>) {
        let Some(cover) = self.dialog.content_mut() else {
            return;
        };
        if cover.loading || cover.loading_more {
            return;
        }
        let Some(serde_json::Value::String(cursor)) = cover.next.clone() else {
            return;
        };
        cover.loading_more = true;
        self.load(Some(cursor), cx);
        cx.notify();
    }

    fn save_cover(&mut self, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let Some(cover) = self.dialog.content() else {
            return;
        };
        let Some(selected) = cover.selected.clone() else {
            return;
        };
        let crop = cover.crop;
        let (library, thumbnail, scratch) = {
            let session = self.session.read(cx);
            (
                session.writable_root(cx),
                session.thumbnail(&selected.hash),
                session.scratch(),
            )
        };
        let Some(library) = library else {
            ui::notify::error(
                t("Could not set library cover"),
                en("No library is open."),
                cx,
            );
            return;
        };
        self.saving = true;
        cx.notify();
        let work = cx.background_spawn(async move {
            write_cover(&library, &thumbnail, &selected.hash, crop)?;
            // The cover is read from a new file name each time, so that no
            // image cache shows the old cover.
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_millis());
            Ok::<_, String>(crate::cover::prepare_into(
                &library,
                &scratch.join(format!("library-cover-{stamp}.png")),
            ))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok(prepared) => {
                        this.session.update(cx, |session, cx| {
                            session.library_cover = prepared;
                            cx.emit(Change::Navigation);
                        });
                        this.dialog.close(cx);
                    }
                    Err(error) => {
                        ui::notify::error(t("Could not set library cover"), error, cx);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn dismiss_cover(&mut self, cx: &mut Context<Self>) {
        self.dialog.dismiss(cx);
    }
}

impl Render for LibraryCover {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = div().absolute().inset_0();
        let Some(layer) = self.dialog.layer(window, cx) else {
            return root;
        };
        let (presence, focus) = (layer.presence, layer.focus.clone());
        root.child(cover_layer(self, presence, focus, window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::cover_frame;
    use crate::cover::Crop;

    /// The cover is written only into the library's folder. A cover file that
    /// is a link to a place outside the folder is not followed and not
    /// replaced.
    #[cfg(unix)]
    #[test]
    fn the_cover_is_written_only_inside_the_library() {
        let library = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let root = library.path().canonicalize().unwrap();
        let thumbnail = root.join("blobs/t/ab/abcd.jpg");
        std::fs::create_dir_all(thumbnail.parent().unwrap()).unwrap();
        std::fs::write(&thumbnail, b"jpeg").unwrap();
        let crop = Crop {
            focus_x: 500.,
            focus_y: 500.,
            zoom: 100.,
        };
        super::write_cover(&root, &thumbnail, "ab", crop).unwrap();
        assert_eq!(
            std::fs::read(root.join(".picto-library-cover.jpg")).unwrap(),
            b"jpeg"
        );
        assert!(root.join(".picto-library-cover.json").is_file());
        // The frame's file is a link to another folder: the save is refused
        // and the link's target is left as it was.
        std::fs::remove_file(root.join(".picto-library-cover.json")).unwrap();
        let target = elsewhere.path().join("frame.json");
        std::fs::write(&target, b"theirs").unwrap();
        std::os::unix::fs::symlink(&target, root.join(".picto-library-cover.json")).unwrap();
        assert!(super::write_cover(&root, &thumbnail, "ab", crop).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"theirs");
        assert_eq!(std::fs::read_dir(elsewhere.path()).unwrap().count(), 1);
    }

    #[test]
    fn the_frame_is_written_as_production_writes_it() {
        let frame = cover_frame(
            "ab",
            Crop {
                focus_x: 250.,
                focus_y: 500.,
                zoom: 150.,
            },
        );
        assert_eq!(
            frame,
            "{\n  \"imageHash\": \"ab\",\n  \"imageFocusX\": 250,\n  \"imageFocusY\": 500,\n  \"imageZoomPercent\": 150\n}"
        );
        let read: serde_json::Value = serde_json::from_str(&frame).unwrap();
        assert_eq!(read["imageHash"], "ab");
    }
}
