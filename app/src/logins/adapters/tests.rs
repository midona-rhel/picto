use super::*;
use crate::logins::sites::{resolve, with_service_host};
use futures::executor::block_on;

/// A recorded page: its address, the cookies its store holds by the
/// address asked for, and what its scripts answer.
#[derive(Default)]
pub struct FakePage {
    pub url: RefCell<String>,
    pub cookies: RefCell<Vec<(String, String)>>,
    /// Answers by a fragment of the script asked for.
    pub scripts: RefCell<Vec<(&'static str, Value)>>,
    pub asked: RefCell<Vec<String>>,
}

impl FakePage {
    pub fn at(url: &str) -> Self {
        Self {
            url: RefCell::new(url.to_owned()),
            ..Self::default()
        }
    }
    pub fn with_cookies(self, cookies: &[(&str, &str)]) -> Self {
        *self.cookies.borrow_mut() = cookies
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        self
    }
    pub fn answering(self, fragment: &'static str, value: Value) -> Self {
        self.scripts.borrow_mut().push((fragment, value));
        self
    }
}

impl Page for FakePage {
    fn current_url(&self) -> String {
        self.url.borrow().clone()
    }
    async fn cookies(&self, url: &str) -> Vec<Cookie> {
        self.asked.borrow_mut().push(url.to_owned());
        self.cookies
            .borrow()
            .iter()
            .map(|(name, value)| Cookie {
                name: name.clone(),
                value: value.clone(),
                domain: String::new(),
                path: "/".into(),
                secure: false,
            })
            .collect()
    }
    async fn eval(&self, script: &str) -> Option<Value> {
        self.scripts
            .borrow()
            .iter()
            .find(|(fragment, _)| script.contains(fragment))
            .map(|(_, value)| value.clone())
    }
}

/// Recorded answers to the network and the application.
#[derive(Default)]
pub struct FakeServices {
    pub posts: RefCell<Vec<HttpRequest>>,
    pub answers: RefCell<Vec<HttpResponse>>,
    pub commands: RefCell<Vec<(String, Value)>>,
    pub command_answers: RefCell<Vec<Value>>,
}

impl Services for FakeServices {
    async fn post(&self, request: HttpRequest) -> Result<HttpResponse, String> {
        self.posts.borrow_mut().push(request);
        Ok(self.answers.borrow_mut().remove(0))
    }
    async fn dispatch(&self, command: &str, args: Value) -> Result<Value, String> {
        self.commands.borrow_mut().push((command.to_owned(), args));
        Ok(if self.command_answers.borrow().is_empty() {
            Value::Null
        } else {
            self.command_answers.borrow_mut().remove(0)
        })
    }
}

fn cookies_of(outcome: &Outcome) -> Vec<(String, String)> {
    outcome
        .credential
        .as_ref()
        .and_then(|credential| credential.cookies.clone())
        .unwrap_or_default()
        .into_iter()
        .collect()
}

fn pairs(values: &[(&str, &str)]) -> Vec<(String, String)> {
    values
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

fn inspect(adapter: &Adapter, page: &FakePage) -> Outcome {
    block_on(adapter.inspect(page))
}

// 'captures only the selected SubscribeStar host'
#[test]
fn subscribestar_keeps_the_chosen_services_cookies_and_origin() {
    for host in [
        "www.subscribestar.com",
        "subscribestar.adult",
        "subscribestar.art",
    ] {
        let site = with_service_host(resolve("subscribestar").unwrap(), host).unwrap();
        let adapter = adapter(site).unwrap();
        let page = FakePage::at(&format!("https://{host}/account"))
            .with_cookies(&[("_personalization_id", "fixture-session")]);
        let outcome = inspect(&adapter, &page);
        assert!(
            page.asked
                .borrow()
                .iter()
                .all(|url| *url == format!("https://{host}"))
        );
        let credential = outcome.credential.unwrap();
        assert_eq!(credential.site_category, "subscribestar");
        assert_eq!(
            credential.headers,
            Some(BTreeMap::from([(
                "Origin".into(),
                format!("https://{host}")
            )]))
        );
        assert_eq!(
            credential.cookies,
            Some(BTreeMap::from([(
                "_personalization_id".into(),
                "fixture-session".into()
            )]))
        );
    }
}

// 'does not capture a SubscribeStar session from a different origin'
// and 'does not capture a different SubscribeStar service after a
// redirect'
#[test]
fn subscribestar_reads_no_cookies_off_its_service() {
    for (host, current) in [
        (
            "www.subscribestar.com",
            "https://subscribestar.com.evil.test/account",
        ),
        (
            "subscribestar.adult",
            "https://www.subscribestar.com/account",
        ),
    ] {
        let site = with_service_host(resolve("subscribestar").unwrap(), host).unwrap();
        let adapter = adapter(site).unwrap();
        let page = FakePage::at(current).with_cookies(&[("_personalization_id", "fixture")]);
        let outcome = inspect(&adapter, &page);
        assert!(page.asked.borrow().is_empty());
        assert!(outcome.credential.is_none());
        assert_eq!(
            outcome.message.as_deref(),
            Some("Sign in on your SubscribeStar service to continue.")
        );
    }
}

// 'accepts an existing ExHentai session when the optional igneous
// cookie is absent'
#[test]
fn exhentai_needs_only_the_member_cookies() {
    let adapter = adapter(resolve("ehentai").unwrap()).unwrap();
    let page = FakePage::at("https://exhentai.org/")
        .with_cookies(&[("ipb_member_id", "member"), ("ipb_pass_hash", "hash")]);
    assert_eq!(
        cookies_of(&inspect(&adapter, &page)),
        pairs(&[("ipb_member_id", "member"), ("ipb_pass_hash", "hash")])
    );
}

#[test]
fn a_refused_exhentai_check_asks_to_sign_in_again() {
    let adapter = adapter(resolve("ehentai").unwrap()).unwrap();
    let page = FakePage::at("https://exhentai.org/")
        .with_cookies(&[("ipb_member_id", "member"), ("ipb_pass_hash", "hash")])
        .answering(
            "sadpanda",
            json!({ "challenge": false, "denied": true, "blank": false }),
        );
    let outcome = inspect(&adapter, &page);
    assert!(outcome.credential.is_none());
    assert_eq!(
        outcome.message.as_deref(),
        Some("ExHentai access was not accepted. Sign in again to refresh it.")
    );
}

// 'persists cookie credentials in the host and never sends secrets to
// the renderer': a service without required cookies keeps every cookie
// once the page shows the session.
#[test]
fn danbooru_keeps_every_cookie_once_the_page_shows_a_session() {
    let adapter = adapter(resolve("danbooru").unwrap()).unwrap();
    let page = FakePage::at("https://danbooru.donmai.us/")
        .with_cookies(&[("_session", "secret"), ("user", "42")]);
    assert_eq!(
        inspect(&adapter, &page).message.as_deref(),
        Some("Log in with Danbooru to continue.")
    );
    let page = page.answering("logoutPattern", json!(true));
    let outcome = inspect(&adapter, &page);
    assert_eq!(
        outcome.credential,
        Some(Credential {
            site_category: "danbooru".into(),
            credential_type: "cookies".into(),
            cookies: Some(BTreeMap::from([
                ("_session".into(), "secret".into()),
                ("user".into(), "42".into())
            ])),
            ..Credential::default()
        })
    );
}

// 'captures only cookies required by a source'
#[test]
fn fur_affinity_keeps_only_its_two_cookies() {
    let adapter = adapter(resolve("furaffinity").unwrap()).unwrap();
    let page = FakePage::at("https://www.furaffinity.net/").with_cookies(&[
        ("a", "required-a"),
        ("b", "required-b"),
        ("analytics", "ignored"),
    ]);
    assert_eq!(
        cookies_of(&inspect(&adapter, &page)),
        pairs(&[("a", "required-a"), ("b", "required-b")])
    );
    let half = FakePage::at("https://www.furaffinity.net/").with_cookies(&[("a", "x")]);
    assert!(inspect(&adapter, &half).credential.is_none());
}

#[test]
fn newgrounds_waits_for_its_session_cookie() {
    let adapter = adapter(resolve("newgrounds").unwrap()).unwrap();
    let before = FakePage::at("https://www.newgrounds.com/login");
    let outcome = inspect(&adapter, &before);
    assert_eq!(outcome.status, Some(Status::Active));
    assert_eq!(
        outcome.message.as_deref(),
        Some("Log in with Newgrounds to continue.")
    );
    let after = FakePage::at("https://www.newgrounds.com/")
        .with_cookies(&[("ng_session", " signed-in "), ("vmk1du5I8m", "x")]);
    let outcome = inspect(&adapter, &after);
    assert_eq!(cookies_of(&outcome), pairs(&[("ng_session", "signed-in")]));
    assert_eq!(
        outcome.message.as_deref(),
        Some("Newgrounds session captured.")
    );
}

// 'does not save a FANBOX cookie before the login redirects to FANBOX'
#[test]
fn fanbox_waits_until_the_login_reaches_fanbox() {
    let adapter = adapter(resolve("fanbox").unwrap()).unwrap();
    let page = FakePage::at("https://accounts.pixiv.net/login")
        .with_cookies(&[("FANBOXSESSID", "session")]);
    let outcome = inspect(&adapter, &page);
    assert!(outcome.credential.is_none());
    assert_eq!(outcome.status, Some(Status::Active));
}

// 'saves a FANBOX cookie after the login redirects to FANBOX' and 'does
// not treat FANBOX guest cookies as an authenticated login'
#[test]
fn fanbox_keeps_its_cookies_only_when_its_api_answers_for_the_session() {
    let adapter = adapter(resolve("fanbox").unwrap()).unwrap();
    let guest = FakePage::at("https://www.fanbox.cc/")
        .with_cookies(&[("FANBOXSESSID", "guest")])
        .answering("__pictoFanboxSession", json!(false));
    assert!(inspect(&adapter, &guest).credential.is_none());
    let pending = FakePage::at("https://www.fanbox.cc/")
        .with_cookies(&[("FANBOXSESSID", "valid")])
        .answering("__pictoFanboxSession", Value::Null);
    assert!(inspect(&adapter, &pending).credential.is_none());

    let page = FakePage::at("https://www.fanbox.cc/")
        .with_cookies(&[("FANBOXSESSID", "valid"), ("__cf_bm", "browser-session")])
        .answering("__pictoFanboxSession", json!(true))
        .answering(
            "navigator.userAgent",
            json!({ "user-agent": "WebKit browser" }),
        );
    let outcome = inspect(&adapter, &page);
    let credential = outcome.credential.unwrap();
    assert_eq!(
        credential.cookies,
        Some(BTreeMap::from([
            ("FANBOXSESSID".into(), "valid".into()),
            ("__cf_bm".into(), "browser-session".into())
        ]))
    );
    assert_eq!(
        credential.headers,
        Some(BTreeMap::from([(
            "user-agent".into(),
            "WebKit browser".into()
        )]))
    );
    assert_eq!(*page.asked.borrow(), ["https://api.fanbox.cc"]);
}

fn account_page(url: &str, fields: Value) -> FakePage {
    let mut answer = json!({
        "href": url, "title": "Account", "acceptedConsent": false, "hasChallenge": false,
        "hasLoginForm": false, "authenticated": false, "onOptions": false,
        "apiKey": null, "userId": null,
    });
    for (key, value) in fields.as_object().unwrap() {
        answer[key] = value.clone();
    }
    FakePage::at(url).answering("inputs", answer)
}

#[test]
fn a_booru_options_page_without_a_key_is_an_error_shown_in_the_window() {
    let adapter = adapter(resolve("gelbooru").unwrap()).unwrap();
    let home = account_page("https://gelbooru.com/", json!({ "authenticated": true }));
    inspect(&adapter, &home);
    let options = account_page(
        "https://gelbooru.com/index.php?page=account&s=options",
        json!({ "authenticated": true, "onOptions": true }),
    );
    let outcome = inspect(&adapter, &options);
    assert!(outcome.show);
    assert_eq!(outcome.status, Some(Status::Error));
    assert_eq!(
        outcome.message.as_deref(),
        Some("Logged in, but Picto could not find Gelbooru user_id and api_key.")
    );
}

// 'keeps browser challenges active without saving a false login'
#[test]
fn a_browser_check_keeps_the_login_waiting() {
    let adapter = adapter(resolve("rule34").unwrap()).unwrap();
    let page = account_page(
        "https://rule34.xxx/challenge",
        json!({ "hasChallenge": true, "title": "Just a moment" }),
    );
    let outcome = inspect(&adapter, &page);
    assert!(outcome.credential.is_none());
    assert_eq!(outcome.status, Some(Status::Active));
    assert!(outcome.message.unwrap().contains("browser check"));
    let form = account_page("https://rule34.xxx/", json!({ "hasLoginForm": true }));
    assert_eq!(
        inspect(&adapter, &form).message.as_deref(),
        Some("Log in with Rule34.xxx to continue.")
    );
    let blank = account_page("https://rule34.xxx/", json!({}));
    assert_eq!(
        inspect(&adapter, &blank).message.as_deref(),
        Some("Waiting for Rule34.xxx login…")
    );
}

#[test]
fn an_oauth2_callback_with_another_state_is_refused() {
    let adapter = adapter(resolve("deviantart").unwrap()).unwrap();
    let services = FakeServices::default();
    block_on(adapter.prepare(&services)).unwrap();
    let page = FakePage::default();
    assert_eq!(
        block_on(adapter.complete(
            "https://mikf.github.io/gallery-dl/oauth-redirect.html?code=c&state=forged",
            &page,
            &services
        ))
        .unwrap_err(),
        "DeviantArt OAuth state did not match."
    );
}

#[test]
fn an_oauth2_callback_refused_then_accepted_in_the_same_window_completes() {
    let adapter = adapter(resolve("deviantart").unwrap()).unwrap();
    let services = FakeServices::default();
    let prepared = block_on(adapter.prepare(&services)).unwrap();
    let state = url::Url::parse(&prepared.url)
        .unwrap()
        .query_pairs()
        .find(|(name, _)| name == "state")
        .unwrap()
        .1
        .into_owned();
    let page = FakePage::default();
    let denied = format!(
        "https://mikf.github.io/gallery-dl/oauth-redirect.html?error=access_denied&state={state}"
    );
    assert_eq!(
        block_on(adapter.complete(&denied, &page, &services)).unwrap_err(),
        "No code in DeviantArt OAuth callback."
    );
    services.answers.borrow_mut().push(HttpResponse {
        status: 200,
        body: r#"{"access_token":"a","refresh_token":"durable"}"#.into(),
    });
    let accepted = format!(
        "https://mikf.github.io/gallery-dl/oauth-redirect.html?code=c&state={state}"
    );
    let outcome = block_on(adapter.complete(&accepted, &page, &services)).unwrap();
    assert_eq!(
        outcome.credential.unwrap().oauth_token.as_deref(),
        Some("durable")
    );
}

// 'persists both OAuth1 token values through the same host boundary'
#[test]
fn tumblr_keeps_both_access_token_values() {
    let adapter = adapter(resolve("tumblr").unwrap()).unwrap();
    let services = FakeServices::default();
    services.answers.borrow_mut().extend([
        HttpResponse {
            status: 200,
            body: "oauth_token=request&oauth_token_secret=request-secret".into(),
        },
        HttpResponse {
            status: 200,
            body: "oauth_token=access&oauth_token_secret=access-secret".into(),
        },
    ]);
    let prepared = block_on(adapter.prepare(&services)).unwrap();
    assert_eq!(
        prepared.url,
        "https://www.tumblr.com/oauth/authorize?oauth_token=request&perms=read"
    );
    let callback =
        "https://picto.app/oauth/callback?oauth_token=request&oauth_verifier=verified";
    assert!(adapter.handles(callback));
    let outcome =
        block_on(adapter.complete(callback, &FakePage::default(), &services)).unwrap();
    let credential = outcome.credential.unwrap();
    assert_eq!(credential.oauth_token.as_deref(), Some("access"));
    assert_eq!(credential.password.as_deref(), Some("access-secret"));
    let posts = services.posts.borrow();
    assert_eq!(posts[0].url, "https://www.tumblr.com/oauth/request_token");
    assert_eq!(
        posts[0].body,
        "oauth_callback=https%3A%2F%2Fpicto.app%2Foauth%2Fcallback"
    );
    assert!(posts[1].body.contains("oauth_verifier=verified"));
    let authorization = &posts[1]
        .headers
        .iter()
        .find(|(name, _)| name == "Authorization")
        .unwrap()
        .1;
    assert!(authorization.starts_with("OAuth "));
    assert!(authorization.contains("oauth_token=\"request\""));
}

// 'uses one host-owned Pixiv OAuth flow for Pixiv and Pixiv-user
// queries'
#[test]
fn pixiv_hands_its_code_and_session_to_the_application() {
    let adapter = adapter(resolve("pixivuser").unwrap()).unwrap();
    let services = FakeServices::default();
    services.command_answers.borrow_mut().push(json!({
        "login_url": "https://app-api.pixiv.net/web/v1/login",
        "code_verifier": "verifier",
    }));
    let prepared = block_on(adapter.prepare(&services)).unwrap();
    assert_eq!(prepared.url, "https://app-api.pixiv.net/web/v1/login");
    assert!(adapter.handles("pixiv://account/login?code=oauth-code"));
    let page = FakePage::default().with_cookies(&[("PHPSESSID", "pixiv-session")]);
    let outcome =
        block_on(adapter.complete("pixiv://account/login?code=oauth-code", &page, &services))
            .unwrap();
    assert!(outcome.already_persisted);
    assert_eq!(
        outcome.message.as_deref(),
        Some("Pixiv authorization completed.")
    );
    let commands = services.commands.borrow();
    assert_eq!(commands[0].0, "pixiv_oauth_start");
    assert_eq!(
        commands[1],
        (
            "pixiv_oauth_exchange".to_owned(),
            json!({ "code": "oauth-code", "code_verifier": "verifier", "phpsessid": "pixiv-session" })
        )
    );
}
