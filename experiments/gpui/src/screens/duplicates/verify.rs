//! Eagle's "Verify Before Merge", for the checked sets whose files differ
//! in what a merge combines: their names, links, folders, tags or notes.
//! The sets are listed on the left; on the right, only what differs in the
//! one chosen, and what the kept file ends up with. The library's merge
//! decides that itself (the kept item takes the most descriptive name and
//! every link, folder and tag; notes are joined, each said once), so this
//! is a check that shows what the merge will write, not a choice; identical
//! files merge without it.
use super::list::summary;
use super::{DuplicatesScreen, Entry, Mode};
use crate::grid::layout::Tile;
use crate::grid::prefs::ViewPrefs;
use crate::grid::tile::{TileLook, tile_art};
use crate::i18n::{en, t};
use crate::theme::Text as _;
use crate::ui::action_button::Look;
use crate::{theme, ui};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use std::collections::BTreeSet;

/// Eagle's merge editor: a 240px list of 56px rows with 44px pictures.
const LIST: f32 = 240.;
const ROW: f32 = 56.;
const THUMB: f32 = 44.;
/// The most tag chips a set shows.
const MAX_CHIPS: usize = 40;

/// What the files of a set hold that a merge combines.
struct Combined {
    names: BTreeSet<String>,
    links: BTreeSet<String>,
    folders: Vec<BTreeSet<u32>>,
    tags: Vec<BTreeSet<u32>>,
    notes: BTreeSet<String>,
}

impl Combined {
    fn of(entry: &Entry) -> Self {
        let details = || {
            entry
                .facts
                .iter()
                .filter_map(|facts| facts.details.as_ref())
        };
        Self {
            names: (0..entry.set.files.len())
                .map(|file| entry.facts[file].name(&entry.set, file))
                .collect(),
            links: details()
                .flat_map(|details| details.root.source_urls.iter().cloned())
                .collect(),
            folders: details()
                .map(|details| details.folder_ids.iter().map(|id| id.0).collect())
                .collect(),
            tags: details()
                .map(|details| details.tag_ids.iter().map(|id| id.0).collect())
                .collect(),
            notes: details()
                .filter_map(|details| details.root.notes.clone())
                .map(|notes| notes.trim().to_owned())
                .filter(|notes| !notes.is_empty())
                .collect(),
        }
    }

    fn links_differ(entry: &Entry) -> bool {
        let sets = entry
            .facts
            .iter()
            .filter_map(|facts| facts.details.as_ref())
            .map(|details| details.root.source_urls.iter().collect::<BTreeSet<_>>())
            .collect::<Vec<_>>();
        sets.windows(2).any(|pair| pair[0] != pair[1])
    }

    fn differs(sets: &[BTreeSet<u32>]) -> bool {
        sets.windows(2).any(|pair| pair[0] != pair[1])
    }
}

