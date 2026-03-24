# Inspector / Selection Parity Checklist

Reference: PBI-593 (inspector and selection surface rebuild)
Fixtures: `src/test-harness/fixtures/inspector.ts`

## Single entity details

- [ ] Entity name displays (or hash fallback when name is null)
- [ ] Mime type displays as human-readable format (e.g. "JPEG Image", not "image/jpeg")
- [ ] File size displays formatted (e.g. "4.2 MB")
- [ ] Dimensions display as "3840 × 2160" for images
- [ ] Duration displays as "0:32" for videos
- [ ] Frame count displays for animated content
- [ ] Audio indicator shows for has_audio=true
- [ ] Rating stars render and are editable
- [ ] Status badge shows current status
- [ ] date_added, date_created, date_modified all display
- [ ] Dominant color swatch renders when hex is present
- [ ] Perceptual hash displays when present (debug/advanced section)

## Tags section

- [ ] Tags render as pills/chips with namespace:subtag format
- [ ] Tags with empty namespace show subtag only (no leading colon)
- [ ] Tag source indicator (local vs ai_tagger) is visible
- [ ] Add tag input works (autocomplete)
- [ ] Remove tag button works
- [ ] Tag click navigates to tag scope in grid

## Folders section

- [ ] Folder memberships list with folder names
- [ ] Remove from folder action works
- [ ] Add to folder action works

## Notes section

- [ ] Notes display as key-value pairs from JSON
- [ ] Notes are editable
- [ ] Empty notes section shows "No notes" or similar

## Source URLs section

- [ ] Source URLs display as clickable links
- [ ] Add/remove source URL works
- [ ] Empty source URLs shows appropriate empty state

## Collection details

- [ ] member_count displays (e.g. "47 items")
- [ ] total_size_bytes displays formatted
- [ ] Collection name is editable
- [ ] Collection-specific actions available (split, manage members)

## Multi-selection state

- [ ] Multi-selection shows count (e.g. "3 items selected")
- [ ] Shared tags show across all selected entities
- [ ] Bulk tag add/remove works
- [ ] Bulk rating change works
- [ ] Bulk status change works
- [ ] Inspector shows shared metadata, not first-item-only metadata

## Virtual select-all

- [ ] Virtual select-all shows total count from backend (not enumerated count)
- [ ] Bulk actions work with query_results EntityTarget
- [ ] Inspector shows "1247 items selected" (not blank or "0 selected")

## Edge cases

- [ ] No selection: inspector shows empty/placeholder state
- [ ] Sparse entity (no tags, no folders, no notes): all sections render without error
- [ ] Inbox item (status=0): inspector renders correctly
- [ ] Entity with no thumbnail: preview area shows placeholder
- [ ] Very long name: text truncates or wraps appropriately
- [ ] Entity with many tags (50+): tag section scrolls or wraps
- [ ] Entity with many folders (10+): folder section scrolls or wraps

## Legacy behavior to preserve

- [ ] Inspector panel occupies right side, resizable via drag handle
- [ ] Inspector updates immediately on selection change (no visible lag)
- [ ] Inline editing fields save on blur or Enter
- [ ] Rating hover preview shows before click
- [ ] Context actions available from inspector (open, reveal, export, delete)
