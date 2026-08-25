# PBI-613: Centralize renderer shortcut ownership and suspension

## Problem

Picto registers shortcut listeners independently in AppShell, GridScreen, GridToolbar, viewers,
collections, duplicates, tags, subscriptions, and several floating surfaces. Components attempt to
avoid collisions with local target checks and `stopPropagation()`, but window listeners do not form
one ordered system and a focused embedded runtime such as Ruffle cannot reliably prevent application
shortcuts from consuming its game controls.

## Contract

- Replace independent application-level `window.keydown` listeners with one renderer shortcut
  dispatcher and one ordered registry of active shortcut scopes.
- Expose a reference-counted suspension lease, not a shared boolean. Acquiring a lease disables all
  Picto application shortcuts; releasing that exact lease restores them only when no other lease is
  active. Unmount must always release its lease.
- Ruffle acquires the lease when its interactive surface receives focus or pointer activation and
  releases it on focus leaving the player or unmount. Keydown and keyup events must continue reaching
  Ruffle unchanged while Picto is suspended.
- Text editors, shortcut recorders, and future embedded interactive runtimes use the same suspension
  API where full application-shortcut suppression is required. Ordinary local controls may instead
  register a higher-priority scope.
- Preserve the existing shortcut registry, user overrides, EU alternative keys, platform modifier
  display, and native application-menu accelerators. This PBI consolidates renderer dispatch only.
- Delete migrated global listeners and their duplicated editable-target/viewer/modal guards. Do not
  leave compatibility listeners or component-specific Flash exceptions.

## Current ownership inventory

Migrate these application scopes into the ordered dispatcher:

- App shell navigation, panel toggles, and Settings.
- Grid actions, grid arrow navigation, and toolbar search focus.
- Collection reader/editor actions, Duplicates, Tags, and Subscriptions.
- Media View, Quick Look, standalone Detail Window, video playback, and font preview.
- Context menu, modal, and overlay capture listeners become higher-priority active scopes; they do
  not remain independent window listeners.

Keep component-local DOM keyboard handling local: rename and editable fields, combobox/listbox
navigation, rule editors, and Enter/Escape handling whose effect is contained by the focused control.
These are not application shortcuts and should not be routed through the global dispatcher.

The current shortcut editor is also not a persistence owner: its override maps are component state
and are lost when Settings closes. Move overrides and keyboard preset resolution into the same
registry snapshot used by dispatch and display, then let PBI-615 include that snapshot in the
Settings transaction.

## Verification

- A registry test proves deterministic priority, handled-event termination, user overrides, and
  alternative key definitions.
- Nested suspension leases prove that releasing one consumer cannot prematurely re-enable shortcuts.
- Focused Ruffle receives arrows, Space, Enter, Escape, digits, letters, and modifier combinations,
  while no Picto navigation, rating, panel, delete, zoom, or viewer action fires.
- Blurring or unmounting Ruffle restores Picto shortcuts exactly once.
- Text editing, modal/context-menu keyboard navigation, grid navigation, viewer navigation, playback
  controls, and standalone detail windows retain their existing accepted behavior.
- A source check fails if production feature code adds a new application-level `window.keydown`
  listener outside the dispatcher.

Delete this PBI when the acceptance checks pass. Git history is the archive.
