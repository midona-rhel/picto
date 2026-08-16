# Inbox Workflow

[← User Guide](README.md)

The inbox is a staging area for newly imported files. It gives you a chance to review, tag, and organize files before they become part of your accepted library.

## How It Works

Files imported with **inbox** status (the default) appear in the Inbox view. They do not show up in All Active, folders, or smart folders until you accept them.

The three file statuses are:

| Status | Meaning | Visible in |
|--------|---------|------------|
| Inbox (0) | Awaiting review | Inbox view only |
| Active (1) | Part of your accepted library | All Active, folders, smart folders, tags |
| Trash (2) | Marked for deletion | Trash view only |

## Reviewing the Inbox

Navigate to the Inbox (`Ctrl+2`) and open the detail view (`Enter`) to review files one at a time.

| Key | Action |
|-----|--------|
| `Enter` | **Accept** — Move to active status |
| `Backspace` | **Reject** — Move to trash |
| `Left/Right` | Navigate between inbox files |

As you accept or reject files, the detail view automatically advances to the next unreviewed file. When all files are reviewed, the detail view closes.

## Bulk Review

You can also review from the grid:
- Select multiple files
- Right-click → Set Status → Active (to accept all)
- Or right-click → Set Status → Trash (to reject all)

## Changing the Default Import Status

If you prefer to skip the inbox and import files directly as active:

1. Open [Settings](settings.md) → Download Services
2. Change **Default Import Status** to **Active**

This affects manual imports, folder imports, drag-and-drop, and watched folder imports. Subscription imports have their own default (typically inbox).

## Inbox Cap

Subscription downloads automatically pause when the inbox reaches 1,000 items. This prevents runaway downloads from filling your inbox. Process some inbox items to resume downloads.
