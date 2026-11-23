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
- **Entries**, which record what you imported and what you deleted on each computer.

## What reaches your other computers

A second computer gets the library with **Open from Cloud…**. It then has the items, tags and
folders that the library had at that moment.

After that, each computer keeps its own copy, and two kinds of change are carried from one
computer to the others by themselves:

- an **import**: an item or a collection that you add on one computer appears on the others, as
  described in the next section;
- a **deletion**, which is described in the section after it.

A change to an item that the computers already have does not travel yet: a tag, a rating, a new
name, notes, or a move into a folder that you give an item on one computer does not appear on the
others. To take over everything that another computer has, restore one of its snapshots (see
[Restore a snapshot](#restore-a-snapshot)).

## Imports

An item that you import on one computer, by hand, from a watched folder or through a subscription,
appears on your other computers with the same file. It has the name, the notes, the source links,
the rating, the tags and the folders that it had when the first computer synced it, and it is in
the Inbox or in All as it was there. A collection arrives with its files in the same order and
with the same cover. A tag or a folder that the other computer does not have yet is created there;
a new folder appears at the top level. An import that you stop and undo is removed from the other
computers too.

The computer that imported the item writes its original and its entry at its next sync. Another
computer takes the entry in at its own next sync, or within about 30 seconds while Picto is open
with the library and the library is idle. It then copies the original from the cloud folder and
imports the item.

- **The original is not there yet.** The provider's desktop app carries the files, and an original
  can arrive later than the entry. Until it is there, and also while the provider's app is not
  running, the item waits. Picto looks for the original again every minute and shows no error.
- **Undo.** An item that arrives from another computer is not in **Edit → Undo** on this computer.
- **The same file on two computers.** If you import the same file on two computers, each keeps one
  item for it, and what you delete on one is deleted on the other. A collection on one computer
  that holds more files than the collection that was deleted on the other loses only the deleted
  files.
- **Deleted before it arrived.** An item that was deleted on the first computer before its
  original reached this one is not imported here.
- **What was there before.** The items that a library has when you first enable sync reach another
  computer through **Open from Cloud…**, and Picto writes no entry for them. From the first sync
  on, every import is recorded, also while sync is paused or off.

## Deletions

When you delete something, Picto writes a **tombstone** for it: a record that the thing was
deleted, and when. A tombstone is also what stops a subscription from bringing back an item that
you deleted. Cloud sync shares the tombstones as entries. Each computer writes its entries into the
cloud folder and takes in the entries of the others, so what is deleted on one computer is deleted
on all of them. This covers:

- deleting an item or a collection permanently, which includes **Empty Trash** and an Inbox item
  that is rejected with the setting **Rejected items: Delete permanently**;
- moving an item or a collection to the Trash, and restoring it from the Trash;
- deleting a tag, a folder or a smart folder.

The computer that made the deletion writes it at its next sync. Another computer takes it in at
its own next sync, or within about 30 seconds while Picto is open with the library and the library
is idle. Both computers must have the provider's desktop app running, because that app carries the
files.

A deletion that arrives from another computer is not in **Edit → Undo** on this computer.

### One rule: the later one wins

Every tombstone has a time. When two computers did different things to the same item, the thing
that was done later wins on both. Picto does not rely on the computers' clocks alone for that: what
you do on a computer after it has taken in a change is always later than that change.

- **Deleted, then imported again.** When you import an item again after it was deleted, or a
  subscription that you reset brings it again, the tombstone is removed, and that removal is shared
  too. The item stays on the computer that has it, no other computer deletes it because of the
  older tombstone, and the other computers import it again.
- **Imported again, then deleted.** A deletion that is made after the item was imported again is
  the later one. The item ends deleted on every computer.
- **Moved to the Trash on one computer, restored on another.** The later move wins on both.
- **Deleted permanently on one computer, edited or moved on another.** A permanent deletion is not
  undone by an edit or by a move into or out of the Trash. The item ends deleted on both.
- **In the Trash on one computer, edited on another.** The item is in the Trash on both. The edit
  stays on the computer where it was made.
- **Undo.** If you undo a deletion, the tombstone is removed like after an import. An item that
  another computer has already deleted is imported there again, as long as its original is still
  in the cloud folder (see **Keep deleted files** under [Retention](#retention)). A tag or a folder
  that another computer has already deleted does not come back there.

### What stays safe

- A tombstone deletes the one thing it names. An item that you imported here after the deletion
  was made stays.
- A folder or a smart folder is deleted here only when every folder inside it here was deleted
  too. If you made a subfolder here that the other computer never had, the folder stays with that
  subfolder. The items of a deleted folder stay in the library.
- A tag, a folder or a smart folder that you made here with the same name as a deleted one stays.
  Only the one that both computers got from the same library is deleted.
- A computer that was switched off for a long time takes in the tombstones it missed at its next
  sync. It does not bring a deleted item back to the others, and what was added on it stays.
- The order in which the files arrive does not matter, and a file that arrives twice changes
  nothing.
- If an entry file in the cloud folder is damaged, Picto takes in nothing and changes nothing.
  The sync state becomes **Needs attention**, and the message names the file. Choose **Sync Now**
  to try again.
- An entry file that the provider's desktop app has not downloaded yet is taken in when it is
  there. Until then Picto shows no error.

## Enable sync for a library

1. Open the library, then open the Library Manager (**Library → Library Manager…**).
2. Select the library. In the **Cloud Sync** card, click **Synchronize Library with Google Drive**
   or **Synchronize Library with Dropbox** for an account that Picto found. Or click **Connect
   Google Drive…** or **Connect Dropbox…** to choose the provider's folder yourself. The folder
   must be the top folder of an installed provider account.

You can also click the **⋯** (Library actions) button of the library and choose **Enable Cloud
Sync**.

From then on Picto syncs by itself when the library has changes that are not synced and has been
idle for about 30 seconds. A sync takes in the entries of your other computers, copies the
originals, writes this computer's entries, and publishes a new snapshot. While the library syncs,
a bar along the bottom of the main window shows the progress. The Library Manager shows it too.

### A library that is already in the cloud folder

A library reaches a second computer with **Open from Cloud…**. If the second computer has a copy
of the same library that never synced, for example one that was copied there by hand, Picto does
not connect that copy to the cloud folder and says so: nothing would tell the two computers what
each copy has that the other lacks. Open the library with **Open from Cloud…** instead. To bring
in what only the old copy has, merge the old copy into the opened library with **Merge
Libraries…** in the Library Manager (see
[Merge two libraries](library-management.md#merge-two-libraries)). What the merge adds reaches your other computers like any import.

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
  originals and the entries in it. This cannot be undone.
- The library on this computer is not changed. It keeps every item and every file.
- Cloud sync is off for the library on this computer. You can enable it again later; Picto then
  writes new cloud data.
- Your other computers keep their local libraries. Each of them notices at its next sync that the
  cloud data is gone, turns its own sync off, and shows the message "The cloud data of this
  library was deleted." in the Cloud Sync card and in a notification. It deletes nothing.

Picto removes only files that its sync wrote, by their names. If the library's folder in the cloud
folder holds anything else, for example a file that you put there, Picto leaves that file and the
folder, and the message in the Library Manager names what was left.

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

A restore does not take back what was synced after the snapshot was made. The entries are in the
cloud folder, and the next sync applies them to the restored library again: an item that was
imported since is imported again, and an item that was deleted since is deleted again. To get a
deleted item back, import its file again after the restore. That removes the tombstone on every
computer.

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
