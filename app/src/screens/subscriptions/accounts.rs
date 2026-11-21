//! The "Site accounts" dialog. On the left it lists every service that a
//! login can be saved for. On the right it shows, for the chosen service,
//! the state of its login, the buttons that log in and remove the login, and
//! the progress of a login that is under way (`crate::logins`).
//!
//! The list holds the services that own their logins. For each service the
//! dialog reads the number of the subscriptions' sources that use it, the
//! saved login, the last check of that login, and the open login problems
//! of the subscriptions that use it.
use super::format;
use crate::i18n::{en, t};
use crate::logins::{Logins, Status, onlyfans::ManualInput, sites::SUBSCRIBESTAR_HOSTS};
use crate::session::Session;
use crate::theme::{self, Text as _};
use crate::ui::{
    self,
    action_button::{ActionButton, Look as ButtonLook},
    field::Look,
    status_badge::Tone,
    text_input::{Kind, TextInput},
};
use gpui::{prelude::FluentBuilder as _, *};
use picto_core::subscriptions::sites::SiteEntry;
use serde_json::{Value, json};

/// One service as the dialog shows it.
#[derive(Clone)]
pub struct Account {
    pub site: &'static SiteEntry,
    /// The name that the dialog shows for the service. It is the site's
    /// name, except that E-Hentai's login is shown under the name ExHentai.
    pub name: String,
    pub domain: String,
    pub query_count: usize,
    pub credential_type: Option<String>,
    pub health: Option<Health>,
    pub issues: Vec<AuthIssue>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Health {
    pub status: String,
    pub checked_at: Option<String>,
    pub last_error: Option<String>,
}

/// An open login problem of a subscription that uses the service.
#[derive(Clone, Debug, PartialEq)]
pub struct AuthIssue {
    pub id: i64,
    pub message: String,
    pub detail: Option<String>,
    pub last_seen_at: String,
}

impl Account {
    /// Returns the label of the state of the service's login and the tone
    /// the label is drawn in. A service without a saved login is "Not
    /// signed in". A service with a saved login is "Needs attention" when
    /// the last check of the login failed or the service has an open login
    /// problem, and "Signed in" otherwise.
    pub fn status(&self) -> (&'static str, Tone) {
        let health = self
            .health
            .as_ref()
            .map(|health| health.status.to_lowercase())
            .unwrap_or_default();
        let unhealthy = matches!(
            health.as_str(),
            "unauthorized" | "expired" | "error" | "missing"
        ) || !self.issues.is_empty();
        match (&self.credential_type, unhealthy) {
            (Some(_), true) => (t("Needs attention"), Tone::Attention),
            (Some(_), false) => (t("Signed in"), Tone::Success),
            (None, _) => (t("Not signed in"), Tone::Idle),
        }
    }
}

/// Returns whether a subscription's problem is about a login. It is when
/// its kind is one of the four kinds in `KINDS`, or when its message or its
/// detail contains "credential", "login" or "auth".
pub fn is_auth_issue(kind: &str, message: &str, detail: Option<&str>) -> bool {
    // i18n: issue kinds and words matched, not shown
    const KINDS: [&str; 4] = [
        "unauthorized",
        "expired",
        "credential_missing",
        "credential_blocked",
    ];
    if KINDS.contains(&kind) {
        return true;
    }
    let haystack = format!("{message} {}", detail.unwrap_or_default()).to_lowercase();
    // i18n: words matched, not shown
    ["credential", "login", "auth"]
        .iter()
        .any(|word| haystack.contains(word))
}

/// Returns the services that own their logins, sorted by name. Each service
/// has the number of sources that use it, the type of its saved login, the
/// last check of that login, and the open login problems of the
/// subscriptions that use it.
///
/// `usage` holds, for each subscription, the sites of its sources, in the
/// order the subscriptions are listed. `credentials` holds each service's
/// id and the type of its saved login, and `health` holds each service's id
/// and the last check of its login. `issues` holds the subscriptions' open
/// problems as (subscription, id, kind, message, detail, last seen), newest
/// first.
pub fn accounts(
    usage: &[(i64, Vec<String>)],
    credentials: &[(String, String)],
    health: &[(String, Health)],
    issues: &[(i64, i64, String, String, Option<String>, String)],
) -> Vec<Account> {
    let owner = |site_id: &str| {
        format::site(site_id).map_or_else(
            || site_id.to_owned(),
            |site| site.credential_owner_site_id.to_owned(),
        )
    };
    let mut list: Vec<Account> = format::sites()
        .filter(|site| site.id == site.credential_owner_site_id)
        .map(|site| {
            let key = site.credential_owner_site_id;
            let query_count = usage
                .iter()
                .map(|(_, sites)| sites.iter().filter(|id| owner(id) == key).count())
                .sum();
            let mut found = Vec::new();
            for (subscription, sites) in usage {
                if !sites.iter().any(|id| owner(id) == key) {
                    continue;
                }
                found.extend(
                    issues
                        .iter()
                        .filter(|issue| issue.0 == *subscription)
                        .filter(|issue| is_auth_issue(&issue.2, &issue.3, issue.4.as_deref()))
                        .map(|issue| AuthIssue {
                            id: issue.1,
                            message: issue.3.clone(),
                            detail: issue.4.clone(),
                            last_seen_at: issue.5.clone(),
                        }),
                );
            }
            let (name, domain) = if site.id == "ehentai" {
                // i18n: a service's name and domain
                ("ExHentai".to_owned(), "exhentai.org".to_owned())
            } else {
                (site.name.to_owned(), site.domain.to_owned())
            };
            Account {
                site,
                name,
                domain,
                query_count,
                credential_type: credentials
                    .iter()
                    .find(|(id, _)| id == key)
                    .map(|(_, kind)| kind.clone()),
                health: health
                    .iter()
                    .find(|(id, _)| id == key)
                    .map(|(_, health)| health.clone()),
                issues: found,
            }
        })
        .collect();
    // Upper and lower case make no difference to the order.
    list.sort_by_cached_key(|account| account.name.to_lowercase());
    list
}

/// Returns how long ago the time `value` was, in minutes, hours or days
/// (`format::relative_time_in_days`).
fn relative(value: Option<&str>) -> String {
    format::relative_time_in_days(value, chrono::Local::now())
}

/// The content of the "Site accounts" dialog: the list of services and the
/// details of the chosen service.
pub struct Accounts {
    session: Entity<Session>,
    logins: Entity<Logins>,
    usage: Vec<(i64, Vec<String>)>,
    accounts: Option<Vec<Account>>,
    selected: Option<String>,
    busy: bool,
    message: Option<String>,
    subscribestar_host: &'static str,
    manual_open: bool,
    cookie: Entity<TextInput>,
    user_agent: Entity<TextInput>,
    x_bc: Entity<TextInput>,
    list_scroll: ScrollHandle,
    issue_scroll: ScrollHandle,
    /// True from the time `login_changed` sees a completed login until the
    /// services have been read again. While it is true, `login_changed`
    /// does not handle the same completed login a second time.
    finishing: bool,
    _subscriptions: Vec<Subscription>,
}

impl Accounts {
    /// Creates the dialog's content and reads the services. The service
    /// `focus_site` is chosen at first. Without it, the first service in
    /// the list is chosen.
    pub fn new(
        session: Entity<Session>,
        usage: Vec<(i64, Vec<String>)>,
        focus_site: Option<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let logins = Logins::global(&session, cx);
        let field = |cx: &mut Context<Self>| cx.new(|cx| TextInput::new(Kind::Form, cx));
        // The Cookie and X-BC fields hide the text that is typed in them.
        let secret = |cx: &mut Context<Self>| cx.new(|cx| TextInput::new(Kind::Form, cx).masked());
        let cookie = secret(cx);
        let user_agent = field(cx);
        let x_bc = secret(cx);
        let icons = crate::site_icons::entity(cx);
        let subscriptions = vec![
            cx.observe(&logins, |this, _, cx| this.login_changed(cx)),
            cx.observe(&cookie, |_, _, cx| cx.notify()),
            cx.observe(&user_agent, |_, _, cx| cx.notify()),
            cx.observe(&x_bc, |_, _, cx| cx.notify()),
            // The dialog is drawn again when the site icons change, which
            // happens when a service's icon has been loaded.
            cx.observe(&icons, |_, _, cx| cx.notify()),
        ];
        let mut accounts = Self {
            session,
            logins,
            usage,
            accounts: None,
            selected: focus_site,
            busy: false,
            message: None,
            subscribestar_host: SUBSCRIBESTAR_HOSTS[0],
            manual_open: false,
            cookie,
            user_agent,
            x_bc,
            list_scroll: ScrollHandle::new(),
            issue_scroll: ScrollHandle::new(),
            finishing: false,
            _subscriptions: subscriptions,
        };
        // A login that completed while the dialog was closed is handled
        // now, in the way a login that completes while the dialog is open
        // is handled.
        if accounts.logins.read(cx).state().status == Status::Completed {
            accounts.login_changed(cx);
        } else {
            accounts.refresh(|_, _| {}, cx);
        }
        accounts
    }

