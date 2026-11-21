//! One sign-in at a time (`createAuthSessions`): the state the Accounts
//! window shows, and what each event from the sign-in window, its adapter
//! or the application leads to. It does nothing itself; it hands back the
//! effects for the driver (`logins::Logins`) to perform, so production's
//! session tests run against it directly.
use super::adapters::{self, Adapter, Outcome, Prepared};
use super::sites::{self, Site, Strategy, WindowSize};
use super::{AuthState, Credential, Status};
use std::rc::Rc;

/// What the driver does for the machine.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Opens the sign-in window, hidden until its first page is ready, on a
    /// web view whose storage is fresh and its own.
    OpenWindow {
        title: String,
        size: WindowSize,
    },
    /// Runs the adapter's `prepare` and reports [`Machine::prepared`].
    Prepare,
    Load(String),
    /// Looks at the page every 750 ms ([`Machine::tick`]).
    StartPolling,
    Focus,
    Show,
    Hide,
    /// Runs the adapter's `inspect`, bounded to 4 s, and reports
    /// [`Machine::inspected`].
    Inspect,
    /// Runs the adapter's `complete` on a callback and reports
    /// [`Machine::completed`].
    Complete(String),
    /// Saves the credential through `auth.credentials.set` and reports
    /// [`Machine::persisted`] with the session it was asked for.
    Persist { credential: Credential, session: u64 },
    /// Closes the window. Its storage goes with it: every sign-in window
    /// has storage of its own.
    CloseWindow,
    /// Opens OnlyFans in a browser of the user's outside the app, reports
    /// [`Machine::external_launched`], and then [`Machine::external_finished`]
    /// with the session it was asked for.
    LaunchExternal { session: u64 },
    /// Ends that browser's login.
    CloseExternal,
}

/// The sign-in under way and what the Accounts window is told of it.
pub struct Machine {
    state: AuthState,
    adapter: Option<Rc<Adapter>>,
    window_open: bool,
    external: bool,
    inspecting: bool,
    finishing: bool,
    completed: bool,
    /// What the Accounts window says once the credential is saved.
    pending_message: String,
    /// Counts sessions: each start, cancel and pasted session is a new
    /// one, and a late answer for an earlier one changes nothing
    /// (production's `externalAuth !== session`).
    session: u64,
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

impl Machine {
    pub fn new() -> Self {
        Self {
            state: AuthState::idle(),
            adapter: None,
            window_open: false,
            external: false,
            inspecting: false,
            finishing: false,
            completed: false,
            pending_message: String::new(),
            session: 0,
        }
    }

    /// `getAuthSessionState`.
    pub fn state(&self) -> &AuthState {
        &self.state
    }

    pub fn adapter(&self) -> Option<Rc<Adapter>> {
        self.adapter.clone()
    }

    /// `startAuthSession`: the sign-in for `site_id` (and, for
    /// SubscribeStar, the service chosen), after whatever was under way is
    /// cancelled. An unknown service or host is refused before anything
    /// opens.
    pub fn start(
        &mut self,
        site_id: &str,
        service_host: Option<&str>,
    ) -> Result<Vec<Effect>, String> {
        let mut site: Site = sites::resolve(site_id)
            .ok_or_else(|| format!("Unsupported auth site: {}", site_id.trim().to_lowercase()))?;
        if let Some(host) = service_host {
            site = sites::with_service_host(site, host)?;
        }
        let mut effects = self.cancel();
        self.completed = false;
        self.finishing = false;
        self.inspecting = false;
        let title = format!("Login: {}", site.label);
        if site.strategy == Strategy::OnlyFans {
            self.external = true;
            self.emit(AuthState {
                site_category: Some(site.id.to_owned()),
                status: Status::Starting,
                title: Some(title),
                current_url: site.login_url.clone(),
                message: Some("Opening OnlyFans in an external browser…".into()),
            });
            effects.push(Effect::LaunchExternal {
                session: self.session,
            });
            return Ok(effects);
        }
        let size = site.window;
        let id = site.id;
        self.adapter = Some(Rc::new(adapters::adapter(site)?));
        self.window_open = true;
        effects.push(Effect::OpenWindow {
            title: title.clone(),
            size,
        });
        self.emit(AuthState {
            site_category: Some(id.to_owned()),
            status: Status::Starting,
            title: Some(title),
            current_url: None,
            message: Some("Preparing login…".into()),
        });
        effects.push(Effect::Prepare);
        Ok(effects)
    }

    /// The system could not open the sign-in window (no web view: the
    /// WebView2 Runtime or WebKitGTK missing); the login ends with why.
    pub fn could_not_open(&mut self, error: String) -> Vec<Effect> {
        self.window_open = false;
        self.adapter = None;
        self.patch(Some(Status::Error), Some(error), None);
        Vec::new()
    }

