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
- **Tombstones**, which record what you deleted on each computer.

## What reaches your other computers

A second computer gets the library with **Open from Cloud…**. After that, each computer sends the
changes you make to the others through the cloud folder. For a picture that both computers have,
renaming it, changing its notes, rating or status, tagging it, putting it into a folder and
deleting it reach the other computer by themselves. So do deleting a tag or a folder. A picture is
recognised by the content of its file, so the same file imported on two computers is one picture.

Pictures that you import on one computer don't reach the others yet, because Picto doesn't fetch
their files from the cloud folder yet. Collections and the pictures inside them don't sync yet,
and neither do new tags and folders or the names and order of folders.

A computer sends its changes at its next sync and takes in the others' changes at its own next
sync, or within about 30 seconds while the library is open and idle. Both computers need the
provider's desktop app running. A change that arrives from another computer isn't in **Edit →
Undo**.

When two computers change the same thing, the later change wins on both. If you give a picture 3
stars on one computer at 10:00 and 5 stars on the other at 10:05, both end with 5 stars. Picto
doesn't rely on the clocks alone: whatever you do after a computer has taken in a change counts as
later than that change. A permanent deletion removes the picture everywhere until its file is
imported again. Deleting a tag takes it off the pictures that had it at that moment; tagging a
picture with it afterwards works normally.

When you turn on sync for a library that already has pictures, they are all added to what is
shared. The order in which changes arrive doesn't matter. If a file of changes in the cloud folder
is damaged, Picto takes in nothing, the sync state becomes **Needs attention** and the message
names the file; choose **Sync Now** to try again.

## Enable sync for a library

1. Open the library, then open the Library Manager (**Library → Library Manager…**).
2. Select the library. In the **Cloud Sync** card, click **Synchronize Library with Google Drive**
   or **Synchronize Library with Dropbox** for an account that Picto found. Or click **Connect
   Google Drive…** or **Connect Dropbox…** to choose the provider's folder yourself. The folder
   must be the top folder of an installed provider account.

You can also click the **⋯** (Library actions) button of the library and choose **Enable Cloud
Sync**.

From then on Picto syncs by itself when the library has changes that are not synced and has been
idle for about 30 seconds. A sync takes in the changes of your other computers, writes this
computer's changes, copies the originals, and publishes a new snapshot. While the library syncs,
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
| Delete Cloud Data… | It deletes the library's copy in the cloud folder and turns sync off, after a question. See below. |

**Settings → Cloud** has the buttons **Sync now**, **Pause** or **Resume**, and **Create
snapshot**, which do the same as the first three commands. It also shows the provider, the folder,
the state and the time of the last sync. Under **Pending Work** it shows the numbers of mutations,
of files, and of **Unavailable files**, which are originals that are missing both on this computer
and in the cloud folder.

## Delete a library's cloud data

**Delete Cloud Data…** removes the library's copy from the cloud folder. The button is at the end
of the **Cloud Sync** card of the open library in the Library Manager, and the **⋯** (Library
actions) menu has the same command.

After you confirm:

- The folder `picto/<library id>/` is removed from the cloud folder, with the snapshots, the
  originals and the changes in it. This cannot be undone.
- The library on this computer is not changed. It keeps every item and every file.
- Cloud sync is off for the library on this computer. You can enable it again later; Picto then
  writes new cloud data.
- Your other computers keep their local libraries. Each of them notices at its next sync that the
  cloud data is gone, turns its own sync off, and shows the message "The cloud data of this
  library was deleted." in the Cloud Sync card and in a notification. It deletes nothing.

Picto removes only files that its sync wrote, by their names. If the library's folder in the cloud
folder holds anything else, for example a file that you put there, Picto leaves that file and the
folder, and the message in the Library Manager names what was left.

A computer that still runs Picto 0.6.23 or older does not notice that the cloud data is gone. It
writes a new snapshot at its next sync, and the folder appears again. Turn sync off on such a
computer first.

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

A restore doesn't undo changes that were synced after the snapshot was made: they are in the cloud
folder, and the next sync applies them to the restored library again. To get back a picture that
was deleted, import its file again after the restore. That brings it back on every computer.

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
