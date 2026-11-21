//! The JavaScript that the adapters run in the page of the sign-in window
//! to read what the page shows.

/// Reads a booru's account page: whether the user is signed in, whether
/// the page is the options page, a login form or a browser check, and the
/// API key and the user id wherever the page shows them. It also clicks
/// "Accept" on the page's cookie notice if there is one.
pub(super) const ACCOUNT_API_SCRIPT: &str = r#"JSON.stringify((() => {
  const text = document.body?.innerText || '';
  const href = location.href;
  const consent = /agree to the usage of cookies according to our cookie policy/i.test(text)
    ? Array.from(document.querySelectorAll('button, a, [role="button"], input[type="button"], input[type="submit"]'))
        .find((element) => ((element.textContent || element.value || '').trim().toLowerCase() === 'accept'))
    : null;
  if (consent) consent.click();
  const inputs = Array.from(document.querySelectorAll('input, textarea'));
  const readInput = (...needles) => {
    for (const input of inputs) {
      const key = (input.name || input.id || '').toLowerCase();
      const value = (input.value || '').trim();
      if (value && needles.some((needle) => key.includes(needle))) return value;
    }
    return null;
  };
  const snippets = [
    text,
    href,
    ...inputs.map((input) => (input.value || '').trim()),
    ...Array.from(document.querySelectorAll('a[href]')).map((anchor) => anchor.getAttribute('href') || ''),
  ];
  const combined = snippets.map((value) => {
    const apiKey = value.match(/api[_-]?key=([a-f0-9]{16,})/i)?.[1] || null;
    const userId = value.match(/user[_-]?id=(\d+)/i)?.[1] || null;
    return apiKey ? { apiKey, userId } : null;
  }).find(Boolean);
  const apiKeyInput = readInput('api_key', 'api-key');
  const apiKey = /^[a-f0-9]{16,}$/i.test(apiKeyInput || '')
    ? apiKeyInput
    : text.match(/api[_ -]?key\s*[:\n]\s*([a-f0-9]{16,})/i)?.[1] || combined?.apiKey || null;
  const userId = readInput('user_id', 'user-id')
    || text.match(/user[_ -]?id\s*[:\n]\s*(\d+)/i)?.[1]
    || combined?.userId
    || null;
  const labels = Array.from(document.querySelectorAll('a')).map((anchor) =>
    (anchor.textContent || '').trim().toLowerCase().replace(/[^a-z]+/g, ' ').trim()
  );
  const hasChallenge = Boolean(document.querySelector('#challenge-running, #challenge-stage, .cf-challenge'))
    || /just a moment|verify (?:that )?you are human|checking your browser|cloudflare/i.test((document.title || '') + '\n' + text);
  return {
    href,
    title: document.title || null,
    acceptedConsent: Boolean(consent),
    hasChallenge,
    hasLoginForm: Boolean(document.querySelector('input[name="user"], input[name="username"], input[type="password"]')),
    authenticated: labels.some((label) => {
      const collapsed = label.replace(/ /g, '');
      return collapsed === 'logout' || collapsed.endsWith('logout');
    })
      || /account home/i.test(text)
      || (/[?&]page=account\b/i.test(href) && /[?&]s=home\b/i.test(href)),
    onOptions: /[?&]s=options(?:&|$)/i.test(href),
    apiKey,
    userId,
  };
})())"#;

/// Returns whether the page shows that the user is signed in: it has a
/// control to log out, or it has no password field and has a link or text
/// for the account, the profile or the dashboard.
pub(super) const SIGNED_IN_SCRIPT: &str = r#"JSON.stringify((() => {
  const normalize = (value) => (value || '').replace(/\s+/g, ' ').trim().toLowerCase();
  const hasLoginForm = Boolean(document.querySelector('input[type="password"], input[name*="password" i]'));
  const controls = Array.from(document.querySelectorAll('a, button, [role="button"], [role="link"], [role="menuitem"]'));
  const describe = (element) => ({
    label: normalize([element.textContent, element.getAttribute('aria-label'), element.getAttribute('title')].filter(Boolean).join(' ')),
    href: normalize(element.getAttribute('href')),
  });
  const logoutPattern = /\b(?:log ?out|sign ?out)\b/i;
  if (controls.some((element) => {
    const { label, href } = describe(element);
    return logoutPattern.test(label) || /\/(?:logout|sign_out)(?:[/?#]|$)/i.test(href);
  })) return true;
  if (hasLoginForm) return false;
  const authPattern = /\b(?:my account|account home|account settings|profile|dashboard)\b/i;
  return controls.some((element) => {
    const { label, href } = describe(element);
    const consent = /(?:cookie|privacy|tracking|consent)/i.test(label)
      && /(?:accept|agree|allow|reject|settings|preferences)/i.test(label);
    return !consent && (authPattern.test(label) || /\/(?:profile|account|dashboard)(?:[/?#]|$)/i.test(href));
  }) || authPattern.test(normalize(document.body?.innerText));
})())"#;

/// Reads ExHentai's verification page and returns whether it is a browser
/// check (`challenge`), a refusal (`denied`) or a page without any content
/// (`blank`).
pub(super) const VERIFICATION_SCRIPT: &str = r#"JSON.stringify((() => {
  const text = [
    document.title,
    document.body?.innerText || '',
    ...Array.from(document.images).flatMap((image) => [image.src, image.alt, image.title]),
  ].join(' ');
  return {
    challenge: /just a moment|verify (?:that )?you are human|checking your browser|cloudflare/i.test(text),
    denied: /sad\s*panda|sadpanda|kokomade/i.test(text),
    blank: (document.body?.innerText || '').trim().length === 0
      && document.links.length === 0
      && document.images.length === 0,
  };
})())"#;

/// Returns the web view's user agent as a request header. FANBOX's API
/// accepts a session only from the user agent that signed in.
pub(super) const BROWSER_HEADERS_SCRIPT: &str =
    r#"JSON.stringify({ 'user-agent': navigator.userAgent || '' })"#;

/// Returns a script that tells whether FANBOX's API at `check_url` accepts
/// the session. A script run in a web view cannot return the value of a
/// promise. The first run therefore starts the request and returns `null`,
/// the page stores the result in `window.__pictoFanboxSession` when it
/// arrives, and a later run returns the stored `true` or `false`.
pub(super) fn fanbox_session_script(check_url: &str) -> String {
    let url = serde_json::to_string(check_url).unwrap_or_else(|_| "''".into());
    format!(
        r#"JSON.stringify((() => {{
  const answer = window.__pictoFanboxSession;
  if (answer === true || answer === false) {{
    window.__pictoFanboxSession = undefined;
    return answer;
  }}
  if (answer !== null) {{
    window.__pictoFanboxSession = null;
    fetch({url}, {{ credentials: 'include', headers: {{ accept: 'application/json' }} }})
      .then((response) => response.ok ? response.json() : null)
      .then((payload) => {{ window.__pictoFanboxSession = Boolean(payload && payload.body !== null && payload.body !== undefined); }})
      .catch(() => {{ window.__pictoFanboxSession = false; }});
  }}
  return null;
}})())"#
    )
}
