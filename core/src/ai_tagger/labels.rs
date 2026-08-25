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
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("Failed to read labels CSV: {e}"))?;

    let mut labels = Vec::new();
    let mut lines = content.lines().enumerate();

    // Skip header line
    if let Some((line_number, header)) = lines.next() {
        let fields = parse_csv_fields(header)
            .map_err(|e| format!("Invalid labels CSV on line {}: {e}", line_number + 1))?;
        let is_header = fields
            .iter()
            .any(|field| field.eq_ignore_ascii_case("name"))
            && fields
                .iter()
                .any(|field| field.eq_ignore_ascii_case("category"));
        if !is_header {
            labels.push(
                parse_csv_line(header)
                    .map_err(|e| format!("Invalid labels CSV on line {}: {e}", line_number + 1))?,
            );
        }
    }

    for (line_number, line) in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        labels.push(
            parse_csv_line(line)
                .map_err(|e| format!("Invalid labels CSV on line {}: {e}", line_number + 1))?,
        );
    }

    if labels.is_empty() {
        return Err("Labels CSV contains no valid entries".into());
    }

    Ok(labels)
}

fn parse_csv_line(line: &str) -> Result<LabelEntry, String> {
    // Format: tag_id,name,category,count
    let parts = parse_csv_fields(line)?;
    if parts.len() < 3 {
        return Err("expected at least tag_id, name, and category columns".into());
    }

    let raw_name = parts[1].trim();
    if raw_name.is_empty() {
        return Err("tag name is empty".into());
    }
    let category: u32 = parts[2]
        .trim()
        .parse()
        .map_err(|_| "category is not an unsigned integer".to_string())?;
    let namespace = category_to_namespace(category).to_string();
    let name = raw_name
        .strip_prefix(&format!("{namespace}:"))
        .unwrap_or(raw_name)
        .to_string();

    Ok(LabelEntry { name, namespace })
}

fn parse_csv_fields(line: &str) -> Result<Vec<String>, String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut chars = line.chars().peekable();
    let mut quoted = false;

    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' if field.is_empty() && !quoted => quoted = true,
            '"' if quoted => quoted = false,
            ',' if !quoted => {
                fields.push(std::mem::take(&mut field));
            }
            _ => field.push(ch),
        }
    }

    if quoted {
        return Err("unterminated quoted field".into());
    }
    fields.push(field);
    Ok(fields)
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
        assert_eq!(entry.name, "general");
        assert_eq!(entry.namespace, "rating");
    }

    #[test]
    fn parse_labels_csv_accepts_quoted_names_and_rejects_bad_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("labels.csv");
        std::fs::write(
            &path,
            "tag_id,name,category,count\n0,\"tag,with comma\",0,1\n",
        )
        .unwrap();

        let labels = parse_labels_csv(&path).unwrap();
        assert_eq!(labels[0].name, "tag,with comma");

        std::fs::write(&path, "tag_id,name,category,count\n0,broken,nope,1\n").unwrap();
        assert!(parse_labels_csv(&path).is_err());
    }
}
