//! Makes an OnlyFans login from the values of a session that the user
//! copied out of a browser and pasted into the Accounts dialog: the `Cookie`
//! header, the browser's `User-Agent`, and the `X-BC` header that the
//! requests to OnlyFans' API carry.
use super::Credential;
use std::collections::BTreeMap;

/// The names of the OnlyFans cookies that are kept as the login.
const COOKIE_NAMES: [&str; 3] = ["sess", "auth_id", "auth_uid"];

/// The three values that the user pastes into the form.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ManualInput {
    pub cookie: String,
    pub user_agent: String,
    pub x_bc: String,
}

/// Returns the cookies named in [`COOKIE_NAMES`] that the `Cookie` header
/// `value` holds with a value that is not empty.
pub fn parse_cookie_header(value: &str) -> BTreeMap<String, String> {
    value
        .split(';')
        .filter_map(|segment| {
            let (name, value) = segment.split_once('=')?;
            let (name, value) = (name.trim(), value.trim());
            (!name.is_empty() && COOKIE_NAMES.contains(&name) && !value.is_empty())
                .then(|| (name.to_owned(), value.to_owned()))
        })
        .collect()
}

/// Returns the credential made from the pasted values. Returns an error
/// that names what is missing if the cookies `sess` or `auth_id`, the
/// User-Agent or the X-BC value is absent.
pub fn manual_credential(input: &ManualInput) -> Result<Credential, String> {
    let cookies = parse_cookie_header(&input.cookie);
    let missing: Vec<&str> = ["sess", "auth_id"]
        .into_iter()
        .filter(|name| !cookies.contains_key(*name))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "OnlyFans Cookie is missing {}.",
            missing.join(" and ")
        ));
    }
    let user_agent = input.user_agent.trim();
    let x_bc = input.x_bc.trim();
    if user_agent.is_empty() {
        return Err("OnlyFans User-Agent is required.".into());
    }
    if x_bc.is_empty() {
        return Err("OnlyFans X-BC is required.".into());
    }
    Ok(Credential {
        site_category: "onlyfans".into(),
        credential_type: "cookies".into(),
        cookies: Some(cookies),
        headers: Some(BTreeMap::from([
            ("user-agent".to_owned(), user_agent.to_owned()),
            ("x-bc".to_owned(), x_bc.to_owned()),
        ])),
        ..Credential::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Of the pasted `Cookie` header only the three session cookies are
    // kept, and the two headers are kept under lower-case names.
    #[test]
    fn a_pasted_session_keeps_only_its_session_cookies_and_headers() {
        let credential = manual_credential(&ManualInput {
            cookie: "analytics=ignored; sess=session; auth_id=42; auth_uid=42".into(),
            user_agent: "Chrome browser".into(),
            x_bc: "signature".into(),
        })
        .unwrap();
        assert_eq!(credential.site_category, "onlyfans");
        assert_eq!(credential.credential_type, "cookies");
        assert_eq!(
            credential.cookies,
            Some(BTreeMap::from([
                ("auth_id".into(), "42".into()),
                ("auth_uid".into(), "42".into()),
                ("sess".into(), "session".into())
            ]))
        );
        assert_eq!(
            credential.headers,
            Some(BTreeMap::from([
                ("user-agent".into(), "Chrome browser".into()),
                ("x-bc".into(), "signature".into())
            ]))
        );
    }

    #[test]
    fn a_pasted_session_says_what_it_is_missing() {
        let input = |cookie: &str, user_agent: &str, x_bc: &str| ManualInput {
            cookie: cookie.into(),
            user_agent: user_agent.into(),
            x_bc: x_bc.into(),
        };
        assert_eq!(
            manual_credential(&input("other=1", "UA", "X")).unwrap_err(),
            "OnlyFans Cookie is missing sess and auth_id."
        );
        assert_eq!(
            manual_credential(&input("sess=1", "UA", "X")).unwrap_err(),
            "OnlyFans Cookie is missing auth_id."
        );
        assert_eq!(
            manual_credential(&input("sess=1; auth_id=2", " ", "X")).unwrap_err(),
            "OnlyFans User-Agent is required."
        );
        assert_eq!(
            manual_credential(&input("sess=1; auth_id=2", "UA", "")).unwrap_err(),
            "OnlyFans X-BC is required."
        );
        assert!(parse_cookie_header("=x; sess=; auth_id = 7 ").contains_key("auth_id"));
        assert!(!parse_cookie_header("sess=").contains_key("sess"));
    }
}
