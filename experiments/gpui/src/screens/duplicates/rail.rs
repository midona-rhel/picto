//! The desk's rail, in the inspector's place and built as the inspector
//! is: the files side by side (each fact in a row, what differs between
//! them in the primary colour, the library's suggestion of a file to keep
//! offered as a suggestion only), and what the one item left after the
//! merge will hold. That is worked out with the library's own rules, step
//! by step in the merge's order: the most descriptive name
//! (`picto_library::preferred_name`), the notes joined each said once
//! (`picto_library::merged_notes`), and every tag, folder and link and the
//! higher rating handed to the kept item (`mutation/merge.rs`
//! `hand_over`). A file inside a collection with other members leaves it
//! and hands nothing over, as the library does.
use super::{DuplicatesScreen, Entry};
use crate::i18n::{en, t};
use crate::theme::Text as _;
use crate::{theme, ui};
use gpui::*;
use picto_library::{DuplicateResolutionChoice, Rating};
use std::collections::{BTreeSet, HashMap};

/// The label column of the files' rows; the files share the rest.
const LABEL: f32 = 70.;

/// What the one item left after a merge holds.
struct Outcome {
    name: String,
    /// The other files' names, which the item does not take.
    dropped: Vec<String>,
    tags: usize,
    /// Tags the kept file does not have now.
    gained: usize,
    folders: Vec<String>,
    links: usize,
    notes: Option<String>,
    rating: Rating,
    /// Files that leave a collection rather than hand anything over.
    members: Vec<usize>,
}

