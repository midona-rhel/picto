//! Site logins: what production's main process does for the Accounts
//! window (`electron/windows/authSessions.mjs`). A login opens the
//! service's own page in a sign-in window (`platform::login_window`), its
//! adapter decides from the page and its cookies when the user has signed
//! in, and the credential goes to the application through
//! `auth.credentials.set`, the command production's main process uses. One
//! login is under way at a time.
//!
//! - `sites.rs`: the services and their addresses (`authSites.mjs`);
//! - `adapters.rs`: when each kind of login has finished and what it keeps;
//! - `machine.rs`: the session's states and what each event leads to;
//! - `onlyfans.rs`: OnlyFans' pasted session; `browser.rs`: its login in a
//!   browser of the user's outside the app;
//! - `oauth.rs`: OAuth 1 signatures; `cookies.rs`: which cookies a page is
//!   sent.
//!
//! [`Logins`] performs what the machine asks for: it opens and closes the
//! window, looks at its page every 750 ms, and sends the credential.
pub mod adapters;
pub mod browser;
#[cfg(debug_assertions)]
pub mod check;
pub mod cookies;
pub mod machine;
pub mod onlyfans;
pub mod sites;

use crate::platform::login_window::{self, LoginWindow};
use crate::session::Session;
use adapters::{HttpRequest, HttpResponse, Page, Services};
use futures::StreamExt as _;
use gpui::*;
use machine::{Effect, Machine};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Duration;

/// Keeps every site login of this process in the application core's
/// process-local store (`PICTO_EPHEMERAL_CREDENTIALS`) instead of the
/// system's keychain. The core files a login under the service's name alone
/// (`picto` / `<site>`), not under a library, so a login saved, removed or
/// settled here would replace or delete the one the user's Picto keeps for
/// that service, which the user's Picto shares; until the owner decides
/// otherwise, this app's logins go with the process. Called first in `main`, before any thread starts.
pub fn keep_credentials_in_process() {
    // SAFETY: nothing else runs yet, so no thread reads the environment
    // while it changes.
    unsafe { std::env::set_var("PICTO_EPHEMERAL_CREDENTIALS", "1") };
}

/// Whether [`keep_credentials_in_process`] holds, checked again before any
/// login is saved or removed (a test harness that did not call it never
/// reaches the keychain through a login).
pub fn credentials_in_process() -> bool {
    std::env::var("PICTO_EPHEMERAL_CREDENTIALS").as_deref() == Ok("1")
}

/// Why a login is not saved or removed when [`credentials_in_process`] does
/// not hold.
pub const KEYCHAIN_REFUSED: &str =
    "Logins are kept only for this session here; the system keychain is not used.";

/// Why a login that would reach a real site whatever the sign-in window
/// loads is not started while `PICTO_GPUI_LOGIN_ORIGIN` keeps logins on
/// local pages.
pub const LOCAL_ONLY: &str =
    "This login reaches the real site and is not run while logins are kept on local pages.";

/// `AuthSessionState.status`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Idle,
    Starting,
    Active,
    Loading,
    Completed,
    Cancelled,
    Error,
}

/// `AuthSessionState`: what the Accounts window is told of the login under
/// way. It never holds a secret.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AuthState {
    pub site_category: Option<String>,
    pub status: Status,
    pub title: Option<String>,
    pub current_url: Option<String>,
    pub message: Option<String>,
}

impl AuthState {
    pub fn idle() -> Self {
        Self::default()
    }
}

/// A captured login, as `auth.credentials.set` takes it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Credential {
    pub site_category: String,
    pub credential_type: String,
    pub display_name: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub cookies: Option<BTreeMap<String, String>>,
    pub headers: Option<BTreeMap<String, String>>,
    pub oauth_token: Option<String>,
}

impl Credential {
    /// `persistCredential`'s argument.
    pub fn arguments(&self) -> Value {
        json!({
            "site_id": self.site_category,
            "credential_type": self.credential_type,
            "display_name": self.display_name.clone().unwrap_or_else(|| self.site_category.clone()),
            "username": self.username,
            "password": self.password,
            "cookies": self.cookies,
            "headers": self.headers,
            "oauth_token": self.oauth_token,
        })
    }
}