/// What the kept file ends up with, its name and its notes, worked out as
/// the library's merge writes them: pair by pair in the merge's order, each
/// kept file taking the more descriptive name of the two and the notes the
/// library joins (`picto_library::preferred_name`, `merged_notes`).
fn outcome(entry: &Entry, keeper: usize) -> (String, Option<String>) {
    let set = &entry.set;
    let mut names = (0..set.files.len())
        .map(|file| {
            (
                set.files[file].file.file_id,
                entry.facts[file].name(set, file),
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    let mut notes = (0..set.files.len())
        .map(|file| {
            let notes = entry.facts[file]
                .details
                .as_ref()
                .and_then(|details| details.root.notes.clone());
            (set.files[file].file.file_id, notes)
        })
        .collect::<std::collections::HashMap<_, _>>();
    for (a, b, choice) in set.merge_steps(keeper) {
        let picto_library::DuplicateResolutionChoice::KeepFile { winner_file_id } = choice else {
            continue;
        };
        let loser = if a == winner_file_id { b } else { a };
        let name =
            picto_library::preferred_name(&names[&winner_file_id], &names[&loser]).to_owned();
        names.insert(winner_file_id, name);
        let joined = picto_library::merged_notes(
            notes[&winner_file_id].as_deref(),
            notes[&loser].as_deref(),
        );
        notes.insert(winner_file_id, joined);
    }
    let kept = set.files[keeper].file.file_id;
    (
        names.remove(&kept).unwrap_or_default(),
        notes.remove(&kept).flatten(),
    )
}

/// Whether a merge of this set combines anything that differs.
pub fn differs(entry: &Entry) -> bool {
    let combined = Combined::of(entry);
    combined.names.len() > 1
        || Combined::links_differ(entry)
        || Combined::differs(&combined.folders)
        || Combined::differs(&combined.tags)
        || combined.notes.len() > 1
}

impl DuplicatesScreen {
    pub(super) fn verify_view(
        &mut self,
        keys: Vec<u32>,
        current: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = theme::theme(cx);
        let (chosen, _) = self.selected();
        let mut rows = div().flex().flex_col();
        for (position, key) in keys.iter().copied().enumerate() {
            rows = rows.child(self.verify_row(key, position, position == current, window, cx));
        }
        let merge = ui::action_button("duplicates-verify-merge", Look::Primary)
            .icon("action-arrows-join", 16.)
            .label(format!("{} ({})", en("Merge"), chosen.len()))
            .full_width()
            .disabled(self.busy || chosen.is_empty())
            .tip(
                ui::tip(en("Merge every checked set, keeping each marked file"))
                    .shortcut("dup.merge", cx),
            )
            .on_click(cx.listener(|this, _, _, cx| this.merge_checked(cx)));
        let list = div()
            .w(px(LIST))
            .flex_none()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(theme.border_secondary())
            .child(
                div()
                    .id("duplicate-verify-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .py(px(4.))
                    .child(rows),
            )
            .child(
                div()
                    .flex_none()
                    .p(px(12.))
                    .border_t_1()
                    .border_color(theme.border_secondary())
                    .child(merge),
            );
        let detail = keys
            .get(current)
            .and_then(|key| self.entry(*key))
            .map(|entry| self.verify_detail(entry, cx));
        div()
            .size_full()
            .flex()
            .child(list)
            .child(
                div()
                    .id("duplicate-verify-detail")
                    .flex_1()
                    .min_w_0()
                    .overflow_y_scroll()
                    .px(px(20.))
                    .pb(px(16.))
                    .children(detail),
            )
            .into_any_element()
    }

    /// A set in the list: its kept file's picture and name, and its count.
    fn verify_row(
        &mut self,
        key: u32,
        position: usize,
        current: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let Some(index) = self.entry_index(key) else {
            return div().id(("duplicate-verify-row", position));
        };
        let keeper = self.keeper(index);
        let entry = &self.entries[index];
        let side = &entry.set.files[keeper];
        let name = entry.facts[keeper].name(&entry.set, keeper);
        let thumbnail = self.session.read(cx).thumbnail(&side.file.file_hash);
        let resource = Resource::Path(thumbnail.clone().into());
        let thumb = self
            .thumbnails
            .read(cx)
            .thumb(&resource, &side.file.mime_type);
        let count = entry.set.files.len();
        let look = TileLook {
            tile: Tile {
                x: 0.,
                y: 0.,
                w: THUMB,
                h: THUMB,
            },
            item: summary(side, &entry.facts[keeper], name.clone()),
            thumbnail,
            selected: false,
            thumb,
            original: None,
            alphas: (1., 0.),
            magnifier: false,
            grayscale: false,
        };
        let prefs = ViewPrefs {
            mode: crate::grid::layout::ViewMode::Grid,
            show_name: false,
            show_resolution: false,
            show_extension_label: false,
            fit_cover: false,
            ..ViewPrefs::default()
        };
        let art = tile_art(look, &prefs, &mut self.art, &self.thumbnails, window, cx);
        div()
            .id(("duplicate-verify-row", position))
            .role(Role::Tab)
            .aria_selected(current)
            .aria_label(name.clone())
            .h(px(ROW))
            .mx(px(6.))
            .px(px(6.))
            .flex()
            .items_center()
            .gap(px(10.))
            .rounded(px(theme::RADIUS_MD))
            .cursor_pointer()
            .when(current, |row| row.bg(theme.surface_active()))
            .when(!current, |row| {
                row.hover(|row| row.bg(theme.surface_hover()))
            })
            .child(div().relative().size(px(THUMB)).flex_none().child(art))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text(theme::TEXT_SM)
                            .text_color(theme.text())
                            .truncate()
                            .child(name),
                    )
                    .child(
                        div()
                            .text(theme::TEXT_CAPTION)
                            .text_color(theme.text_tertiary())
                            .child(format!("{count} {}", en("files"))),
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Mode::Verify { current, .. } = &mut this.mode {
                    *current = position;
                    this.changed(cx);
                }
            }))
    }

    /// What differs in one set, section by section, with what the kept
    /// file ends up with.
    fn verify_detail(&self, entry: &Entry, cx: &App) -> Div {
        let theme = theme::theme(cx);
        let combined = Combined::of(entry);
        let keeper = self
            .entry_index(entry.set.key)
            .map_or(entry.set.best, |index| self.keeper(index));
        let (kept_name, kept_notes) = outcome(entry, keeper);
        let session = self.session.read(cx);
        let note = |text: &'static str| {
            div()
                .mb(px(6.))
                .text(theme::TEXT_CAPTION)
                .text_color(theme.text_tertiary())
                .child(text)
        };
        let line = |text: String| {
            div()
                .h(px(20.))
                .flex()
                .items_center()
                .text(theme::TEXT_SM)
                .text_color(theme.text())
                .child(div().min_w_0().truncate().child(text))
        };
        let keeper = self
            .entry_index(entry.set.key)
            .map_or(entry.set.best, |index| self.keeper(index));
        let title = entry.facts[keeper].name(&entry.set, keeper);
        let mut sections = div().flex().flex_col().child(
            div()
                .pt(px(14.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text(theme::TEXT_MD)
                        .text_color(theme.text())
                        .truncate()
                        .child(title),
                )
                .child(
                    div()
                        .text(theme::TEXT_CAPTION)
                        .text_color(theme.text_tertiary())
                        .child(format!(
                            "{} {}, {}",
                            entry.set.files.len(),
                            en("files"),
                            en("merged into the marked one")
                        )),
                ),
        );
        if combined.names.len() > 1 {
            sections =
                sections.child(
                    ui::panel_section("verify-names", t("File name")).content(Some(
                        div()
                            .child(note(en("The most descriptive name is kept.")))
                            .children(combined.names.iter().map(|name| {
                                // The name kept is marked; the others go with
                                // their files, dimmed as the list dims them.
                                let kept = *name == kept_name;
                                div()
                                    .h(px(20.))
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .text(theme::TEXT_SM)
                                    .text_color(if kept {
                                        theme.text()
                                    } else {
                                        theme.text_tertiary()
                                    })
                                    .child(div().w(px(12.)).flex_none().children(kept.then(|| {
                                        ui::icon("check", 12., rgb(theme::GRID_SELECTION))
                                    })))
                                    .child(div().min_w_0().truncate().child(name.clone()))
                            }))
                            .into_any_element(),
                    )),
                );
        }
        if Combined::links_differ(entry) {
            sections = sections.child(
                ui::panel_section("verify-links", en("Links")).content(Some(
                    div()
                        .child(note(en("Every link is kept.")))
                        .children(combined.links.iter().cloned().map(line))
                        .into_any_element(),
                )),
            );
        }
        if Combined::differs(&combined.folders) {
            let all = crate::inspector::folder_picker::folders(&session.nav);
            let union = combined
                .folders
                .iter()
                .flatten()
                .copied()
                .collect::<BTreeSet<_>>();
            let chips = union.into_iter().filter_map(|id| {
                let folder = all.iter().find(|folder| folder.id.0 == id)?;
                Some(ui::chip(
                    ("verify-folder", id as usize),
                    folder.name.clone(),
                    folder.color.unwrap_or(theme::FOLDER_DEFAULT),
                ))
            });
            sections = sections.child(
                ui::panel_section("verify-folders", t("Folders")).content(Some(
                    div()
                        .child(note(en("The kept file is placed in every folder.")))
                        .child(div().flex().flex_wrap().gap(px(4.)).children(chips))
                        .into_any_element(),
                )),
            );
        }
        if Combined::differs(&combined.tags) {
            let union = combined
                .tags
                .iter()
                .flatten()
                .copied()
                .collect::<BTreeSet<_>>();
            let shared = combined.tags.iter().skip(1).fold(
                combined.tags.first().cloned().unwrap_or_default(),
                |shared, tags| shared.intersection(tags).copied().collect(),
            );
            // Only the tags some files lack, which the merge adds.
            let added = union.difference(&shared).copied().collect::<Vec<_>>();
            // A set of heavily tagged files would fill the pane: the first
            // few show, and how many more there are.
            let known = added
                .iter()
                .filter_map(|id| session.tags.get(id))
                .collect::<Vec<_>>();
            let more = added.len().saturating_sub(MAX_CHIPS);
            let chips = known
                .into_iter()
                .take(MAX_CHIPS)
                .map(|tag| crate::inspector::tag_chip(tag, cx));
            sections = sections.child(
                ui::panel_section("verify-tags", t("Tags")).content(Some(
                    div()
                        .child(note(en(
                            "The kept file has every tag. These are not on every file:",
                        )))
                        .child(div().flex().flex_wrap().gap(px(4.)).children(chips))
                        .children((more > 0).then(|| {
                            div()
                                .mt(px(6.))
                                .text(theme::TEXT_CAPTION)
                                .text_color(theme.text_tertiary())
                                .child(format!("{} {more} {}", t("and"), en("more")))
                        }))
                        .into_any_element(),
                )),
            );
        }
        if combined.notes.len() > 1 {
            sections = sections.child(
                ui::panel_section("verify-notes", t("Notes")).content(Some(
                    div()
                        .child(note(en("The kept file's notes after the merge:")))
                        .children(kept_notes.clone().map(|notes| {
                            div()
                                .py(px(3.))
                                .text(theme::TEXT_SM)
                                .text_color(theme.text())
                                .child(notes)
                        }))
                        .into_any_element(),
                )),
            );
        }
        sections
    }
}
