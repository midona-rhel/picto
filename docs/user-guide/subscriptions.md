# Subscriptions

[← User Guide](README.md)

A subscription follows one subject, for example an artist, a tag or a character, on one or more
sources, and downloads new posts into the Inbox.

- A **subscription** has a name, a schedule, a limit of posts per run, and the folders and tags
  that its new files get.
- A **query** (also called a source) is one search or one account on one site. A subscription can
  have many queries.

## Create a subscription

1. Open **Subscriptions** in the sidebar. The screen shows each subscription as a tile.
2. Click **Add** in the title bar, enter a name, and click **Add**.
3. Open the subscription. At the bottom of the **Sources** tab, pick a site, type the search or
   the account, and click **Add**. The field shows an example for the site you picked.
4. Add more queries if the same subject is on other sites.

A new subscription gets the schedule and the posts per run that are set in **Settings →
Subscriptions**. A new query gets the grouping that is set there.

## Supported sources

| Source | Query type | Example | Login |
|---|---|---|---|
| Pixiv | Search | `風景` | Required (Pixiv account) |
| Pixiv (user) | Account | `12345` | Required (the same Pixiv account) |
| Gelbooru | Search | `1girl solo` | Required (API key) |
| Rule34.xxx | Search | `1girl solo` | Required (API key) |
| Danbooru | Search | `1girl solo` | Optional |
| Safebooru | Search | `1girl solo` | Optional |
| Yande.re | Search | `landscape` | Optional |
| e621 | Search | `canine solo` | Optional |
| Hentai Foundry | Account | `username` | Optional |
| DeviantArt | Account | `username` | Optional |
| Twitter / X | Account | `username` | Required |
| Newgrounds | Account | `username` | Recommended for full access |
| Fur Affinity | Account | `username` | Recommended for full access |
| Patreon | Account | `creator-slug` | Recommended for full access |
| pixivFANBOX | Account | `creator-slug` | Recommended for full access |
| SubscribeStar | Account | `https://www.subscribestar.com/creator-slug` | Recommended for full access |
| OnlyFans | Account | `creator-name` | Required |
| OnlyHaven | Account | `onlyfans:12345` | None |
| Pawchive | Account (URL) | `https://pawchive.pw/patreon/user/12345` | None |
| E-Hentai / ExHentai | Gallery URL | `https://e-hentai.org/g/12345/67890abcde/` | Optional. ExHentai needs an E-Hentai login. |

A "Search" source takes tags or search text. An "Account" source takes a user name, an ID or the
URL of a creator. A Danbooru search without a login allows at most two tags.

Konachan, Sankaku, Idol Complex and Baraag are no longer supported. Subscriptions to them are
paused with a note that says so, and their saved logins are removed. The items that they imported
stay in the library.

