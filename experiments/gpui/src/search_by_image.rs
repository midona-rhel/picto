//! Search by Image (`runReverseImageSearch` in production's
//! `registerHandlers.mjs`): the item's picture is handed to a search
//! engine's own upload page, in a web view that is never shown, and the
//! results page it leads to opens in the browser.
//!
//! The steps are production's for every engine:
//!
//! 1. Load the engine's page.
//! 2. Run the engine's setup script. Where the engine keeps its upload
//!    control behind a button, the script presses that button.
//! 3. Put the picture in the page's file field as the system's file chooser
//!    would (production's `DOM.setFileInputFiles`; here the page's own
//!    `DataTransfer`, with the `input` and `change` events the field fires).
//! 4. Run the engine's submit script, where it has one.
//! 5. Wait for the address of a results page and open it in the browser.
//!    For SauceNAO the address opened is one that holds the uploaded
//!    picture's address, as production's is. SauceNAO's own results
//!    address, `search.php`, answers the upload form and shows no results
//!    when the browser opens it.
//!
//! A page that asks for a window of its own has it opened in the browser
//! instead.
//!
//! Three things differ from production:
//!
//! - A file larger than 1 MiB, or of a type other than JPEG, PNG, GIF and
//!   WebP, is sent as a JPEG whose long edge is at most 1,600 pixels
//!   ([`picture`]). The file is read, made smaller and written into the
//!   script of step 3 on a background thread while the engine's page loads.
//! - Yandex's setup script does not wait one second after it presses the
//!   button. It waits until Yandex's page is ready to take a file, which is
//!   100 to 200 ms after the press (the comment in [`config`] says how).
//! - TinEye is given 90 seconds to answer where production gives 30,
//!   because in November 2026 TinEye took between 15 and 63 seconds to
//!   search for a picture that it had not been sent before.
//!
//! It sends the user's picture to a third party, so it runs only from the
//! user's choice of an engine's row in an item menu; nothing starts it
//! ahead of that. Under a check, the engines' addresses are the local
//! fixtures' (`crate::net`), and the results address is written on
//! standard error instead of being opened.
use crate::i18n::t;
use crate::platform::login_window::{Event, LoginWindow, Options};
use base64::Engine as _;
use futures::channel::mpsc;
use gpui::{App, AppContext as _, AsyncApp, Task};
use serde_json::Value;
use std::io::Cursor;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// How long the search waits before it looks again at the window's events,
/// at the state of a script it started, or at the window's address.
/// Production looks every 100 ms (`setInterval(checkUrl, 100)`). A search
/// waits four or five times, and each wait ends up to 80 ms sooner when it
/// looks every 20 ms.
const POLL: Duration = Duration::from_millis(20);
/// The longest the page's load, and each of the engine's scripts, may take:
/// production's 15 s.
const STEP_LIMIT: Duration = Duration::from_secs(15);
/// The longest an engine may take to show a results address, counted from
/// the moment the picture is in the page's file field
/// (`waitForReverseSearchResult`'s 30 s).
const RESULT_LIMIT: Duration = Duration::from_secs(30);
/// The same for TinEye. Measured in November 2026, from this app and with
/// a plain HTTP request, TinEye answered in 15 to 63 seconds for a picture
/// it had not been sent before, and in about one second for a picture it
/// had been sent before.
const TINEYE_RESULT_LIMIT: Duration = Duration::from_secs(90);

/// A file of at most this many bytes is sent as it is, when its type is one
/// every engine reads ([`SENT_AS_IT_IS`]).
const ORIGINAL_LIMIT: usize = 1 << 20;
/// The types every engine reads.
const SENT_AS_IT_IS: [&str; 4] = ["image/jpeg", "image/png", "image/gif", "image/webp"];
/// The long edge, in pixels, of the JPEG sent in place of any other file.
/// No engine uses more: Yandex's own page makes a 4,014 × 6,000 picture
/// 1,124 × 1,680 before it uploads it.
const LONG_EDGE: u32 = 1600;
/// The quality of that JPEG, from 1 to 100.
const JPEG_QUALITY: u8 = 90;

/// Production's function `__waitFor(selector, timeout)`, which every
/// engine's scripts use. It waits until the page has an element that
/// matches the selector, and fails after `timeout` milliseconds (10,000
/// when none is given).
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

/// The search engines that Search by Image offers (production's
/// `REVERSE_IMAGE_SEARCH_ENGINES`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    TinEye,
    SauceNao,
    Yandex,
    Sogou,
    Bing,
}

