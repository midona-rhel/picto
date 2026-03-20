use super::*;

#[test]
fn test_danbooru_adapter_tags() {
    let json = serde_json::json!({
        "id": 1,
        "category": "danbooru",
        "tags_artist": ["artist_a"],
        "tags_character": ["char_a", "char_b"],
        "tags_copyright": ["series_a"],
        "tags_general": ["tag_1", "tag_2", "tag_3"],
        "tags_meta": ["meta_1"]
    });
    let tags = parse_tags(&json);
    assert_eq!(tags.len(), 8);
    assert!(tags.contains(&("creator".to_string(), "artist_a".to_string())));
    assert!(tags.contains(&("character".to_string(), "char_a".to_string())));
    assert!(tags.contains(&("series".to_string(), "series_a".to_string())));
    assert!(tags.contains(&(String::new(), "tag_1".to_string())));
    assert!(tags.contains(&("meta".to_string(), "meta_1".to_string())));
}

#[test]
fn test_rule34_adapter_tags_from_flat_string() {
    // Rule34/Gelbooru use a flat `tags` string, not tag_string_* or tags_*
    let json = serde_json::json!({
        "id": 1,
        "category": "rule34",
        "tags": "tag_a tag_b tag_c artist_name",
        "file_url": "https://example.com/file.jpg"
    });
    let tags = parse_tags(&json);
    assert_eq!(tags.len(), 4);
    assert!(tags.contains(&(String::new(), "tag_a".to_string())));
    assert!(tags.contains(&(String::new(), "tag_b".to_string())));
    assert!(tags.contains(&(String::new(), "tag_c".to_string())));
    assert!(tags.contains(&(String::new(), "artist_name".to_string())));
}

#[test]
fn test_gelbooru_adapter_categorized_string_tags() {
    // Gelbooru with tags=true provides tags_artist etc. as space-separated strings
    let json = serde_json::json!({
        "id": 1,
        "category": "gelbooru",
        "tags": "1girl princess_peach tony_welt mario_(series)",
        "tags_artist": "tony_welt",
        "tags_character": "princess_peach",
        "tags_copyright": "mario_(series) nintendo",
        "tags_general": "1girl blonde_hair blue_eyes",
        "file_url": "https://example.com/file.jpg"
    });
    let tags = parse_tags(&json);
    assert!(tags.contains(&("creator".to_string(), "tony_welt".to_string())));
    assert!(tags.contains(&("character".to_string(), "princess_peach".to_string())));
    assert!(tags.contains(&("series".to_string(), "mario_(series)".to_string())));
    assert!(tags.contains(&("series".to_string(), "nintendo".to_string())));
    assert!(tags.contains(&(String::new(), "1girl".to_string())));
    assert!(tags.contains(&(String::new(), "blonde_hair".to_string())));
}

#[test]
fn test_gelbooru_adapter_tags_from_flat_string() {
    let json = serde_json::json!({
        "id": 1,
        "category": "gelbooru",
        "tags": "1girl solo smile",
        "file_url": "https://example.com/file.jpg"
    });
    let tags = parse_tags(&json);
    assert_eq!(tags.len(), 3);
    assert!(tags.contains(&(String::new(), "1girl".to_string())));
}

#[test]
fn test_e621_adapter_tags() {
    let json = serde_json::json!({
        "id": 1,
        "category": "e621",
        "tags": {
            "general": ["tag_a", "tag_b"],
            "artist": ["artist_a"],
            "character": ["char_a"],
            "copyright": ["series_a"],
            "species": ["species_a"],
            "meta": ["meta_a"]
        }
    });
    let tags = parse_tags(&json);
    assert_eq!(tags.len(), 7);
    assert!(tags.contains(&("creator".to_string(), "artist_a".to_string())));
    assert!(tags.contains(&("species".to_string(), "species_a".to_string())));
    assert!(tags.contains(&(String::new(), "tag_a".to_string())));
}

