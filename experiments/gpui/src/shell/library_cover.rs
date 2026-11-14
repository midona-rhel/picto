//! `LibraryCoverDialogHost`: the open library's cover, chosen from its own
//! media in the shared cover dialog (`MediaCoverDialog`) and framed there.
//! "Set as Library Cover" opens it on one item, already being framed.
//! Saving does what `setLibraryMeta` does to the library's folder: the
//! item's thumbnail is copied beside the database as
//! `.picto-library-cover.<ext>` and the frame written to
//! `.picto-library-cover.json`; the sidebar and the library list then show
//! the new cover.
use crate::grid::thumbnails::{self, Thumbnails};
use crate::session::{Change, Session};
use crate::ui;
use crate::ui::cover_dialog::{Candidate, Cover, CoverHost, Target, add_page, cover_layer};
use gpui::*;
use picto_library::RootKind;
use picto_library::query::{ItemScope, PageRequest, RootQuery, RootSummary};
use std::path::{Path, PathBuf};

const INSTRUCTIONS: &str =
    "Select a media item from this library, then adjust its position and zoom.";
const EMPTY: &str = "This library has no media available for a cover.";
/// `PAGE_SIZE` in `libraryAppearance.ts`.
const PAGE: usize = 200;
/// The cover file's names beside the library's database.
const COVER_EXTENSIONS: [&str; 2] = ["jpg", "png"];

/// A media item as the dialog offers it.
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
    /// The candidates decode into a cache of their own.
    thumbnails: Entity<Thumbnails>,
    dialog: ui::modal::Host<Cover>,
    /// Where the keyboard goes back to.
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

    /// `openCurrentLibraryCoverPicker`: the dialog, framing `initial` at
    /// once when it is given, with the library's media loading behind it.
    pub fn open(&mut self, initial: Option<Candidate>, cx: &mut Context<Self>) {
        if self.session.read(cx).library_root().is_none() {
            ui::notify::error("Could not set library cover", "No library is open.", cx);
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

    /// `loadLibraryCoverCandidates`: the All destination's media, newest
    /// import first, a page at a time.
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
                        // Production's words, from the subscription dialog
                        // it shares.
                        let title = if more {
                            "Unable to load more subscription media"
                        } else {
                            cover.candidates = Default::default();
                            "Unable to load subscription media"
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

/// `setLibraryMeta` for an image: the thumbnail of `hash` copied beside the
/// database as the cover (replacing one of the other type), and the frame
/// written beside it. Only a marked disposable copy is written to.
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
        let _ = std::fs::remove_file(library.join(format!(".picto-library-cover.{other}")));
    }
    let frame = cover_frame(hash, crop);
    replace(library.join(".picto-library-cover.json"), &|temporary| {
        std::fs::write(temporary, &frame)
    })
}

/// `writeMaterializedCoverMetadata`: the frame as production writes it.
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
        let (library, thumbnail, scratch, disposable) = {
            let session = self.session.read(cx);
            (
                session.library_root().map(Path::to_path_buf),
                session.thumbnail(&selected.hash),
                session.scratch().to_path_buf(),
                session.disposable(),
            )
        };
        let Some(library) = library.filter(|_| disposable) else {
            ui::notify::error("Could not set library cover", "No library is open.", cx);
            return;
        };
        self.saving = true;
        cx.notify();
        let work = cx.background_spawn(async move {
            write_cover(&library, &thumbnail, &selected.hash, crop)?;
            // A file of its own each time, so no image cache shows the old.
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
                        ui::notify::error("Could not set library cover", error, cx);
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