// The item menu translates these names where it draws their rows. The
// next line is a marker that `i18n::guard` reads, so its words are fixed.
// i18n: translated where drawn
pub const ENGINES: [(Engine, &str); 5] = [
    (Engine::TinEye, "TinEye"),
    (Engine::SauceNao, "SauceNAO"),
    (Engine::Yandex, "Yandex Images"),
    (Engine::Sogou, "Sogou"),
    (Engine::Bing, "Bing Visual Search"),
];

impl Engine {
    /// Production's key for the engine.
    pub fn key(self) -> &'static str {
        match self {
            Engine::TinEye => "tineye",
            Engine::SauceNao => "saucenao",
            Engine::Yandex => "yandex",
            Engine::Sogou => "sogou",
            Engine::Bing => "bing",
        }
    }
}

/// One engine as production configures it (`createReverseSearchConfigs`).
struct Config {
    /// Production's key for the engine, used in error messages.
    key: &'static str,
    /// The address of the engine's page.
    url: &'static str,
    /// The setup script, run once the page has loaded. When it ends, the
    /// page has the file field.
    pre_setup: &'static str,
    /// The CSS selector of the page's file field.
    file_input: &'static str,
    /// The submit script, run once the picture is in the file field, for
    /// an engine whose page does not start the search by itself.
    post_setup: Option<&'static str>,
    /// Whether an address is one of the engine's results pages.
    is_result: fn(&str) -> bool,
    /// A script that runs in the results page and returns the address to
    /// open in the browser, for an engine whose results address cannot be
    /// opened as it is.
    portable_result: Option<&'static str>,
    /// The longest the engine may take to show a results address.
    result_limit: Duration,
}

fn config(engine: Engine) -> Config {
    match engine {
        Engine::TinEye => Config {
            key: engine.key(),
            url: "https://tineye.com/",
            pre_setup: "await __waitFor('#upload-box');",
            file_input: "input[type='file']",
            post_setup: None,
            is_result: |href| href.contains("tineye.com/search/"),
            portable_result: None,
            result_limit: TINEYE_RESULT_LIMIT,
        },
        Engine::SauceNao => Config {
            key: engine.key(),
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
            result_limit: RESULT_LIMIT,
        },
        Engine::Yandex => Config {
            key: engine.key(),
            url: "https://yandex.com/images/",
            // Production's script waits one second after it presses the
            // button. This one waits for the file field the button brings
            // up, and then until Yandex's page has given the field's parent
            // the class `Root_inited`. Measured in the page: the field is
            // there 50 to 100 ms after the press and the class 100 to
            // 200 ms after it, and a file put in the field before the class
            // is there is not uploaded. If Yandex stops using that class,
            // the script goes on one second after the field appeared, which
            // is longer than production waits.
            pre_setup: r#"await __waitFor('.input__cbir-button, [data-bem*="cbir"], .HeaderDesktopForm-CbirButton, button[aria-label*="image"]');
        const btn = document.querySelector('.input__cbir-button')
          || document.querySelector('[data-bem*="cbir"]')
          || document.querySelector('.HeaderDesktopForm-CbirButton')
          || document.querySelector('button[aria-label*="image"]');
        if (btn) btn.click();
        const input = await __waitFor("input[type='file']", 8000);
        const appeared = performance.now();
        while (!input.closest('.Root_inited') && performance.now() - appeared < 1000) {
          await new Promise(r => setTimeout(r, 20));
        }"#,
            file_input: "input[type='file']",
            post_setup: None,
            is_result: |href| {
                ["yandex.com", "yandex.eu", "yandex.ru"]
                    .iter()
                    .any(|host| href.contains(&format!("{host}/images/search")))
            },
            portable_result: None,
            result_limit: RESULT_LIMIT,
        },
        Engine::Sogou => Config {
            key: engine.key(),
            url: "https://pic.sogou.com/",
            pre_setup: r#"await __waitFor('#cameraIco, .camera-ico, [class*="camera"]');
        const btn = document.querySelector('#cameraIco') || document.querySelector('.camera-ico') || document.querySelector('[class*="camera"]');
        if (btn) { btn.click(); await new Promise(r => setTimeout(r, 800)); }
        await __waitFor("input[type='file']", 5000);"#,
            file_input: "input[type='file']",
            post_setup: None,
            is_result: |href| href.contains("/ris"),
            portable_result: None,
            result_limit: RESULT_LIMIT,
        },
        Engine::Bing => Config {
            key: engine.key(),
            url: "https://www.bing.com/images",
            pre_setup: r#"const btn = await __waitFor('#sb_sbi, #sb_sbip');
        btn.click();
        await __waitFor('#sb_fileinput', 8000);"#,
            file_input: "#sb_fileinput",
            // Bing's page uploads the file and goes to the results page as
            // soon as the file is in the field, so it needs no submit
            // script.
            post_setup: None,
            is_result: |href| {
                href.contains("bing.com/search?")
                    && (href.contains("?bcid=") || href.contains("&bcid="))
            },
            portable_result: None,
            result_limit: RESULT_LIMIT,
        },
    }
}

