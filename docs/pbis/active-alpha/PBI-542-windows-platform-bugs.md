# PBI-542: Windows platform bugs

## Priority
P1

## Issues

### Process windows appearing
- ffmpeg and gallery-dl spawn visible console windows on Windows. Need `CREATE_NO_WINDOW` flag or `windowsHide: true` on all subprocess spawns.
- Batch delete triggers deferred phash/color workers that also spawn process windows.

### Burger menu not respected
- On Windows (no traffic lights), the burger menu positioning/spacing doesn't account for the different titlebar layout.

### Library name shows full filepath
- Sidebar/titlebar shows `<personal-home>\Pictures\Main.library` instead of just `Main`.

### Minimum window size
- No minimum window size set for the main window — can be resized to unusable dimensions.

### Grid combobox background
- Select/combobox dropdowns in the grid toolbar have a white background box behind them instead of matching the theme.

### Sort buttons not visually updating
- Asc/desc sort toggle buttons don't visibly change state when pressed on Windows (maybe macOS too).
