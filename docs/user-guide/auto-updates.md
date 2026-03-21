# Auto-Updates

[← User Guide](README.md)

Picto checks for updates automatically using GitHub Releases.

## How It Works

1. **Startup check** — 10 seconds after launching, Picto checks for a newer version
2. **Periodic checks** — Every 4 hours while Picto is running
3. **Manual download** — Updates are not downloaded automatically. When an update is available, you'll be notified and can choose to download it
4. **Install on quit** — After downloading, the update installs automatically the next time you close Picto

## Platform Details

| Platform | Update Format | Notes |
|----------|--------------|-------|
| Windows | NSIS installer | Full installer replacement, seamless |
| Linux | AppImage | Self-contained, replaces the running AppImage |
| macOS | ZIP | Replaces the app bundle |

The `.deb` package on Linux does **not** support auto-update. Use the AppImage version if you want automatic updates.

## Releases

Updates are published as [GitHub Releases](https://github.com/midona-rhel/picto/releases). Pre-release versions (alpha, rc) are also distributed through this channel.

## Troubleshooting

If auto-update isn't working:

- Make sure you're running a packaged version (not a development build)
- Check your internet connection
- On Linux, ensure the AppImage file is writable (auto-update replaces it in place)
- Check the developer console for error messages (Settings → Developer or `Ctrl+Shift+I`)
