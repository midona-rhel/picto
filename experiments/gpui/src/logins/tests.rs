//! The login session against production's `authSessions.test.mjs`: the
//! machine driven by the events a sign-in window reports, its adapter run on
//! recorded pages, and the credential it asks to save.
use super::adapters::tests::{FakePage, FakeServices};
use super::adapters::{HttpResponse, Outcome};
use super::machine::{Effect, Machine};
use super::onlyfans::ManualInput;
use super::sites::{AUTH_WINDOW, Strategy, WIDE_AUTH_WINDOW, login_routes, resolve};
use super::{AuthState, Credential, Status};
use futures::executor::block_on;
use serde_json::json;
use std::collections::BTreeMap;

/// Plays a login the way the driver does: each effect performed against a
/// recorded page and recorded services; what was persisted, loaded, shown
/// and hidden written down.
#[derive(Default)]
struct Harness {
    machine: Machine,
    page: FakePage,
    services: FakeServices,
    persisted: Vec<Credential>,
    loaded: Vec<String>,
    opened: Vec<Effect>,
    closed: Vec<bool>,
    shown: usize,
    hidden: usize,
    polling: bool,
}

impl Harness {
    fn with_page(page: FakePage) -> Self {
        Self {
            page,
            ..Self::default()
        }
    }

    fn start(&mut self, site: &str, host: Option<&str>) -> Result<(), String> {
        let effects = self.machine.start(site, host)?;
        self.run(effects);
        Ok(())
    }

    fn run(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            let next = match effect {
                Effect::OpenWindow { .. } => {
                    self.opened.push(effect);
                    Vec::new()
                }
                Effect::Prepare => {
                    let adapter = self.machine.adapter().unwrap();
                    let prepared = block_on(adapter.prepare(&self.services));
                    self.machine.prepared(prepared)
                }
                Effect::Load(url) => {
                    *self.page.url.borrow_mut() = url.clone();
                    self.loaded.push(url);
                    Vec::new()
                }
                Effect::StartPolling => {
                    self.polling = true;
                    Vec::new()
                }
                Effect::Focus => Vec::new(),
                Effect::Show => {
                    self.shown += 1;
                    Vec::new()
                }
                Effect::Hide => {
                    self.hidden += 1;
                    Vec::new()
                }
                Effect::Inspect => {
                    let adapter = self.machine.adapter().unwrap();
                    let outcome = block_on(adapter.inspect(&self.page));
                    self.machine.inspected(Some(outcome))
                }
                Effect::Complete(url) => {
                    let adapter = self.machine.adapter().unwrap();
                    let outcome = block_on(adapter.complete(&url, &self.page, &self.services));
                    self.machine.completed(outcome)
                }
                Effect::Persist(credential) => {
                    self.persisted.push(credential);
                    self.machine.persisted(Ok(()))
                }
                Effect::CloseWindow { clear_storage } => {
                    self.closed.push(clear_storage);
                    Vec::new()
                }
                Effect::LaunchExternal | Effect::CloseExternal => Vec::new(),
            };
            self.run(next);
        }
    }

    /// `did-finish-load` at the page's address.
    fn finish_load(&mut self) {
        let url = self.page.url.borrow().clone();
        let effects = self.machine.loaded(&url);
        self.run(effects);
    }

    fn state(&self) -> &AuthState {
        self.machine.state()
    }
}

// 'rejects unsupported sites without creating a browser window' and
// 'rejects an unsupported SubscribeStar login host'
#[test]
fn unknown_services_and_hosts_open_nothing() {
    let mut harness = Harness::default();
    assert_eq!(
        harness.start(" Missing ", None).unwrap_err(),
        "Unsupported auth site: missing"
    );
    assert_eq!(
        harness
            .start("subscribestar", Some("subscribestar.com.evil.test"))
            .unwrap_err(),
        "Choose a supported SubscribeStar service."
    );
    assert!(harness.opened.is_empty());
    assert_eq!(harness.state().status, Status::Idle);
}

