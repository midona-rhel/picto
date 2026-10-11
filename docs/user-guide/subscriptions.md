# Subscriptions

[← User Guide](README.md)

A subscription follows one subject (an artist, a tag, a character) across one or more sources and
downloads new posts into the Inbox.

- **Subscription** — a name, a schedule, a posts-per-run limit, and optional folders and tags for
  new files.
- **Query** (source) — one search or one account on one site. A subscription can have many.

## Create a subscription

1. Open **Subscriptions** in the sidebar.
2. Click **Add**, enter a name, and click **Add**.
3. In the subscription, at the bottom of **Sources**, pick a site, type the search or account (the
   box shows an example for the chosen site) and click **Add**.
4. Add more queries if the same subject lives on other sites.

New subscriptions use the defaults from **Settings → Subscriptions** (schedule, posts per run,
grouping).

## Supported sources

| Source | Query type | Example | Login |
|---|---|---|---|
| Pixiv | Search | `風景` | Required (Pixiv account) |
| Pixiv (user) | Account | `12345` | Required (same Pixiv account) |
| Gelbooru | Search | `1girl solo` | Required (API key) |
| Rule34.xxx | Search | `1girl solo` | Required (API key) |
| Danbooru | Search | `1girl solo` | Optional |
| Safebooru | Search | `1girl solo` | Optional |
| Yande.re | Search | `landscape` | Optional |
| Konachan | Search | `landscape` | Optional |
| e621 | Search | `canine solo` | Optional |
| Idol Complex | Search | `solo` | Recommended for full access |
| Sankaku | Search | `solo rating:safe` | Recommended for full access |
| Hentai Foundry | Account | `username` | Optional |
| Baraag | Account | `username` | Optional |
| DeviantArt | Account | `username` | Optional |
| Twitter / X | Account | `username` | Required |
| Newgrounds | Account | `username` | Recommended for full access |
| Fur Affinity | Account | `username` | Recommended for full access |
| Patreon | Account | `creator-slug` | Recommended for full access |
| pixivFANBOX | Account | `creator-slug` | Recommended for full access |
| SubscribeStar | Account | `https://www.subscribestar.com/creator-slug` | Recommended for full access |
| OnlyFans | Account | `creator-name` | Required |
| Pawchive | Account (URL) | `https://pawchive.pw/patreon/user/12345` | None |
| E-Hentai / ExHentai | Gallery URL | `https://e-hentai.org/g/12345/67890abcde/` | Optional; ExHentai needs an E-Hentai login |

"Search" sources take tags or search text. "Account" sources take a user name, ID or creator URL.
Anonymous Danbooru searches allow at most two tags.

E-Hentai is not in the source picker. See [Gallery downloads](#gallery-downloads).

## Accounts

Click **Accounts** in the Subscriptions view (or **Fix login** on a problem) to open **Site
accounts**. Pick a site and click **Log In**: Picto opens the site's own login page and stores the
resulting session, OAuth token or API key. **Refresh Login** renews it and **Remove** deletes it.

- Pixiv and Pixiv (user) share one login.
- SubscribeStar stores one login at a time; choose the service (`.com`, `.adult`, `.art`) first.
- OnlyFans also accepts a session entered by hand (**Enter session manually…**: Cookie,
  User-Agent, X-BC).

Credentials are stored in the operating system's credential store (Keychain on macOS, Credential
Manager on Windows, Secret Service on Linux).

## Run a subscription

Click **Run now**, or right-click a subscription → **Run Now**. Right-click lets you run several
selected subscriptions at once.

During a run:

- Each post is processed once. Media already in the library or already handled is skipped.
- New media goes to the **Inbox**, into the subscription's folders and with its tags (set under
  **New files** in the subscription: **Automatically add to folders** and **Add tags**).
- **Pause** stops the run so it can be resumed; **Stop** ends it. Posts already added stay.
- The next run continues from where the last one stopped.

### Posts per run

**Posts per run** (1–10,000) caps how many posts a run settles. Both **added** and **skipped**
posts count toward the limit; failed posts do not. Set the default in **Settings → Subscriptions**
and override it per subscription.

### Multi-file posts

By default, a post with several files is imported as one collection after all of its files arrive
("Group multi-media posts"). Click the group button on a query row to switch it to **Keep post
media separate**, which imports each file as its own item. The default for new queries is in
**Settings → Subscriptions**.

### Inbox limit

Subscription work waits when the Inbox holds **Settings → Subscriptions → Maximum Inbox items**
top-level items (default 1,000). The subscription shows **Inbox full**. Accept or reject Inbox
items, then run again.

## Schedules

| Schedule | Runs |
|---|---|
| Manual | Only when you click **Run now** |
| Daily | Once a day |
| Weekly | Once a week |
| Monthly | Once a month |

Scheduled runs happen while Picto is running.

## Hold, reset and edit

**Subscription** (right-click, or the buttons in the header):

- **Hold** — skips future runs until you **Release** it.
- **Schedule**, **Rename**, **Set Cover Photo…**
- **Reset Sync History…** — forgets which posts were handled and scans every query from the
  beginning. Existing media and logins are untouched; media you deleted may download again.
- **Delete Subscription…** — downloaded files stay in the library.

**Query** (buttons on each row):

- **Run only this source now**
- **Put query on hold** / **Resume query** — the source is skipped in subscription runs while held
- Group toggle (see above)
- **Edit** — change the site, query, an optional display name and notes
- **Delete** — the query stops syncing; downloaded files stay
- Double-click a row for **Source details** (posts, media, last check, state, last error)

## History and problems

- **History** lists past runs with start time, status and counts.
- **Problems** lists failed posts and source issues with the reason. Use **Retry**, **Retry all**,
  **Open post**, **Fix login** or **Review source**.

The header shows totals: posts traversed, added and skipped, files downloaded and files already in
the library.

## Gallery downloads

E-Hentai and ExHentai galleries are downloaded one at a time, not followed.

1. In Subscriptions, click **Add Gallery**.
2. Choose **E-Hentai** or **ExHentai** and paste the gallery URL.
3. Click **Add Gallery**.

The gallery is imported into the Inbox as one collection. Progress appears under **Gallery
downloads**, where you can pause, resume, retry or stop it. Only one gallery downloads at a time.
