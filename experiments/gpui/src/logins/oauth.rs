//! OAuth 1.0a request signing (`oauth1AuthorizationHeader`): HMAC-SHA1 over
//! the method, the address and every parameter, RFC 5849.
use hmac::{Hmac, Mac};

/// What a request is signed over.
pub struct Signing<'a> {
    pub method: &'a str,
    pub url: &'a str,
    pub params: &'a [(&'a str, &'a str)],
    pub consumer_key: &'a str,
    pub consumer_secret: &'a str,
    pub token: Option<&'a str>,
    pub token_secret: Option<&'a str>,
}

/// `oauthEncode`: RFC 3986 percent-encoding, everything but letters,
/// digits and `-._~`.
pub fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// A fresh nonce: a random UUID without its dashes, as production makes it.
pub fn nonce() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Seconds since 1970.
pub fn timestamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
        .to_string()
}

/// The `Authorization` header for `signing`, with the nonce and time given.
pub fn authorization_header(signing: &Signing, nonce: &str, timestamp: &str) -> String {
    // i18n: OAuth protocol fields
    let mut oauth: Vec<(&str, &str)> = vec![
        ("oauth_consumer_key", signing.consumer_key),
        ("oauth_nonce", nonce),
        ("oauth_signature_method", "HMAC-SHA1"),
        ("oauth_timestamp", timestamp),
        ("oauth_version", "1.0"),
    ];
    if let Some(token) = signing.token {
        oauth.push(("oauth_token", token));
    }
    // Every parameter, encoded and sorted by name then value. (Production
    // sorts with `localeCompare`, which agrees with byte order on these
    // lower-case names.)
    let mut pairs: Vec<(String, String)> = signing
        .params
        .iter()
        .chain(oauth.iter())
        .map(|(name, value)| (encode(name), encode(value)))
        .collect();
    pairs.sort();
    let parameters = pairs
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    let base = [
        signing.method.to_uppercase(),
        signing.url.to_owned(),
        parameters,
    ]
    .iter()
    .map(|part| encode(part))
    .collect::<Vec<_>>()
    .join("&");
    let key = format!(
        "{}&{}",
        encode(signing.consumer_secret),
        encode(signing.token_secret.unwrap_or_default())
    );
    let signature = {
        use base64::Engine as _;
        let mut mac = Hmac::<sha1::Sha1>::new_from_slice(key.as_bytes())
            .expect("HMAC takes a key of any length");
        mac.update(base.as_bytes());
        base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
    };
    let fields = oauth
        .iter()
        .map(|(name, value)| (*name, (*value).to_owned()))
        .chain(std::iter::once(("oauth_signature", signature)))
        .map(|(name, value)| format!("{}=\"{}\"", encode(name), encode(&value)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("OAuth {fields}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_keeps_only_unreserved_characters() {
        assert_eq!(encode("a-b._~Z9"), "a-b._~Z9");
        assert_eq!(encode("Ladies + Gentlemen"), "Ladies%20%2B%20Gentlemen");
        assert_eq!(encode("!'()*"), "%21%27%28%29%2A");
        assert_eq!(encode("é"), "%C3%A9");
    }

    /// The signature of RFC 5849 §3.4.1's example request is what any
    /// OAuth 1 library computes for it.
    #[test]
    fn a_request_is_signed_as_rfc_5849_signs_it() {
        // Twitter's documented example ("Creating a signature").
        let header = authorization_header(
            &Signing {
                method: "post",
                url: "https://api.twitter.com/1.1/statuses/update.json",
                params: &[
                    ("include_entities", "true"),
                    (
                        "status",
                        "Hello Ladies + Gentlemen, a signed OAuth request!",
                    ),
                ],
                consumer_key: "xvz1evFS4wEEPTGEFPHBog",
                consumer_secret: "kAcSOqF21Fu85e7zjz7ZN2U4ZRhfV3WpwPAoE3Z7kBw",
                token: Some("370773112-GmHxMAgYyLbNEtIKZeRNFsMKPR9EyMZeS9weJAEb"),
                token_secret: Some("LswwdoUaIvS8ltyTt5jkRh4J50vUPVVHtR2YPi5kE"),
            },
            "kYjzVBB8Y0ZFabxSWbWovY3uYSQ2pTgmZeNu2VS4cg",
            "1318622958",
        );
        assert!(header.starts_with(
            "OAuth oauth_consumer_key=\"xvz1evFS4wEEPTGEFPHBog\", oauth_nonce=\"kYjzVBB8Y0ZFabxSWbWovY3uYSQ2pTgmZeNu2VS4cg\", oauth_signature_method=\"HMAC-SHA1\", oauth_timestamp=\"1318622958\", oauth_version=\"1.0\", oauth_token=\"370773112-GmHxMAgYyLbNEtIKZeRNFsMKPR9EyMZeS9weJAEb\""
        ));
        assert!(
            header.ends_with("oauth_signature=\"hCtSmYh%2BiHYCEqBWrE7C7hYmtUk%3D\""),
            "{header}"
        );
    }

    #[test]
    fn a_nonce_is_thirty_two_hex_digits() {
        let nonce = nonce();
        assert_eq!(nonce.len(), 32);
        assert!(nonce.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
