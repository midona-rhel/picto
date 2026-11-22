//! The state machine of the one sign-in that can be under way. It holds
//! the state that the Accounts dialog shows, and decides what follows from
//! each event of the sign-in window, of the adapter and of the application.
//! It performs nothing itself: every function returns the [`Effect`]s that
//! `logins::Logins` then performs. The tests therefore call it directly,
//! without a window.
use super::adapters::{self, Adapter, Outcome, Prepared};
use super::sites::{self, Site, Strategy, WindowSize};
use super::{AuthState, Credential, Status};
use std::rc::Rc;

/// One thing that `logins::Logins` does when the machine returns it.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Opens the sign-in window. It stays hidden until its first page has
    /// loaded. Its web view has an empty storage that no other window
    /// shares.
    OpenWindow {
        title: String,
        size: WindowSize,
    },
    /// Runs the adapter's `prepare` and calls [`Machine::prepared`] with
    /// the result.
    Prepare,
    Load(String),
    /// Starts calling [`Machine::tick`] every 750 ms.
    StartPolling,
    Focus,
    Show,
    Hide,
    /// Runs the adapter's `inspect` for at most 4 s and calls
    /// [`Machine::inspected`] with the result.
    Inspect,
    /// Runs the adapter's `complete` with the address of a callback and
    /// calls [`Machine::completed`] with the result.
    Complete(String),
    /// Saves the credential with the command `auth.credentials.set` and
    /// calls [`Machine::persisted`] with `session` and the result.
    Persist { credential: Credential, session: u64 },
    /// Closes the sign-in window. The storage of its web view is deleted
    /// with it.
    CloseWindow,
    /// Opens OnlyFans in a browser that is installed on the computer,
    /// outside the app. Calls [`Machine::external_launched`] with whether
    /// the browser started, and later [`Machine::external_finished`] with
    /// `session` and the login that was read from the browser.
    LaunchExternal { session: u64 },
    /// Ends the login in that browser and closes the browser.
    CloseExternal,
}

/// Holds the sign-in that is under way and the state that the Accounts
/// dialog shows for it.
pub struct Machine {
    state: AuthState,
    adapter: Option<Rc<Adapter>>,
    window_open: bool,
    external: bool,
    inspecting: bool,
    finishing: bool,
    completed: bool,
    /// The message that the Accounts dialog shows after the credential has
    /// been saved.
    pending_message: String,
    /// The number of the current session. Every start, every cancel and
    /// every pasted session adds one to it. A result that arrives for an
    /// earlier number is ignored.
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

    /// Returns the state that the Accounts dialog shows.
    pub fn state(&self) -> &AuthState {
        &self.state
    }

    pub fn adapter(&self) -> Option<Rc<Adapter>> {
        self.adapter.clone()
    }

    /// Starts the sign-in for `site_id`, and for SubscribeStar for the
    /// service at `service_host`. A sign-in that is under way is cancelled
    /// first. Returns an error, before anything is cancelled or opened, if
    /// the service or the host is not known.
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

    /// Ends the login with the status `Error` and `error` as its message.
    /// It is called when the sign-in window could not be opened, for
    /// example because the WebView2 Runtime or WebKitGTK is not installed.
    pub fn could_not_open(&mut self, error: String) -> Vec<Effect> {
        self.window_open = false;
        self.adapter = None;
        self.patch(Some(Status::Error), Some(error), None);
        Vec::new()
    }

    /// Takes the result of the adapter's `prepare`. On success the window
    /// loads the returned address; on failure the window is shown and the
    /// state holds the error.
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

    /// Records that the sign-in window started to load the page at `url`,
    /// and asks for a look at the page.
    pub fn navigated(&mut self, url: &str) -> Vec<Effect> {
        if !self.window_open {
            return Vec::new();
        }
        self.patch(Some(Status::Loading), None, Some(url.to_owned()));
        self.inspect()
    }

    /// Records that the page at `url` has finished loading, and asks for a
    /// look at the page.
    pub fn loaded(&mut self, url: &str) -> Vec<Effect> {
        if !self.window_open {
            return Vec::new();
        }
        self.patch(Some(Status::Active), None, Some(url.to_owned()));
        self.inspect()
    }

    /// Records that the page at `url` could not be loaded. The window is
    /// shown, and the state gets the status `Error` and a message with
    /// `description`. Nothing is saved, and the login is not ended: the next
    /// look at the page sets the state again from what the page shows.
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

    /// Records the title of the page in the sign-in window.
    pub fn titled(&mut self, title: &str) {
        if self.window_open {
            self.state.title = Some(title.to_owned());
        }
    }

    /// Returns whether the sign-in window may load `url`. The address of
    /// the login's callback is not loaded: for it this returns false and
    /// the effect [`Effect::Complete`], which gives the address to the
    /// adapter.
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

    /// Asks for a look at the page. `logins::Logins` calls it every 750 ms
    /// while the window is open.
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

    /// Takes the result of a look at the page. `outcome` is `None` when
    /// the look took longer than 4 s, which happens when the page moves to
    /// another address while a script runs and the script never returns.
    pub fn inspected(&mut self, outcome: Option<Outcome>) -> Vec<Effect> {
        self.inspecting = false;
        match outcome {
            Some(outcome) => self.apply(outcome),
            None => Vec::new(),
        }
    }

    /// Takes the result of the adapter's `complete`, which ran with the
    /// address of the callback.
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

    /// Applies an [`Outcome`]: hides or shows the window, saves the
    /// credential if there is one, and otherwise sets the status and the
    /// message and loads the next address.
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

    /// Saves the credential of `outcome`, unless the application has
    /// already saved it (`already_persisted`). A second call while the first
    /// save is under way, or after it, does nothing.
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

    /// Takes the result of saving the credential. On success the login is
    /// completed, and the sign-in window or the browser outside the app is
    /// closed. On failure the state holds the error and nothing is closed.
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

    /// Records that the user closed the sign-in window. The login gets the
    /// status `Cancelled`.
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

    /// Takes whether the browser outside the app started.
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

    /// Takes the login that was read from the browser outside the app, or
    /// the reason why that login ended without one.
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

    /// Saves an OnlyFans session that the user pasted. A sign-in that is
    /// under way is cancelled first, also when the pasted session is then
    /// refused, so the effects of the cancellation are returned in both
    /// cases.
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

    /// Cancels the sign-in: closes the sign-in window or the browser
    /// outside the app, and sets the state back to idle.
    pub fn cancel(&mut self) -> Vec<Effect> {
        let effects = self.close_effects();
        self.session += 1;
        self.completed = false;
        self.finishing = false;
        self.emit(AuthState::idle());
        effects
    }

    /// Returns the effects that close the sign-in window and the browser
    /// outside the app, whichever is open, and drops the adapter.
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

    /// Changes those of the state's status, message and address that are
    /// given as `Some`, and leaves the others as they are.
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
