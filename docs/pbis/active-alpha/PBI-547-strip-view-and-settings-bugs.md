# PBI-547: Strip view and settings bugs

## Priority
P2

## Issues

### Smooth zoom not working
- The smooth zoom transition (200ms ease-out) may not be applying in all cases or on all platforms.

### Strip view minimum size
- No minimum size enforced for the strip view — can be resized too small.

### Strip view background color
- Strip view background doesn't use the theme color on initial load. Only updates after a reload. Should be driven by CSS variable, not runtime state.

### Default view settings not applied
- The `stripDefaultFitMode` and other grid default settings don't seem to take effect when opening a new view.

### Schedule dropdown below modal
- When creating a new subscription group, the schedule dropdown renders below/behind the modal instead of inside it.

### AI model download progress
- No progress indicator when downloading AI tagger models.
- Models can't be enabled until downloaded but there's no clear UI feedback about download state.