// 'opens every cookie and account source at its declared direct-site login
// route'
#[test]
fn every_page_login_opens_its_login_page_and_is_active() {
    for (site, login) in login_routes() {
        let strategy = resolve(&site).unwrap().strategy;
        if !matches!(strategy, Strategy::Cookies | Strategy::AccountApi) {
            continue;
        }
        let mut harness = Harness::default();
        harness.start(&site, None).unwrap();
        assert_eq!(harness.loaded, [login], "{site}");
        assert_eq!(
            harness.state().site_category.as_deref(),
            Some(site.as_str())
        );
        assert_eq!(harness.state().status, Status::Active, "{site}");
        assert!(harness.polling, "{site}");
        assert_eq!(
            harness.state().title.as_deref(),
            Some(format!("Login: {}", resolve(&site).unwrap().label).as_str())
        );
    }
}

// 'opens Newgrounds and DeviantArt at desktop width without changing other
// login windows'
#[test]
fn the_window_opens_at_the_services_size() {
    for (site, size) in [
        ("newgrounds", WIDE_AUTH_WINDOW),
        ("deviantart", WIDE_AUTH_WINDOW),
        ("patreon", AUTH_WINDOW),
    ] {
        let mut harness = Harness::default();
        harness.start(site, None).unwrap();
        assert!(matches!(
            &harness.opened[0],
            Effect::OpenWindow { size: opened, .. } if *opened == size
        ));
    }
}

// 'persists cookie credentials in the host and never sends secrets to the
// renderer'
#[test]
fn a_captured_login_is_saved_and_the_state_holds_no_secret() {
    let page = FakePage::at("https://danbooru.donmai.us/session/new")
        .with_cookies(&[("_session", "secret"), ("user", "42")])
        .answering("logoutPattern", json!(true));
    let mut harness = Harness::with_page(page);
    harness.start("danbooru", None).unwrap();
    harness.finish_load();
    assert_eq!(
        harness.persisted,
        [Credential {
            site_category: "danbooru".into(),
            credential_type: "cookies".into(),
            cookies: Some(BTreeMap::from([
                ("_session".into(), "secret".into()),
                ("user".into(), "42".into())
            ])),
            ..Credential::default()
        }]
    );
    assert_eq!(
        harness.persisted[0].arguments(),
        json!({
            "site_id": "danbooru", "credential_type": "cookies", "display_name": "danbooru",
            "username": null, "password": null, "cookies": { "_session": "secret", "user": "42" },
            "headers": null, "oauth_token": null,
        })
    );
    assert_eq!(harness.state().status, Status::Completed);
    assert_eq!(
        harness.state().message.as_deref(),
        Some("Danbooru session captured.")
    );
    // The window closes with its storage discarded.
    assert_eq!(harness.closed, [true]);
    let state = format!("{:?}", harness.state());
    assert!(!state.contains("secret"));
}

// 'verifies ExHentai in the managed login popup before persisting its
// gallery-dl cookies'
#[test]
fn exhentai_moves_to_its_check_then_saves() {
    let page = FakePage::at("").with_cookies(&[
        ("ipb_member_id", "member"),
        ("ipb_pass_hash", "hash"),
        ("igneous", "igneous"),
    ]);
    let mut harness = Harness::with_page(page);
    harness.start("ehentai", None).unwrap();
    assert_eq!(
        harness.loaded,
        ["https://forums.e-hentai.org/index.php?act=Login&CODE=00"]
    );
    harness.finish_load();
    assert_eq!(
        harness.loaded.last().map(String::as_str),
        Some("https://exhentai.org/")
    );
    assert!(harness.persisted.is_empty());
    harness.finish_load();
    assert_eq!(harness.persisted.len(), 1);
    assert_eq!(
        harness.state().message.as_deref(),
        Some("ExHentai session captured.")
    );
}

// 'reads booru API credentials from account settings after login'
#[test]
fn a_booru_login_hides_the_window_while_it_reads_the_key() {
    let page = FakePage::at("").answering(
        "inputs",
        json!({ "authenticated": true, "onOptions": false }),
    );
    let mut harness = Harness::with_page(page);
    harness.start("rule34", None).unwrap();
    harness.finish_load();
    assert!(harness.loaded.last().unwrap().contains("s=options"));
    assert_eq!(harness.hidden, 1);
    assert_eq!(harness.state().status, Status::Loading);
    *harness.page.scripts.borrow_mut() = vec![(
        "inputs",
        json!({ "authenticated": true, "onOptions": true, "apiKey": "a".repeat(32), "userId": "599664" }),
    )];
    harness.finish_load();
    assert_eq!(harness.persisted[0].credential_type, "api_key");
    assert_eq!(harness.persisted[0].username.as_deref(), Some("599664"));
}

