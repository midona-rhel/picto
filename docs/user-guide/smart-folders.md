# Smart Folders

[← User Guide](README.md)

Smart folders are dynamic, rule-based folders that automatically collect files matching tag predicates. Unlike regular [folders](folders.md), you don't manually add files — the contents update automatically as tags change.

## Creating a Smart Folder

Press `Ctrl+Shift+Alt+N` or right-click in the Smart Folders section → **New Smart Folder**.

The creation dialog lets you configure:

1. **Name** — A descriptive name for the smart folder
2. **Icon** — Choose from the built-in icon picker
3. **Color** — Set a color label for visual distinction
4. **Predicates** — The rules that determine which files appear

## Predicates

Smart folders use three types of predicates:

### Include All (AND)

Files must have **every** tag in this list. For example, if you include `creator:john` AND `series:landscape`, only files with both tags appear.

### Include Any (OR)

Files must have **at least one** tag from this list. For example, include `character:alice` OR `character:bob` to show files with either character.

### Do Not Include (Exclude)

Files with **any** of these tags are excluded. For example, excluding `meta:sketch` removes all sketches from the results, even if they match the include rules.

## Live Count Preview

While editing predicates, a live count shows how many files currently match your rules. This updates with a short delay as you add or remove predicates.

## Nesting Smart Folders

Smart folders can be nested under other smart folders. A child inherits its parent's predicates and combines them with its own:

- Parent: `Include All: series:landscape`
- Child: `Include All: meta:wallpaper`
- Result: Files that are BOTH landscapes AND wallpapers

This lets you build increasingly specific filters without repeating rules.

## Editing and Deleting

- **Edit** — Double-click a smart folder or right-click → Edit to modify its predicates, name, icon, or color
- **Delete** — Right-click → Delete. Files are not affected — only the smart folder definition is removed

## Sort Override

Each smart folder can optionally override the global sort field and order. This lets you sort one smart folder by rating while another sorts by date added.

## Undo Support

Creating, editing, and deleting smart folders all support undo (`Ctrl+Z`).
