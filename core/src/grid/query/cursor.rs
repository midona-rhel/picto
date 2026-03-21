use crate::sqlite::files::FileMetadataSlim;

pub(super) fn slim_cursor_value_for_sort(
    item: &FileMetadataSlim,
    sort_field: &str,
    random_seed: Option<i64>,
) -> Option<String> {
    let sort_val = match sort_field {
        "random" => {
            let seed = random_seed.unwrap_or(0);
            Some(
                ((item.entity_id.wrapping_mul(2654435761).wrapping_add(seed)) % 2147483647)
                    .to_string(),
            )
        }
        "position_rank" => item.position_rank.map(|r| r.to_string()),
        "date_added" => Some(item.imported_at.to_string()),
        "size" => Some(item.size.to_string()),
        "rating" => Some(item.rating.unwrap_or(0).to_string()),
        "view_count" => Some(item.view_count.to_string()),
        "name" => Some(item.name.clone().unwrap_or_default()),
        "mime" => Some(item.mime.clone()),
        _ => Some(item.imported_at.to_string()),
    };
    sort_val.map(|v| format!("{}\0{}", v, item.entity_id))
}
