use std::collections::BTreeMap;

use crate::MediaDescriptor;

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

    pub fn build(self) -> MediaDescriptor {
        self.descriptor
    }
}