/// How often an open sign-in window's page is looked at (`POLL_INTERVAL_MS`).
const POLL: Duration = Duration::from_millis(750);
/// How long a look may take before it is given up (a page mid-navigation
/// can leave a script unanswered).
const INSPECT_LIMIT: Duration = Duration::from_secs(4);

/// The app's one login session.
pub struct Logins {
    machine: Machine,
    session: Entity<Session>,
    window: Option<LoginWindow>,
    /// Counts windows, so a late answer about a closed one is dropped.
    generation: u64,
    shown_once: bool,
    window_events: Option<Task<()>>,
    poll: Option<Task<()>>,
    external: Option<browser::ExternalLogin>,
}

struct GlobalLogins(Entity<Logins>);
impl Global for GlobalLogins {}

impl Logins {
    /// The app's login session, made on first use. Another library's
    /// session (the library was switched) cancels a login under way for the
    /// one before, whose credential would go to the wrong library.
    pub fn global(session: &Entity<Session>, cx: &mut App) -> Entity<Logins> {
        if let Some(global) = cx.try_global::<GlobalLogins>() {
            let logins = global.0.clone();
            logins.update(cx, |logins, cx| {
                if logins.session != *session {
                    logins.cancel(cx);
                    logins.session = session.clone();
                }
            });
            return logins;
        }
        let session = session.clone();
        let logins = cx.new(|cx| {
            // `closeAuthWindow` on quit, as production cancels the session
            // when its main window goes: the sign-in window, or OnlyFans'
            // browser and its profile, never outlive the app.
            cx.on_app_quit(|logins: &mut Logins, cx| {
                logins.cancel(cx);
                async {}
            })
            .detach();
            Logins {
            machine: Machine::new(),
            session,
            window: None,
            generation: 0,
            shown_once: false,
            window_events: None,
            poll: None,
            external: None,
            }
        });
        cx.set_global(GlobalLogins(logins.clone()));
        logins
    }

    /// The open library is closing (a switch, Close Library, quitting):
    /// a login under way for it ends, whether or not the Accounts dialog
    /// is open, so its credential never reaches another library.
    pub fn library_closing(cx: &mut App) {
        if let Some(global) = cx.try_global::<GlobalLogins>() {
            let logins = global.0.clone();
            logins.update(cx, |logins, cx| logins.cancel(cx));
        }
    }

    /// `auth_session_state`.
    pub fn state(&self) -> &AuthState {
        self.machine.state()
    }

    /// The sign-in window open now, for the checks (`check.rs`).
    #[cfg(debug_assertions)]
    pub fn window(&self) -> Option<&LoginWindow> {
        self.window.as_ref()
    }

    /// `auth_session_start`: signs in to `site_id` (and, for SubscribeStar,
    /// the service chosen), cancelling any login under way.
    pub fn start(
        &mut self,
        site_id: &str,
        service_host: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let effects = self.machine.start(site_id, service_host)?;
        self.perform(effects, cx);
        Ok(())
    }

