//! The tag picker's pure list model: which tags show and in what order,
//! the offer to create one and the arrow keys' steps. What it shares with
//! the other tag browsers is in `crate::tags`.
use crate::tags::{View, record, record_name as name};
use crate::ui::overlay_shell::Nav;
use picto_library::TagRecord;
use std::collections::BTreeSet;

/// `displayTagName`: the name, without its group unless prefixes show.
pub(super) fn display_name(tag: &TagRecord, prefixes: bool) -> String {
    let full = name(tag);
    if prefixes {
        return full;
    }
    crate::tags::split(&full).1.to_owned()
}

/// Everything [`display_tags`] chooses from.
pub struct Sources<'a> {
    pub view: &'a View,
    /// What the search field holds now, and the search the loaded `tags`
    /// answer.
    pub query: &'a str,
    pub settled: &'a str,
    /// The items' tags by name, and their records where known.
    pub entity: &'a [String],
    pub entity_records: &'a [TagRecord],
    /// The names that were selected when the picker opened, which list first.
    pub opening: &'a [String],
    pub tags: &'a [TagRecord],
    pub starred: &'a [String],
}

/// `displayTags`: the rows the list shows, best match first, each visible
/// name once.
pub fn display_tags(sources: &Sources) -> Vec<TagRecord> {
    let local = matches!(sources.view, View::Selected | View::Starred);
    let candidates = match sources.view {
        View::Selected => crate::tags::records_by_group(sources.entity),
        View::Starred => {
            let mut records = sources
                .starred
                .iter()
                .map(|name| record(name))
                .collect::<Vec<_>>();
            records.sort_by(|left, right| crate::text::natural_order(&name(left), &name(right)));
            records
        }
        View::All | View::Group(_) | View::Recent => {
            let known = |wanted: &str| {
                sources
                    .tags
                    .iter()
                    .chain(sources.entity_records)
                    .find(|tag| name(tag) == wanted)
                    .cloned()
            };
            let group = match sources.view {
                View::Group(group) => Some(group.as_str()),
                _ => None,
            };
            let opening = sources
                .opening
                .iter()
                .map(|wanted| known(wanted).unwrap_or_else(|| record(wanted)))
                .filter(|tag| group.is_none_or(|group| tag.namespace == group))
                .collect::<Vec<_>>();
            let opening_names = opening.iter().map(name).collect::<BTreeSet<_>>();
            opening
                .into_iter()
                .chain(
                    sources
                        .tags
                        .iter()
                        .filter(|tag| !opening_names.contains(&name(tag)))
                        .cloned(),
                )
                .collect()
        }
    };
    let query = if local {
        sources.query
    } else {
        sources.settled
    }
    .trim()
    .to_lowercase();
    // `namespace:subname` searches match the full name; others the subname.
    let full = query.contains(':');
    let keyed = candidates
        .into_iter()
        .map(|tag| {
            let key = if full || tag.subname.is_empty() {
                name(&tag)
            } else {
                tag.subname.clone()
            };
            (key, tag)
        })
        .collect::<Vec<_>>();
    let ranked = crate::text::rank_matches(keyed, &query, |(key, _)| key.as_str());
    let mut seen = BTreeSet::new();
    ranked
        .into_iter()
        .map(|(_, tag)| tag)
        .filter(|tag| {
            let visible = if tag.subname.is_empty() {
                name(tag)
            } else {
                tag.subname.clone()
            };
            seen.insert(visible.to_lowercase())
        })
        .collect()
}

/// `createTag`: the tag a search would create, when it names none loaded:
/// what was typed, in the browsed group unless it names one itself.
pub fn create_candidate(
    view: &View,
    settled: &str,
    settled_group: Option<&str>,
    tags: &[TagRecord],
) -> Option<TagRecord> {
    if matches!(view, View::Selected | View::Starred) {
        return None;
    }
    let typed = settled.trim();
    if typed.is_empty() {
        return None;
    }
    let wanted = match settled_group {
        Some(group) if !typed.contains(':') => format!("{group}:{typed}"),
        _ => typed.to_owned(),
    };
    let exists = tags
        .iter()
        .any(|tag| name(tag).to_lowercase() == wanted.to_lowercase());
    (!exists).then(|| record(&wanted))
}

/// The row the arrow keys move to among `count` rows in `columns`.
pub fn step(focus: Option<usize>, key: Nav, columns: usize, count: usize) -> Option<usize> {
    if count == 0 {
        return focus;
    }
    let last = count - 1;
    let Some(at) = focus else {
        return Some(0);
    };
    Some(match key {
        Nav::Down => (at + columns).min(last),
        Nav::Up => at.saturating_sub(columns),
        Nav::Right => (at + 1).min(last),
        Nav::Left => at.saturating_sub(1),
        _ => at,
    })
}

#[cfg(test)]
mod tests {
    use super::{Sources, View, create_candidate, display_tags, name, step};
    use crate::ui::overlay_shell::Nav;
    use picto_library::{TagId, TagNamespaceId, TagRecord};

