//! Which of a web view's cookies a page at an address is sent, as
//! Electron's `session.cookies.get({ url })` answers (RFC 6265: the domain,
//! the path, and `Secure` only over HTTPS or to the loopback).

/// A cookie as a web view's store holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    /// The domain as stored: `.example.com` for a cookie its domain's
    /// subdomains share, `example.com` for one only that host is sent.
    pub domain: String,
    pub path: String,
    pub secure: bool,
}

/// The cookies of `all` that a request to `url` carries, in their order.
pub fn sent_to(all: &[Cookie], url: &str) -> Vec<Cookie> {
    let Ok(url) = url::Url::parse(url) else {
        return Vec::new();
    };
    let Some(host) = url.host_str().map(str::to_ascii_lowercase) else {
        return Vec::new();
    };
    let loopback = host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    let path = url.path();
    all.iter()
        .filter(|cookie| {
            let domain = cookie.domain.to_ascii_lowercase();
            let domain_matches = match domain.strip_prefix('.') {
                Some(shared) => host == shared || host.ends_with(&format!(".{shared}")),
                None => host == domain,
            };
            domain_matches
                && path_matches(&cookie.path, path)
                && (!cookie.secure || url.scheme() == "https" || loopback)
        })
        .cloned()
        .collect()
}

/// RFC 6265 §5.1.4: the request's path is the cookie's or below it.
fn path_matches(cookie: &str, request: &str) -> bool {
    let cookie = if cookie.is_empty() { "/" } else { cookie };
    request == cookie
        || (request.starts_with(cookie)
            && (cookie.ends_with('/') || request[cookie.len()..].starts_with('/')))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cookie(name: &str, domain: &str, path: &str, secure: bool) -> Cookie {
        Cookie {
            name: name.into(),
            value: format!("{name}-value"),
            domain: domain.into(),
            path: path.into(),
            secure,
        }
    }

    fn names(cookies: Vec<Cookie>) -> Vec<String> {
        cookies.into_iter().map(|cookie| cookie.name).collect()
    }

    #[test]
    fn a_page_is_sent_its_hosts_cookies_and_its_parent_domains() {
        let all = [
            cookie("shared", ".fanbox.cc", "/", true),
            cookie("api", "api.fanbox.cc", "/", true),
            cookie("www", "www.fanbox.cc", "/", true),
            cookie("other", ".pixiv.net", "/", true),
            cookie("lookalike", "evilfanbox.cc", "/", false),
        ];
        assert_eq!(
            names(sent_to(&all, "https://api.fanbox.cc")),
            ["shared", "api"]
        );
        assert_eq!(names(sent_to(&all, "https://fanbox.cc/")), ["shared"]);
    }

    #[test]
    fn a_secure_cookie_goes_only_over_https_or_to_the_loopback() {
        let all = [cookie("secure", "example.com", "/", true)];
        assert!(sent_to(&all, "http://example.com/").is_empty());
        assert_eq!(sent_to(&all, "https://example.com/").len(), 1);
        let local = [cookie("secure", "127.0.0.1", "/", true)];
        assert_eq!(sent_to(&local, "http://127.0.0.1:8123/").len(), 1);
    }

    #[test]
    fn a_cookie_of_a_path_goes_only_below_it() {
        let all = [
            cookie("root", "example.com", "/", false),
            cookie("account", "example.com", "/account", false),
        ];
        assert_eq!(names(sent_to(&all, "https://example.com")), ["root"]);
        assert_eq!(
            names(sent_to(&all, "https://example.com/account/home")),
            ["root", "account"]
        );
        assert_eq!(
            names(sent_to(&all, "https://example.com/accounting")),
            ["root"]
        );
    }
}
