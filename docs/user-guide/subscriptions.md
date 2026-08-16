# Subscriptions

[← User Guide](README.md)

Subscriptions keep one subject up to date from one or more supported sources. Each query downloads
new posts, persists progress, and imports media continuously into Inbox.

## Concepts

- **Subscription** — A named subject with one or more source queries and one schedule
- **Query** — A search or account on one source, such as a Gelbooru tag or Pixiv user
- **Schedule** — When the subscription runs: manually, daily, weekly, or monthly
- **Account** — Login material for a source that requires authentication

## Adding a Subscription

1. Open **Subscriptions** from the sidebar
2. Click **New subscription**
3. Enter a name for the subject
4. Click **Add**

The subscription does not own a source. Add each source query from its detail view:

1. Choose a source
2. Enter the source-specific search or account
3. Click **Add**

Add more queries when the same subject should be followed across several sources.

## Running

Click **Run now** to run every enabled query. Each query:

1. Connects to its source
2. Discovers posts from its durable cursor
3. Skips media already recorded in its download archive
4. Downloads and imports new media into Inbox as it arrives
5. Imports every file from a multi-file post as an independent image or video, copying shared
   source-post metadata and preserving source order on each media entity

The subscription remains running until every query's downloads and imports have settled. A
successful run with no new media is shown as up to date.

Click **Stop** to cancel pending work and active downloads. Media already imported remains in the
library. The next run resumes from durable query and archive state.

## Schedules

| Schedule | Behavior |
|----------|----------|
| Manual | Only runs when you click **Run now** |
| Daily | Runs once per day |
| Weekly | Runs once per week |
| Monthly | Runs once per month |

The schedule belongs to the subscription and runs all enabled queries while Picto is open.

## Accounts

Open **Accounts** from the subscriptions view. Only the login methods supported by the selected
source are available. Pixiv search and Pixiv user queries share one Pixiv account. Gelbooru requires
both its account user ID and API key.

Secrets are stored in the operating system's credential store: Keychain on macOS, Credential
Manager on Windows, or Secret Service on Linux. Picto records health from real runs and reports
missing, expired, or rejected credentials.

## Inbox Limit

A subscription stops discovering new work when Inbox reaches the configured limit. Review or accept
Inbox media, then run the subscription again to continue from its saved position.

## Supported Sources

Picto supports:

- searches on supported booru sources
- creator/account feeds on supported art and social sources
- Pixiv searches and Pixiv users through one shared Pixiv login

The source picker is the current supported-source list. A source is release-ready only after its
production-path certification proves independent multi-file media, shared source metadata, source
order, restart/resume behavior, and its real Electron login and run workflow.