// 'uses gallery-dl DeviantArt OAuth and keeps its refresh token plus session
// cookies'
#[test]
fn an_oauth_callback_is_not_loaded_but_completed() {
    let page = FakePage::at("").with_cookies(&[("auth", "a"), ("userinfo", "u")]);
    let mut harness = Harness::with_page(page);
    harness.start("deviantart", None).unwrap();
    assert!(!harness.polling);
    let state = url::Url::parse(&harness.loaded[0])
        .unwrap()
        .query_pairs()
        .find(|(name, _)| name == "state")
        .unwrap()
        .1
        .into_owned();
    harness.services.answers.borrow_mut().push(HttpResponse {
        status: 200,
        body: r#"{"refresh_token":"durable"}"#.into(),
    });
    // A page of the login itself loads.
    let (allowed, effects) = harness
        .machine
        .may_navigate("https://www.deviantart.com/users/login");
    assert!(allowed && effects.is_empty());
    let callback =
        format!("https://mikf.github.io/gallery-dl/oauth-redirect.html?code=c&state={state}");
    let (allowed, effects) = harness.machine.may_navigate(&callback);
    assert!(!allowed);
    assert_eq!(
        harness.state().message.as_deref(),
        Some("Completing DeviantArt login…")
    );
    harness.run(effects);
    assert_eq!(harness.persisted[0].oauth_token.as_deref(), Some("durable"));
    assert_eq!(harness.state().status, Status::Completed);
    assert_eq!(
        harness.state().message.as_deref(),
        Some("DeviantArt authorization completed.")
    );
}

// 'uses one host-owned Pixiv OAuth flow for Pixiv and Pixiv-user queries'
#[test]
fn pixiv_is_saved_by_the_application_not_here() {
    let mut harness =
        Harness::with_page(FakePage::at("").with_cookies(&[("PHPSESSID", "pixiv-session")]));
    harness.services.command_answers.borrow_mut().push(json!({
        "login_url": "https://app-api.pixiv.net/web/v1/login", "code_verifier": "verifier",
    }));
    harness.start("pixivuser", None).unwrap();
    assert_eq!(harness.state().site_category.as_deref(), Some("pixiv"));
    assert_eq!(harness.state().status, Status::Active);
    let (_, effects) = harness
        .machine
        .may_navigate("pixiv://account/login?code=oauth-code");
    harness.run(effects);
    assert!(harness.persisted.is_empty());
    assert_eq!(harness.state().status, Status::Completed);
    assert_eq!(
        harness.services.commands.borrow()[1].0,
        "pixiv_oauth_exchange"
    );
}

// 'retains active state for renderer remounts and cancels cleanly'
#[test]
fn a_login_cancels_to_idle_and_closes_its_window_keeping_nothing() {
    let mut harness = Harness::default();
    harness.start("deviantart", None).unwrap();
    assert_eq!(harness.state().status, Status::Active);
    let effects = harness.machine.cancel();
    harness.run(effects);
    assert_eq!(*harness.state(), AuthState::idle());
    assert_eq!(harness.closed, [false]);
    // Events from the closed window change nothing.
    assert!(
        harness
            .machine
            .loaded("https://www.deviantart.com/")
            .is_empty()
    );
    assert_eq!(harness.state().status, Status::Idle);
}

#[test]
fn closing_the_window_cancels_the_login() {
    let mut harness = Harness::default();
    harness.start("patreon", None).unwrap();
    harness.machine.window_closed();
    assert_eq!(
        *harness.state(),
        AuthState {
            status: Status::Cancelled,
            message: Some("Login window closed.".into()),
            ..AuthState::idle()
        }
    );
    assert!(harness.machine.tick().is_empty());
}

#[test]
fn a_window_the_system_cannot_open_ends_the_login_with_why() {
    let mut harness = Harness::default();
    let effects = harness.machine.start("patreon", None).unwrap();
    assert!(matches!(effects[0], Effect::OpenWindow { .. }));
    harness
        .machine
        .could_not_open("Signing in needs the Microsoft Edge WebView2 Runtime.".into());
    assert_eq!(harness.state().status, Status::Error);
    assert!(harness.machine.tick().is_empty());
}

