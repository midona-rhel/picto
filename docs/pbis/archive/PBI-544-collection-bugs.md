# PBI-544: Collection bugs

## Priority
P1

## Issues

### Merging collections creates dummy
- Merging two collections creates a dummy empty collection instead of properly merging images from one into the other.

### Empty collections left behind
- Sometimes empty collection entities remain in the database after all members are removed or trashed. Cannot be deleted via UI — `delete_entities` returns success but the collection persists.

### Can't DnD collections to folders
- Dragging a collection tile to a folder in the sidebar doesn't work. The internal drag system uses file hashes which don't resolve for collection entities.

### Can't add collections to folders
- Inspector folder add/remove for collections doesn't work properly (partially addressed but still broken on some paths).

### Auto-tag doesn't tag collection children
- Selecting multiple collections and pressing auto-tag only tags the collection entities, not their member files.

### Thumbnails not cleared after delete
- After deleting files, their thumbnails remain cached and the grid shows stale tiles until a full refresh.