E-Hentai is not in the list of sites of a query. See [Gallery downloads](#gallery-downloads).

## Accounts

Click **Accounts** in the title bar of the Subscriptions screen, or **Fix login** on a problem, to
open the **Site accounts** dialog. Pick a site and click **Log In**. Picto opens the site's own
login page in a sign-in window and stores the session, the OAuth token or the API key that the
login gives. **Refresh Login** renews the login, and **Remove** deletes it.

- Pixiv and Pixiv (user) share one login.
- SubscribeStar stores one login at a time. Choose the service (`.com`, `.adult` or `.art`) first.
- OnlyFans also takes a session that you enter by hand. Click **Enter session manually…** and fill
  in Cookie, User-Agent and X-BC.

Picto stores the logins in the credential store of the operating system: the Keychain on macOS,
the Credential Manager on Windows and the Secret Service on Linux.

The sign-in window uses the system's web view. On Windows it needs the WebView2 Runtime. On Linux
it needs WebKitGTK 4.1.

## Run a subscription

Open the subscription and click **Run now**, or right-click a subscription and choose **Run Now**.
When several subscriptions are selected, the right-click menu runs, holds or deletes all of them.

During a run:

- Each post is handled once. Media that is already in the library, or that was handled before, is
  skipped.
- New media goes to the **Inbox**. It is added to the subscription's folders and gets the
  subscription's tags. You set both under **New files** in the subscription, with **Add to
  folders** and **Add tags**.
- **Pause** stops the run so that it can be resumed. **Stop** ends the run. The posts that were
  already added stay.
- The next run continues where the last run stopped.

### Posts per run

**Posts per run** is a number from 1 to 10,000. A run stops when it has handled that many posts
for each source. Posts that were **added** and posts that were **skipped** both count. Failed posts
do not count. Set the default in **Settings → Subscriptions**, and change it for one subscription
in that subscription.

### Multi-file posts

By default a post with several files is imported as one collection after all of its files have
arrived. Each query row has a button that changes this for the query. Its tooltip is **Group
multi-media posts** or **Keep post media separate**. With **Keep post media separate**, each file
is imported as an item of its own. The default for new queries is in **Settings → Subscriptions**.

### Inbox limit

The work of the subscriptions waits when the Inbox holds as many top-level items as **Settings →
Subscriptions → Maximum Inbox items** says, which is 1,000 by default. The subscription then shows
**Inbox full**. Accept or reject items in the Inbox, then run the subscription again.

## Schedules

| Schedule | When the subscription runs |
|---|---|
| Manual | Only when you click **Run now**. |
| Daily | Once a day. |
| Weekly | Once a week. |
| Monthly | Once a month. |

Scheduled runs happen only while Picto is running.

## Hold, reset and edit

For a **subscription**, use the right-click menu or the buttons in the title bar:

- **Hold** skips the future runs until you choose **Release** (**Release Hold** in the menu).
- **Schedule**, **Rename** and **Set Cover Photo…** change the schedule, the name and the picture
  of the tile.
- **Reset Sync History…** makes Picto forget which posts it handled and scan every query from the
  beginning. The media and the logins are not changed. Media that you deleted may be downloaded
  again.
- **Delete Subscription…** deletes the subscription and its queries. The downloaded files stay in
  the library.

For a **query**, use the buttons on its row:

- **Run only this source now** runs this query alone.
- The hold button puts the query on hold until it is resumed. A subscription run skips a query
  that is on hold.
- The group button is described under [Multi-file posts](#multi-file-posts).
- **Edit** changes the site, the query, a display name and notes. The display name and the notes
  are optional.
- **Delete** stops the query. The downloaded files stay in the library.
- A double-click on the row opens **Source details**, with the numbers of posts and media, the
  last check, the state and the last error.

## History and problems

A subscription has three tabs: **Sources**, **History** and **Problems**.

- **History** lists the past runs with the time they started, their status and their counts.
- **Problems** lists the posts that failed and the problems of the sources. Each row has the
  status, the post, the reason and the time it was last seen. A click on the post opens it in your
  browser. A problem with a login has a **Fix login** button, which opens the **Site accounts**
  dialog at that site.

When a run stopped on a failure, the title bar has a **Retry** button.

A rail at the right of the subscription, in the place of the inspector, shows the totals: Posts
traversed, Posts added, Posts skipped, Files downloaded, Files already in library and Last check.
It also holds the schedule, the posts per run and the **New files** section. `Mod+Alt+2` hides or
shows the rail.

A subscription shows a warning only when it will stay broken until you act: when a run stopped on
a failure and waits for **Retry**, or when the last run of a source failed for a lasting reason.
The badge then reads **Login required** or **Failed**. A network error, a rate limit, a full Inbox
and single files that could not be downloaded do not show a warning. The **Problems** tab still
lists them.

## Gallery downloads

An E-Hentai or ExHentai gallery is downloaded once. It is not followed for new posts.

1. In Subscriptions, click **Add Gallery** in the title bar.
2. Choose **E-Hentai** or **ExHentai** and paste the URL of the gallery.
3. Click **Add Gallery**.

The gallery is imported into the Inbox as one collection. A bar above the subscription tiles has
one row for each gallery download, with its progress and with buttons to pause, resume, retry or
stop it. Only one gallery is downloaded at a time.