#[test]
fn test_pixiv_adapter_tags() {
    let json = serde_json::json!({
        "id": 1,
        "category": "pixiv",
        "tags": ["tag_a", "tag_b", "tag_c"],
        "user": { "id": 1, "name": "creator_a" }
    });
    let meta = parse_metadata(&json);
    assert_eq!(meta.tags.len(), 4); // 3 tags + 1 creator
    assert!(meta.tags.contains(&(String::new(), "tag_a".to_string())));
    assert!(meta.tags.contains(&(String::new(), "tag_b".to_string())));
    assert!(meta.tags.contains(&("creator".to_string(), "creator_a".to_string())));
}

#[test]
fn test_twitter_metadata() {
    let json = serde_json::json!({
        "tweet_id": 1234567890u64,
        "date": "2025-01-15 12:00:00",
        "content": "Hello world",
        "author": { "id": 99u64, "name": "testuser", "nick": "Test" },
        "user": { "id": 99u64, "name": "testuser", "nick": "Test" },
        "category": "twitter",
        "subcategory": "timeline",
        "type": "photo"
    });

    let meta = parse_metadata(&json);

    assert_eq!(meta.post_id.as_deref(), Some("1234567890"));
    assert_eq!(meta.description.as_deref(), Some("Hello world"));
    assert_eq!(meta.source_urls, vec!["https://x.com/testuser/status/1234567890"]);
    assert_eq!(meta.source_url.as_deref(), Some("https://x.com/testuser/status/1234567890"));
    assert!(meta.tags.contains(&("creator".to_string(), "testuser".to_string())));
    assert_eq!(meta.tags.len(), 1);
    assert!(meta.rating.is_none());
    assert!(meta.created_at.is_some());
    assert_eq!(meta.category.as_deref(), Some("twitter"));
}

#[test]
fn test_e621_adapter_full_metadata() {
    let json = serde_json::json!({
        "id": 1,
        "created_at": "2025-01-01T12:00:00.000-04:00",
        "file": {
            "url": "https://example.com/file.png"
        },
        "rating": "s",
        "sources": ["https://example.com/source1", "https://example.com/source2"],
        "date": "2025-01-01 16:00:00",
        "category": "e621",
        "tags": {
            "general": ["tag_a", "tag_b"],
            "artist": ["artist_a"],
            "contributor": [],
            "copyright": ["series_a"],
            "character": ["char_a"],
            "species": ["species_a"],
            "invalid": [],
            "meta": ["meta_a"],
            "lore": []
        }
    });

    let meta = parse_metadata(&json);

    assert_eq!(meta.post_id.as_deref(), Some("1"));
    assert_eq!(meta.rating.as_deref(), Some("s"));
    assert_eq!(meta.source_urls.len(), 3); // file.url + 2 sources
    assert!(meta.source_urls.contains(&"https://example.com/file.png".to_string()));
    assert!(meta.source_urls.contains(&"https://example.com/source1".to_string()));
    assert!(meta.created_at.is_some());

    let tags = &meta.tags;
    assert!(tags.contains(&("creator".to_string(), "artist_a".to_string())));
    assert!(tags.contains(&("character".to_string(), "char_a".to_string())));
    assert!(tags.contains(&("series".to_string(), "series_a".to_string())));
    assert!(tags.contains(&("species".to_string(), "species_a".to_string())));
    assert!(tags.contains(&("meta".to_string(), "meta_a".to_string())));
    assert!(tags.contains(&(String::new(), "tag_a".to_string())));
    // 2 general + 1 artist + 1 copyright + 1 character + 1 species + 1 meta = 7
    assert_eq!(tags.len(), 7);
}

#[test]
fn test_pixiv_adapter_object_tags() {
    // Legacy object format still supported
    let json = serde_json::json!({
        "id": 1,
        "category": "pixiv",
        "tags": [
            {"name": "tag_a", "translated_name": "translated_a"},
            {"name": "tag_b", "translated_name": null}
        ]
    });
    let tags = parse_tags(&json);
    assert_eq!(tags.len(), 2);
    assert!(tags.contains(&(String::new(), "tag_a".to_string())));
    assert!(tags.contains(&(String::new(), "tag_b".to_string())));
}

