//! Search by Image (`runReverseImageSearch` in production's
//! `registerHandlers.mjs`): the item's original file is handed to a search
//! engine's own upload page, in a web view that is never shown, and the
//! results page it leads to opens in the browser.
//!
//! The steps are production's for every engine: load the engine's page,
//! run its setup script (which opens the engine's upload control where it
//! is behind a button), put the file in the page's file field as the
//! system's file chooser would (production's `DOM.setFileInputFiles`; here
//! the page's own `DataTransfer`, with the `input` and `change` events the
//! field fires), run the engine's submit script where it has one, wait for
//! the address of a results page, and open that (for SauceNAO, a results
//! address that holds the uploaded picture's address instead, as
//! production's does). A page that asks for a window of its own has it
//! opened in the browser instead.
//!
//! It sends the user's file to a third party, so it runs only from the
//! user's choice of an engine's row in an item menu; nothing starts it
//! ahead of that. Under a check, the engines' addresses are the local
//! fixtures' (`crate::net`), and the results address is written on
//! standard error instead of being opened.
use crate::grid::menu::Engine;
use crate::i18n::t;
use crate::platform::login_window::{Event, LoginWindow, Options};
use base64::Engine as _;
use futures::channel::mpsc;
use gpui::{App, AppContext as _, AsyncApp, Task};
use serde_json::Value;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// `setInterval(checkUrl, 100)`
const POLL: Duration = Duration::from_millis(100);
/// The page load's and the setup's 15 s.
const STEP_LIMIT: Duration = Duration::from_secs(15);
/// `waitForReverseSearchResult`'s 30 s.
const RESULT_LIMIT: Duration = Duration::from_secs(30);

/// `__waitFor`, which every engine's scripts use.
const WAIT_FOR: &str = r#"
    function __waitFor(sel, timeout = 10000) {
      return new Promise((resolve, reject) => {
        if (document.querySelector(sel)) return resolve(document.querySelector(sel));
        const obs = new MutationObserver(() => {
          const el = document.querySelector(sel);
          if (el) { obs.disconnect(); resolve(el); }
        });
        obs.observe(document.documentElement, { childList: true, subtree: true, attributes: true });
        setTimeout(() => { obs.disconnect(); reject(new Error('Timeout waiting for: ' + sel)); }, timeout);
      });
    }
"#;

/// One engine as production configures it (`createReverseSearchConfigs`).
struct Config {
    key: &'static str,
    url: &'static str,
    pre_setup: &'static str,
    file_input: &'static str,
    post_setup: Option<&'static str>,
    is_result: fn(&str) -> bool,
    portable_result: Option<&'static str>,
}

fn config(engine: Engine) -> Config {
    match engine {
        Engine::TinEye => Config {
            key: "tineye",
            url: "https://tineye.com/",
            pre_setup: "await __waitFor('#upload-box');",
            file_input: "input[type='file']",
            post_setup: None,
            is_result: |href| href.contains("tineye.com/search/"),
            portable_result: None,
        },
        Engine::SauceNao => Config {
            key: "saucenao",
            url: "https://saucenao.com/",
            pre_setup: "await __waitFor('#searchForm');",
            file_input: "#fileInput",
            post_setup: Some(
                r#"const input = document.querySelector('#fileInput');
        if (!input?.files?.length) throw new Error('SauceNAO did not receive the image');
        document.querySelector('#searchForm')?.submit();"#,
            ),
            is_result: |href| href.contains("saucenao.com/search.php"),
            portable_result: Some(
                r#"const thumbnail = await __waitFor('#yourimage a img');
        const imageUrl = new URL(thumbnail.currentSrc || thumbnail.src, location.href).href;
        return 'https://saucenao.com/search.php?db=999&url=' + encodeURIComponent(imageUrl);"#,
            ),
        },
        Engine::Yandex => Config {
            key: "yandex",
            url: "https://yandex.com/images/",
            pre_setup: r#"await __waitFor('.input__cbir-button, [data-bem*="cbir"], .HeaderDesktopForm-CbirButton, button[aria-label*="image"]');
        const btn = document.querySelector('.input__cbir-button')
          || document.querySelector('[data-bem*="cbir"]')
          || document.querySelector('.HeaderDesktopForm-CbirButton')
          || document.querySelector('button[aria-label*="image"]');
        if (btn) { btn.click(); await new Promise(r => setTimeout(r, 1000)); }
        await __waitFor("input[type='file']", 8000);"#,
            file_input: "input[type='file']",
            post_setup: None,
            is_result: |href| {
                ["yandex.com", "yandex.eu", "yandex.ru"]
                    .iter()
                    .any(|host| href.contains(&format!("{host}/images/search")))
            },
            portable_result: None,
        },
        Engine::Sogou => Config {
            key: "sogou",
            url: "https://pic.sogou.com/",
            pre_setup: r#"await __waitFor('#cameraIco, .camera-ico, [class*="camera"]');
        const btn = document.querySelector('#cameraIco') || document.querySelector('.camera-ico') || document.querySelector('[class*="camera"]');
        if (btn) { btn.click(); await new Promise(r => setTimeout(r, 800)); }
        await __waitFor("input[type='file']", 5000);"#,
            file_input: "input[type='file']",
            post_setup: None,
            is_result: |href| href.contains("/ris"),
            portable_result: None,
        },
        Engine::Bing => Config {
            key: "bing",
            url: "https://www.bing.com/images",
            pre_setup: r#"const btn = await __waitFor('#sb_sbi, #sb_sbip');
        btn.click();
        await __waitFor('#sb_fileinput', 8000);"#,
            file_input: "#sb_fileinput",
            // Setting the file starts Bing's upload and its navigation.
            post_setup: None,
            is_result: |href| {
                href.contains("bing.com/search?")
                    && (href.contains("?bcid=") || href.contains("&bcid="))
            },
            portable_result: None,
        },
    }
}

