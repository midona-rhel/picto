# Getting Started

[← User Guide](README.md)

## Install

Download the latest release from the [releases page](https://github.com/midona-rhel/picto/releases).

| Platform | Package |
|----------|---------|
| Windows | `.exe` installer |
| macOS | `.dmg` (drag Picto to Applications) |
| Linux | `.AppImage` (make it executable and run it) |

FFmpeg is bundled. Nothing else needs to be installed.

### The first start

Picto is not signed by a developer that Apple or Microsoft knows, because the project has no
Apple Developer ID and no Windows code-signing certificate. macOS and Windows therefore stop the
first start of a downloaded Picto and ask you to allow it. You do this once.

**macOS.** When you open Picto for the first time, macOS says that it could not check the app for
malicious software, and the message has no button that opens the app. Then:

1. Close the message with **Done**. Do not choose **Move to Trash**.
2. Open **System Settings**, choose **Privacy & Security**, and scroll down to **Security**.
3. Next to the line about Picto, click **Open Anyway**. The button is there for about an hour
   after you tried to open Picto.
4. Enter your login password and confirm that you want to open Picto.

macOS remembers this. From then on Picto opens with a double-click.

**Windows.** When you start the installer, Microsoft Defender SmartScreen shows a window with
the title **Windows protected your PC**, because the installer is not signed. Click **More
info**, then **Run anyway**. The installer then runs as usual.

If Windows says instead that **Smart App Control** blocked the installer, there is no button to
run it anyway. Smart App Control blocks every program that is not signed, and it has no
exception for a single program. Picto can then be installed only after Smart App Control is
turned off in **Windows Security** under **App & browser control**. That lowers the protection
of your computer, so decide for yourself whether you want that.

On a computer that your employer or school manages, the administrator may have removed
**Run anyway**. Ask the administrator in that case.

**Linux.** Nothing warns. Make the `.AppImage` file executable (`chmod +x`), then start it.

### Upgrading to the rewritten Picto

Picto is being rewritten as a native app that does not use Electron. From the first release that
is built from the rewritten app, the downloads on the releases page are that app; its release
notes say so. It has the same name as before and installs over the version you have: run the new
installer on Windows, replace Picto in the Applications folder on macOS, and replace the
`.AppImage` file on Linux. Quit Picto first. Only one Picto runs at a time: a second one that
you start while Picto is open ends at once.

After the upgrade:

- Your libraries are untouched, with all their items, tags, folders and settings.
- Your libraries are listed in the Library Manager, but none opens by itself the first time.
  Choose your library there. Picto asks once for each library whether it may open that library.
- Your site logins and the downloaded AI models are still there.
- The window's size and position, the widths of the sidebar and the inspector, saved filters,
  changed keyboard shortcuts, the language and the zoom start from their defaults. Set them again.
- On Linux the rewritten Picto needs GTK 3 and WebKitGTK 4.1, which most desktop systems have. On
  Ubuntu 24.04 the packages are `libgtk-3-0t64` and `libwebkit2gtk-4.1-0`.

## Create your first library

A library is a folder on disk (`<name>.library`) that holds Picto's database and a copy of every
imported file.

1. On first launch Picto shows **Open a library to start**. Click **Choose Library…**.
2. In the Library Manager, click **New Library…** and enter a name.
3. Click **Choose Location…** and pick the folder that will contain the library.

Picto creates the library and opens it. On later launches it reopens the last library you used.
See [Library Management](library-management.md) for switching, renaming, and moving libraries.

## Import some files

- Drag files or a folder onto the grid.
- **File → Import Files…** (`Mod+I`) or **File → Import Folder…** (`Mod+Shift+I`).
- Copy files or an image and press `Mod+V` in the grid.

Picto copies files into the library. Your originals are not changed.

Files imported while the **Inbox** view is open go to the Inbox for review. Files imported
anywhere else go straight into the library. See [Importing Files](importing-files.md).

## Find your way around

- **Sidebar** (left): views, Quick Access, smart folders, folders.
- **Grid** (center): thumbnails of the current view.
- **Inspector** (right): name, notes, tags, folders, rating, and properties of the selection.
- **Title bar** (top): back/forward, view options, filters, search.

`Mod+Alt+1` toggles the sidebar, `Mod+Alt+2` the inspector, `Tab` both.
See [Interface Overview](interface-overview.md).

## Next steps

- Organize with [tags](tags.md), [folders](folders.md), and [smart folders](smart-folders.md)
- Group related media into [collections](collections.md)
- Download automatically with [subscriptions](subscriptions.md)
- Learn the keyboard shortcuts in **Settings → Shortcuts**
