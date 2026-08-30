use std::collections::BTreeMap;

use crate::{MediaDescriptor, MediaFallback};

pub(crate) fn is_unsupported_archive(raw: &str) -> bool {
    let path = raw
        .split(['?', '#'])
        .next()
        .unwrap_or(raw)
        .to_ascii_lowercase();
    [".rar", ".7z", ".tar", ".gz", ".bz2", ".xz", ".cbr"]
        .iter()
        .any(|extension| path.ends_with(extension))
}

#[derive(Debug, Clone)]
pub struct MediaDescriptorBuilder {
    descriptor: MediaDescriptor,
}

impl MediaDescriptorBuilder {
    pub fn new(stable_id: impl Into<String>, position: u32, url: impl Into<String>) -> Self {
        Self {
            descriptor: MediaDescriptor {
                stable_id: stable_id.into(),
                position,
                url: url.into(),
                canonical_url: None,
                file_name: None,
                mime_hint: None,
                expected_size: None,
                headers: BTreeMap::new(),
                fallbacks: Vec::new(),
                rejected_final_paths: Vec::new(),
            },
        }
    }

    pub fn canonical_url(mut self, value: impl Into<String>) -> Self {
        self.descriptor.canonical_url = Some(value.into());
        self
    }

    pub fn file_name(mut self, value: impl Into<String>) -> Self {
        let value = value.into();
        self.descriptor.mime_hint = mime_guess::from_path(&value)
            .first_raw()
            .map(ToOwned::to_owned);
        self.descriptor.file_name = Some(value);
        self
    }

    pub fn expected_size(mut self, value: Option<u64>) -> Self {
        self.descriptor.expected_size = value;
        self
    }

    pub fn headers(mut self, value: BTreeMap<String, String>) -> Self {
        self.descriptor.headers = value;
        self
    }

    pub fn fallback(mut self, value: MediaFallback) -> Self {
        self.descriptor.fallbacks.push(value);
        self
    }

    pub fn reject_final_path(mut self, value: impl Into<String>) -> Self {
        self.descriptor.rejected_final_paths.push(value.into());
        self
    }

    pub fn build(self) -> MediaDescriptor {
        self.descriptor
    }
}

#[cfg(test)]
mod tests {
    use super::is_unsupported_archive;

    #[test]
    fn provider_archive_policy_allows_zip_and_rejects_unhandled_containers() {
        assert!(!is_unsupported_archive(
            "https://cdn.test/files/post.ZIP?token=1"
        ));
        assert!(!is_unsupported_archive("comic.cbz"));
        assert!(is_unsupported_archive("bundle.rar"));
        assert!(is_unsupported_archive("bundle.7z#download"));
    }
}
