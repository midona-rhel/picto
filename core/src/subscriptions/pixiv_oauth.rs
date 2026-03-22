//! Pixiv OAuth PKCE flow — generates auth URL and exchanges code for refresh token.

use serde::Serialize;

const CLIENT_ID: &str = "MOBrBDS8blbauoSck0ZfDbtuzpyT";
const CLIENT_SECRET: &str = "lsACyCD94FhDUtGTXi3QzcFE2uU1hqtDaKeqrdwj";
const REDIRECT_URI: &str = "https://app-api.pixiv.net/web/v1/users/auth/pixiv/callback";

#[derive(Debug, Serialize)]
pub struct PixivOAuthChallenge {
    pub login_url: String,
    pub code_verifier: String,
}

/// Generate the Pixiv OAuth login URL + PKCE code verifier.
///
/// The caller should:
/// 1. Open `login_url` in the system browser
/// 2. Have the user log in and copy the `code` from the callback URL
/// 3. Call `exchange_code` with the code + code_verifier
pub fn generate_challenge() -> PixivOAuthChallenge {
    use base64::Engine;
    use sha2::Digest;

    // 32 random bytes → 43-char base64url verifier
    let random_bytes: [u8; 32] = rand::random();
    let code_verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random_bytes);

    let digest = sha2::Sha256::digest(code_verifier.as_bytes());
    let code_challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);

    let login_url = format!(
        "https://app-api.pixiv.net/web/v1/login?code_challenge={}&code_challenge_method=S256&client=pixiv-android&via=login",
        code_challenge
    );

    PixivOAuthChallenge {
        login_url,
        code_verifier,
    }
}

/// Exchange the authorization code for a refresh token.
///
/// `code` is the value from the callback URL's `code` query parameter.
/// `code_verifier` is the PKCE verifier from `generate_challenge`.
pub async fn exchange_code(code: &str, code_verifier: &str) -> Result<String, String> {
    // Extract just the code value if the user pasted a full URL or extra params
    let clean_code = code.rsplit('=').next().unwrap_or(code).trim();

    let client = reqwest::Client::new();
    let resp = client
        .post("https://oauth.secure.pixiv.net/auth/token")
        .header(
            "User-Agent",
            "PixivAndroidApp/5.0.234 (Android 11; Pixel 5)",
        )
        .form(&[
            ("client_id", CLIENT_ID),
            ("client_secret", CLIENT_SECRET),
            ("code", clean_code),
            ("code_verifier", code_verifier),
            ("grant_type", "authorization_code"),
            ("include_policy", "true"),
            ("redirect_uri", REDIRECT_URI),
        ])
        .send()
        .await
        .map_err(|e| format!("Pixiv OAuth request failed: {e}"))?;

    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Pixiv OAuth response parse error: {e}"))?;

    if let Some(err) = data.get("error").and_then(|v| v.as_str()) {
        let detail = data
            .get("errors")
            .or_else(|| data.get("message"))
            .map(|v| format!(" — {v}"))
            .unwrap_or_default();
        if err == "invalid_request" || err == "invalid_grant" {
            return Err(format!(
                "Code expired or invalid. Please try again.{detail}"
            ));
        }
        return Err(format!("Pixiv OAuth error: {err}{detail}"));
    }

    data.get("refresh_token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "No refresh_token in Pixiv OAuth response".to_string())
}
