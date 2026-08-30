use crate::NativeSourceAdapter;

use super::sankaku::{adapter_for, SankakuConfig};

const CONFIG: SankakuConfig = SankakuConfig {
    id: "idolcomplex",
    display_name: "Idol Complex",
    domain: "idolcomplex.com",
    root: "https://www.idolcomplex.com/en",
    api_root: "https://i.sankakuapi.com",
};

pub(crate) fn adapter() -> impl NativeSourceAdapter {
    adapter_for(CONFIG)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::{CanonicalTag, DiscoveryRequest, SourcePartition};

    #[test]
    fn uses_idol_complex_api_identity_and_canonical_urls() {
        let descriptor = adapter().descriptor();
        assert_eq!(descriptor.id, "idolcomplex");
        assert_eq!(descriptor.domain, "idolcomplex.com");

        let request = DiscoveryRequest {
            query: "solo".into(),
            partition: SourcePartition::new("posts"),
            cursor: Some("idol-next".into()),
            page_size: 10,
        };
        let url = super::super::sankaku::request_url(CONFIG, &request).unwrap();
        assert_eq!(url.host_str(), Some("i.sankakuapi.com"));
        let query = url.query_pairs().collect::<BTreeMap<_, _>>();
        assert_eq!(
            query.get("next").map(|value| value.as_ref()),
            Some("idol-next")
        );

        let response = serde_json::from_value(json!({
            "data": [{
                "id": "60rvNVpQr3A",
                "rating": "e",
                "file_url": "https://iv.sankakucomplex.com/data/example.jpg",
                "file_type": "jpg",
                "tag_names": ["solo"]
            }],
            "meta": {"next": "next-idol-page"}
        }))
        .unwrap();
        let batch = super::super::sankaku::normalize_page(CONFIG, &request, response).unwrap();
        assert_eq!(
            batch.posts[0].canonical_url.as_deref(),
            Some("https://www.idolcomplex.com/en/posts/60rvNVpQr3A")
        );
        assert!(batch.posts[0]
            .tags
            .contains(&CanonicalTag::new("rating", "explicit")));
    }

    #[test]
    fn rejects_idol_complex_traversal_controls() {
        let adapter = adapter();
        assert!(adapter.validate_query("solo").is_ok());
        assert!(adapter.validate_query("solo id_range:123").is_err());
        assert!(adapter.validate_query("solo page:2").is_err());
    }
}