/// Works out the merge keeping `keeper`, pair by pair in its order.
fn outcome(entry: &Entry, keeper: usize, folder_names: &HashMap<u32, String>) -> Outcome {
    let set = &entry.set;
    let details = |file: usize| entry.facts[file].details.as_ref();
    // The name a standalone item shows is the better of its media's and
    // its own, as the library takes it.
    let name = |file: usize| {
        let media = entry.facts[file].name(set, file);
        match details(file).map(|details| details.root.name.as_str()) {
            Some(root) if !root.is_empty() => {
                picto_library::preferred_name(&media, root).to_owned()
            }
            _ => media,
        }
    };
    // Only an item that is not a member of a collection hands what it has
    // to the item kept.
    let hands_over = |file: usize| {
        set.files[file]
            .occurrences
            .first()
            .is_none_or(|occurrence| occurrence.collection_id.is_none())
    };
    let id = |file: usize| set.files[file].file.file_id;
    let mut names = (0..set.files.len())
        .map(|file| (id(file), name(file)))
        .collect::<HashMap<_, _>>();
    let mut notes = (0..set.files.len())
        .map(|file| {
            (
                id(file),
                details(file).and_then(|details| details.root.notes.clone()),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut tags = (0..set.files.len())
        .map(|file| {
            let tags = details(file)
                .map(|details| {
                    details
                        .tag_ids
                        .iter()
                        .map(|tag| tag.0)
                        .collect::<BTreeSet<_>>()
                })
                .unwrap_or_default();
            (id(file), tags)
        })
        .collect::<HashMap<_, _>>();
    let mut folders = (0..set.files.len())
        .map(|file| {
            let folders = details(file)
                .map(|details| {
                    details
                        .folder_ids
                        .iter()
                        .map(|folder| folder.0)
                        .collect::<BTreeSet<_>>()
                })
                .unwrap_or_default();
            (id(file), folders)
        })
        .collect::<HashMap<_, _>>();
    let mut links = (0..set.files.len())
        .map(|file| {
            let links = details(file)
                .map(|details| {
                    details
                        .root
                        .source_urls
                        .iter()
                        .cloned()
                        .collect::<BTreeSet<_>>()
                })
                .unwrap_or_default();
            (id(file), links)
        })
        .collect::<HashMap<_, _>>();
    let rank = |rating: Rating| {
        Rating::ALL
            .iter()
            .position(|each| *each == rating)
            .unwrap_or(0)
    };
    let mut ratings = (0..set.files.len())
        .map(|file| {
            (
                id(file),
                details(file).map_or(Rating::Unrated, |details| details.rating),
            )
        })
        .collect::<HashMap<_, _>>();
    let index_of = |file| set.index_of(file).unwrap_or(0);
    for (a, b, choice) in set.merge_steps(keeper) {
        let DuplicateResolutionChoice::KeepFile {
            winner_file_id: winner,
        } = choice
        else {
            continue;
        };
        let loser = if a == winner { b } else { a };
        let name = picto_library::preferred_name(&names[&winner], &names[&loser]).to_owned();
        names.insert(winner, name);
        if !hands_over(index_of(loser)) {
            continue;
        }
        let joined =
            picto_library::merged_notes(notes[&winner].as_deref(), notes[&loser].as_deref());
        notes.insert(winner, joined);
        let from = tags[&loser].clone();
        tags.get_mut(&winner)
            .expect("a file of the set")
            .extend(from);
        let from = folders[&loser].clone();
        folders
            .get_mut(&winner)
            .expect("a file of the set")
            .extend(from);
        let from = links[&loser].clone();
        links
            .get_mut(&winner)
            .expect("a file of the set")
            .extend(from);
        if rank(ratings[&loser]) > rank(ratings[&winner]) {
            ratings.insert(winner, ratings[&loser]);
        }
    }
    let kept = id(keeper);
    let name = names.remove(&kept).unwrap_or_default();
    let mut dropped = (0..set.files.len())
        .map(&name_of(entry))
        .filter(|other| *other != name)
        .collect::<Vec<_>>();
    dropped.sort();
    dropped.dedup();
    let before = details(keeper).map_or(0, |details| details.tag_ids.len());
    let tags = tags.remove(&kept).unwrap_or_default().len();
    Outcome {
        name,
        dropped,
        tags,
        gained: tags.saturating_sub(before),
        folders: folders
            .remove(&kept)
            .unwrap_or_default()
            .into_iter()
            .map(|folder| {
                folder_names
                    .get(&folder)
                    .cloned()
                    .unwrap_or_else(|| folder.to_string())
            })
            .collect(),
        links: links.remove(&kept).map_or(0, |links| links.len()),
        notes: notes.remove(&kept).flatten(),
        rating: ratings.remove(&kept).unwrap_or(Rating::Unrated),
        members: (0..set.files.len())
            .filter(|&file| file != keeper && !hands_over(file))
            .collect(),
    }
}

/// A file's name as the desk shows it.
fn name_of(entry: &Entry) -> impl Fn(usize) -> String + '_ {
    move |file| entry.facts[file].name(&entry.set, file)
}

/// Why the library would keep the file it suggests over the others: the
/// first of its own rules that sets it apart.
fn reason(entry: &Entry, suggested: usize) -> &'static str {
    let files = &entry.set.files;
    let area = |file: usize| {
        files[file].file.pixel_width.unwrap_or(0) as u64
            * files[file].file.pixel_height.unwrap_or(0) as u64
    };
    let others = (0..files.len())
        .filter(|file| *file != suggested)
        .collect::<Vec<_>>();
    let lossless = |file: usize| {
        matches!(
            files[file].file.mime_type.as_str(),
            "image/png" | "image/bmp" | "image/tiff" | "image/gif"
        )
    };
    if entry.set.equal_quality() {
        en("equal quality, the older file")
    } else if others.iter().all(|&other| area(suggested) > area(other)) {
        en("larger picture")
    } else if lossless(suggested) && others.iter().all(|&other| !lossless(other)) {
        en("lossless format")
    } else {
        en("more data")
    }
}

/// A rating as the inspector writes it.
fn rating_text(rating: Rating) -> String {
    match rating {
        Rating::Unrated => t("Unrated").to_owned(),
        rating => "★".repeat(rating.bitmap_key() as usize),
    }
}

impl DuplicatesScreen {
    /// The rail for the set on the stage.
    pub(super) fn rail(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = theme::theme(cx);
        let Some(index) = self.current_index() else {
            return div().into_any_element();
        };
        let entry = &self.entries[index];
        let set = &entry.set;
        // A pair side by side; in a larger set, the file shown and the one
        // it is measured against.
        let columns = if set.files.len() == 2 {
            vec![0, 1]
        } else {
            let mut columns = vec![self.shown];
            columns.extend(self.partner());
            columns
        };
        let folder_names = {
            let session = self.session.read(cx);
            crate::inspector::folder_picker::folders(&session.nav)
                .into_iter()
                .map(|folder| (folder.id.0, folder.name))
                .collect::<HashMap<_, _>>()
        };
        let source = |file: usize| {
            entry.facts[file]
                .details
                .as_ref()
                .and_then(|details| details.root.source_urls.first())
                .map(|url| {
                    let host = url
                        .split("://")
                        .nth(1)
                        .unwrap_or(url)
                        .split('/')
                        .next()
                        .unwrap_or(url);
                    host.strip_prefix("www.").unwrap_or(host).to_owned()
                })
                .unwrap_or_else(|| "—".to_owned())
        };
        let added = |file: usize| {
            entry.facts[file].details.as_ref().map_or_else(
                || "—".to_owned(),
                |details| super::compare::short_date(details.root.imported_at_ms),
            )
        };
        let file = |file: usize| &set.files[file].file;
        type Fact<'a> = Box<dyn Fn(usize) -> String + 'a>;
        let facts: Vec<(&'static str, Fact)> = vec![
            (
                t("Dimensions"),
                Box::new(|at| {
                    super::compare::dimensions(file(at).pixel_width, file(at).pixel_height)
                }),
            ),
            (
                t("Size"),
                Box::new(|at| crate::text::size_to_megabytes(file(at).size_bytes)),
            ),
            (
                t("Format"),
                Box::new(|at| crate::grid::format::format_label(&file(at).mime_type)),
            ),
            (t("Added"), Box::new(added)),
            (t("Source"), Box::new(source)),
        ];
        let value = |text: String, bright: bool| {
            div()
                .flex_1()
                .min_w_0()
                .text_right()
                .truncate()
                .font_family(theme::FONT_INSPECTOR_MONO)
                .font_features(theme::tabular_figures())
                .text_color(if bright {
                    theme.contrast(80.)
                } else {
                    theme.text_tertiary()
                })
                .child(text)
        };
        let row = |label: &'static str| {
            div()
                .h(px(20.))
                .flex()
                .items_center()
                .gap(px(8.))
                .whitespace_nowrap()
                .text(theme::TEXT_CAPTION)
                .child(
                    div()
                        .w(px(LABEL))
                        .flex_none()
                        .text_color(theme.text_secondary())
                        .child(label),
                )
        };
        // Each file's column heading: its number in the strip and whether
        // the merge keeps it (the file shown).
        let mut heading = row("").h(px(24.));
        for &at in &columns {
            let kept = at == self.shown;
            heading = heading.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .items_end()
                    .child(
                        div()
                            .text(theme::TEXT_SM)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if kept {
                                rgb(theme::GRID_SELECTION)
                            } else {
                                theme.text()
                            })
                            .child(format!(
                                "{} {}",
                                at + 1,
                                if kept { en("Kept") } else { en("Merged") }
                            )),
                    )
            );
        }
        // Which file the library would keep and by which of its rules, on
        // a line of its own so the reason is read whole.
        let suggestion = set.suggested.map(|at| {
            div()
                .pb(px(6.))
                .text(theme::TEXT_CAPTION)
                .line_height(px(16.))
                .text_color(theme.text_tertiary())
                .child(format!(
                    "{} {}: {}",
                    t("Suggested"),
                    at + 1,
                    reason(entry, at)
                ))
        });
        let mut table = div().flex().flex_col().child(heading).children(suggestion);
        for (label, fact) in &facts {
            let values = columns.iter().map(|&at| fact(at)).collect::<Vec<_>>();
            let differ = values.windows(2).any(|pair| pair[0] != pair[1]);
            let mut line = row(label);
            for text in values {
                line = line.child(value(text, differ));
            }
            table = table.child(line);
        }
        let outcome = outcome(entry, self.shown, &folder_names);
        let outcome_members = outcome.members.clone();
        let property = |label: &'static str, text: String| ui::property_row(label, text);
        let mut after = div()
            .flex()
            .flex_col()
            .text_color(theme.text_secondary())
            .child(property(t("Name"), outcome.name.clone()));
        for dropped in &outcome.dropped {
            after = after.child(
                div()
                    .h(px(18.))
                    .text_right()
                    .text(theme::TEXT_CAPTION)
                    .text_color(theme.text_tertiary())
                    .truncate()
                    .child(format!("{} {dropped}", en("drops"))),
            );
        }
        after = after
            .child(property(
                t("Tags"),
                if outcome.gained > 0 {
                    format!("{} (+{})", outcome.tags, outcome.gained)
                } else {
                    outcome.tags.to_string()
                },
            ))
            .child(property(
                t("Folders"),
                if outcome.folders.is_empty() {
                    t("None").to_owned()
                } else {
                    outcome.folders.join(", ")
                },
            ))
            .child(property(
                en("Links"),
                if outcome.links == 0 {
                    t("None").to_owned()
                } else {
                    outcome.links.to_string()
                },
            ))
            .child(property(t("Rating"), rating_text(outcome.rating)))
            .child(property(
                t("Notes"),
                if outcome.notes.is_some() {
                    String::new()
                } else {
                    t("None").to_owned()
                },
            ))
            .children(outcome.notes.map(|notes| {
                div()
                    .mt(px(2.))
                    .text(theme::TEXT_CAPTION)
                    .line_height(px(16.))
                    .text_color(theme.text_secondary())
                    .child(notes)
            }));
        // A file inside a collection with other members leaves it and gives
        // the kept item nothing: said under what the merge writes.
        let members = outcome_members.into_iter().map(|file| {
            div()
                .mt(px(6.))
                .text(theme::TEXT_CAPTION)
                .line_height(px(16.))
                .text_color(theme.text_tertiary())
                .child(format!(
                    "{} {} {}",
                    t("File"),
                    file + 1,
                    en("is in a collection: it leaves it and hands nothing over.")
                ))
        });
        let after = after.children(members);
        div()
            .id("duplicate-rail")
            .size_full()
            .overflow_y_scroll()
            // As a subscription's rail sits in the same place.
            .pl(px(16.))
            .pr(px(16. + ui::scrollbar::GUTTER))
            .pt(px(4.))
            .pb(px(48.))
            .flex()
            .flex_col()
            .child(
                ui::panel_section("duplicate-files", t("Files"))
                    .content(Some(table.into_any_element())),
            )
            .child(
                ui::panel_section("duplicate-after", en("After merge"))
                    .content(Some(after.into_any_element())),
            )
            .into_any_element()
    }
}
