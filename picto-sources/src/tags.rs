use std::collections::{BTreeMap, BTreeSet};

use crate::CanonicalTag;

#[derive(Debug, Clone, Default)]
pub struct CanonicalTagSet(BTreeSet<(String, String)>);

impl CanonicalTagSet {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn insert(&mut self, namespace: impl Into<String>, value: impl AsRef<str>) {
        let value = value.as_ref().trim();
        if !value.is_empty() {
            self.0.insert((namespace.into(), value.to_string()));
        }
    }

    pub fn extend_values<'a>(
        &mut self,
        namespace: impl Into<String>,
        values: impl IntoIterator<Item = &'a String>,
    ) {
        let namespace = namespace.into();
        for value in values {
            self.insert(namespace.clone(), value);
        }
    }

    pub fn into_vec(self) -> Vec<CanonicalTag> {
        self.0
            .into_iter()
            .map(|(namespace, value)| CanonicalTag::new(namespace, value))
            .collect()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct NamespaceMap {
    mappings: &'static [(&'static str, &'static str)],
}

impl NamespaceMap {
    pub const fn new(mappings: &'static [(&'static str, &'static str)]) -> Self {
        Self { mappings }
    }

    pub fn normalize(&self, groups: &BTreeMap<String, Vec<String>>) -> CanonicalTagSet {
        let mut tags = CanonicalTagSet::default();
        for (group, values) in groups {
            let namespace = self
                .mappings
                .iter()
                .find_map(|(source, target)| (*source == group).then_some(*target))
                .unwrap_or("");
            tags.extend_values(namespace, values);
        }
        tags
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RatingMap {
    mappings: &'static [(&'static str, &'static str)],
}

impl RatingMap {
    pub const fn new(mappings: &'static [(&'static str, &'static str)]) -> Self {
        Self { mappings }
    }

    pub fn add(&self, tags: &mut CanonicalTagSet, rating: Option<&str>) {
        let Some(rating) = rating.map(str::trim) else {
            return;
        };
        if let Some((_, normalized)) = self
            .mappings
            .iter()
            .find(|(source, _)| source.eq_ignore_ascii_case(rating))
        {
            tags.insert("rating", *normalized);
        }
    }
}
