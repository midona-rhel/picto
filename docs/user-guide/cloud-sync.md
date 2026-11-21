# Cloud Sync

[← User Guide](README.md)

> **Experimental.** Cloud sync is new and may change. Keep a separate backup of every library that
> matters to you.

Cloud sync copies a library into the **Google Drive** or **Dropbox** folder on your computer. The
provider's desktop app uploads it from there. Picto does not sign in to the provider. The desktop
app must be installed, and it must sync a folder on this computer. Google Drive has no desktop app
for Linux, so on Linux Picto finds only Dropbox by itself.

Picto stores two things in the cloud folder, under `picto/<library id>/`:

- **Recovery snapshots**, which are compressed and verified copies of the library's database.
- **Originals**, which are all media files of the library.

## Enable sync for a library

1. Open the library, then open the Library Manager (**Library → Library Manager…**).
2. Select the library. In the **Cloud Sync** card, click **Synchronize Library with Google Drive**
   or **Synchronize Library with Dropbox** for an account that Picto found. Or click **Connect
   Google Drive…** or **Connect Dropbox…** to choose the provider's folder yourself. The folder
   must be the top folder of an installed provider account.

You can also click the **⋯** (Library actions) button of the library and choose **Enable Cloud
Sync**.

From then on Picto publishes a new snapshot by itself when the library has changes that are not
synced and has been idle for about 30 seconds. While the library syncs, a bar along the bottom of
the main window shows the progress. The Library Manager shows it too.

## Day-to-day controls

The **⋯** (Library actions) menu of the open library in the Library Manager has these commands:

| Action | What it does |
|---|---|
| Sync Now | It uploads the changes and publishes a snapshot at once. |
| Pause Sync, Resume Sync | It stops the automatic syncing, or starts it again. |
| Create Recovery Snapshot | It publishes a snapshot at once. |
| Show Cloud Folder | It opens the cloud folder in the system's file manager. |
| Stop Syncing | It turns sync off, after a question. The local files and the copy in the cloud folder are not deleted. |

**Settings → Cloud** has the buttons **Sync now**, **Pause** or **Resume**, and **Create
snapshot**, which do the same as the first three commands. It also shows the provider, the folder,
the state and the time of the last sync. Under **Pending Work** it shows the numbers of mutations,
of files, and of **Unavailable files**, which are originals that are missing both on this computer
and in the cloud folder.

## Open a library on another computer

1. In the Library Manager, click **Open from Cloud…**.
2. Connect the Google Drive or Dropbox folder. Picto lists the Picto libraries that it finds
   there.
3. Pick a library, enter a **Local library name**, and click **Choose Location…**.
4. Choose where the local copy is stored.

Picto puts the database of the latest snapshot in place and opens the library. It then copies the
originals from the cloud folder. A dialog over the main window shows the progress until the
copying is done. A library that is already in the list on this computer cannot be opened from the
cloud a second time.

## Restore a snapshot

**Settings → Cloud → Restore History** lists the recovery snapshots with their date and size.
Click **Restore** next to a snapshot and confirm. Picto closes the library, keeps the current
database as an emergency copy inside the library folder, puts the snapshot's database in its
place, and opens the library again.

## Retention

**Settings → Cloud → Recovery Retention** has these settings:

| Setting | Range | Default |
|---|---|---|
| Daily snapshots | 2 to 365 | 30 |
| Weekly snapshots | 0 to 260 | 26 |
| Yearly snapshots | 0 to 100 | 5 |
| Keep deleted files | 0 to 3,650 days | 7 |

**Keep deleted files** says for how many days an original that you deleted from the library stays
in the cloud folder. During that time it can still be restored. After that time Picto removes it
from the cloud folder.
