//! Selection summary.

use chrono::Utc;
use roaring::RoaringBitmap;
use std::collections::{BTreeSet, HashMap, HashSet};

use crate::db::types::{EntityGridItem, EntityTarget, QueryPage};
use crate::selection::helpers::RatingStats;
use crate::types::{
    SelectionFolderInfo, SelectionSummary, SelectionSummaryStats, SelectionTagCount,
};

use super::{target, ApplicationEngine};

impl ApplicationEngine {
    pub async fn get_selection_summary(
        &self,
        target: EntityTarget,
    ) -> Result<SelectionSummary, String> {
        let resolved = target::resolve(&self.db, &target)?;
        match resolved {
            target::ResolvedTarget::Ids(ids) => {
                let hashes = self.db.get_entity_hashes_by_ids(&ids)?;
                let items = self.db.get_entity_grid_items(&hashes)?;
                self.build_selection_summary(items.len() as i64, items)
            }
            target::ResolvedTarget::Query {
                mut view_query,
                exclusions,
            } => {
                view_query.page = QueryPage {
                    limit: i64::MAX,
                    cursor: None,
                };
                let page = self.db.query_entity_view(&view_query)?;
                let total_count = page.total_count.unwrap_or(page.items.len() as i64);
                let excluded: HashSet<&str> = exclusions.iter().map(String::as_str).collect();
                let items = page
                    .items
                    .into_iter()
                    .filter(|item| !excluded.contains(item.entity_hash.as_str()))
                    .collect::<Vec<_>>();
                self.build_selection_summary(total_count, items)
            }
        }
    }

    fn build_selection_summary(
        &self,
        total_count: i64,
        mut items: Vec<EntityGridItem>,
    ) -> Result<SelectionSummary, String> {
        let bitmap = RoaringBitmap::from_iter(items.iter().map(|item| item.entity_id as u32));
        let selected_count = bitmap.len() as i64;

        items.sort_by(|a, b| b.date_added.cmp(&a.date_added));
        let sample_hashes = items
            .iter()
            .take(10)
            .map(|item| item.entity_hash.clone())
            .collect::<Vec<_>>();

        let (shared_tags, top_tags) = self.summarize_tags_from_bitmap(&bitmap)?;
        let shared_folders = self.summarize_folders_from_bitmap(&bitmap)?;
        let (total_size_bytes, mime_counts, rating_stats) = summarize_entity_items(&items);

        Ok(SelectionSummary {
            total_count,
            selected_count,
            sample_hashes,
            shared_tags,
            top_tags,
            shared_folders,
            stats: SelectionSummaryStats {
                total_size_bytes: Some(total_size_bytes),
                mime_counts: Some(mime_counts),
                rating_stats: Some(serde_json::json!({
                    "min": rating_stats.min,
                    "max": rating_stats.max,
                    "shared": rating_stats.shared,
                })),
            },
            pending: false,
            generated_at: Utc::now().to_rfc3339(),
        })
    }

    fn summarize_tags_from_bitmap(
        &self,
        selected_bitmap: &RoaringBitmap,
    ) -> Result<(Vec<SelectionTagCount>, Vec<SelectionTagCount>), String> {
        let selected_count = selected_bitmap.len() as i64;
        if selected_count <= 0 {
            return Ok((Vec::new(), Vec::new()));
        }

        let all_counts = self.db.get_all_tags_with_counts()?;
        let mut top = Vec::new();
        let mut shared = Vec::new();

        for tag in all_counts {
            let mut bitmap =
                self.db
                    .bitmaps
                    .get(&crate::db::projection::bitmaps::BitmapKey::EffectiveTag(
                        tag.tag_id,
                    ));
            if bitmap.is_empty() {
                continue;
            }
            bitmap &= selected_bitmap;
            let count = bitmap.len() as i64;
            if count <= 0 {
                continue;
            }
            let tag_str = crate::types::tag_display_key(&tag.namespace, &tag.subtag);
            if count == selected_count {
                shared.push(SelectionTagCount {
                    tag: tag_str.clone(),
                    count,
                });
            }
            top.push(SelectionTagCount {
                tag: tag_str,
                count,
            });
        }

        top.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.tag.cmp(&b.tag)));
        top.truncate(30);
        shared.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.tag.cmp(&b.tag)));
        shared.truncate(30);
        Ok((shared, top))
    }

    fn summarize_folders_from_bitmap(
        &self,
        selected_bitmap: &RoaringBitmap,
    ) -> Result<Vec<SelectionFolderInfo>, String> {
        let selected_count = selected_bitmap.len() as u64;
        if selected_count == 0 {
            return Ok(Vec::new());
        }

        let all_folders = self
            .db
            .get_sidebar_tree()?
            .into_iter()
            .filter(|node| node.kind == "folder")
            .filter_map(|node| {
                node.node_id
                    .strip_prefix("folder:")
                    .and_then(|id| id.parse::<i64>().ok())
                    .map(|folder_id| (folder_id, node.name))
            })
            .collect::<Vec<_>>();

        let mut shared = Vec::new();
        for (folder_id, name) in all_folders {
            let mut bitmap =
                self.db
                    .bitmaps
                    .get(&crate::db::projection::bitmaps::BitmapKey::Folder(
                        folder_id,
                    ));
            if bitmap.is_empty() {
                continue;
            }
            bitmap &= selected_bitmap;
            if bitmap.len() as u64 == selected_count {
                shared.push(SelectionFolderInfo { folder_id, name });
            }
        }

        Ok(shared)
    }
}

fn summarize_entity_items(items: &[EntityGridItem]) -> (i64, HashMap<String, i64>, RatingStats) {
    let mut total_size_bytes = 0_i64;
    let mut mime_counts = HashMap::new();
    let mut min_rating: Option<i64> = None;
    let mut max_rating: Option<i64> = None;
    let mut distinct_ratings = BTreeSet::new();

    for item in items {
        total_size_bytes = total_size_bytes.saturating_add(item.size_bytes);
        *mime_counts.entry(item.mime_type.clone()).or_insert(0) += 1;
        let rating = item.rating.unwrap_or(0);
        min_rating = Some(min_rating.map_or(rating, |current| current.min(rating)));
        max_rating = Some(max_rating.map_or(rating, |current| current.max(rating)));
        distinct_ratings.insert(rating);
    }

    let shared = if distinct_ratings.len() == 1 {
        distinct_ratings.iter().next().copied()
    } else {
        None
    };

    (
        total_size_bytes,
        mime_counts,
        RatingStats {
            min: min_rating,
            max: max_rating,
            shared,
        },
    )
}