    /// `auth_session_cancel`.
    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        let effects = self.machine.cancel();
        self.perform(effects, cx);
    }

    /// `auth_onlyfans_manual`.
    pub fn save_manual_onlyfans(
        &mut self,
        input: &onlyfans::ManualInput,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let (effects, saved) = self.machine.save_manual_onlyfans(input);
        self.perform(effects, cx);
        saved
    }

    fn perform(&mut self, effects: Vec<Effect>, cx: &mut Context<Self>) {
        for effect in effects {
            match effect {
                Effect::OpenWindow { title, size } => {
                    if let Err(error) = self.open_window(title, size, cx) {
                        let effects = self.machine.could_not_open(error);
                        self.perform(effects, cx);
                        // Nothing else of this start applies.
                        break;
                    }
                }
                Effect::Prepare => self.prepare(cx),
                Effect::Load(url) => {
                    if let Some(window) = &self.window {
                        window.load(&url);
                    }
                }
                Effect::StartPolling => self.start_polling(cx),
                Effect::Focus => {
                    if let Some(window) = &self.window {
                        window.focus(cx);
                    }
                }
                Effect::Show => {
                    if let Some(window) = &self.window {
                        window.show(cx);
                    }
                }
                Effect::Hide => {
                    if let Some(window) = &self.window {
                        window.hide(cx);
                    }
                }
                Effect::Inspect => self.inspect(cx),
                Effect::Complete(url) => self.complete(url, cx),
                Effect::Persist {
                    credential,
                    session,
                } => self.persist(credential, session, cx),
                Effect::CloseWindow => {
                    self.poll = None;
                    self.window_events = None;
                    if let Some(window) = self.window.take() {
                        window.close(cx);
                    }
                }
                Effect::LaunchExternal { session } => self.launch_external(session, cx),
                Effect::CloseExternal => {
                    if let Some(external) = self.external.take() {
                        external.close();
                    }
                }
            }
        }
        cx.notify();
    }

    fn open_window(
        &mut self,
        title: String,
        size: sites::WindowSize,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let adapter = self.machine.adapter();
        let (events, mut received) = futures::channel::mpsc::unbounded();
        let window = LoginWindow::open(
            login_window::Options {
                title,
                width: size.width,
                height: size.height,
                min_width: size.min_width,
                min_height: size.min_height,
                intercept: Rc::new(move |url| adapter.as_ref().is_some_and(|a| a.handles(url))),
                events,
                local_origin: crate::seams::login_origin(),
            },
            cx,
        )?;
        self.generation += 1;
        self.shown_once = false;
        self.window = Some(window);
        let generation = self.generation;
        self.window_events = Some(cx.spawn(async move |this, cx| {
            while let Some(event) = received.next().await {
                let alive = this.update(cx, |this, cx| {
                    if this.generation != generation {
                        return false;
                    }
                    this.window_event(event, cx);
                    true
                });
                if !matches!(alive, Ok(true)) {
                    break;
                }
            }
        }));
        Ok(())
    }

    fn window_event(&mut self, event: login_window::Event, cx: &mut Context<Self>) {
        use login_window::Event;
        let effects = match event {
            Event::Started(url) => self.machine.navigated(&url),
            Event::Loaded(url) => {
                let mut effects = Vec::new();
                // `ready-to-show`: the window appears once its first page
                // is there.
                if !self.shown_once {
                    self.shown_once = true;
                    effects.push(Effect::Show);
                }
                effects.extend(self.machine.loaded(&url));
                effects
            }
            Event::Title(title) => {
                self.machine.titled(&title);
                Vec::new()
            }
            Event::Callback(url) => self.machine.may_navigate(&url).1,
            Event::OpenHere(url) => vec![Effect::Load(url)],
            Event::Failed { url, description } => self.machine.load_failed(&url, &description),
            Event::Unavailable(error) => {
                // The window holds no web view; it goes, and the login ends
                // with why.
                self.poll = None;
                if let Some(window) = self.window.take() {
                    window.close(cx);
                }
                self.machine.could_not_open(error)
            }
            Event::Closed => {
                // The platform closed the window; what is left of it (its
                // user data folder, GTK's last events) goes now.
                self.poll = None;
                if let Some(window) = self.window.take() {
                    window.close(cx);
                }
                self.machine.window_closed()
            }
        };
        self.perform(effects, cx);
    }

    fn prepare(&mut self, cx: &mut Context<Self>) {
        let Some(adapter) = self.machine.adapter() else {
            return;
        };
        let services = AppServices::new(self.session.clone(), cx);
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            let prepared = adapter.prepare(&services).await;
            let _ = this.update(cx, |this, cx| {
                if this.generation == generation {
                    let effects = this.machine.prepared(prepared);
                    this.perform(effects, cx);
                }
            });
        })
        .detach();
    }

    fn start_polling(&mut self, cx: &mut Context<Self>) {
        let generation = self.generation;
        self.poll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let alive = this.update(cx, |this, cx| {
                    if this.generation != generation {
                        return false;
                    }
                    let effects = this.machine.tick();
                    this.perform(effects, cx);
                    true
                });
                if !matches!(alive, Ok(true)) {
                    break;
                }
            }
        }));
    }

    fn inspect(&mut self, cx: &mut Context<Self>) {
        let Some(adapter) = self.machine.adapter() else {
            return;
        };
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            let page = WindowPage::new(this.clone(), cx);
            let look = adapter.inspect(&page);
            let limit = cx.background_executor().timer(INSPECT_LIMIT);
            let outcome = match futures::future::select(std::pin::pin!(look), limit).await {
                futures::future::Either::Left((outcome, _)) => Some(outcome),
                futures::future::Either::Right(_) => None,
            };
            let _ = this.update(cx, |this, cx| {
                if this.generation == generation {
                    let effects = this.machine.inspected(outcome);
                    this.perform(effects, cx);
                }
            });
        })
        .detach();
    }

    fn complete(&mut self, url: String, cx: &mut Context<Self>) {
        let Some(adapter) = self.machine.adapter() else {
            return;
        };
        let services = AppServices::new(self.session.clone(), cx);
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            let page = WindowPage::new(this.clone(), cx);
            let outcome = adapter.complete(&url, &page, &services).await;
            let _ = this.update(cx, |this, cx| {
                if this.generation == generation {
                    let effects = this.machine.completed(outcome);
                    this.perform(effects, cx);
                }
            });
        })
        .detach();
    }

    fn persist(&mut self, credential: Credential, session: u64, cx: &mut Context<Self>) {
        if !credentials_in_process() {
            let effects = self
                .machine
                .persisted(session, Err(KEYCHAIN_REFUSED.to_owned()));
            self.perform(effects, cx);
            return;
        }
        let site = credential.site_category.clone();
        let answer = self
            .session
            .read(cx)
            .call("auth.credentials.set", credential.arguments());
        cx.spawn(async move |this, cx| {
            let saved = answer.await.map(|_| ());
            let _ = this.update(cx, |this, cx| {
                if saved.is_ok() {
                    report_local_login(&this.session, &site, cx);
                }
                let effects = this.machine.persisted(session, saved);
                this.perform(effects, cx);
            });
        })
        .detach();
    }

    fn launch_external(&mut self, session: u64, cx: &mut Context<Self>) {
        // OnlyFans' own browser can go anywhere; a debug build keeping
        // logins on local pages does not start it.
        if crate::seams::login_origin().is_some() {
            let effects = self.machine.external_launched(Err(LOCAL_ONLY.to_owned()));
            self.perform(effects, cx);
            return;
        }
        let (launched, finished) = browser::launch();
        match launched {
            Ok(external) => {
                self.external = Some(external);
                let effects = self.machine.external_launched(Ok(()));
                self.perform(effects, cx);
                cx.spawn(async move |this, cx| {
                    let credential = finished.await.unwrap_or_else(|_| {
                        Err("OnlyFans login was closed before authentication completed.".to_owned())
                    });
                    let _ = this.update(cx, |this, cx| {
                        let effects = this.machine.external_finished(session, credential);
                        this.perform(effects, cx);
                    });
                })
                .detach();
            }
            Err(error) => {
                let effects = self.machine.external_launched(Err(error));
                self.perform(effects, cx);
            }
        }
    }
}

