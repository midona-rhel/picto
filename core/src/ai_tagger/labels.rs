//! Label CSV parsing for WD14 / E621 tagger models.
//!
//! The `selected_tags.csv` files shipped with SmilingWolf's models have the
//! format: `tag_id,name,category,count`.  Category integers map to Picto
//! tag namespaces as follows:
//!
//! | CSV category | Picto namespace |
//! |:------------:|:---------------:|
//! | 0            | general         |
//! | 1            | artist          |
//! | 3            | copyright       |
//! | 4            | character       |
//! | 5            | species         |
//! | 9            | rating          |

use std::path::Path;

/// A single label entry parsed from the CSV.
#[derive(Debug, Clone)]
pub struct LabelEntry {
    /// The raw tag name from the CSV (e.g. `"1girl"`, `"solo"`).
    pub name: String,
    /// Picto namespace derived from the CSV category column.
    pub namespace: String,
}

/// Parse a `selected_tags.csv` file into an ordered `Vec<LabelEntry>`.
///
/// The output order matches the model's output tensor indices, so
/// `labels[i]` corresponds to `output[i]` from inference.
pub fn parse_labels_csv(path: &Path) -> Result<Vec<LabelEntry>, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read labels CSV: {e}"))?;

    let mut labels = Vec::new();
    let mut lines = content.lines();

    // Skip header line
    if let Some(header) = lines.next() {
        // Validate it looks like a header
        let lower = header.to_lowercase();
        if !lower.contains("name") && !lower.contains("tag") {
            // Not a header — treat as data
            if let Some(entry) = parse_csv_line(header) {
                labels.push(entry);
            }
        }
    }

    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(entry) = parse_csv_line(line) {
            labels.push(entry);
        }
    }

    if labels.is_empty() {
        return Err("Labels CSV contains no valid entries".into());
    }

    Ok(labels)
}

fn parse_csv_line(line: &str) -> Option<LabelEntry> {
    // Format: tag_id,name,category,count
    let parts: Vec<&str> = line.splitn(4, ',').collect();
    if parts.len() < 3 {
        return None;
    }

    let name = parts[1].trim().to_string();
    let category: u32 = parts[2].trim().parse().ok()?;
    let namespace = category_to_namespace(category).to_string();

    Some(LabelEntry { name, namespace })
}

/// Map WD14/E621 CSV category integers to Picto tag namespaces.
fn category_to_namespace(category: u32) -> &'static str {
    match category {
        0 => "general",
        1 => "artist",
        3 => "copyright",
        4 => "character",
        5 => "species",
        9 => "rating",
        _ => "general", // fallback for unknown categories
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_category_mapping() {
        assert_eq!(category_to_namespace(0), "general");
        assert_eq!(category_to_namespace(1), "artist");
        assert_eq!(category_to_namespace(3), "copyright");
        assert_eq!(category_to_namespace(4), "character");
        assert_eq!(category_to_namespace(5), "species");
        assert_eq!(category_to_namespace(9), "rating");
        assert_eq!(category_to_namespace(99), "general");
    }

    #[test]
    fn test_parse_csv_line() {
        let entry = parse_csv_line("0,1girl,0,4000000").unwrap();
        assert_eq!(entry.name, "1girl");
        assert_eq!(entry.namespace, "general");

        let entry = parse_csv_line("100,hatsune_miku,4,500000").unwrap();
        assert_eq!(entry.name, "hatsune_miku");
        assert_eq!(entry.namespace, "character");

        let entry = parse_csv_line("200,rating:general,9,100").unwrap();
        assert_eq!(entry.name, "rating:general");
        assert_eq!(entry.namespace, "rating");
    }
}
