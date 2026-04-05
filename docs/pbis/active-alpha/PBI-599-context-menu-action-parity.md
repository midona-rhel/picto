# PBI-599 — Context Menu Action Parity

Wire all disabled/stubbed/missing context menu actions to reach feature parity with the legacy frontend.

## Grid Tile Context Menu

### Disabled — needs wiring
- [x] **Rename** — inline edit overlay on tile name in canvas grid
- [x] **Copy Tags** — serialize entity tags to clipboard JSON + in-memory store
- [x] **Paste Tags** — read from in-memory store, apply to selection via `addTargetTags`
- [x] **Regenerate Thumbnail(s)** — call `regenerate_thumbnails_batch` IPC

### Missing — needs adding
- [x] **Copy Name** — copy entity filename to clipboard
- [x] **Copy as Link** — copy `media://localhost/file/{hash}.{ext}` to clipboard
- [ ] **Copy Thumbnail** — copy thumbnail image blob to clipboard
- [ ] **Search by Image** submenu — TinEye, SauceNAO, Yandex, Bing (open browser with URL)
- [ ] **Find Visually Similar** — navigate to duplicates view filtered by perceptual hash
- [ ] **New Folder with Selection** — create folder via API, then add selected items
- [ ] **Merge into Collection** — when 1 collection + loose files selected, add files to collection
- [ ] **Merge Collections** — when 2+ collections selected, merge members into first
- [ ] **Show in Grayscale** — toggle CSS `filter: grayscale(1)` on canvas container

## Sidebar Folder Context Menu

### Stubbed — needs wiring
- [ ] **Set Auto-Tags** — open auto-tags editor (new modal or popover)
- [ ] **Change Icon** — open IconPicker, save via `updateFolder`
- [ ] **Move** — open folder picker for reparenting, call `moveFolder`

### Missing — needs adding
- [x] **Remove Watched Folder** — danger action with confirm, calls `clearFolderWatchConfig`
- [ ] **Sort submenu** — Current Level A→Z / Z→A, All Levels A→Z / Z→A

### Disabled — needs backend
- [ ] **Duplicate** — clone folder + membership via backend `duplicate_folder`
- [ ] **Export** — open ExportModal scoped to this folder (wired, needs backend export command)

## Sidebar Smart Folder Context Menu

### Stubbed — needs wiring
- [ ] **Edit Smart Folder** — open SmartFolderModal in edit mode with existing predicate/sort/color/icon
- [ ] **New Child Smart Folder** — create smart folder with `parent_id` set to this one
- [ ] **Change Icon** — open IconPicker, save via `updateSmartFolder`
- [ ] **Color Picker** — wire save to `updateSmartFolder` (backend supports it, frontend doesn't call)

### Missing — needs adding
- [ ] **Sort By submenu** — date/name/size/rating + asc/desc, saved per smart folder

### Disabled — needs backend
- [ ] **Duplicate** — clone smart folder + predicate via backend

## Tag Chip Context Menu (Inspector) — NEW

Right-click on a tag chip in the inspector:
- [x] **Copy** — copy tag string to clipboard
- [x] **Remove** — same as × button, via context menu
- [ ] **Show Items** — navigate to tag-filtered grid view (needs tag scope support)

## Folder Chip Context Menu (Inspector) — NEW

Right-click on a folder chip in the inspector:
- [x] **Open Folder** — navigate to that folder's grid view
- [x] **Remove** — same as × button, via context menu

## Tag Manager Context Menu — NEW

Right-click on a tag row in the tag manager/tag select panel:
- [ ] **Show Items** — navigate to tag-filtered view
- [ ] **Rename** — inline rename or prompt, calls `rename_tag`
- [ ] **Merge into...** — open tag picker, merge this tag into target
- [ ] **Copy** — copy tag string
- [ ] **View Relations** — open TagRelationsModal
- [ ] **Aliases** submenu — list existing + "Add alias..."
- [ ] **Implications** submenu — list parent tags + "Add implication..."
- [ ] **Implied By** submenu — list child tags + "Add implied-by..."
- [ ] **Delete** — danger, remove tag from all entities

## Priority

**P0** (core UX, most visible) — DONE:
1. ~~Rename (grid)~~ — inline canvas overlay
2. ~~Copy/Paste Tags~~
3. ~~Regenerate Thumbnail~~
4. ~~Copy Name / Copy as Link~~
5. ~~Tag chip context menu (inspector)~~
6. ~~Folder chip context menu (inspector)~~
7. ~~Remove Watched Folder~~

**P1** (next up):
8. Change Icon (folder + smart folder)
9. Edit Smart Folder modal
10. Smart folder color wiring
11. Set Auto-Tags editor

**P2** (missing actions):
12. Copy Thumbnail
13. Search by Image submenu
14. Find Visually Similar
15. Merge into/Merge Collections
16. New Folder with Selection
17. Smart folder Sort By submenu
18. Duplicate folder/smart folder
19. Move folder
20. Export scoped to folder

**P3** (nice to have):
21. Show in Grayscale
22. Full tag manager context menu (rename, merge, relations, aliases, implications)