/// The file handed to the engine.
pub struct Upload {
    /// The original in the library.
    pub path: PathBuf,
    /// Its type, which the page's file carries.
    pub mime: String,
}

/// Searches the engine for the file, from the user's click on the engine's
/// row. Failures are told as production tells them ("Reverse image search
/// failed").
pub fn search(engine: Engine, upload: Upload, cx: &mut App) {
    if crate::net::reach() == crate::net::Reach::Nowhere {
        eprintln!("picto: Search by Image reaches no engine in a check without local pages");
        return;
    }
    let found = results_address(engine, upload, cx);
    cx.spawn(async move |cx| {
        let outcome = found.await;
        cx.update(|cx| match outcome {
            Ok(url) => crate::net::open_in_browser(&url, cx),
            Err(message) => {
                eprintln!("picto: Search by Image failed: {message}");
                crate::ui::notify::error(t("Reverse image search failed"), message, cx)
            }
        });
    })
    .detach();
}

/// The engine's results address for the file: the whole search but the
/// opening of that address.
pub fn results_address(
    engine: Engine,
    upload: Upload,
    cx: &mut App,
) -> Task<Result<String, String>> {
    let local = match crate::net::reach() {
        crate::net::Reach::Local(origin) => Some(origin),
        _ => None,
    };
    let read = cx.background_spawn({
        let path = upload.path.clone();
        async move { std::fs::read(&path) }
    });
    cx.spawn(async move |cx| match read.await {
        Ok(bytes) => run(engine, &upload, bytes, local, cx).await,
        Err(error) => Err(format!("{}: {error}", upload.path.display())),
    })
}

async fn run(
    engine: Engine,
    upload: &Upload,
    bytes: Vec<u8>,
    local: Option<String>,
    cx: &mut AsyncApp,
) -> Result<String, String> {
    let config = config(engine);
    let (events, received) = mpsc::unbounded();
    let window = cx.update(|cx| {
        LoginWindow::open(
            Options {
                title: t("Search by Image").to_owned(),
                width: 1100.,
                height: 800.,
                min_width: 400.,
                min_height: 300.,
                intercept: Rc::new(|_| false),
                events,
                local_origin: local.clone(),
            },
            cx,
        )
    })?;
    let mut search = Search {
        window: &window,
        events: received,
    };
    let outcome = search.drive(&config, upload, bytes, cx).await;
    cx.update(|cx| window.close(cx));
    outcome
}

struct Search<'a> {
    window: &'a LoginWindow,
    events: mpsc::UnboundedReceiver<Event>,
}