/// The file the user chose to search for.
pub struct Upload {
    /// The original in the library.
    pub path: PathBuf,
    /// Its type.
    pub mime: String,
}

/// What the engine's page is given: the file as it is, or a smaller JPEG of
/// it ([`picture`]).
struct Picture {
    /// The file's name, with the extension `jpg` when it is the JPEG.
    name: String,
    /// The type the page's file carries.
    mime: String,
    bytes: Vec<u8>,
}

/// The picture to send for a file whose bytes have been read.
///
/// A file of at most [`ORIGINAL_LIMIT`] bytes whose type every engine reads
/// is sent as it is. Any other file is decoded and sent as a JPEG whose
/// long edge is at most [`LONG_EDGE`] pixels, with white where the picture
/// is transparent. That keeps the upload short on a slow connection and
/// under the engines' size limits, and gives the engines a type they read
/// (a BMP or a TIFF, for example, becomes a JPEG). A file that cannot be
/// decoded (a video, for example) is sent as it is, and so is a file of a
/// type every engine reads whose JPEG would not be smaller.
///
/// Decoding a large picture takes tenths of a second, so this runs on a
/// background thread.
fn picture(upload: &Upload, bytes: Vec<u8>) -> Picture {
    let name = upload
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let read_by_every_engine = SENT_AS_IT_IS.contains(&upload.mime.as_str());
    if !(read_by_every_engine && bytes.len() <= ORIGINAL_LIMIT) {
        // A decoder can panic on a damaged file, and a panic on a
        // background thread would end the app (`document::guarded`).
        let jpeg = crate::viewer::document::guarded(|| smaller_jpeg(&bytes));
        if let Ok(jpeg) = jpeg
            && !(read_by_every_engine && jpeg.len() >= bytes.len())
        {
            return Picture {
                name: PathBuf::from(name)
                    .with_extension("jpg")
                    .to_string_lossy()
                    .into_owned(),
                mime: "image/jpeg".to_owned(),
                bytes: jpeg,
            };
        }
    }
    Picture {
        name,
        mime: upload.mime.clone(),
        bytes,
    }
}

/// The picture in `bytes` as a JPEG whose long edge is at most
/// [`LONG_EDGE`] pixels. A picture that is already that small keeps its
/// size.
fn smaller_jpeg(bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    let (width, height) = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()?
        .into_dimensions()?;
    let long = u64::from(width.max(height).max(1));
    let fitted = |side: u32| {
        let side = u64::from(side);
        (side * u64::from(LONG_EDGE).min(long) / long).max(1) as u32
    };
    // A JPEG is decoded at a half, a quarter or an eighth of its size when
    // that still covers the size wanted, which takes a fraction of the time
    // of a full decode.
    let mut image =
        crate::viewer::scaled::decode_bytes(bytes, (fitted(width), fitted(height)), true)?;
    if image.width().max(image.height()) > LONG_EDGE {
        image = image.resize(LONG_EDGE, LONG_EDGE, image::imageops::FilterType::Triangle);
    }
    // JPEG has no transparency: the picture is put on white, as a browser
    // shows a transparent picture on a white page.
    let opaque = if image.color().has_alpha() {
        let mut rgba = image.into_rgba8();
        for pixel in rgba.pixels_mut() {
            let alpha = u32::from(pixel[3]);
            for channel in 0..3 {
                pixel[channel] =
                    ((u32::from(pixel[channel]) * alpha + 255 * (255 - alpha)) / 255) as u8;
            }
        }
        image::DynamicImage::from(rgba).into_rgb8()
    } else {
        image.into_rgb8()
    };
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, JPEG_QUALITY)
        .encode_image(&opaque)?;
    Ok(jpeg)
}

/// Searches the engine for the file, from the user's click on the engine's
/// row, and opens the results page in the browser. A failure is shown in a
/// notification titled "Reverse image search failed", as production shows
/// it.
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

/// The engine's results address for the file. This is the whole search
/// except the opening of that address in the browser.
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
    cx.spawn(async move |cx| {
        // A file that cannot be read ends the search before any engine is
        // reached.
        let bytes = read
            .await
            .map_err(|error| format!("{}: {error}", upload.path.display()))?;
        let config = config(engine);
        // The picture is made, and written into the script that puts it in
        // the page's file field, on a background thread while the engine's
        // page loads. The script holds the whole picture as text, so
        // building it is not work for the thread that draws.
        let file_input = config.file_input;
        let put_in_field =
            cx.background_spawn(async move { inject(file_input, &picture(&upload, bytes)) });
        run(&config, put_in_field, local, cx).await
    })
}

