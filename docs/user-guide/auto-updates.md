# Updates

[← User Guide](README.md)

Picto checks GitHub Releases for new versions. Pre-release versions (such as alpha builds) are
included.

## When Picto checks

- 15 seconds after launch
- every 6 hours while Picto is running
- when you click **Check Now** in **Settings → Updates** or **Settings → About**, or choose
  **File → Check for Updates…**

Update checks only run in installed (packaged) builds.

## Windows and Linux

1. When a newer version exists, Picto downloads it in the background and shows a notification.
2. When the download finishes, Picto notifies you again.
3. The update installs when you quit Picto. To install right away, open **Settings → Updates** and
   click **Restart and Install**.

| Platform | Package | Update |
|---|---|---|
| Windows (x64) | NSIS installer (`Picto-Setup-<version>.exe`) | Automatic |
| Linux (x64) | AppImage | Automatic |

## macOS

macOS builds (Apple silicon, `.dmg` and `.zip`) are not signed for automatic installation. When a
newer version exists, Picto notifies you and **Open Download Page** opens the release on GitHub.
Download and install it yourself.

## After an update

The first time a new version starts, Picto shows its release notes. They are bundled with the app,
so this works offline.

## If updates do not work

- Make sure you are running an installed release, not a development build.
- Check your internet connection; the check contacts github.com.
- **Settings → Updates** shows the last error, if any.
