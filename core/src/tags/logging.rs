use serde::Serialize;

use super::normalize;

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct TagSummary {
    pub total: usize,
    pub creator: usize,
    pub character: usize,
    pub series: usize,
    pub meta: usize,
    pub general: usize,
    pub other_namespaced: usize,
}

impl TagSummary {
    pub fn namespaced_count(self) -> usize {
        self.total.saturating_sub(self.general)
    }

    fn add_namespace(&mut self, namespace: &str) {
        self.total += 1;
        match namespace {
            "" | "general" => self.general += 1,
            "creator" => self.creator += 1,
            "character" => self.character += 1,
            "series" => self.series += 1,
            "meta" => self.meta += 1,
            _ => self.other_namespaced += 1,
        }
    }
}

pub fn summarize_tag_pairs(tags: &[(String, String)]) -> TagSummary {
    let mut summary = TagSummary::default();
    for (namespace, _) in tags {
        summary.add_namespace(namespace);
    }
    summary
}

pub fn summarize_tag_strings(tags: &[String]) -> TagSummary {
    let mut summary = TagSummary::default();
    for tag in tags {
        if let Some((namespace, _)) = normalize::parse_tag_ingest(tag) {
            summary.add_namespace(&namespace);
        }
    }
    summary
}

pub fn preview_tag_strings(tags: &[String], limit: usize) -> Vec<String> {
    tags.iter().take(limit).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::{preview_tag_strings, summarize_tag_pairs, summarize_tag_strings};

    #[test]
    fn summarize_tag_strings_counts_namespaces() {
        let tags = vec![
            "creator:huffslove".to_string(),
            "character:tiefling".to_string(),
            "series:original".to_string(),
            "meta:highres".to_string(),
            "solo".to_string(),
            "species:dragon".to_string(),
        ];
        let summary = summarize_tag_strings(&tags);
        assert_eq!(summary.total, 6);
        assert_eq!(summary.creator, 1);
        assert_eq!(summary.character, 1);
        assert_eq!(summary.series, 1);
        assert_eq!(summary.meta, 1);
        assert_eq!(summary.general, 1);
        assert_eq!(summary.other_namespaced, 1);
        assert_eq!(summary.namespaced_count(), 5);
        assert_eq!(preview_tag_strings(&tags, 3).len(), 3);
    }

    #[test]
    fn summarize_tag_pairs_counts_namespaces() {
        let tags = vec![
            ("creator".to_string(), "huffslove".to_string()),
            ("character".to_string(), "tiefling".to_string()),
            ("".to_string(), "solo".to_string()),
        ];
        let summary = summarize_tag_pairs(&tags);
        assert_eq!(summary.total, 3);
        assert_eq!(summary.creator, 1);
        assert_eq!(summary.character, 1);
        assert_eq!(summary.general, 1);
    }
}
