# Getting Started

[← User Guide](README.md)

## Install

Download the latest release from the [releases page](https://github.com/midona-rhel/picto/releases).

| Platform | Package | How to install it |
|----------|---------|-------------------|
| Windows | `.exe` installer | Run the installer. |
| macOS | `.dmg` disk image | Open the disk image and drag Picto to the Applications folder. |
| Linux | `.AppImage` file | Make the file executable and run it. |

FFmpeg is included in every package. On Windows and macOS nothing else needs to be installed.

On Linux, Picto uses these libraries of the system: GTK 3, WebKitGTK 4.1 with libsoup 3, ALSA,
D-Bus, xcb and xkbcommon. Ubuntu and Xubuntu have all of them after a normal installation. A
system with the KDE desktop, such as Kubuntu, usually has no WebKitGTK 4.1. When a library is
missing, Picto does not start and shows a message that names the missing library and the package
to install: `libwebkit2gtk-4.1-0` on Ubuntu and Debian, `webkit2gtk4.1` on Fedora, and
`webkit2gtk-4.1` on Arch. On Ubuntu 24.04 the full list of packages is `libwebkit2gtk-4.1-0`,
`libgtk-3-0t64`, `libasound2t64`, `libfontconfig1`, `libxkbcommon-x11-0`, `libwayland-client0`
and `libvulkan1`.

Only one Picto runs at a time. A second Picto that you start while Picto is open ends at once.

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

**Linux.** Linux shows no warning. Make the `.AppImage` file executable (`chmod +x`), then start
it.

### Upgrading from Picto 0.6.19 or earlier

This version has the same name as Picto 0.6.19 and earlier, and it installs over the version you
have. Quit Picto first. Then run the new installer on Windows, replace Picto in the Applications
folder on macOS, or replace the `.AppImage` file on Linux.

After the upgrade:

- Your libraries are unchanged. They keep all their items, tags, folders and settings.
- The library that was open in the version you had opens when Picto starts. Your other
  libraries are listed in the Library Manager.
- Your site logins and the downloaded AI models are still there.
- The window's size and position, the widths of the sidebar and the inspector, the saved filters,
  the recent tags and folders, the keyboard layout and the changed keyboard shortcuts, the language
  and the interface zoom start from their defaults. Set them again.
- A library whose stored theme made the whole window translucent opens in the Dark theme, because
  this version does not have those themes. **Settings → General** has a switch that makes the
  sidebar and the inspector translucent on macOS and Windows. See [Settings](settings.md).
- A view whose thumbnail size was saved by Picto 0.6.19 or earlier can open with one column more
  or one column fewer than before.
- On Linux this version needs the system libraries that are listed under [Install](#install).
  Picto 0.6.19 and earlier did not need them.

## Create your first library

A library is a folder on disk named `<name>.library`. It holds Picto's database and a copy of
every imported file.

1. On the first start Picto shows **Open a library to start**. Click **Choose Library…**.
2. In the Library Manager, click **New Library…** and enter a name.
3. Click **Choose Location…** and pick the folder that will contain the library.

Picto creates the library and opens it. On later starts Picto opens the library that was open
last. [Library Management](library-management.md) says how to switch, rename and move libraries.

## Import some files

You can import files in three ways:

- Drag files or a folder onto the grid.
- Choose **File → Import Files…** (`Mod+I`) or **File → Import Folder…** (`Mod+Shift+I`).
- Copy files or an image, then press `Mod+V` in the grid.

Picto copies the files into the library. Your original files are not changed.

Files that you import while the **Inbox** view is open go to the Inbox for review. Files that you
import in any other view go straight into the library. See [Importing Files](importing-files.md).

## Find your way around

- The **sidebar** is on the left. It lists the views, Quick Access, the smart folders and the
  folders.
- The **grid** is in the center. It shows thumbnails of the items in the current view.
- The **inspector** is on the right. It shows the name, notes, tags, folders, rating and properties
  of the selection.
- The **title bar** is at the top. It has the Back and Forward buttons, the thumbnail size control,
  the view options, the filters and the search field.

`Mod+Alt+1` hides or shows the sidebar, and `Mod+Alt+2` hides or shows the inspector. `Tab` hides
both when at least one of them is showing, and shows both when both are hidden. `Mod+T` opens
Quick Search, which finds commands, views, folders, tags and items by name.
See [Interface Overview](interface-overview.md).

## Next steps

- Organize your media with [tags](tags.md), [folders](folders.md) and
  [smart folders](smart-folders.md).
- Group related media into [collections](collections.md).
- Download new posts automatically with [subscriptions](subscriptions.md).
- Read the keyboard shortcuts in **Settings → Shortcuts**.