/// With `PICTO_GPUI_LOGIN_ORIGIN` (a debug build signing in to local pages
/// only), writes to standard error what the application now reports for
/// `site`: its row of `auth.credentials.list`, read through the command,
/// and the names of the cookies its credential store holds. No value is
/// written out: a session pasted by hand is a real one.
pub fn report_local_login(session: &Entity<Session>, site: &str, cx: &mut App) {
    if crate::seams::login_origin().is_none() {
        return;
    }
    let listed = session
        .read(cx)
        .dispatch("auth.credentials.list", json!({}));
    let site = site.to_owned();
    cx.spawn(async move |_| {
        let listed = listed.await.ok().and_then(Result::ok).unwrap_or_default();
        let row = listed
            .as_array()
            .into_iter()
            .flatten()
            .find(|record| record["site_id"] == site.as_str())
            .cloned();
        let stored = picto_core::credential_store::get_credential(&site)
            .ok()
            .flatten()
            .and_then(|credential| credential.cookies)
            .map(|cookies| cookies.into_keys().collect::<std::collections::BTreeSet<_>>());
        eprintln!(
            "picto: local login for {site}: auth.credentials.list row {}; stored cookie names {:?}",
            row.map_or_else(|| "none".to_owned(), |row| row.to_string()),
            stored
        );
    })
    .detach();
}