/// Opens the hidden window, runs the search in it and closes it.
/// `put_in_field` is the script that puts the picture in the page's file
/// field, which a background thread is still building.
async fn run(
    config: &Config,
    put_in_field: Task<String>,
    local: Option<String>,
    cx: &mut AsyncApp,
) -> Result<String, String> {
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
    let outcome = search.drive(config, put_in_field, cx).await;
    cx.update(|cx| window.close(cx));
    outcome
}

/// One search: the hidden window, and the events that window reports.
struct Search<'a> {
    window: &'a LoginWindow,
    events: mpsc::UnboundedReceiver<Event>,
}

impl Search<'_> {
    /// The five steps of the module's documentation, to the results
    /// address.
    async fn drive(
        &mut self,
        config: &Config,
        put_in_field: Task<String>,
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

        let injected = self.evaluate(&put_in_field.await, cx).await;
        match injected {
            Some(Value::Bool(true)) => {}
            _ => return Err(format!("File input not found: {}", config.file_input)),
        }

        if let Some(post) = config.post_setup {
            self.step(post, "Post-setup timed out", cx).await?;
        }

        let result = self.result(config, cx).await?;
        match config.portable_result {
            Some(portable) => self.portable_result(config, portable, cx).await,
            None => Ok(result),
        }
    }

    /// Runs the engine's `portable_result` script in its results page and
    /// hands back the address the script returns.
    ///
    /// The window's address becomes the results page's as soon as the
    /// engine starts to answer, while the window still holds the search
    /// page (for about one second with SauceNAO, measured in WKWebView). A
    /// script started in the search page is lost when the results page
    /// takes its place. So the script is started only once the page itself
    /// says that its address is a results address, and it is started again
    /// if the page is replaced while it runs.
    async fn portable_result(
        &mut self,
        config: &Config,
        script: &str,
        cx: &mut AsyncApp,
    ) -> Result<String, String> {
        let since = Instant::now();
        loop {
            self.drain(cx)?;
            let in_results_page = match self.evaluate("JSON.stringify(location.href)", cx).await {
                Some(Value::String(href)) => (config.is_result)(&href),
                _ => false,
            };
            if in_results_page {
                match self.step(script, "Portable result timed out", cx).await? {
                    Some(Value::String(url)) => return Ok(url),
                    // The page was replaced while the script ran.
                    None => {}
                    Some(_) => return Err(format!("{}: no results address", config.key)),
                }
            }
            if since.elapsed() >= config.result_limit {
                return Err(format!(
                    "{}: timed out. Last URL: {}",
                    config.key,
                    self.window.current_url()
                ));
            }
            cx.background_executor().timer(POLL).await;
        }
    }

    /// The window's events since the last look. A page that asked for a
    /// window of its own has that address opened in the browser. The search
    /// ends with an error when the web view could not be made or the window
    /// was closed.
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

    /// Waits until the engine's page has loaded (`loadURL`), for at most
    /// [`STEP_LIMIT`].
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

    /// Runs one of an engine's scripts (the body of an async function) to
    /// its end, for at most [`STEP_LIMIT`], and hands back what it
    /// returned. A script whose page was replaced by another page while it
    /// ran (a form's submit does that) has ended and returned nothing.
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
                // Another page has replaced the one the script ran in.
                Some("left") | None => return Ok(None),
                _ => {}
            }
            if since.elapsed() >= STEP_LIMIT {
                return Err(timed_out.to_owned());
            }
            cx.background_executor().timer(POLL).await;
        }
    }

    /// Waits until the window is at one of the engine's results addresses
    /// and hands that address back (`waitForReverseSearchResult`). It fails
    /// when the engine's `result_limit` has passed.
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
            if since.elapsed() >= config.result_limit {
                return Err(format!("{}: timed out. Last URL: {href}", config.key));
            }
            cx.background_executor().timer(POLL).await;
        }
    }

    /// Runs `script` in the page and hands back the JSON it evaluates to,
    /// or nothing when the page could not run it.
    async fn evaluate(&self, script: &str, _cx: &mut AsyncApp) -> Option<Value> {
        self.window.eval(script).await.ok().flatten()
    }
}

/// The script that puts the picture in the page's file field, as choosing
/// it in the system's file chooser does: the field holds that one file and
/// fires `input` and `change`. The script evaluates to `true`, or to
/// `false` when the page has no such field.
fn inject(selector: &str, picture: &Picture) -> String {
    let data = base64::engine::general_purpose::STANDARD.encode(&picture.bytes);
    let selector = Value::String(selector.to_owned());
    let name = Value::String(picture.name.clone());
    let mime = Value::String(picture.mime.clone());
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
