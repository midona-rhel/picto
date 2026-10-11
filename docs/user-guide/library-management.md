# Library Management

[← User Guide](README.md)

A library is a folder named `<name>.library`. Everything Picto knows about your media lives in it,
together with a copy of every imported file. You can keep several libraries and switch between them.

## What is in a library folder

```
My Library.library/
├── library.sqlite   ← database: items, tags, folders, settings, history
└── blobs/
    ├── f/           ← original files, stored by content hash
    └── t/           ← thumbnails
```

Picto also creates working folders (for example `blobs/tmp/`, `temp/`, and `cloud/` when cloud sync
is on). Do not edit anything inside a library folder by hand.

## Open the Library Manager

- **Library → Library Manager…**, or
- click the library switcher at the top of the sidebar and choose **Library Manager…**.

The left column lists the libraries on this device, pinned ones first. Select one to see its
appearance, statistics, and (for the open library) cloud sync status.

## Create a library

1. Click **New Library…** and type a name.
2. Click **Choose Location…** and pick the folder that will contain it.

Picto creates `<name>.library` there and opens it. A library cannot be created inside another library.

## Open an existing library

Click **Open Existing…** (or **Library → Open Library…**) and select a folder ending in `.library`.
Picto opens it and adds it to the list.

To open a library stored in a synced Google Drive or Dropbox folder, use **Open from Cloud…**.
Cloud sync is experimental; see [Settings](settings.md).

## Switch libraries

Any of these closes the current library and opens the other one:

- double-click it in the Library Manager, or select it and click **Open Library**;
- pick it in the sidebar library switcher;
- **Library → Recent Libraries**.

On launch Picto opens the library you used last.

## Library actions

Select a library and click the **⋯** (Library actions) button.

| Action | What it does |
|--------|--------------|
| Add to / Remove from Quick Access | Pins the library to the top of the lists. |
| Rename | Renames the `.library` folder on disk. If it is the open library, Picto closes and reopens it. |
| Show in File Manager | Reveals the library folder. |
| Move… | Asks for a destination folder and moves the library folder there. If it is the open library, Picto reopens it from the new place. For a library marked **Missing on disk** this reads **Locate…**: pick the folder that now contains it. |
| Remove from List | Forgets the library. Its folder stays on disk; open it again with **Open Existing…**. Not available for the open library. |
| Delete… | After confirmation, permanently deletes the library folder and everything in it. Picto refuses to delete the open library; switch to another one first. **Cannot be undone.** |

For the open library the menu also has the cloud sync commands (experimental).

## Appearance

In the **Appearance** card you can set a color, an icon, and a cover image for each library. The
cover is chosen from the library's own media, so open the library first.

## A library that moved or disappeared

The list marks missing libraries with **Missing on disk**. If no library in the list can be opened
at launch and the last-used one is missing, Picto asks what to do: **Locate…** (pick the folder
that now contains it), **Remove from List**, or **Cancel**.