/// The open sign-in window as adapters read it.
struct WindowPage {
    logins: WeakEntity<Logins>,
    cx: RefCell<AsyncApp>,
}

impl WindowPage {
    fn new(logins: WeakEntity<Logins>, cx: &AsyncApp) -> Self {
        Self {
            logins,
            cx: RefCell::new(cx.clone()),
        }
    }

    fn with_window<R>(&self, work: impl FnOnce(&LoginWindow) -> R) -> Option<R> {
        let mut cx = self.cx.borrow_mut();
        self.logins
            .read_with(&mut *cx, |logins, _| logins.window.as_ref().map(work))
            .ok()
            .flatten()
    }
}

impl Page for WindowPage {
    fn current_url(&self) -> String {
        self.with_window(|window| window.current_url())
            .unwrap_or_default()
    }

    async fn cookies(&self, url: &str) -> Vec<cookies::Cookie> {
        match self.with_window(|window| window.cookies(url)) {
            Some(answer) => answer.await.unwrap_or_default(),
            None => Vec::new(),
        }
    }

    async fn eval(&self, script: &str) -> Option<Value> {
        self.with_window(|window| window.eval(script))?
            .await
            .ok()
            .flatten()
    }
}

/// The network and the application, as adapters reach them.
struct AppServices {
    session: Entity<Session>,
    cx: RefCell<AsyncApp>,
}

impl AppServices {
    fn new(session: Entity<Session>, cx: &mut Context<Logins>) -> Self {
        Self {
            session,
            cx: RefCell::new(cx.to_async()),
        }
    }
}

impl Services for AppServices {
    async fn post(&self, request: HttpRequest) -> Result<HttpResponse, String> {
        let (answer, received) = futures::channel::oneshot::channel();
        // The token requests run on a thread of their own, with a runtime
        // of their own, off the thread that draws.
        std::thread::spawn(move || {
            let _ = answer.send(post(request));
        });
        received
            .await
            .unwrap_or_else(|_| Err("The request was interrupted.".to_owned()))
    }

    async fn dispatch(&self, command: &str, args: Value) -> Result<Value, String> {
        // Pixiv's code exchange saves the login inside the application,
        // past `Effect::Persist`: the same guard as a saved login, and no
        // exchange with the real site while logins are kept local.
        if command == "pixiv_oauth_exchange" {
            if !credentials_in_process() {
                return Err(KEYCHAIN_REFUSED.to_owned());
            }
            if crate::seams::login_origin().is_some() {
                return Err(LOCAL_ONLY.to_owned());
            }
        }
        let answer = {
            let cx = self.cx.borrow();
            self.session
                .read_with(&*cx, |session, _| session.call(command.to_owned(), args))
        };
        answer.await
    }
}

/// A form `POST`, as production's `fetch` sends it.
fn post(request: HttpRequest) -> Result<HttpResponse, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(async move {
        let client = reqwest::Client::new();
        let mut builder = client.post(&request.url).body(request.body);
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        let response = builder.send().await.map_err(|error| error.to_string())?;
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        Ok(HttpResponse { status, body })
    })
}

#[cfg(test)]
mod tests;