#[test]
fn a_look_already_under_way_is_not_started_twice() {
    let mut harness = Harness::default();
    harness.start("patreon", None).unwrap();
    assert_eq!(harness.machine.tick(), [Effect::Inspect]);
    assert!(harness.machine.tick().is_empty());
    assert!(
        harness
            .machine
            .loaded("https://www.patreon.com/")
            .is_empty()
    );
    // A look given up after 4 s lets the next one start.
    assert!(harness.machine.inspected(None).is_empty());
    assert_eq!(harness.machine.tick(), [Effect::Inspect]);
}

#[test]
fn a_credential_the_application_refuses_is_an_error() {
    let mut machine = Machine::new();
    machine.start("patreon", None).unwrap();
    machine.prepared(Ok(super::adapters::Prepared {
        url: "https://www.patreon.com/login".into(),
        message: "Log in with Patreon in the popup window.".into(),
    }));
    machine.tick();
    let effects = machine.inspected(Some(Outcome {
        credential: Some(Credential {
            site_category: "patreon".into(),
            credential_type: "cookies".into(),
            ..Credential::default()
        }),
        message: Some("Patreon session captured.".into()),
        ..Outcome::default()
    }));
    assert!(matches!(effects[0], Effect::Persist(_)));
    assert_eq!(
        machine.state().message.as_deref(),
        Some("Saving login securely…")
    );
    machine.persisted(Err("Captured cookies credential is empty".into()));
    assert_eq!(machine.state().status, Status::Error);
    assert_eq!(
        machine.state().message.as_deref(),
        Some("Captured cookies credential is empty")
    );
}

// 'uses an external browser for OnlyFans and persists only the captured
// session'
#[test]
fn onlyfans_signs_in_in_a_browser_outside_the_app() {
    let mut machine = Machine::new();
    let effects = machine.start("onlyfans", None).unwrap();
    assert_eq!(effects, [Effect::LaunchExternal]);
    assert_eq!(
        machine.state().message.as_deref(),
        Some("Opening OnlyFans in an external browser…")
    );
    machine.external_launched(Ok(()));
    assert_eq!(machine.state().status, Status::Active);
    assert_eq!(
        machine.state().message.as_deref(),
        Some("Complete the OnlyFans login in the external browser.")
    );
    let credential = Credential {
        site_category: "onlyfans".into(),
        credential_type: "cookies".into(),
        cookies: Some(BTreeMap::from([("sess".into(), "session".into())])),
        ..Credential::default()
    };
    let effects = machine.external_finished(Ok(credential.clone()));
    assert_eq!(effects, [Effect::Persist(credential)]);
    let effects = machine.persisted(Ok(()));
    assert_eq!(effects, [Effect::CloseExternal]);
    assert_eq!(machine.state().status, Status::Completed);
    assert_eq!(
        machine.state().message.as_deref(),
        Some("OnlyFans session captured.")
    );

    let mut missing = Machine::new();
    missing.start("onlyfans", None).unwrap();
    missing.external_launched(Err(
        "Browser login requires Chrome, Edge, Brave, or Chromium.".into(),
    ));
    assert_eq!(missing.state().status, Status::Error);
}

// 'validates and saves a manually supplied OnlyFans session'
#[test]
fn a_pasted_onlyfans_session_is_saved() {
    let mut machine = Machine::new();
    let effects = machine
        .save_manual_onlyfans(&ManualInput {
            cookie: "analytics=ignored; sess=session; auth_id=42; auth_uid=42".into(),
            user_agent: "Chrome browser".into(),
            x_bc: "signature".into(),
        })
        .unwrap();
    assert!(
        matches!(&effects[0], Effect::Persist(credential) if credential.site_category == "onlyfans")
    );
    assert_eq!(
        machine.state().message.as_deref(),
        Some("Saving login securely…")
    );
    machine.persisted(Ok(()));
    assert_eq!(machine.state().status, Status::Completed);
    assert_eq!(
        machine.state().message.as_deref(),
        Some("OnlyFans session saved.")
    );
    let mut refused = Machine::new();
    assert_eq!(
        refused
            .save_manual_onlyfans(&ManualInput::default())
            .unwrap_err(),
        "OnlyFans Cookie is missing sess and auth_id."
    );
}