    /// Called when the dialog is asked for while it is already open. It
    /// chooses the service `site`, when one is given, and reads the
    /// services again.
    pub fn focus(&mut self, site: Option<String>, cx: &mut Context<Self>) {
        if let Some(site) = site {
            self.select(&site, cx);
        }
        self.refresh(|_, _| {}, cx);
    }

    /// Reads the logins, their checks and the login problems again, then
    /// runs `then`.
    fn refresh(
        &mut self,
        then: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let session = self.session.read(cx);
        let credentials = session.dispatch("auth.credentials.list", json!({}));
        let health = session.dispatch("auth.health.list", json!({}));
        let issues = session.run(|library| {
            library
                .auxiliary_read(
                    picto_library::database::WorkPriority::VisibleRead,
                    |connection| {
                        let mut statement = connection.prepare(
                            "SELECT subscription_id, issue_id, issue_kind, message, detail,
                                    last_seen_at
                             FROM subscription_issue
                             WHERE status != 'resolved'
                             ORDER BY subscription_id, last_seen_at DESC, issue_id DESC",
                        )?;
                        let rows = statement
                            .query_map([], |row| {
                                Ok((
                                    row.get::<_, i64>(0)?,
                                    row.get::<_, i64>(1)?,
                                    row.get::<_, String>(2)?,
                                    row.get::<_, String>(3)?,
                                    row.get::<_, Option<String>>(4)?,
                                    row.get::<_, String>(5)?,
                                ))
                            })?
                            .collect::<Result<Vec<_>, _>>()
                            .map_err(picto_library::LibraryError::from)?;
                        Ok(rows)
                    },
                )
                .unwrap_or_default()
        });
        cx.spawn(async move |this, cx| {
            let credentials = credentials.await.ok().and_then(Result::ok);
            let health = health.await.ok().and_then(Result::ok);
            let issues = issues.await.unwrap_or_default();
            let _ = this.update(cx, |this, cx| {
                let credentials = read_credentials(credentials.as_ref());
                let health = read_health(health.as_ref());
                let accounts = accounts(&this.usage, &credentials, &health, &issues);
                let keep = this.selected.as_ref().filter(|id| {
                    accounts
                        .iter()
                        .any(|account| account.site.id == id.as_str())
                });
                let selected = keep
                    .cloned()
                    .or_else(|| accounts.first().map(|account| account.site.id.to_owned()));
                let before = this
                    .selected_account()
                    .map(|account| (account.site.id, account.credential_type.is_some()));
                this.accounts = Some(accounts);
                this.selected = selected;
                let after = this
                    .selected_account()
                    .map(|account| (account.site.id, account.credential_type.is_some()));
                // The notice is cleared when the service shown, or whether
                // it has a login, changes.
                if before != after {
                    this.message = None;
                }
                then(this, cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn selected_account(&self) -> Option<&Account> {
        let selected = self.selected.as_deref()?;
        self.accounts
            .as_ref()?
            .iter()
            .find(|account| account.site.id == selected)
    }

    /// Chooses the service `site`. A login that is under way for another
    /// service is cancelled. When the chosen service changes, the notice is
    /// cleared, the manual login form is closed and its fields are emptied.
    fn select(&mut self, site: &str, cx: &mut Context<Self>) {
        let state = self.logins.read(cx).state().clone();
        if state.status != Status::Idle
            && state
                .site_category
                .as_deref()
                .is_some_and(|category| category != site)
        {
            self.logins.update(cx, |logins, cx| logins.cancel(cx));
        }
        if self.selected.as_deref() != Some(site) {
            self.message = None;
            self.manual_open = false;
            for field in [&self.cookie, &self.user_agent, &self.x_bc] {
                field.update(cx, |input, cx| input.clear(cx));
            }
        }
        self.selected = Some(site.to_owned());
        cx.notify();
    }

    /// Called when the state of the login under way changes. When a login
    /// has completed, the services are read again and then the login is
    /// ended (`Logins::cancel`). In a build that keeps logins in memory,
    /// the notice then says that the login is not kept after the app quits.
    fn login_changed(&mut self, cx: &mut Context<Self>) {
        let completed = self.logins.read(cx).state().status == Status::Completed;
        if completed && !self.finishing {
            self.finishing = true;
            self.refresh(
                |this, cx| {
                    this.logins.update(cx, |logins, cx| logins.cancel(cx));
                    // A build that keeps logins in memory says that the
                    // login will be gone when the app quits. A build that
                    // keeps them in the system keychain has nothing to say.
                    this.message = crate::logins::credentials_in_process().then(|| {
                        en("Login saved for this session; it is not kept after the app quits.")
                            .into()
                    });
                    this.finishing = false;
                },
                cx,
            );
        }
        cx.notify();
    }

    fn start_login(&mut self, cx: &mut Context<Self>) {
        let Some(account) = self.selected_account() else {
            return;
        };
        let site = account.site.id;
        let host = (site == "subscribestar").then_some(self.subscribestar_host);
        self.message = None;
        let started = self
            .logins
            .update(cx, |logins, cx| logins.start(site, host, cx));
        if let Err(error) = started {
            self.message = Some(error);
        }
        cx.notify();
    }

    fn cancel_login(&mut self, cx: &mut Context<Self>) {
        self.logins.update(cx, |logins, cx| logins.cancel(cx));
    }

    /// Deletes the chosen service's saved login (`auth.credentials.delete`)
    /// and reads the services again. When the app may not change logins
    /// (`logins::may_change_logins`), it only shows a notice that says so.
    fn remove(&mut self, cx: &mut Context<Self>) {
        let Some(account) = self.selected_account() else {
            return;
        };
        let owner = account.site.credential_owner_site_id;
        if !crate::logins::may_change_logins() {
            self.message = Some(crate::logins::KEYCHAIN_REFUSED.to_owned());
            cx.notify();
            return;
        }
        self.busy = true;
        self.message = None;
        cx.notify();
        let answer = self
            .session
            .read(cx)
            .call("auth.credentials.delete", json!({ "site_id": owner }));
        cx.spawn(async move |this, cx| {
            let removed = answer.await;
            let _ = this.update(cx, |this, cx| match removed {
                Ok(_) => {
                    crate::logins::report_local_login(&this.session, owner, cx);
                    this.refresh(
                        |this, cx| {
                            this.message = Some(t("Credential removed.").into());
                            this.busy = false;
                            cx.notify();
                        },
                        cx,
                    )
                }
                Err(error) => {
                    this.message = Some(error);
                    this.busy = false;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Saves the OnlyFans login that was typed into the Cookie, User-Agent
    /// and X-BC fields (`Logins::save_manual_onlyfans`). When it has been
    /// saved, the fields are emptied and the form is closed. An error is
    /// shown as the notice.
    fn save_manual(&mut self, cx: &mut Context<Self>) {
        let input = ManualInput {
            cookie: self.cookie.read(cx).value().to_string(),
            user_agent: self.user_agent.read(cx).value().to_string(),
            x_bc: self.x_bc.read(cx).value().to_string(),
        };
        self.message = None;
        let saved = self
            .logins
            .update(cx, |logins, cx| logins.save_manual_onlyfans(&input, cx));
        match saved {
            Ok(()) => {
                for field in [&self.cookie, &self.user_agent, &self.x_bc] {
                    field.update(cx, |input, cx| input.clear(cx));
                }
                self.manual_open = false;
            }
            Err(error) => self.message = Some(error),
        }
        cx.notify();
    }
}

/// Reads the answer to `auth.credentials.list`: each service's id and the
/// type of its saved login.
fn read_credentials(answer: Option<&Value>) -> Vec<(String, String)> {
    answer
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|record| {
            Some((
                record["site_id"].as_str()?.to_owned(),
                record["credential_type"].as_str()?.to_owned(),
            ))
        })
        .collect()
}

/// Reads the answer to `auth.health.list`: each service's id and the last
/// check of its login.
fn read_health(answer: Option<&Value>) -> Vec<(String, Health)> {
    answer
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|record| {
            Some((
                record["site_id"].as_str()?.to_owned(),
                Health {
                    status: record["status"].as_str()?.to_owned(),
                    checked_at: record["checked_at"].as_str().map(str::to_owned),
                    last_error: record["last_error"].as_str().map(str::to_owned),
                },
            ))
        })
        .collect()
}

// The functions below draw the dialog.

/// Builds the bordered square that holds a service's icon. The square is
/// `size` wide and its corners have the radius `radius`. The icon is `glyph`
/// wide. It is the service's own icon (`crate::site_icons`), or a globe
/// while there is none or it cannot be drawn.
fn mark(domain: &str, size: f32, glyph: f32, radius: f32, cx: &mut App) -> Div {
    let theme = theme::theme(cx);
    let globe = theme.text_tertiary();
    let content = match crate::site_icons::get(domain, cx) {
        Some(icon) => img(icon)
            .size(px(glyph))
            .object_fit(ObjectFit::Contain)
            .rounded(px(3.))
            .with_fallback(move || ui::icon("world", glyph, globe).into_any_element())
            .into_any_element(),
        None => ui::icon("world", glyph, globe).into_any_element(),
    };
    div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(radius))
        .border_1()
        .border_color(theme.border())
        .bg(theme.control_surface())
        .child(content)
}

/// Returns the colour of a service's dot in the list: the success colour
/// for `Tone::Success`, the danger colour for `Tone::Attention`, and the
/// colour of disabled text for every other tone.
fn dot_color(tone: Tone, cx: &App) -> Rgba {
    match tone {
        Tone::Success => rgb(theme::SUCCESS),
        Tone::Attention => rgb(theme::DANGER),
        _ => theme::theme(cx).text_disabled(),
    }
}

/// Builds one of the dialog's buttons. It is the shared action button, 30px
/// tall with 13px of padding at its left and right. A click calls `press`.
fn button(
    id: &'static str,
    label: impl Into<SharedString>,
    look: ButtonLook,
    disabled: bool,
    cx: &mut Context<Accounts>,
    press: fn(&mut Accounts, &mut Context<Accounts>),
) -> ActionButton {
    let view = cx.entity().downgrade();
    ui::action_button(id, look)
        .label(label)
        .height(30.)
        .padding(13.)
        .disabled(disabled)
        .on_click(move |_, _, cx| {
            let _ = view.update(cx, |this, cx| press(this, cx));
        })
}

impl Render for Accounts {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        div()
            .id("accounts")
            .size_full()
            .flex()
            .overflow_hidden()
            .bg(theme.rail())
            .font_family(theme::FONT_UI)
            .text(theme::TEXT_MD)
            .line_height(px(theme::LINE_HEIGHT_MD))
            .text_color(theme.text())
            .child(self.sidebar(window, cx))
            .child(self.detail(window, cx))
    }
}

impl Accounts {
    /// Builds the list of services at the left of the dialog. A row shows
    /// the service's icon, its name and a dot in the colour of the state of
    /// its login. A click on a row chooses the service.
    fn sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let accounts = self.accounts.clone().unwrap_or_default();
        let rows = accounts.iter().map(|account| {
            let (status, tone) = account.status();
            let selected = self.selected.as_deref() == Some(account.site.id);
            let site = account.site.id;
            div()
                .id(SharedString::from(format!("account-{site}")))
                .role(Role::Button)
                .aria_label(SharedString::from(account.name.clone()))
                .aria_selected(selected)
                .w_full()
                .h(px(34.))
                .px(px(8.))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(7.))
                .rounded(px(theme::RADIUS_MD))
                .cursor_pointer()
                .when(selected, |row| row.bg(theme.surface_active()))
                .when(!selected, |row| {
                    row.hover(move |style| style.bg(theme.surface_hover()))
                })
                .child(
                    mark(&account.domain, 22., 14., theme::RADIUS_CONTROL, cx)
                        .text(theme::TEXT_CAPTION),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(account.name.clone()),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("account-dot-{site}")))
                        .aria_label(status)
                        .size(px(6.))
                        .mr(px(1.))
                        .flex_none()
                        .rounded_full()
                        .bg(dot_color(tone, cx)),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.select(site, cx)))
        });
        div()
            .w(px(224.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .bg(theme.footer_surface())
            .border_r_1()
            .border_color(theme.border_secondary())
            .child(
                div()
                    .h(px(44.))
                    .flex_none()
                    .pl(px(16.))
                    .pr(px(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_color(theme.text_secondary())
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t("Services"))
                    .child(
                        div()
                            .min_w(px(24.))
                            .text_right()
                            .font_family(theme::FONT_MONO)
                            .font_weight(FontWeight::NORMAL)
                            .text(theme::TEXT_CAPTION)
                            .text_color(theme.text_tertiary())
                            .child(accounts.len().to_string()),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("accounts-services")
                            .role(Role::List)
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.list_scroll)
                            .pl(px(6.))
                            .pr(px(6. + ui::scrollbar::GUTTER))
                            .pb(px(8.))
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .children(rows),
                    )
                    .child(ui::scrollbar(
                        "accounts-services-scrollbar",
                        &self.list_scroll,
                        window,
                        cx,
                    )),
            )
    }

    /// Builds the details of the chosen service at the right of the dialog:
    /// a header with the icon, the name, the domain and the state badge,
    /// then the account panel, the sign-in panel and the panel of login
    /// problems. When no service is chosen, it shows a panel that says
    /// "Select a site".
    fn detail(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = theme::theme(cx);
        let Some(account) = self.selected_account().cloned() else {
            return div()
                .flex_1()
                .min_w_0()
                .h_full()
                .bg(theme.rail())
                .child(
                    div()
                        .m(px(12.))
                        .p(px(18.))
                        .rounded(px(theme::RADIUS_LG))
                        .border_1()
                        .border_color(theme.border_secondary())
                        .bg(theme.footer_surface())
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(t("Select a site")),
                        )
                        .child(t(
                            "Authentication is managed per site here, not per subscription.",
                        )),
                )
                .into_any_element();
        };
        let session = self.logins.read(cx).state().clone();
        let owner = account.site.credential_owner_site_id;
        let ours = session.site_category.as_deref() == Some(owner);
        let session_active = ours
            && matches!(
                session.status,
                Status::Starting | Status::Active | Status::Loading
            );
        let embedded = ours
            && matches!(
                session.status,
                Status::Starting | Status::Active | Status::Loading | Status::Error
            );
        let concrete = account
            .health
            .as_ref()
            .and_then(|health| health.last_error.clone())
            .or_else(|| account.issues.first().map(|issue| issue.message.clone()))
            .unwrap_or_default();
        let notice = if concrete.is_empty() {
            self.message.clone().unwrap_or_default()
        } else {
            concrete
        };
        let (state_label, tone) = account.status();

        let header = div()
            .h(px(76.))
            .flex_none()
            .px(px(18.))
            .flex()
            .items_center()
            .gap(px(12.))
            .border_b_1()
            .border_color(theme.border_secondary())
            .child(mark(&account.domain, 38., 22., theme::RADIUS_LG, cx).text(theme::TEXT_XL))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .text(theme::TEXT_XL)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(account.name.clone()),
                    )
                    .child(
                        div()
                            .mt(px(3.))
                            .truncate()
                            .text(theme::TEXT_SM)
                            .text_color(theme.text_tertiary())
                            .child(account.domain.clone()),
                    ),
            )
            .child(
                div()
                    .w(px(96.))
                    .flex_none()
                    .flex()
                    .justify_end()
                    .child(ui::status_badge::status_badge("accounts-state", tone, state_label).large()),
            );

        // The body has two columns. The first gets 1 part of the body's
        // width and is at least 300px wide. The second gets 0.75 parts and
        // is at least 220px wide. The body's width is the dialog's width
        // (900px, or the window's width less 48px when that is smaller),
        // less the dialog's 2px of border, the scrollbar's gutter, the
        // 224px list of services, the 24px of padding and the 10px gap
        // between the columns.
        let viewport = f32::from(window.viewport_size().width);
        let inner = 900_f32.min(viewport - 48.) - 2. - ui::scrollbar::GUTTER - 224. - 24. - 10.;
        let first = (inner / 1.75).max(300.);
        let second = (inner - first).max(220.);
        let body = div()
            .flex_1()
            .min_h_0()
            .p(px(12.))
            .flex()
            .gap(px(10.))
            .child(self.account_panel(&account, session_active, &notice, first, cx))
            .child(
                div()
                    .w(px(second))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(self.login_panel(&account, &session, embedded, cx))
                    .child(self.issues_panel(&account, window, cx)),
            );

        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(theme.rail())
            .child(header)
            .child(body)
            .into_any_element()
    }

    /// Builds the account panel: the rows of facts about the login, the
    /// buttons that log in, cancel a login and remove the login, the
    /// controls that only SubscribeStar and OnlyFans have, and the notice.
    fn account_panel(
        &mut self,
        account: &Account,
        session_active: bool,
        notice: &str,
        width: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = theme::theme(cx);
        let (state_label, _) = account.status();
        // "Not used by a subscription" and "queries" have no key in the
        // locale files, so they are shown in English in every language.
        let usage = match account.query_count {
            0 => en("Not used by a subscription").to_owned(),
            1 => format!("1 {}", t("query")),
            count => format!("{count} {}", en("queries")),
        };
        let checked = account
            .health
            .as_ref()
            .and_then(|health| health.checked_at.as_deref())
            .filter(|checked| !checked.is_empty())
            .map(|checked| relative(Some(checked)))
            .unwrap_or_else(|| t("Never").to_owned());
        let fact = |label: &'static str, value: String| {
            div()
                .h(px(34.))
                .flex()
                .items_center()
                .child(
                    div()
                        .w(px(106.))
                        .flex_none()
                        .text_color(theme.text_secondary())
                        .child(label),
                )
                .child(div().flex_1().min_w_0().truncate().child(value))
        };
        let busy = self.busy;
        let has_credential = account.credential_type.is_some();
        let mut actions = div()
            .min_h(px(32.))
            .mt(px(16.))
            .flex()
            .items_center()
            .gap(px(7.))
            .child(button(
                "accounts-log-in",
                if session_active {
                    t("Logging in…")
                } else if has_credential {
                    t("Refresh Login")
                } else {
                    t("Log In")
                },
                ButtonLook::Primary,
                busy || session_active,
                cx,
                |this, cx| this.start_login(cx),
            ));
        if session_active {
            actions = actions.child(button(
                "accounts-cancel",
                t("Cancel"),
                ButtonLook::Settings,
                busy,
                cx,
                |this, cx| this.cancel_login(cx),
            ));
        }
        if has_credential {
            actions = actions.child(div().ml_auto().child(button(
                "accounts-remove",
                t("Remove"),
                ButtonLook::Danger,
                busy,
                cx,
                |this, cx| this.remove(cx),
            )));
        }
        let view = cx.entity().downgrade();
        let subscribestar = (account.site.id == "subscribestar").then(|| {
            div()
                .mt(px(10.))
                .flex()
                .flex_col()
                .gap(px(9.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(div().w(px(76.)).flex_none().child(t("Service")))
                        .child(
                            ui::select(
                                "accounts-subscribestar-service",
                                self.subscribestar_host,
                                SUBSCRIBESTAR_HOSTS
                                    .iter()
                                    // i18n: host names, shown as they are
                                    .map(|host| ui::choice(host, *host))
                                    .collect(),
                            )
                            // The service cannot be changed while a login is
                            // under way or the login is being removed.
                            .disabled(busy || session_active)
                            .on_change(move |host, _, cx| {
                                let _ = view.update(cx, |this, cx| {
                                    if let Some(host) =
                                        SUBSCRIBESTAR_HOSTS.iter().find(|known| **known == host)
                                    {
                                        this.subscribestar_host = host;
                                        cx.notify();
                                    }
                                });
                            }),
                        ),
                )
                .child(div().text_color(theme.text_secondary()).child(t(
                    "One service login is stored at a time. Use a full creator URL in subscriptions; existing bare creator names use .art.",
                )))
        });
        let manual = (account.site.id == "onlyfans").then(|| {
            let toggle_disabled = busy || session_active;
            let mut manual = div().mt(px(10.)).child(
                div()
                    .id("accounts-manual-toggle")
                    .role(Role::Button)
                    .text(theme::TEXT_SM)
                    .text_color(theme.text_secondary())
                    .when(toggle_disabled, |toggle| toggle.opacity(0.35))
                    .when(!toggle_disabled, |toggle| {
                        toggle
                            .cursor_pointer()
                            .hover(move |style| style.text_color(theme.text()))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.manual_open = !this.manual_open;
                                cx.notify();
                            }))
                    })
                    .child(if self.manual_open {
                        t("Hide manual login")
                    } else {
                        t("Enter session manually…")
                    }),
            );
            if self.manual_open {
                let row = |label: &'static str, id: &'static str, input: &Entity<TextInput>| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(div().w(px(76.)).flex_none().child(label))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(ui::field(id, input, Look::Modal)),
                        )
                };
                let filled = [&self.cookie, &self.user_agent, &self.x_bc]
                    .iter()
                    .all(|input| !input.read(cx).value().is_empty());
                manual = manual.child(
                    div()
                        .mt(px(10.))
                        .flex()
                        .flex_col()
                        .gap(px(9.))
                        .child(row(t("Cookie"), "accounts-cookie", &self.cookie))
                        .child(row(
                            t("User-Agent"),
                            "accounts-user-agent",
                            &self.user_agent,
                        ))
                        .child(row(t("X-BC"), "accounts-x-bc", &self.x_bc))
                        .child(div().flex().justify_end().child(button(
                            "accounts-save-session",
                            t("Save Session"),
                            ButtonLook::Settings,
                            busy || !filled,
                            cx,
                            |this, cx| this.save_manual(cx),
                        ))),
                );
            }
            manual
        });
        let visible = !notice.is_empty();
        div()
            .id("accounts-account")
            .role(Role::Group)
            .aria_label(t("Account details"))
            .w(px(width))
            .flex_none()
            .h_full()
            .p(px(14.))
            // The padding at the right includes the width of a scrollbar,
            // so the content is as wide with a scrollbar as without one.
            .pr(px(14. + ui::scrollbar::GUTTER))
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .rounded(px(theme::RADIUS_LG))
            .border_1()
            .border_color(theme.border_secondary())
            .bg(theme.footer_surface())
            .child(fact(t("Account"), state_label.to_owned()))
            .child(fact(
                t("Credential"),
                account
                    .credential_type
                    .clone()
                    .unwrap_or_else(|| t("Not configured").to_owned()),
            ))
            .child(fact(t("Subscriptions"), usage))
            .child(fact(t("Last checked"), checked))
            .children(subscribestar)
            .child(actions)
            .children(manual)
            .child(
                div()
                    .min_h(px(34.))
                    .max_h(px(52.))
                    .mt(px(12.))
                    .px(px(9.))
                    .py(px(7.))
                    .flex_none()
                    .overflow_hidden()
                    .rounded(px(theme::RADIUS_MD))
                    .text(theme::TEXT_SM)
                    .line_height(px(18.))
                    .when(visible, |slot| {
                        slot.text_color(rgb(theme::FAILURE_TEXT))
                            .bg(rgba(0xff9a6614))
                            .child(notice.to_owned())
                    }),
            )
    }

    /// Builds the sign-in panel. It holds a text that says what signing in
    /// to this service does. While a login for this service is under way or
    /// has failed, it also holds a dot and the login's message, or the
    /// address of the page the login shows when it has no message.
    fn login_panel(
        &self,
        account: &Account,
        session: &crate::logins::AuthState,
        embedded: bool,
        cx: &App,
    ) -> impl IntoElement {
        let theme = theme::theme(cx);
        let name = &account.name;
        let description = if account.site.id == "pixiv" {
            en("Sign in on Pixiv. Picto captures the OAuth session when the site completes authentication.").to_owned()
        } else if account.site.id == "ehentai" {
            en("Sign in with your E-Hentai account. Picto verifies ExHentai access before saving the browser session.").to_owned()
        } else if account.site.credential_types.contains(&"api_key") {
            format!("Sign in on {name}. Picto retrieves and stores the account API credential.")
        } else {
            format!("Sign in on {name}. Picto stores the resulting browser session securely.")
        };
        let status = session
            .message
            .clone()
            .or_else(|| session.current_url.clone())
            .unwrap_or_else(|| en("Waiting for the site…").to_owned());
        div()
            .h(px(206.))
            .flex_none()
            .p(px(12.))
            .rounded(px(theme::RADIUS_LG))
            .border_1()
            .border_color(theme.border_secondary())
            .bg(theme.footer_surface())
            .child(div().font_weight(FontWeight::SEMIBOLD).child(t("Sign in")))
            .child(
                div()
                    .h(px(54.))
                    .mt(px(8.))
                    .mb(px(10.))
                    .overflow_hidden()
                    .text(theme::TEXT_SM)
                    .line_height(px(theme::LINE_HEIGHT_MD))
                    .text_color(theme.text_tertiary())
                    .child(description),
            )
            .child(
                div()
                    .h(px(34.))
                    .px(px(2.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .overflow_hidden()
                    .text(theme::TEXT_SM)
                    .text_color(theme.text_secondary())
                    .when(embedded, |slot| {
                        slot.child(
                            div()
                                .size(px(6.))
                                .flex_none()
                                .rounded_full()
                                .bg(theme.primary()),
                        )
                        .child(div().min_w_0().truncate().child(status))
                    }),
            )
    }

    /// Builds the "Runtime Issues" panel: the open login problems of the
    /// subscriptions that use the service. Each problem is shown with its
    /// message, its detail and the time it was last seen.
    fn issues_panel(
        &self,
        account: &Account,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = theme::theme(cx);
        let list: Vec<AnyElement> =
            if account.issues.is_empty() {
                vec![
                    div()
                        .py(px(9.))
                        .text(theme::TEXT_SM)
                        .text_color(theme.text_tertiary())
                        .child(t("No authentication issues"))
                        .into_any_element(),
                ]
            } else {
                account
                    .issues
                    .iter()
                    .map(|issue| {
                        div()
                            .py(px(8.))
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .border_b_1()
                            .border_color(theme.border_secondary())
                            .text(theme::TEXT_SM)
                            .child(
                                div()
                                    .truncate()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(issue.message.clone()),
                            )
                            .children(issue.detail.clone().map(|detail| {
                                div().text_color(theme.text_tertiary()).child(detail)
                            }))
                            .child(div().text_color(theme.text_tertiary()).child(format!(
                                "{}{}",
                                t("Seen "),
                                relative(Some(&issue.last_seen_at))
                            )))
                            .into_any_element()
                    })
                    .collect()
            };
        div()
            .flex_1()
            .min_h_0()
            .p(px(12.))
            .flex()
            .flex_col()
            .rounded(px(theme::RADIUS_LG))
            .border_1()
            .border_color(theme.border_secondary())
            .bg(theme.footer_surface())
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t("Runtime Issues")),
            )
            .child(
                div()
                    .relative()
                    .mt(px(9.))
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("accounts-issues")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.issue_scroll)
                            .pr(px(ui::scrollbar::GUTTER))
                            .children(list),
                    )
                    .child(ui::scrollbar(
                        "accounts-issues-scrollbar",
                        &self.issue_scroll,
                        window,
                        cx,
                    )),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{Health, Tone, accounts, is_auth_issue, read_credentials, read_health};
    use crate::i18n::t;
    use serde_json::json;

    fn usage(entries: &[(i64, &[&str])]) -> Vec<(i64, Vec<String>)> {
        entries
            .iter()
            .map(|(id, sites)| (*id, sites.iter().map(|site| (*site).to_owned()).collect()))
            .collect()
    }

    #[test]
    fn the_services_are_those_that_own_their_logins_sorted_by_name() {
        let list = accounts(&[], &[], &[], &[]);
        assert!(
            list.iter()
                .all(|account| account.site.id == account.site.credential_owner_site_id)
        );
        assert!(list.iter().all(|account| account.site.id != "pixivuser"));
        let names: Vec<String> = list
            .iter()
            .map(|account| account.name.to_lowercase())
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
        let ehentai = list
            .iter()
            .find(|account| account.site.id == "ehentai")
            .unwrap();
        assert_eq!(
            (ehentai.name.as_str(), ehentai.domain.as_str()),
            ("ExHentai", "exhentai.org")
        );
        // The interface leaves Tumblr out.
        assert!(list.iter().all(|account| account.site.id != "tumblr"));
    }

    #[test]
    fn a_services_queries_and_login_problems_come_from_the_subscriptions_using_it() {
        let usage = usage(&[
            (1, &["pixivuser", "pixiv", "e621"]),
            (2, &["e621"]),
            (3, &["patreon"]),
        ]);
        let issues = vec![
            (
                1,
                10,
                "unauthorized".to_owned(),
                "Login rejected".to_owned(),
                None,
                "2026-01-02T00:00:00Z".to_owned(),
            ),
            (
                1,
                11,
                "network".to_owned(),
                "Timed out".to_owned(),
                None,
                "2026-01-01T00:00:00Z".to_owned(),
            ),
            (
                3,
                12,
                "network".to_owned(),
                "Auth cookie refused".to_owned(),
                Some("403".to_owned()),
                "2026-01-03T00:00:00Z".to_owned(),
            ),
        ];
        let list = accounts(
            &usage,
            &[("pixiv".to_owned(), "oauth_token".to_owned())],
            &[(
                "pixiv".to_owned(),
                Health {
                    status: "Expired".into(),
                    ..Health::default()
                },
            )],
            &issues,
        );
        let find = |id: &str| list.iter().find(|account| account.site.id == id).unwrap();
        assert_eq!(find("pixiv").query_count, 2);
        assert_eq!(find("e621").query_count, 2);
        assert_eq!(find("patreon").query_count, 1);
        // Subscription 1's login problem is shown on every service that
        // subscription 1 uses.
        assert_eq!(
            find("pixiv")
                .issues
                .iter()
                .map(|issue| issue.id)
                .collect::<Vec<_>>(),
            [10]
        );
        assert_eq!(find("e621").issues.len(), 1);
        assert_eq!(find("patreon").issues[0].id, 12);
        assert_eq!(find("pixiv").status().1, Tone::Attention);
        assert_eq!(find("e621").status().1, Tone::Idle);
        assert_eq!(find("danbooru").query_count, 0);
    }

    #[test]
    fn a_saved_login_reads_signed_in_until_a_check_or_problem_says_otherwise() {
        let saved = [("patreon".to_owned(), "cookies".to_owned())];
        let list = accounts(&[], &saved, &[], &[]);
        let patreon = list
            .iter()
            .find(|account| account.site.id == "patreon")
            .unwrap();
        assert_eq!(patreon.status(), (t("Signed in"), Tone::Success));
        for status in ["unauthorized", "EXPIRED", "error", "missing"] {
            let health = [(
                "patreon".to_owned(),
                Health {
                    status: status.into(),
                    ..Health::default()
                },
            )];
            let list = accounts(&[], &saved, &health, &[]);
            let patreon = list
                .iter()
                .find(|account| account.site.id == "patreon")
                .unwrap();
            assert_eq!(
                patreon.status(),
                (t("Needs attention"), Tone::Attention),
                "{status}"
            );
        }
        let health = [(
            "patreon".to_owned(),
            Health {
                status: "unknown".into(),
                ..Health::default()
            },
        )];
        let list = accounts(&[], &saved, &health, &[]);
        let patreon = list
            .iter()
            .find(|account| account.site.id == "patreon")
            .unwrap();
        assert_eq!(patreon.status().1, Tone::Success);
    }

    #[test]
    fn login_problems_are_known_by_kind_or_words() {
        assert!(is_auth_issue("credential_missing", "x", None));
        assert!(is_auth_issue("network", "Login required", None));
        assert!(is_auth_issue("network", "x", Some("bad credential")));
        assert!(!is_auth_issue("network", "Timed out", Some("503")));
    }

    #[test]
    fn answers_are_read_by_service() {
        let credentials = read_credentials(Some(&json!([
            { "site_id": "pixiv", "credential_type": "oauth_token", "display_name": "Pixiv", "created_at": "x" }
        ])));
        assert_eq!(
            credentials,
            [("pixiv".to_owned(), "oauth_token".to_owned())]
        );
        let health = read_health(Some(&json!([
            { "site_id": "e621", "status": "valid", "checked_at": null, "last_error": "403" }
        ])));
        assert_eq!(health[0].1.last_error.as_deref(), Some("403"));
        assert!(read_credentials(None).is_empty());
    }
}