    fn tag(id: u32, namespace: &str, subname: &str) -> TagRecord {
        TagRecord {
            tag_id: TagId(id),
            namespace_id: TagNamespaceId(0),
            namespace: namespace.into(),
            subname: subname.into(),
            active_count: 0,
            assignment_count: 0,
        }
    }

    fn names(tags: &[TagRecord]) -> Vec<String> {
        tags.iter().map(name).collect()
    }

    #[test]
    fn the_tags_selected_on_opening_list_first_then_the_rest() {
        let tags = [
            tag(1, "", "apple"),
            tag(2, "", "banana"),
            tag(3, "series", "cherry"),
        ];
        let opening = ["series:cherry".to_owned()];
        let rows = display_tags(&Sources {
            view: &View::All,
            query: "",
            settled: "",
            entity: &opening,
            entity_records: &[],
            opening: &opening,
            tags: &tags,
            starred: &[],
        });
        assert_eq!(names(&rows), ["series:cherry", "apple", "banana"]);
        // In a group, only that group's opening tags come first.
        let rows = display_tags(&Sources {
            view: &View::Group("creator".into()),
            query: "",
            settled: "",
            entity: &opening,
            entity_records: &[],
            opening: &opening,
            tags: &tags[..2],
            starred: &[],
        });
        assert_eq!(names(&rows), ["apple", "banana"]);
    }

    #[test]
    fn searches_rank_by_subname_and_show_each_visible_name_once() {
        let tags = [
            tag(1, "", "bobcat"),
            tag(2, "", "cat"),
            tag(3, "species", "cat"),
            tag(4, "", "catgirl"),
        ];
        let rows = display_tags(&Sources {
            view: &View::All,
            query: "cat",
            settled: "cat",
            entity: &[],
            entity_records: &[],
            opening: &[],
            tags: &tags,
            starred: &[],
        });
        // `species:cat` has the same visible name as `cat` and is dropped.
        assert_eq!(names(&rows), ["cat", "catgirl", "bobcat"]);
        // A search with a colon matches the full name.
        let rows = display_tags(&Sources {
            view: &View::All,
            query: "species:",
            settled: "species:",
            entity: &[],
            entity_records: &[],
            opening: &[],
            tags: &tags,
            starred: &[],
        });
        assert_eq!(names(&rows), ["species:cat"]);
    }

    #[test]
    fn the_selected_and_starred_views_list_by_name_and_search_locally() {
        let entity = ["b".to_owned(), "creator:a".to_owned(), "a".to_owned()];
        let rows = display_tags(&Sources {
            view: &View::Selected,
            query: "",
            settled: "zzz",
            entity: &entity,
            entity_records: &[],
            opening: &[],
            tags: &[],
            starred: &[],
        });
        // By group, then name; `a` shows the same name as `creator:a` and
        // is listed once, as production's de-duplication does.
        assert_eq!(names(&rows), ["creator:a", "b"]);
        let starred = ["zeta".to_owned(), "alpha".to_owned()];
        let rows = display_tags(&Sources {
            view: &View::Starred,
            query: "zet",
            settled: "",
            entity: &[],
            entity_records: &[],
            opening: &[],
            tags: &[],
            starred: &starred,
        });
        assert_eq!(names(&rows), ["zeta"]);
    }

    #[test]
    fn a_search_naming_no_loaded_tag_offers_to_create_it_in_the_browsed_group() {
        let tags = [tag(1, "", "cat")];
        assert!(create_candidate(&View::All, "Cat", None, &tags).is_none());
        let created = create_candidate(&View::All, " dog ", None, &tags).unwrap();
        assert_eq!(name(&created), "dog");
        assert_eq!(created.tag_id, TagId(0));
        let grouped = create_candidate(
            &View::Group("species".into()),
            "dog",
            Some("species"),
            &tags,
        );
        assert_eq!(name(&grouped.unwrap()), "species:dog");
        let named = create_candidate(&View::All, "creator:x", Some("species"), &tags);
        assert_eq!(name(&named.unwrap()), "creator:x");
        assert!(create_candidate(&View::Selected, "dog", None, &tags).is_none());
        assert!(create_candidate(&View::All, "  ", None, &tags).is_none());
    }

    #[test]
    fn the_arrows_move_by_row_and_column_and_stop_at_the_ends() {
        assert_eq!(step(None, Nav::Down, 2, 5), Some(0));
        assert_eq!(step(Some(0), Nav::Down, 2, 5), Some(2));
        assert_eq!(step(Some(3), Nav::Down, 2, 5), Some(4));
        assert_eq!(step(Some(1), Nav::Up, 2, 5), Some(0));
        assert_eq!(step(Some(4), Nav::Right, 2, 5), Some(4));
        assert_eq!(step(Some(0), Nav::Left, 2, 5), Some(0));
        assert_eq!(step(Some(2), Nav::Down, 1, 0), Some(2));
    }
}
