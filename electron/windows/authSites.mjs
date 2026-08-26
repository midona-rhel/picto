const IDOL_COMPLEX_LOGIN_URL = 'https://login.idol.sankakucomplex.com/oidc/auth?response_type=code&scope=openid&client_id=idol-web-app&redirect_uri=https%3A%2F%2Fwww.idolcomplex.com%2Fsso%2Fcallback&state=return_uri%3Dhttps%3A%2F%2Fwww.idolcomplex.com%2Fen%2Flogin&theme=black&route=login&lang=en';
const SANKAKU_LOGIN_URL = 'https://login.sankakucomplex.com/oidc/auth?response_type=code&scope=openid&client_id=sankaku-web-app&redirect_uri=https%3A%2F%2Fsankaku.app%2Fsso%2Fcallback&state=return_uri%3Dhttps%3A%2F%2Fsankaku.app%2F&theme=black&route=login&lang=en';
const WIDE_AUTH_WINDOW = Object.freeze({ width: 1000, height: 760, minWidth: 760, minHeight: 640 });

const cookieSite = (id, label, loginUrl, cookieUrl, options = {}) => Object.freeze({
  id,
  label,
  loginUrl,
  cookieUrl,
  strategy: 'cookies',
  ...options,
});

export const AUTH_SITES = Object.freeze({
  pixiv: Object.freeze({ id: 'pixiv', label: 'Pixiv', strategy: 'pixiv' }),
  gelbooru: Object.freeze({
    id: 'gelbooru',
    label: 'Gelbooru',
    strategy: 'account_api',
    loginUrl: 'https://gelbooru.com/index.php?code=00&page=account&s=login',
    optionsUrl: 'https://gelbooru.com/index.php?page=account&s=options',
  }),
  rule34: Object.freeze({
    id: 'rule34',
    label: 'Rule34.xxx',
    strategy: 'account_api',
    loginUrl: 'https://rule34.xxx/index.php?code=00&page=account&s=login',
    optionsUrl: 'https://rule34.xxx/index.php?page=account&s=options',
  }),
  baraag: Object.freeze({
    id: 'baraag',
    label: 'Baraag',
    strategy: 'oauth2',
    loginUrl: 'https://baraag.net/auth/sign_in',
    registerUrl: 'https://baraag.net/api/v1/apps',
    authorizeUrl: 'https://baraag.net/oauth/authorize',
    tokenUrl: 'https://baraag.net/oauth/token',
  }),
  tumblr: Object.freeze({
    id: 'tumblr',
    label: 'Tumblr',
    strategy: 'oauth1',
    loginUrl: 'https://www.tumblr.com/login',
    requestTokenUrl: 'https://www.tumblr.com/oauth/request_token',
    authorizeUrl: 'https://www.tumblr.com/oauth/authorize',
    tokenUrl: 'https://www.tumblr.com/oauth/access_token',
    consumerKey: 'O3hU2tMi5e4Qs5t3vezEi6L0qRORJ5y9oUpSGsrWu8iA3UCc3B',
    consumerSecret: 'sFdsK3PDdP2QpYMRAoq0oDnw0sFS24XigXmdfnaeNZpJpqAn03',
  }),
  twitter: cookieSite(
    'twitter',
    'Twitter / X',
    'https://x.com/i/flow/login',
    'https://x.com',
    { authenticatedCookieNames: Object.freeze(['auth_token', 'ct0']) },
  ),
  newgrounds: cookieSite(
    'newgrounds',
    'Newgrounds',
    'https://www.newgrounds.com/login',
    'https://www.newgrounds.com',
    {
      cookieNames: Object.freeze(['ng_session']),
      authenticatedCookieNames: Object.freeze(['ng_session']),
      authWindowSize: WIDE_AUTH_WINDOW,
    },
  ),
  hentaifoundry: cookieSite(
    'hentaifoundry',
    'Hentai Foundry',
    'https://www.hentai-foundry.com/site/index',
    'https://www.hentai-foundry.com',
    { cookieNames: Object.freeze(['PHPSESSID']) },
  ),
  furaffinity: cookieSite(
    'furaffinity',
    'Fur Affinity',
    'https://www.furaffinity.net/login/',
    'https://www.furaffinity.net',
    {
      cookieNames: Object.freeze(['a', 'b']),
      authenticatedCookieNames: Object.freeze(['a', 'b']),
      allowStorageAccess: true,
      preserveUserAgent: true,
      resetSessionOnStart: true,
    },
  ),
  danbooru: cookieSite('danbooru', 'Danbooru', 'https://danbooru.donmai.us/session/new', 'https://danbooru.donmai.us'),
  webtoons: cookieSite(
    'webtoons',
    'Webtoons',
    'https://www.webtoons.com/member/login',
    'https://www.webtoons.com',
    { unauthenticatedUrlPattern: /\/(?:member\/(?:login|join)|(?:[a-z]{2}\/)?age-gate)(?:[/?#]|$)/i },
  ),
  // These are gallery-dl's public application credentials and callback. Using
  // the same OAuth client lets Picto hand the resulting refresh token straight
  // back to gallery-dl instead of inventing a parallel DeviantArt login path.
  deviantart: Object.freeze({
    id: 'deviantart',
    label: 'DeviantArt',
    strategy: 'oauth2',
    clientId: '5388',
    clientSecret: '76b08c69cfb27f26d6161f9ab6d061a1',
    authorizeUrl: 'https://www.deviantart.com/oauth2/authorize',
    tokenUrl: 'https://www.deviantart.com/oauth2/token',
    redirectUrl: 'https://mikf.github.io/gallery-dl/oauth-redirect.html',
    scope: 'browse user.manage',
    tokenField: 'refresh_token',
    cookieUrl: 'https://www.deviantart.com',
    cookieNames: Object.freeze(['auth', 'auth_secure', 'userinfo']),
    authWindowSize: WIDE_AUTH_WINDOW,
  }),
  patreon: cookieSite(
    'patreon',
    'Patreon',
    'https://www.patreon.com/login?l=en-GB',
    'https://www.patreon.com',
    { cookieNames: Object.freeze(['session_id']), authenticatedCookieNames: Object.freeze(['session_id']) },
  ),
  fanbox: cookieSite(
    'fanbox',
    'pixivFANBOX',
    'https://accounts.pixiv.net/login?prompt=select_account&return_to=https%3A%2F%2Fwww.fanbox.cc%2Fauth%2Fstart&source=fanbox',
    'https://api.fanbox.cc',
    {
      authenticatedCookieNames: Object.freeze(['FANBOXSESSID']),
    },
  ),
  subscribestar: cookieSite(
    'subscribestar',
    'SubscribeStar',
    'https://subscribestar.art/login',
    'https://subscribestar.art',
    { cookieNames: Object.freeze(['_personalization_id']), authenticatedCookieNames: Object.freeze(['_personalization_id']) },
  ),
  onlyfans: Object.freeze({
    id: 'onlyfans',
    label: 'OnlyFans',
    strategy: 'onlyfans',
    loginUrl: 'https://onlyfans.com/',
    cookieUrl: 'https://onlyfans.com',
    cookieNames: Object.freeze(['sess', 'auth_id', 'auth_uid']),
  }),
  idolcomplex: cookieSite(
    'idolcomplex',
    'Idol Complex',
    IDOL_COMPLEX_LOGIN_URL,
    'https://www.idolcomplex.com',
    {
      cookieNames: Object.freeze(['accessToken', 'refreshToken', 'ssoLoginValid']),
      authenticatedCookieNames: Object.freeze(['accessToken', 'refreshToken', 'ssoLoginValid']),
    },
  ),
  sankaku: cookieSite(
    'sankaku',
    'Sankaku',
    SANKAKU_LOGIN_URL,
    'https://sankaku.app',
    {
      cookieNames: Object.freeze(['accessToken', 'refreshToken', 'ssoLoginValid']),
      authenticatedCookieNames: Object.freeze(['accessToken', 'refreshToken', 'ssoLoginValid']),
    },
  ),
  yandere: cookieSite('yandere', 'Yande.re', 'https://yande.re/user/login', 'https://yande.re'),
  konachan: cookieSite('konachan', 'Konachan', 'https://konachan.com/user/login', 'https://konachan.com'),
  safebooru: cookieSite('safebooru', 'Safebooru', 'https://safebooru.org/index.php?page=account&s=login&code=00', 'https://safebooru.org'),
  e621: cookieSite('e621', 'e621', 'https://e621.net/session/new', 'https://e621.net'),
  ehentai: Object.freeze({
    id: 'ehentai',
    label: 'ExHentai',
    strategy: 'exhentai',
    loginUrl: 'https://forums.e-hentai.org/index.php?act=Login&CODE=00',
    verificationUrl: 'https://exhentai.org/',
    cookieUrls: Object.freeze([
      'https://forums.e-hentai.org',
      'https://e-hentai.org',
      'https://exhentai.org',
    ]),
    cookieNames: Object.freeze(['ipb_member_id', 'ipb_pass_hash', 'igneous']),
    authenticatedCookieNames: Object.freeze(['ipb_member_id', 'ipb_pass_hash']),
    allowStorageAccess: true,
    preserveUserAgent: true,
    resetSessionOnStart: true,
  }),
});

export function resolveAuthSite(siteId) {
  const requested = String(siteId || '').trim().toLowerCase();
  const canonical = requested === 'pixivuser'
    ? 'pixiv'
    : requested === 'exhentai'
      ? 'ehentai'
      : requested;
  return AUTH_SITES[canonical] ?? null;
}

export function getStaticAuthLoginRoutes() {
  return Object.values(AUTH_SITES)
    .filter((site) => site.loginUrl)
    .map((site) => ({ site: site.id, loginUrl: site.loginUrl }));
}
