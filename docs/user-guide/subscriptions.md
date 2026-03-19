# Subscriptions

[← User Guide](README.md)

Subscriptions automate downloading images and media from supported websites. You define what to download (queries), group them, and optionally schedule them to run automatically.

## Concepts

- **Subscription** — A single download source: a site + query (e.g., a tag search on a booru)
- **Group** — A container for related subscriptions that run together
- **Schedule** — When a group runs: manually, daily, weekly, or monthly
- **Credentials** — Login details for sites that require authentication

## Opening the Subscriptions Window

Click **Subscriptions** in the sidebar, or use the menu to open the subscriptions management window.

## Creating a Group

1. Click **New Group**
2. Enter a group name (e.g., "Landscape Art")
3. Choose a schedule: Manual, Daily, Weekly, or Monthly

## Adding Subscriptions to a Group

1. Select a group
2. Click **Add Subscription**
3. Choose a **site** from the catalog
4. Enter one or more **queries** (the search terms for that site)
5. Optionally set file limits:
   - **Initial file limit** — Maximum files on first run
   - **Periodic file limit** — Maximum files on subsequent runs

## Running a Group

Click the play button on a group to run all its subscriptions. Each subscription:

1. Connects to the site
2. Searches for matching content
3. Downloads new files not already in your library
4. Imports them into Picto (typically to the Inbox)

Progress is shown in both the subscriptions window and the sidebar status bar at the bottom.

## Stopping a Group

Click the stop button on a running group to cancel all active downloads. Already-downloaded files are kept.

## Schedules

| Schedule | Behavior |
|----------|----------|
| Manual | Only runs when you click play |
| Daily | Runs once per day automatically |
| Weekly | Runs once per week |
| Monthly | Runs once per month |

Scheduled runs happen in the background while Picto is open.

## Credentials

Some sites require authentication. Open the **Credentials** panel in the subscriptions window to manage login details.

### Adding Credentials

1. Select the site from the dropdown
2. Choose the credential type:
   - **Username + Password** — Standard login
   - **API Key** — For sites that use API authentication
   - **Cookies** — Paste cookie key=value pairs (one per line)
3. Enter the details and save

Credentials are stored securely in your system's credential store (Keychain on macOS, Credential Manager on Windows, Secret Service on Linux).

### Credential Health

Picto monitors credential status and warns about:
- Expired credentials
- Unauthorized (wrong password/key)
- Rate-limited accounts

## Download Settings

In [Settings](settings.md) → Download Services:

- **Rate Limit** — Delay between requests (0.5-30 seconds, default ~1s)
- **Batch Size** — Maximum files per run (or unlimited)
- **Abort Threshold** — Stop after N consecutive already-downloaded files (default: 10)

## Inbox Cap

Subscription downloads automatically pause when the [inbox](inbox-workflow.md) reaches 1,000 items. Review your inbox to resume downloads.

## Supported Sites

The list of supported sites is loaded from the built-in site plugin catalog. Each site has:
- Domain and display name
- Whether authentication is required
- Query format guidance

The exact list depends on your Picto version and may expand with updates.
