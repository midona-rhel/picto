//! Session-local generations for the projections used by a query. They are not
//! persisted: caches are empty when a library opens or recovers a checkpoint.
use crate::predicate::{FilterClause, FilterExpr, SortField};
use crate::projection::ProjectionSnapshot;
use crate::query::{ItemScope, RootQuery};
use std::sync::Arc;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Dependency {
    Lifecycle,
    Ratings,
    Tags,
    Folders,
    FolderOrders,
    TagCount,
    FolderCount,
    TotalBytes,
    MediaCount,
    Width,
    Height,
    Duration,
    ImportedAt,
    CapturedAt,
    ModifiedAt,
    Notes,
    Urls,
    Mime,
    MimeFamily,
    Colors,
    Palettes,
    SmartResults,
    Sql,
}

#[derive(Debug, Clone)]
pub(crate) struct QueryVersions([u64; 23]);

impl QueryVersions {
    pub(crate) fn new(revision: u64) -> Self {
        Self([revision; 23])
    }

    pub(crate) fn advance(previous: &ProjectionSnapshot, next: &ProjectionSnapshot) -> Self {
        let mut versions = previous.query_versions.clone();
        macro_rules! track {
            ($($field:ident => $kind:ident),* $(,)?) => { $(
                if !Arc::ptr_eq(&previous.$field, &next.$field) {
                    versions.0[Dependency::$kind as usize] = next.revision;
                }
            )* };
        }
        track!(lifecycle => Lifecycle, ratings => Ratings, tags => Tags,
            folders => Folders, folder_orders => FolderOrders, tag_count => TagCount,
            folder_count => FolderCount, total_bytes => TotalBytes, media_count => MediaCount,
            width => Width, height => Height, duration => Duration, imported_at => ImportedAt,
            captured_at => CapturedAt, modified_at => ModifiedAt, notes_present => Notes,
            urls_present => Urls, mime => Mime, mime_family => MimeFamily,
            color_cells => Colors, cover_palettes => Palettes, smart_results => SmartResults);
        // SQL-only dependencies have no corresponding shared projection.
        versions.0[Dependency::Sql as usize] = next.query_versions.0[Dependency::Sql as usize];
        versions
    }

    pub(crate) fn invalidate_sql(&mut self, revision: u64) {
        self.0[Dependency::Sql as usize] = revision;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueryKey(Vec<u64>);

impl QueryKey {
    #[cfg(test)]
    pub(crate) fn test_key(version: u64) -> Self {
        Self(vec![version])
    }
    pub(crate) fn matches(snapshot: &ProjectionSnapshot, query: &RootQuery) -> Self {
        use Dependency::*;
        let mut dependencies = vec![Lifecycle, TotalBytes, MediaCount];
        match query.scope {
            ItemScope::All | ItemScope::Inbox | ItemScope::Trash => {}
            ItemScope::Untagged => dependencies.push(TagCount),
            ItemScope::Uncategorized => dependencies.push(FolderCount),
            ItemScope::Folder { .. } => dependencies.push(Folders),
            ItemScope::FolderTree { .. } => dependencies.extend([Folders, Sql]),
            ItemScope::SmartFolder { .. } => dependencies.push(SmartResults),
            ItemScope::RecentlyViewed | ItemScope::MediaMatches { .. } => dependencies.push(Sql),
        }
        filter_dependencies(&query.view.filter, &mut dependencies);
        Self(
            dependencies
                .into_iter()
                .map(|d| snapshot.query_versions.0[d as usize])
                .collect(),
        )
    }

    pub(crate) fn ordered(snapshot: &ProjectionSnapshot, query: &RootQuery) -> Self {
        use Dependency::*;
        let mut key = Self::matches(snapshot, query);
        let dependency = match query.view.sort.field {
            SortField::ImportedAt => ImportedAt,
            SortField::CapturedAt => CapturedAt,
            SortField::Rating => Ratings,
            SortField::TotalSize => TotalBytes,
            SortField::Name => Sql,
            SortField::FolderOrder if matches!(query.scope, ItemScope::Folder { .. }) => {
                FolderOrders
            }
            SortField::FolderOrder => ImportedAt,
            SortField::Random => return key,
        };
        key.0.push(snapshot.query_versions.0[dependency as usize]);
        key
    }
}

fn filter_dependencies(filter: &FilterExpr, output: &mut Vec<Dependency>) {
    use Dependency::*;
    match filter {
        FilterExpr::All(children) | FilterExpr::Any(children) => {
            for child in children {
                filter_dependencies(child, output);
            }
        }
        FilterExpr::Not(child) => filter_dependencies(child, output),
        FilterExpr::Clause(clause) => match clause {
            FilterClause::Tags { .. } => output.extend([Tags, TagCount]),
            FilterClause::Folders { .. } => output.extend([Folders, FolderCount]),
            FilterClause::Ratings { .. } => output.push(Ratings),
            FilterClause::Mime { .. } => output.extend([Mime, MimeFamily]),
            FilterClause::ImportedAt { .. } => output.push(ImportedAt),
            FilterClause::ModifiedAt { .. } => output.push(ModifiedAt),
            FilterClause::CapturedAt { .. } => output.push(CapturedAt),
            FilterClause::Width { .. } => output.push(Width),
            FilterClause::Height { .. } => output.push(Height),
            FilterClause::Duration { .. } => output.push(Duration),
            FilterClause::TotalSize { .. } => output.push(TotalBytes),
            FilterClause::NotesPresent { .. } => output.push(Notes),
            FilterClause::SourceUrlsPresent { .. } => output.push(Urls),
            FilterClause::Color { .. } => output.extend([Colors, Palettes]),
            FilterClause::Text { .. } => output.push(Sql),
        },
    }
}