impl Search<'_> {
    async fn drive(
        &mut self,
        config: &Config,
        upload: &Upload,
        bytes: Vec<u8>,
        cx: &mut AsyncApp,
    ) -> Result<String, String> {
        let start = match crate::net::reach() {
            crate::net::Reach::Local(origin) => crate::net::relocate(config.url, &origin),
            _ => config.url.to_owned(),
        };
        eprintln!("picto: Search by Image, {}: loading {start}", config.key);
        self.window.load(&start);
        self.loaded(cx).await?;

        self.step(config.pre_setup, "Pre-setup timed out", cx)
            .await?;

        let name = upload
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let injected = self
            .evaluate(&inject(config.file_input, &name, &upload.mime, &bytes), cx)
            .await;
        match injected {
            Some(Value::Bool(true)) => {}
            _ => return Err(format!("File input not found: {}", config.file_input)),
        }

        if let Some(post) = config.post_setup {
            self.step(post, "Post-setup timed out", cx).await?;
        }

        let mut result = self.result(config, cx).await?;
        if let Some(portable) = config.portable_result {
            match self.step(portable, "Portable result timed out", cx).await? {
                Some(Value::String(url)) => result = url,
                _ => return Err(format!("{}: no results address", config.key)),
            }
        }
        Ok(result)
    }

    /// The window's events since the last look: a page that asked for a
    /// window of its own goes to the browser; a page that failed, or a web
    /// view that could not be made, ends the search.
    fn drain(&mut self, cx: &mut AsyncApp) -> Result<Vec<Event>, String> {
        let mut seen = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            match &event {
                Event::OpenHere(url) => {
                    let url = url.clone();
                    cx.update(|cx| crate::net::open_in_browser(&url, cx));
                }
                Event::Unavailable(error) => return Err(error.clone()),
                Event::Closed => return Err("Search window was closed".to_owned()),
                _ => {}
            }
            seen.push(event);
        }
        Ok(seen)
    }

    /// `loadURL`, within 15 s.
    async fn loaded(&mut self, cx: &mut AsyncApp) -> Result<(), String> {
        let since = Instant::now();
        loop {
            for event in self.drain(cx)? {
                match event {
                    Event::Loaded(url) if url != "about:blank" => return Ok(()),
                    Event::Failed { description, .. } => return Err(description),
                    _ => {}
                }
            }
            if since.elapsed() >= STEP_LIMIT {
                return Err("Page load timed out".to_owned());
            }
            cx.background_executor().timer(POLL).await;
        }
    }

    /// Runs an engine's script (the body of an async function) to its end
    /// within 15 s, and hands back what it returned. A script that leaves
    /// the page (a form's submit) has ended.
    async fn step(
        &mut self,
        body: &str,
        timed_out: &str,
        cx: &mut AsyncApp,
    ) -> Result<Option<Value>, String> {
        let script = format!(
            r#"(function () {{
  {WAIT_FOR}
  window.__pictoStep = {{ state: 'running' }};
  (async () => {{ {body} }})().then(
    (value) => {{ window.__pictoStep = {{ state: 'done', value: value === undefined ? null : value }}; }},
    (error) => {{ window.__pictoStep = {{ state: 'error', message: String((error && error.message) || error) }}; }});
  return JSON.stringify(true);
}})()"#
        );
        if self.evaluate(&script, cx).await != Some(Value::Bool(true)) {
            return Err("The search page could not run its script".to_owned());
        }
        let since = Instant::now();
        loop {
            self.drain(cx)?;
            let state = self
                .evaluate(
                    "JSON.stringify(window.__pictoStep || { state: 'left' })",
                    cx,
                )
                .await;
            match state
                .as_ref()
                .and_then(|state| state.get("state")?.as_str())
            {
                Some("done") => {
                    return Ok(state.and_then(|mut state| state.get_mut("value").map(Value::take)));
                }
                Some("error") => {
                    let message = state
                        .as_ref()
                        .and_then(|state| state.get("message")?.as_str())
                        .unwrap_or_default();
                    return Err(message.to_owned());
                }
                // The page moved on (a submitted form).
                Some("left") | None => return Ok(None),
                _ => {}
            }
            if since.elapsed() >= STEP_LIMIT {
                return Err(timed_out.to_owned());
            }
            cx.background_executor().timer(POLL).await;
        }
    }

    /// `waitForReverseSearchResult`: the first results address the window
    /// is at, within 30 s.
    async fn result(&mut self, config: &Config, cx: &mut AsyncApp) -> Result<String, String> {
        let since = Instant::now();
        loop {
            for event in self.drain(cx)? {
                if let Event::Started(url) | Event::Loaded(url) = event {
                    if (config.is_result)(&url) {
                        return Ok(url);
                    }
                }
            }
            let href = self.window.current_url();
            if (config.is_result)(&href) {
                return Ok(href);
            }
            if since.elapsed() >= RESULT_LIMIT {
                return Err(format!("{}: timed out. Last URL: {href}", config.key));
            }
            cx.background_executor().timer(POLL).await;
        }
    }

    async fn evaluate(&self, script: &str, _cx: &mut AsyncApp) -> Option<Value> {
        self.window.eval(script).await.ok().flatten()
    }
}

/// The script that puts the file in the page's file field, as choosing it
/// in the system's file chooser does: the field holds that one file and
/// fires `input` and `change`.
fn inject(selector: &str, name: &str, mime: &str, bytes: &[u8]) -> String {
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    let selector = Value::String(selector.to_owned());
    let name = Value::String(name.to_owned());
    let mime = Value::String(mime.to_owned());
    format!(
        r#"(function () {{
  const input = document.querySelector({selector});
  if (!input) return JSON.stringify(false);
  const binary = atob("{data}");
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  const transfer = new DataTransfer();
  transfer.items.add(new File([bytes], {name}, {{ type: {mime} }}));
  input.files = transfer.files;
  input.dispatchEvent(new Event('input', {{ bubbles: true }}));
  input.dispatchEvent(new Event('change', {{ bubbles: true }}));
  return JSON.stringify(true);
}})()"#
    )
}
