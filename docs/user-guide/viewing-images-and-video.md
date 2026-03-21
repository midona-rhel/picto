# Viewing Images and Video

[← User Guide](README.md)

## Detail View

Press `Enter` or double-click a thumbnail to open the detail view — a full-screen image viewer with carousel navigation.

### Navigation

| Key | Action |
|-----|--------|
| `Left Arrow` or `A` | Previous image |
| `Right Arrow` or `D` | Next image |
| `Escape` | Close detail view |

### Zoom and Pan

| Action | How |
|--------|-----|
| Zoom in/out | Scroll wheel, or `+`/`-` keys |
| Fit to window | `` ` `` (backtick) |
| Actual size (100%) | `Ctrl+0` |
| Pan | Click and drag the image |

Zoom follows the cursor — scroll while pointing at a specific area to zoom into that spot.

### Navigator Minimap

When zoomed in, a minimap appears in the corner showing your current viewport position on the full image. Drag within the minimap to pan quickly.

Toggle the navigator with `Ctrl+Alt+8`.

### Image Adjustments

- **Grayscale preview** — `Ctrl+Alt+G` toggles a grayscale filter for evaluating composition
- **Rotation** — Rotate the image 90/180/270 degrees
- **Mirror** — Flip the image horizontally

Adjustments are per-image and cached during your session.

## Quick Look

Press `Space` on a selected thumbnail for a quick preview without entering the full detail view. Press `Space` again or `Escape` to dismiss.

## Detail Window

Press `Ctrl+O` to open the selected image in a separate floating window. The detail window has:

- Independent zoom state from the main detail view
- **Always-on-top** toggle (`Shift+T`) to keep it above other windows
- Auto-hiding toolbar (appears on mouse movement)
- Full zoom, pan, and navigator support

## Slideshow

Press `F5` to start a slideshow. Images advance automatically on a timer. Press `Escape` to stop.

## Video Playback

Picto includes a built-in video player for MP4, WebM, MKV, and other supported formats.

| Key | Action |
|-----|--------|
| `Space` | Play / Pause |
| `Arrow Up` | Volume up |
| `Arrow Down` | Volume down |
| `M` | Toggle mute |
| `L` | Toggle loop |
| `]` | Increase playback speed |
| `[` | Decrease playback speed |
| `Backspace` | Reset speed to 1x |

The video player shows a seek bar, current time, duration, and playback rate. Videos auto-play by default (configurable in [Settings](settings.md)).

## Collections in Detail View

When viewing a file that belongs to a [collection](collections.md), a strip view appears showing all collection members as a horizontal carousel. Navigate between members with the arrow keys.