#[test]
fn test_danbooru_adapter_artist_commentary() {
    let json = serde_json::json!({
        "id": 1,
        "tag_string_artist": "artist_a",
        "tag_string_general": "tag_a tag_b",
        "artist_commentary": {
            "original_title": "Title text",
            "original_description": "Description text",
            "translated_title": "",
            "translated_description": ""
        },
        "file_url": "https://example.com/file.jpg",
        "category": "danbooru"
    });
    let meta = parse_metadata(&json);
    assert_eq!(meta.title.as_deref(), Some("Title text"));
    assert_eq!(meta.description.as_deref(), Some("Description text"));
    assert_eq!(meta.post_id.as_deref(), Some("1"));
}

#[test]
fn test_parse_metadata_artist_commentary_empty_falls_back() {
    // When artist_commentary fields are empty, fall back to direct fields
    let json = serde_json::json!({
        "id": 1,
        "artist_commentary": {
            "original_title": "",
            "original_description": ""
        },
        "description": "A direct description",
        "title": "Direct title",
        "category": "danbooru"
    });
    let meta = parse_metadata(&json);
    assert_eq!(meta.title.as_deref(), Some("Direct title"));
    assert_eq!(meta.description.as_deref(), Some("A direct description"));
}

#[test]
fn test_danbooru_adapter_collects_source_urls_and_date() {
    let json = serde_json::json!({
        "id": 1,
        "file_url": "https://example.com/file.png",
        "url": "https://example.com/post/1",
        "source": "https://example.com/original",
        "date": "2025-01-15T12:00:00+00:00",
        "category": "danbooru"
    });
    let meta = parse_metadata(&json);
    assert_eq!(meta.source_urls.len(), 3);
    assert!(meta.source_urls.contains(&"https://example.com/file.png".to_string()));
    assert!(meta.source_urls.contains(&"https://example.com/post/1".to_string()));
    assert!(meta.source_urls.contains(&"https://example.com/original".to_string()));
    assert!(meta.created_at.is_some());
}

#[test]
fn test_substitute_query() {
    assert_eq!(
        substitute_query(
            "https://danbooru.donmai.us/posts?tags={query}",
            "1girl solo"
        ),
        "https://danbooru.donmai.us/posts?tags=1girl+solo"
    );
    assert_eq!(
        substitute_query(
            "https://e621.net/posts?tags={query}",
            "rating:safe order:score"
        ),
        "https://e621.net/posts?tags=rating%3Asafe+order%3Ascore"
    );
}

#[test]
fn test_extract_domain() {
    assert_eq!(
        extract_domain("https://danbooru.donmai.us/posts?tags=1girl"),
        Some("danbooru.donmai.us".to_string())
    );
    assert_eq!(
        extract_domain("https://www.pixiv.net/artworks/12345"),
        Some("www.pixiv.net".to_string())
    );
    assert_eq!(extract_domain("not-a-url"), None);
}

#[test]
fn test_site_by_id() {
    let dan = site_by_id("danbooru").unwrap();
    assert_eq!(dan.name, "Danbooru");
    assert!(dan.url_template.contains("{query}"));

    assert!(site_by_id("nonexistent_site_xyz").is_none());
    assert_eq!(site_by_id("rule34xxx").unwrap().id, "rule34");
    assert_eq!(canonical_site_id("rule34xxx"), "rule34");
}

