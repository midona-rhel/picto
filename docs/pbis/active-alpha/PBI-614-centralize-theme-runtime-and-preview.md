# PBI-614: Centralize theme runtime and preview ownership

## Problem

Theme state currently has several competing owners. AppShell loads the persisted setting, mirrors a
second value through localStorage, resolves `auto`, mutates document attributes, and listens for OS
changes. Settings separately repeats those mutations, persists some choices immediately despite its
Save/Reset model, and restarts native windows. A theme can therefore differ between the stored
setting, Settings selection, the current document, another Picto window, and the operating system.

## Contract

- Make the persisted application `colorScheme` setting the sole durable authority. localStorage may
  be read once for migration, then must not remain an active theme store.
- Introduce one theme runtime that resolves `auto`, applies the document/Mantine color scheme and
  Picto theme attributes, and publishes the resolved theme to every renderer window.
- The runtime owns the single OS appearance listener. OS changes affect only an active `auto` theme.
- Settings may preview a draft theme immediately, but Save persists it and Cancel/close/Reset restores
  the opening snapshot. Preview state must never masquerade as persisted state.
- Centralize platform-native material changes behind one window-controller command. Unsupported
  material themes must resolve to a truthful supported fallback on macOS, Windows, and Linux.
- Delete duplicated DOM/localStorage theme application from AppShell and Settings. Do not leave a
  compatibility write path after migration.

## Current ownership inventory

- `AppShell` loads backend settings, writes `picto-theme`, mutates three document theme surfaces, and
  owns an OS-theme subscription whose `auto` guard reads localStorage.
- `Settings` repeats the document mutation once at module load and again inside Appearance. Its
  theme control reads localStorage rather than the loaded draft and calls the backend patch path
  immediately while its comment and footer claim persistence occurs on Save.
- Electron `windowManager` synchronously reads the current library's `settings.json` to choose the
  native BrowserWindow background/material, while Electron separately broadcasts every OS theme
  change to every window.
- Secondary renderer entrypoints do not share one bootstrap. Settings happens to apply localStorage;
  other windows depend on their own CSS/bootstrap behavior.

Keep the backend setting and Electron's creation-time native-material decision. Replace the three
renderer application paths, the localStorage authority, and the duplicated OS listeners with the one
runtime described above.

## Verification

- A pure matrix test covers every named theme, `auto` in light/dark OS modes, and unsupported
  platform-material fallbacks.
- Runtime tests prove there is one OS listener, no response to OS changes for an explicit theme, and
  synchronized main/detail/settings windows.
- Settings tests prove preview, Save, Reset, Cancel, and close-without-save semantics.
- A source check rejects new direct writes to `document.documentElement.dataset.theme`, Mantine color
  scheme, CSS `color-scheme`, or `picto-theme` outside the runtime/migration.
- User verification covers light, dark, auto-following-OS, and the current platform material theme
  without restarting the whole application unless the native BrowserWindow material requires it.

Delete this PBI when the acceptance checks pass. Git history is the archive.
