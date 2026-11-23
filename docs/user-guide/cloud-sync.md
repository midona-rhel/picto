# Cloud Sync

[← User Guide](README.md)

> **Experimental.** Cloud sync is new and may change. Keep a separate backup of every library that
> matters to you.

Cloud sync copies a library into the **Google Drive** or **Dropbox** folder on your computer, and
the provider's desktop app uploads it from there. Picto doesn't sign in to the provider, so the
desktop app has to be installed and syncing a folder on this computer. Google Drive has no desktop
app for Linux, so on Linux Picto only finds Dropbox by itself.

Picto keeps three things in the cloud folder, under `picto/<library id>/`:

- Recovery snapshots: compressed copies of the library's database.
- Originals: the media files of the library.
- The changes each computer made to the library, such as an import, a new tag or a deletion. Each
  computer writes its own and reads the others'.

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

After that, Picto syncs by itself once the library has unsynced changes and has been idle for
about 30 seconds. A sync takes in the changes from your other computers, writes this computer's
changes, uploads the originals and saves a new snapshot. A bar along the bottom of the main window
shows the progress, and so does the Library Manager.

## Day-to-day controls

You'll find these commands in the **⋯** (Library actions) menu of the open library in the Library
Manager:

| Action | What it does |
|---|---|
| Sync Now | Syncs now instead of waiting. |
| Pause Sync, Resume Sync | Stops syncing by itself, or lets it start again. |
| Create Recovery Snapshot | Syncs now and saves a snapshot you can restore later. |
| Show Cloud Folder | Opens the library's cloud folder in Finder or your file manager. |
| Stop Syncing | Turns sync off after asking you. Your library and its copy in the cloud folder are kept. |
| Delete Cloud Data… | Deletes the library's copy in the cloud folder and turns sync off, after asking you. See below. |

**Settings → Cloud** has **Sync now**, **Pause** or **Resume**, and **Create snapshot** buttons that
do the same as the first three commands. It also shows the provider, the folder, whether sync is
working and when it last synced. **Pending Work** shows how many changes and files are waiting, and
how many **Unavailable files** there are: originals that are missing both on this computer and in
the cloud folder.

## Delete a library's cloud data

**Delete Cloud Data…** removes the library's copy from the cloud folder. You'll find it at the end
of the **Cloud Sync** card of the open library in the Library Manager, and in the **⋯** (Library
actions) menu.

When you confirm, Picto deletes the folder `picto/<library id>/` from the cloud folder, with its
snapshots, originals and changes. This can't be undone. Your library on this computer keeps all its
pictures and files, and sync is turned off for it; you can turn it on again later, and Picto then
writes new cloud data.

Your other computers keep their libraries. At their next sync they find that the cloud data is
gone, turn their own sync off and show "The cloud data of this library was deleted." in the Cloud
Sync card and in a notification. They don't delete anything.

Picto only removes files that its sync wrote. If you put something else into the library's cloud
folder, Picto leaves that file and its folder, and the message in the Library Manager names them.

A computer that still runs Picto 0.6.23 or older doesn't notice that the cloud data is gone and
writes a new snapshot at its next sync, so the folder comes back. Turn sync off on such a computer
first.

## Open a library on another computer

1. In the Library Manager, click **Open from Cloud…**.
2. Connect the Google Drive or Dropbox folder. Picto lists the Picto libraries that it finds
   there.
3. Pick a library, enter a **Local library name**, and click **Choose Location…**.
4. Choose where the local copy is stored.

Picto opens the library from the latest snapshot and then copies the originals from the cloud
folder, with a dialog that shows the progress. A library that is already in the list on this
computer can't be opened from the cloud a second time.

## Restore a snapshot

**Settings → Cloud → Restore History** lists the recovery snapshots of all your computers with
their date and size. Click **Restore** next to one and confirm. Picto closes the library, keeps the
current database as an emergency copy inside the library folder, replaces it with the snapshot and
opens the library again.

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

**Keep deleted files** is how many days the original of a permanently deleted picture stays in the
cloud folder. After that, the computer you deleted it on removes the original at its next sync,
unless that library has the file again. While the deletion can still be undone on that computer,
the original stays.

The other computers keep the file of a picture that sync deleted on them for the same number of
days, and for at least 7 days. If you undo the deletion, or a computer imports the file again in
that time, the picture comes back on them from that file. Picto doesn't count these files in the
library's statistics.
