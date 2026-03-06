# PBI-144: Folder password protection

## Priority
P3

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. Eagle has folder password/lock with create/edit/remove/lock, Touch ID support (Cmd+Shift+L to lock).
2. Picto has no folder protection.

## Problem
Users cannot protect sensitive folders from casual browsing.

## Scope
- Backend: hashed password storage per folder
- `src/components/dialogs/FolderPasswordDialog.tsx`
- Context menu: "Password" submenu (Create/Edit/Lock/Remove)
- `src/lib/shortcuts.ts` — Cmd+Shift+L shortcut

## Implementation
1. `password_hash TEXT` column on folders table (bcrypt or argon2 hash).
2. When navigating to locked folder, prompt for password.
3. Create/Edit/Remove password via context menu or dialog.
4. Lock folder: Cmd+Shift+L re-locks an unlocked folder.
5. Session-based unlock: unlocked folders stay accessible until app restart.

## Acceptance Criteria
1. Can set password on a folder.
2. Navigating to locked folder prompts for password.
3. Correct password grants access for session.
4. Cmd+Shift+L re-locks the folder.

## Test Cases
1. Set password on folder → close and reopen → prompted for password.
2. Wrong password → access denied.
3. Correct password → folder content shown.

## Risk
Low-Medium. Password hashing + session unlock state.
