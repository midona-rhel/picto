# Cloud Sync

[← User Guide](README.md)

> **Experimental.** Cloud sync is new and may change. Keep a separate backup of libraries you care
> about.

Cloud sync copies a library into the **Google Drive** or **Dropbox** folder on your computer. The
provider's desktop app uploads it. Picto does not sign in to the provider; the desktop app must be
installed and syncing a local folder. On Linux, Google Drive has no desktop app, so only Dropbox is
detected automatically.

What is stored in the cloud folder (under `picto/<library id>/`):

- **Recovery snapshots** — compressed, verified copies of the library database.
- **Originals** — every media file in the library.

## Enable sync for a library

1. Open the library, then open the Library Manager (**Library → Library Manager…**).
2. Select the library. Under **Cloud Sync**, click **Synchronize Library with Google Drive** or
   **with Dropbox** for a detected account, or **Connect Google Drive…** / **Connect Dropbox…** to
   choose the provider's folder yourself. The folder must be the root of an installed provider
   account.

You can also use the library's menu → **Enable Cloud Sync**.

After that, Picto publishes a new snapshot automatically once there are unsynced changes and the
library has been idle for about 30 seconds. The status bar at the bottom of the window and the
Library Manager show progress.

## Day-to-day controls

From the library's menu in the Library Manager, or **Settings → Cloud**:

| Action | What it does |
|---|---|
| Sync Now | Uploads changes and publishes a snapshot now |
| Pause Sync / Resume Sync | Stops or restarts automatic syncing |
| Create Recovery Snapshot | Publishes a snapshot now |
| Show Cloud Folder | Opens the cloud folder in your file manager |
| Stop Syncing | Turns sync off. Local files and the existing cloud copy are not deleted |

**Settings → Cloud** also shows the provider, folder, state, last sync time, and pending work:
mutations, files, and **Unavailable files** (originals missing both locally and in the cloud folder).

## Open a library on another computer

1. In the Library Manager, click **Open from Cloud…**.
2. Connect the Google Drive or Dropbox folder. Picto lists the Picto libraries it finds there.
3. Pick one, enter a **Local library name**, and click **Choose Location…**.
4. Choose where to store the local copy.

Picto restores the latest snapshot's database and opens the library. Originals are copied from the
cloud folder in the background. A library that is already added on this computer cannot be opened
twice.

## Restore a snapshot

**Settings → Cloud → Restore History** lists the recovery snapshots with date and size. Click
**Restore** next to one and confirm. Picto saves the current database as an emergency copy inside
the library folder before replacing it.

## Retention

**Settings → Cloud → Recovery Retention**:

| Setting | Range | Default |
|---|---|---|
| Daily snapshots | 2–365 | 30 |
| Weekly snapshots | 0–260 | 26 |
| Yearly snapshots | 0–100 | 5 |
| Keep deleted files | 0–3,650 days | 7 |

**Keep deleted files** controls how long an original you deleted from the library stays in the
cloud folder, so it can still be restored during that period. Picto removes it from the cloud
folder after that.