#[test]
fn test_build_url() {
    assert_eq!(
        build_url("danbooru", "1girl solo").unwrap(),
        "https://danbooru.donmai.us/posts?tags=1girl+solo"
    );
    assert_eq!(
        build_url("e621", "canine rating:safe").unwrap(),
        "https://e621.net/posts?tags=canine+rating%3Asafe"
    );
    assert_eq!(
        build_url("pixiv", "風景").unwrap(),
        "https://www.pixiv.net/en/tags/%E9%A2%A8%E6%99%AF/artworks?s_mode=s_tag"
    );
    assert!(build_url("nonexistent", "query").is_none());
}

#[test]
fn test_classify_failure_unauthorized() {
    let kind = classify_failure("HTTP Error 403 Forbidden: Login required");
    assert_eq!(kind, FailureKind::Unauthorized);
}

#[test]
fn test_site_capability_contract_representative_matrix() {
    let pixiv = site_by_id("pixiv").expect("pixiv");
    assert!(pixiv.supports_query);
    assert!(pixiv.supports_account);
    assert!(pixiv.auth_supported);
    assert!(pixiv.auth_required_for_full_access);

    let tumblr = site_by_id("tumblr").expect("tumblr");
    assert!(tumblr.supports_query);
    assert!(tumblr.supports_account);
    assert!(!tumblr.auth_supported);
    assert!(!tumblr.auth_required_for_full_access);

    let patreon = site_by_id("patreon").expect("patreon");
    assert!(!patreon.supports_query);
    assert!(patreon.supports_account);
    assert!(patreon.auth_supported);
}

#[test]
fn test_site_contract_auth_required_implies_auth_supported() {
    for site in SITES {
        assert!(
            !site.auth_required_for_full_access || site.auth_supported,
            "site {} requires auth for full access but is marked auth unsupported",
            site.id
        );
    }
}

#[test]
fn test_build_url_contract_for_query_and_account_templates() {
    assert_eq!(
        build_url("patreon", "creatorname").as_deref(),
        Some("https://www.patreon.com/creatorname/posts")
    );
    assert_eq!(
        build_url("tumblr", "myblog").as_deref(),
        Some("https://myblog.tumblr.com")
    );
    assert_eq!(
        build_url("rule34xxx", "solo").as_deref(),
        Some("https://rule34.xxx/index.php?page=post&s=list&tags=solo")
    );
}

#[test]
fn test_parse_metadata_extracts_pixiv_creator_tag() {
    let json = serde_json::json!({
        "id": 100,
        "title": "Pixiv work",
        "url": "https://www.pixiv.net/artworks/100",
        "tags": [{"name":"landscape","translated_name":null}],
        "user": {"id": 77, "name": "artist_name"},
        "page_count": 1,
        "category": "pixiv"
    });
    let meta = parse_metadata(&json);
    assert!(
        meta.tags
            .iter()
            .any(|(ns, subtag)| ns == "creator" && subtag == "artist_name")
    );
}

#[test]
fn test_validate_site_metadata_pixiv_valid_payload() {
    let json = serde_json::json!({
        "id": 123,
        "title": "Pixiv title",
        "caption": "Pixiv caption",
        "url": "https://www.pixiv.net/artworks/123",
        "tags": [{"name":"tag_a","translated_name":null}],
        "user": {"id": 55, "name": "pixiv_user"},
        "page_count": 3,
        "category": "pixiv"
    });
    let res = validate_site_metadata("pixiv", "https://www.pixiv.net/artworks/123", Some(&json));
    assert!(res.valid, "validation errors: {:?}", res.invalid_fields);
    assert!(res.missing_required_fields.is_empty());
    assert!(res.invalid_fields.is_empty());
    assert!(res.normalized_preview.is_some());
}

#[test]
fn test_validate_site_metadata_pixiv_missing_required_keys() {
    let json = serde_json::json!({
        "id": 123,
        "tags": [],
        "user": {},
        "category": "pixiv"
    });
    let res = validate_site_metadata("pixiv", "", Some(&json));
    assert!(!res.valid);
    assert!(
        res.missing_required_fields
            .contains(&"title|caption".to_string())
    );
    assert!(
        res.missing_required_fields
            .contains(&"page_count|meta_pages".to_string())
    );
    assert!(
        res.missing_required_fields
            .contains(&"url|file_url".to_string())
    );
}