    /// The adapter's first page, or why it could not make one.
    pub fn prepared(&mut self, prepared: Result<Prepared, String>) -> Vec<Effect> {
        if !self.window_open {
            return Vec::new();
        }
        match prepared {
            Ok(prepared) => {
                let mut effects = vec![Effect::Load(prepared.url.clone())];
                self.patch(
                    Some(Status::Active),
                    Some(prepared.message),
                    Some(prepared.url),
                );
                if self
                    .adapter
                    .as_ref()
                    .is_some_and(|adapter| adapter.inspects())
                {
                    effects.push(Effect::StartPolling);
                }
                effects.push(Effect::Focus);
                effects
            }
            Err(error) => {
                self.patch(Some(Status::Error), Some(error), None);
                vec![Effect::Show]
            }
        }
    }

    /// `did-navigate`: the window moved to another page.
    pub fn navigated(&mut self, url: &str) -> Vec<Effect> {
        if !self.window_open {
            return Vec::new();
        }
        self.patch(Some(Status::Loading), None, Some(url.to_owned()));
        self.inspect()
    }

    /// `did-finish-load`: the page has loaded.
    pub fn loaded(&mut self, url: &str) -> Vec<Effect> {
        if !self.window_open {
            return Vec::new();
        }
        self.patch(Some(Status::Active), None, Some(url.to_owned()));
        self.inspect()
    }

    /// `did-fail-load` for the main frame: the window is shown, and the
    /// Accounts window told why. Nothing is saved; the next look at the page
    /// says what it shows, as production's does.
    pub fn load_failed(&mut self, url: &str, description: &str) -> Vec<Effect> {
        let Some(adapter) = self.adapter.clone().filter(|_| self.window_open) else {
            return Vec::new();
        };
        let why = if description.trim().is_empty() {
            "error"
        } else {
            description.trim()
        };
        let url = if url.is_empty() {
            self.state.current_url.clone()
        } else {
            Some(url.to_owned())
        };
        self.patch(
            Some(Status::Error),
            Some(format!("{} could not load ({why}).", adapter.site().label)),
            url,
        );
        vec![Effect::Show]
    }

    /// `page-title-updated`.
    pub fn titled(&mut self, title: &str) {
        if self.window_open {
            self.state.title = Some(title.to_owned());
        }
    }

    /// `will-navigate` and `will-redirect`: whether the window may go to
    /// `url`. The login's callback never loads; the adapter completes it.
    pub fn may_navigate(&mut self, url: &str) -> (bool, Vec<Effect>) {
        let Some(adapter) = self.adapter.clone().filter(|_| self.window_open) else {
            return (true, Vec::new());
        };
        if !adapter.handles(url) {
            return (true, Vec::new());
        }
        self.patch(
            Some(Status::Loading),
            Some(format!("Completing {} login…", adapter.site().label)),
            Some(url.to_owned()),
        );
        (false, vec![Effect::Complete(url.to_owned())])
    }

    /// The 750 ms poll.
    pub fn tick(&mut self) -> Vec<Effect> {
        self.inspect()
    }

    fn inspect(&mut self) -> Vec<Effect> {
        let inspects = self
            .adapter
            .as_ref()
            .is_some_and(|adapter| adapter.inspects());
        if self.inspecting || self.completed || !self.window_open || !inspects {
            return Vec::new();
        }
        self.inspecting = true;
        vec![Effect::Inspect]
    }

    /// What a look at the page found; `None` when it took longer than 4 s
    /// (a page mid-navigation can leave a script unanswered).
    pub fn inspected(&mut self, outcome: Option<Outcome>) -> Vec<Effect> {
        self.inspecting = false;
        match outcome {
            Some(outcome) => self.apply(outcome),
            None => Vec::new(),
        }
    }

    /// What the adapter made of the callback.
    pub fn completed(&mut self, outcome: Result<Outcome, String>) -> Vec<Effect> {
        match outcome {
            Ok(outcome) => self.apply(outcome),
            Err(error) => {
                if !self.window_open {
                    return Vec::new();
                }
                self.patch(Some(Status::Error), Some(error), None);
                vec![Effect::Show]
            }
        }
    }

    /// `applyResult`.
    fn apply(&mut self, outcome: Outcome) -> Vec<Effect> {
        if self.completed || !self.window_open {
            return Vec::new();
        }
        let mut effects = Vec::new();
        if outcome.hide {
            effects.push(Effect::Hide);
        }
        if outcome.show {
            effects.push(Effect::Show);
        }
        if outcome.credential.is_some() || outcome.already_persisted {
            effects.extend(self.finish(outcome));
            return effects;
        }
        self.patch(outcome.status, outcome.message, None);
        if let Some(url) = outcome.navigate {
            effects.push(Effect::Load(url));
        }
        effects
    }

