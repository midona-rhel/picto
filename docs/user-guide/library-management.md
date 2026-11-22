# Library Management

[← User Guide](README.md)

A library is a folder named `<name>.library`. It holds everything Picto knows about your media and
a copy of every imported file. You can keep several libraries and switch between them.

## What is in a library folder

```
My Library.library/
├── library.sqlite   ← database: items, tags, folders, settings, history
└── blobs/
    ├── f/           ← original files, stored by content hash
    └── t/           ← thumbnails
```

Picto also creates working folders in the library, for example `blobs/tmp/` and `temp/`, and
`cloud/` when cloud sync is on. Do not change anything inside a library folder by hand.

## Open the Library Manager

Open the Library Manager in one of these ways:

- Choose **Library → Library Manager…** (`Mod+Shift+L`).
- Click the library switcher at the top of the sidebar and choose **Library Manager…**.

The left column lists the libraries on this device, with the pinned libraries first. Select a
library to see its appearance and its statistics. For the open library the Library Manager also
shows the state of cloud sync.

## Create a library

1. Click **New Library…** and type a name.
2. Click **Choose Location…** and pick the folder that will contain the library.

Picto creates `<name>.library` in that folder and opens it. A library cannot be created inside
another library.

## Open an existing library

Click **Open Existing…** in the Library Manager, or choose **Library → Open Library…**, and select
a folder whose name ends in `.library`. Picto opens the library and adds it to the list.

The first time you open a library that Picto did not create on this computer, Picto asks **Open
this library directly?** and names the library's folder. Click **Open Library**. Picto asks this
once for each library. It asks again when the library is found at another place. The Library
Manager asks the same question before it renames, shows, moves or deletes a library that is not
open and that you have not answered the question for.

Picto does not open a library in these cases:

- The folder is not a library. A library is a `.library` folder that holds the file
  `library.sqlite`. An empty `.library` folder is not a library.
- The library's database has an older format. Picto does not convert such a library. It shows a
  message that says so and leaves the library unchanged.
- The `.library` folder was opened from the system's file manager, or was given to Picto when
  Picto was started. Picto opens a library only from the Library Manager, from **Open Library…**,
  from **Recent Libraries** and from the library switcher in the sidebar.

To open a library that is stored in a synced Google Drive or Dropbox folder, use **Open from
Cloud…**. Cloud sync is experimental. See [Cloud Sync](cloud-sync.md).

## Switch libraries

Each of these closes the current library and opens the other one:

- Double-click the library in the Library Manager, or select it and click **Open Library**.
- Choose the library in the library switcher of the sidebar.
- Choose the library under **Library → Recent Libraries**.

When Picto switches to another library, it also closes the separate item windows, the Settings
window and the log window.

When Picto starts, it opens the library that was open last.

## Library actions

Select a library and click the **⋯** (Library actions) button.

| Action | What it does |
|--------|--------------|
| Add to Quick Access, Remove from Quick Access | It pins the library to the top of the lists, or unpins it. |
| Rename | It renames the `.library` folder on disk. If the library is open, Picto closes it and opens it again. |
| Show in File Manager | It shows the library folder in the system's file manager. |
| Move… | It asks for a folder and moves the library folder there. If the library is open, Picto opens it again from the new place. When the new place is on another disk, Picto copies the library, compares every copied file with its original, and only then removes the original. For a library that is marked **Missing on disk**, this row reads **Locate…**: pick the folder that now contains the library. |
| Merge with Another Library… | It opens the dialog that puts everything of two libraries into one of them, as the **Merge Libraries…** button in the bottom bar does. See [Merge two libraries](#merge-two-libraries). The row is disabled while the list has no other library whose folder exists. |
| Remove from List | It removes the library from the list. The library folder stays on disk, and **Open Existing…** opens it again. The row is disabled for the open library. |
| Delete… | After a question, it permanently deletes the library folder and everything in it. This cannot be undone. The row is disabled for the open library, so switch to another library first. Picto deletes only a `.library` folder that holds `library.sqlite` and that no program has open. |

For the open library the menu also has the commands of cloud sync, which is experimental.

## Merge two libraries

A merge puts every item, tag, folder, smart folder and collection of two libraries into one of
them. You choose which of the two is the primary library. The primary library is kept and
receives the content of the other one, and where the two libraries disagree, the primary
library's version is used. The other library is only read: nothing in its folder is changed or
deleted, and you can keep it or delete it yourself afterwards.

1. Select one of the two libraries in the Library Manager and click **Merge Libraries…** at the
   left end of the bottom bar. The row **Merge with Another Library…** of the **⋯** menu opens
   the same dialog. The button is disabled while the list has fewer than two libraries whose
   folders exist.
2. Under **Merge "…" with**, choose the second library. Each row shows a library's name, its
   folder and the number of its items. When the list has only one other library, it is already
   chosen.
3. Under **Primary library**, choose which of the two libraries is kept.
4. Click **Merge**. If the primary library is not open, Picto opens it first.

The main window shows the merge as a task with the number of files that are done and the total.
**Pause** holds it, and **Cancel…** undoes it. You can keep working while it runs.

What the primary library holds afterwards:

- A file is recognised by its content. A file that both libraries hold is one item. It keeps the
  primary library's name, dates and status (active, Inbox or Trash). It has the tags, the folders
  and the source links of both libraries. It gets the other library's notes when it had none, and
  the other library's rating when it had none.
- An item that only the other library holds arrives with its name, tags, folders, rating, notes,
  dates and status. An item of the other library's Inbox arrives in the Inbox, and an item of its
  Trash arrives in the Trash. The file and its thumbnail are copied, not moved.
- Tags are matched by namespace and name. Folders are matched by their names from the top level
  down, so `Travel/2024` of both libraries is one folder. A new folder keeps its icon, colour,
  notes and automatic tags. A folder that both libraries have keeps the primary library's.
- Collections are added, each with its own note. A file that is alone in one library and in a
  collection in the other ends up in the collection. Two collections that share a file become
  one, which keeps the name and the note of the primary library's collection.
- Smart folders are added. A smart folder with the same name and the same rules as one of the
  primary library is not added again. One with the same name and other rules is added with the other
  library's name in brackets.
- Subscriptions, cloud sync settings, watched folders, application settings, view settings,
  Recently Viewed and duplicate decisions of the other library are not carried over.

A merge brings everything or changes nothing. When you cancel it, when a file of the other
library cannot be copied, or when the other library can no longer be read, Picto removes what the
merge added, and the primary library holds what it held before. A finished merge cannot be undone
with **Undo**.

Before the merge changes anything, Picto writes a backup of the primary library's database into
its library folder, named `library.sqlite.pre-merge-<time>.backup`. Picto deletes the backup when the
merge has finished or has been undone. If the undo itself fails, the backup stays.

Picto does not merge a library in these cases:

- The other library is open in another Picto.
- The other library's database has an older format, or Picto was ended while that library was
  open. Open the library in Picto once and close it, then merge again.
- An import is running in the primary library.

## Appearance

In the **Appearance** card you can set a color, an icon and a cover image for each library. The
cover is chosen from the library's own media, so open the library first.

## A library that moved or disappeared

The list marks a library whose folder is gone with **Missing on disk**. If the library that was
open last is missing when Picto starts, Picto shows **Library Not Found** and asks what to do:
**Locate…** lets you pick the folder that now contains the library, **Remove from List** removes
the library from the list, and **Cancel** does neither.
