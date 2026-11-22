# Cloud Sync

[← User Guide](README.md)

> **Experimental.** Cloud sync is new and may change. Keep a separate backup of every library that
> matters to you.

Cloud sync copies a library into the **Google Drive** or **Dropbox** folder on your computer. The
provider's desktop app uploads it from there. Picto does not sign in to the provider. The desktop
app must be installed, and it must sync a folder on this computer. Google Drive has no desktop app
for Linux, so on Linux Picto finds only Dropbox by itself.

Picto stores three things in the cloud folder, under `picto/<library id>/`:

- **Recovery snapshots**, which are compressed and verified copies of the library's database.
- **Originals**, which are all media files of the library.
- **Deletions**, which are lists of what you deleted on each computer.

## What reaches your other computers

A second computer gets the library with **Open from Cloud…**. It then has the items, tags and
folders that the library had at that moment.

After that, each computer keeps its own copy. Only one kind of change is carried from one computer
to the others by itself: a **deletion**, which is described in the next section. An item that you
import, tag, rate, rename or move into a folder on one computer does not appear on the others. To
take over everything that another computer has, restore one of its snapshots (see
[Restore a snapshot](#restore-a-snapshot)).

## Deletions

When you delete something on one computer, Picto deletes the same thing on every other computer
that syncs the library. This covers:

- moving an item or a collection to the Trash, and restoring it from the Trash;
- deleting an item or a collection permanently, which includes **Empty Trash** and an Inbox item
  that is rejected with the setting **Rejected items: Delete permanently**;
- deleting a tag, a folder or a smart folder.

The computer that made the deletion sends it at its next sync. Another computer takes it over at
its own next sync, or within about 30 seconds while Picto is open with the library and the library
is idle. Both computers must have the provider's desktop app running, because that app carries the
files.

A deletion that arrives from another computer is not in **Edit → Undo** on this computer. To get
an item back from the Trash, restore it; the restore is then carried to the other computers too.

### Rules when two computers disagree

- **Deleted permanently on one computer, edited on another.** The item stays deleted. The
  deletion wins over every edit, whatever was done first.
- **In the Trash on one computer, edited on another.** The item is in the Trash on both. The edit
  stays on the computer where it was made.
- **Moved to the Trash on one computer, restored on another.** The move that was made last wins
  on both computers. When the two moves were made before either computer had synced the other's,
  the computers' clocks decide which one was last.
- **A collection that holds other files on this computer.** A collection is moved to the Trash or
  deleted here only when every file it holds here was also in the collection on the computer that
  deleted it. If you added a file to it here, it stays as it is.
- **A folder or a smart folder with subfolders.** A folder is deleted here only when every folder
  inside it here was deleted too. If you made a subfolder here that the other computer never had,
  the folder stays with that subfolder. The items of a deleted folder stay in the library.
- **A tag, a folder or a smart folder with the same name.** Only the tag or folder that both
  computers got from the same library is deleted. One that you made here with the same name
  stays.
- **Undo.** Undo works on the computer where you press it. If you undo a deletion before the next
  sync, nothing is sent. If you undo a permanent deletion, or the deletion of a tag, a folder or a
  smart folder, after it was synced, it comes back on this computer and stays deleted on the
  others.

### What a deletion from another computer cannot do

- It deletes only what the other computer deleted. An item is matched by its identity in the
  library and by the content of its files. An item that you imported here is never deleted
  because another computer deleted something else.
- A computer that was switched off for a long time takes over the deletions it missed at its next
  sync. It does not bring a deleted item back to the others, and what was added on it stays.
- If a deletion file in the cloud folder is damaged, Picto deletes nothing. The sync state becomes
  **Needs attention**, and the message names the file. Choose **Sync Now** to try again. If a
  deletion file has not arrived yet, Picto waits and says so in the Cloud Sync card.
- What you delete while sync is switched off for the library is not sent when you switch it on
  again.

## Enable sync for a library

1. Open the library, then open the Library Manager (**Library → Library Manager…**).
2. Select the library. In the **Cloud Sync** card, click **Synchronize Library with Google Drive**
   or **Synchronize Library with Dropbox** for an account that Picto found. Or click **Connect
   Google Drive…** or **Connect Dropbox…** to choose the provider's folder yourself. The folder
   must be the top folder of an installed provider account.

You can also click the **⋯** (Library actions) button of the library and choose **Enable Cloud
Sync**.

From then on Picto syncs by itself when the library has changes that are not synced and has been
idle for about 30 seconds. A sync takes over the deletions of your other computers, sends this
computer's deletions, copies the originals, and publishes a new snapshot. While the library syncs,
a bar along the bottom of the main window shows the progress. The Library Manager shows it too.

## Day-to-day controls

The **⋯** (Library actions) menu of the open library in the Library Manager has these commands:

| Action | What it does |
|---|---|
| Sync Now | It syncs at once. |
| Pause Sync, Resume Sync | It stops the automatic syncing, or starts it again. |
| Create Recovery Snapshot | It syncs at once and publishes a snapshot. |
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

**Settings → Cloud → Restore History** lists the recovery snapshots of all your computers with
their date and size. Click **Restore** next to a snapshot and confirm. Picto closes the library,
keeps the current database as an emergency copy inside the library folder, puts the snapshot's
database in its place, and opens the library again.

A restore can bring back items that were deleted after the snapshot was made. Those items stay on
this computer: the deletions that were synced before the restore are not applied to the restored
library again. The other computers do not get the items back.

## Retention

**Settings → Cloud → Recovery Retention** has these settings:

| Setting | Range | Default |
|---|---|---|
| Daily snapshots | 2 to 365 | 30 |
| Weekly snapshots | 0 to 260 | 26 |
| Yearly snapshots | 0 to 100 | 5 |
| Keep deleted files | 0 to 3,650 days | 7 |

**Keep deleted files** says for how many days the original of an item that you deleted
permanently stays in the cloud folder. After that time, the computer on which you deleted the item
removes the original from the cloud folder at its next sync, unless the library on that computer
holds the file again. An item that can still be undone on that computer keeps its original until
the deletion leaves the undo history.