    /// `finish`: saves the credential (unless the application already
    /// did), once.
    fn finish(&mut self, outcome: Outcome) -> Vec<Effect> {
        if self.completed || self.finishing {
            return Vec::new();
        }
        self.finishing = true;
        self.pending_message = outcome
            .message
            .clone()
            .unwrap_or_else(|| "Login completed.".into());
        self.patch(
            Some(Status::Loading),
            Some("Saving login securely…".into()),
            None,
        );
        match outcome.credential {
            Some(credential) if !outcome.already_persisted => vec![Effect::Persist {
                credential,
                session: self.session,
            }],
            _ => self.persisted(self.session, Ok(())),
        }
    }

    /// Whether `auth.credentials.set` saved the credential.
    pub fn persisted(&mut self, session: u64, saved: Result<(), String>) -> Vec<Effect> {
        if session != self.session {
            return Vec::new();
        }
        self.finishing = false;
        match saved {
            Ok(()) => {
                self.completed = true;
                let message = std::mem::take(&mut self.pending_message);
                self.patch(Some(Status::Completed), Some(message), None);
                self.close_effects()
            }
            Err(error) => {
                self.patch(Some(Status::Error), Some(error), None);
                Vec::new()
            }
        }
    }

    /// The window was closed by the user rather than by the machine.
    pub fn window_closed(&mut self) -> Vec<Effect> {
        if !self.window_open {
            return Vec::new();
        }
        self.window_open = false;
        self.adapter = None;
        self.inspecting = false;
        self.emit(AuthState {
            status: Status::Cancelled,
            message: Some("Login window closed.".into()),
            ..AuthState::idle()
        });
        Vec::new()
    }

    /// The outside browser opened (or could not).
    pub fn external_launched(&mut self, opened: Result<(), String>) -> Vec<Effect> {
        if !self.external {
            return Vec::new();
        }
        match opened {
            Ok(()) => self.patch(
                Some(Status::Active),
                Some("Complete the OnlyFans login in the external browser.".into()),
                None,
            ),
            Err(error) => self.patch(Some(Status::Error), Some(error), None),
        }
        Vec::new()
    }

    /// What the outside browser's login captured, or why it ended.
    pub fn external_finished(
        &mut self,
        session: u64,
        credential: Result<Credential, String>,
    ) -> Vec<Effect> {
        if session != self.session || !self.external || self.completed {
            return Vec::new();
        }
        match credential {
            Ok(credential) => self.finish(Outcome {
                credential: Some(credential),
                message: Some("OnlyFans session captured.".into()),
                ..Outcome::default()
            }),
            Err(error) => {
                self.patch(Some(Status::Error), Some(error), None);
                Vec::new()
            }
        }
    }

    /// `saveManualOnlyFansCredential`: a session the user pasted. What was
    /// under way is cancelled first, as production's does, so its effects
    /// are handed back also when the pasted session is refused.
    pub fn save_manual_onlyfans(
        &mut self,
        input: &super::onlyfans::ManualInput,
    ) -> (Vec<Effect>, Result<(), String>) {
        let mut effects = self.cancel();
        let credential = match super::onlyfans::manual_credential(input) {
            Ok(credential) => credential,
            Err(error) => return (effects, Err(error)),
        };
        self.completed = false;
        self.finishing = false;
        self.emit(AuthState {
            site_category: Some("onlyfans".into()),
            status: Status::Loading,
            title: Some("Login: OnlyFans".into()),
            current_url: None,
            message: Some("Saving OnlyFans login securely…".into()),
        });
        effects.extend(self.finish(Outcome {
            credential: Some(credential),
            message: Some("OnlyFans session saved.".into()),
            ..Outcome::default()
        }));
        (effects, Ok(()))
    }

    /// `cancelAuthSession`: closes whatever is open and forgets it.
    pub fn cancel(&mut self) -> Vec<Effect> {
        let effects = self.close_effects();
        self.session += 1;
        self.completed = false;
        self.finishing = false;
        self.emit(AuthState::idle());
        effects
    }

    /// `closePopup`.
    fn close_effects(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        if self.external {
            self.external = false;
            effects.push(Effect::CloseExternal);
        }
        if self.window_open {
            self.window_open = false;
            effects.push(Effect::CloseWindow);
        }
        self.adapter = None;
        self.inspecting = false;
        effects
    }

    /// `emit` with only the fields given changed.
    fn patch(&mut self, status: Option<Status>, message: Option<String>, url: Option<String>) {
        if let Some(status) = status {
            self.state.status = status;
        }
        if let Some(message) = message {
            self.state.message = Some(message);
        }
        if let Some(url) = url {
            self.state.current_url = Some(url);
        }
    }

    fn emit(&mut self, state: AuthState) {
        self.state = state;
    }
}
