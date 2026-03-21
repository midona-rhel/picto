# Folders

## Purpose

Define ordered hierarchical containers for media entities.

## Current Truth

- Folder CRUD, membership, tree ordering, auto-tags, and drag/drop policy are split across backend, sidebar, and shared controller code.

## Target Truth

- `Folder` is an ordered hierarchical container.
- Folders can contain subfolders and media entities.
- Auto-tags are applied at folder membership boundaries, not inferred in the UI.
- Reorder semantics are backend-owned.

## Rename Map

- keep `Folder`, `FolderMembership`
- rename any misleading “file folder” public phrasing to “entity folder” where logical item semantics are intended

## Delete List

- Delete duplicate reorder or tree mutation helpers in renderer layers.
- Delete controller wrappers that only pass through folder commands.

## DTOs and Commands Involved

- `Folder`
- `FolderMembership`
- `create_folder`
- `update_folder`
- `move_folder`
- `reorder_folders`
- `reorder_folder_items`

## Workflows

- Create folder -> place under parent -> tree order persists.
- Add entity to folder -> membership stored -> auto-tags applied if configured.
- Reorder items -> folder-scoped order updates without frontend list hacks.

## Acceptance Criteria

- Folder ordering and parentage are backend-owned.
- Sidebar tree consumes one folder read model.
- Auto-tag behavior lives in folder domain, not in the sidebar UI.