#[test]
fn test_validate_site_metadata_gelbooru_valid_payload() {
    let json = serde_json::json!({
        "id": 42,
        "tag_string": "1girl smile",
        "file_url": "https://img3.gelbooru.com/images/a/b/example.jpg",
        "source": "https://twitter.com/example/status/1",
        "rating": "safe",
        "md5": "0123456789abcdef0123456789abcdef",
        "category": "gelbooru"
    });
    let res = validate_site_metadata(
        "gelbooru",
        "https://gelbooru.com/index.php?page=post&s=view&id=42",
        Some(&json),
    );
    assert!(res.valid, "validation errors: {:?}", res.invalid_fields);
    assert!(res.missing_required_fields.is_empty());
    assert!(res.invalid_fields.is_empty());
    assert!(res.normalized_preview.is_some());
}

#[test]
fn test_get_site_metadata_schema_gelbooru() {
    let schema = get_site_metadata_schema("gelbooru").expect("gelbooru schema");
    assert_eq!(schema.site_id, "gelbooru");
    assert!(
        schema
            .required_raw_keys
            .iter()
            .any(|k| k == "tags|tag_string"),
        "schema should accept tags or tag_string"
    );
}

#[test]
fn test_validate_site_metadata_gelbooru_missing_required_keys() {
    let json = serde_json::json!({
        "id": 42,
        "tag_string": "",
        "rating": "safe",
        "category": "gelbooru"
    });
    let res = validate_site_metadata("gelbooru", "", Some(&json));
    assert!(!res.valid);
    assert!(
        res.missing_required_fields
            .contains(&"file_url".to_string())
    );
    assert!(res.missing_required_fields.contains(&"source".to_string()));
    assert!(res.invalid_fields.contains(&"tags[]".to_string()));
}

#[test]
fn test_get_site_metadata_schema_danbooru() {
    let schema = get_site_metadata_schema("danbooru").expect("danbooru schema");
    assert_eq!(schema.site_id, "danbooru");
    assert!(
        schema
            .required_raw_keys
            .iter()
            .any(|k| k == "tags_artist|tags_general|category_tags"),
        "schema should require category tags"
    );
}

#[test]
fn test_validate_site_metadata_danbooru_valid_payload() {
    let json = serde_json::json!({
        "id": 10873290,
        "tags_artist": ["h4sh1rnoto"],
        "tags_character": ["princess_peach"],
        "tags_copyright": ["mario_(series)"],
        "tags_general": ["1girl", "blonde_hair"],
        "tags_meta": ["highres"],
        "artist_commentary": {
            "original_title": "ピーチ姫",
            "original_description": "マリオシリーズ"
        },
        "file_url": "https://cdn.donmai.us/original/test.jpg",
        "source": "https://x.com/example/status/1",
        "rating": "s",
        "category": "danbooru"
    });
    let res = validate_site_metadata(
        "danbooru",
        "https://danbooru.donmai.us/posts/10873290",
        Some(&json),
    );
    assert!(res.valid, "validation errors: {:?}", res.invalid_fields);
    assert!(res.missing_required_fields.is_empty());
    assert!(res.invalid_fields.is_empty());
    assert!(res.normalized_preview.is_some());
}

#[test]
fn test_validate_site_metadata_danbooru_missing_required_keys() {
    let json = serde_json::json!({
        "id": 10873290,
        "tags_general": ["1girl"],
        "file_url": "https://cdn.donmai.us/original/test.jpg",
        "category": "danbooru"
    });
    let res = validate_site_metadata("danbooru", "", Some(&json));
    assert!(!res.valid);
    assert!(res.missing_required_fields.contains(&"source".to_string()));
    assert!(res.missing_required_fields.contains(&"rating".to_string()));
    assert!(res.invalid_fields.contains(&"creator".to_string()));
}
