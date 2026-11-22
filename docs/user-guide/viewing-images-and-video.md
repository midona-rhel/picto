# Viewing Images and Video

[← User Guide](README.md)

Picto has three ways to look at an item:

| Viewer | How to open it | How to close it |
|--------|----------------|-----------------|
| The media view, which fills the area of the grid | Press `Enter` or double-click the item. | Press `Escape`, `Enter` or `Space`, or click the back button. |
| Quick Look, which is a preview over the grid | Press `Space`. | Press `Space`, `Escape` or `Enter`. |
| A separate window | Press `Mod+O`, choose **Open in New Window**, or middle-click the item. | Press `Escape` or click the window's close button. |

In **Settings → Controls** you can make a double-click open the item in its default application,
and you can make a middle-click do nothing.

## Navigate and rate

| Key | Action |
|-----|--------|
| `Left` or `A` | It goes to the previous item. |
| `Right` or `D` | It goes to the next item. |
| `0` to `5` | It sets the rating, in the media view and in Quick Look. |
| `Mod+C` | It copies the item's file. |

At the first and at the last item of the list, Picto shows the hint **First item** or **Last
item**.

Picto reads a picture's original file once the item has been on screen for 200 ms. It waits the
same 200 ms before it opens a video, an audio file, a document or a Flash movie. When you hold an
arrow key down to go through items, none of them is opened until you release the key.

In the Inbox, the media view and Quick Look show Accept and Reject buttons. See
[Importing Files](importing-files.md#reviewing-the-inbox).

## Zoom and pan (images, PDF, JPEG XL)

| Action | How |
|--------|-----|
| Zoom in or out | Turn the mouse wheel, which zooms toward the pointer, or press `+` or `-`. |
| Fit to window | Press `` ` ``. With the EU keyboard layout the key is `Shift+F`. |
| Actual size | Press `Mod+0`. |
| Pan | Drag the image. |

The title bar shows a zoom slider, the zoom in percent, and the **Fit to window** and **Actual
size** buttons. When the image is larger than the window, the media view shows a small picture of
the whole image in its bottom right corner. Drag in that picture to pan. Quick Look does not show
it.

On macOS, **Settings → Controls → Media view gestures** can change the media view to panning with
two fingers on the trackpad and zooming with a pinch. **Settings → Preview** sets **Scaling**
(Smooth or Pixelated), **Default zoom** (Fit to window or Actual size) and **Show transparency
grid**.

## Video and audio

| Key | Action |
|-----|--------|
| `K` | It plays or pauses. A click on the video does the same. |
| `J` and `L` | They go back and forward by 5 seconds. |
| `Up` and `Down` | They turn the volume up and down. |
| `M` | It mutes or unmutes the sound. |
| `O` | It turns the loop on or off. |
| `]` and `[` | They make playback faster and slower. With the EU keyboard layout the keys are `Shift+.` and `Shift+,`. |
| `\` | It sets the speed back to 1x. With the EU keyboard layout the key is `Shift+R`. |
| `F` | It turns full screen on or off, for video only. A double-click on the video does the same. |

The speeds are 0.25x, 0.5x, 0.75x, 1x, 2x and 3x. The bar of controls under the video has a seek
bar, a play button, a loop button, a speed menu, the volume and a full screen button. **Autoplay
videos** and **Loop videos** are on by default; change them in **Settings → Preview**. An audio
file shows a visualization, which is None, Spectrum, Oscilloscope or Orbit and is set in
**Settings → Preview**.

## Documents and comics

PDF files, EPUB books, CBZ comic archives, DjVu files and text files (plain text, Markdown, JSON
and RTF) open in a document viewer. Under the page is a bar with the **Previous page** button, the
text "Page n of m" and the **Next page** button.

| Key | Action |
|-----|--------|
| `J` | It goes to the previous page. |
| `L` | It goes to the next page. |
| `Home`, `End`, `Page Up`, `Page Down` and `Space` | They scroll the pages. |

- A PDF file, a DjVu file and a comic archive show all their pages in one column, one under the
  other, and the column scrolls.
- The zoom controls change the size of the pages of a PDF file. The pages of a DjVu file and of a
  comic archive cannot be zoomed. A page that is wider than the viewer is made smaller until it
  fits.
- A text file is one page. Picto shows the first 4 MB of a longer text file and says so under the
  text.
- An EPUB book shows one chapter at a time. The buttons and the keys go to the chapter before it
  and to the chapter after it.

The document viewers do not do these things: text in a PDF file or on a page cannot be selected,
an RTF file is shown without its character formatting, a Markdown file is shown without its
images and its raw HTML, and an EPUB chapter is shown without its own stylesheet.

Word and PowerPoint files have no viewer. The media view shows the message "Preview is not
available for" followed by the file's type. To open such a file in the application that the
system uses for it, select it in the grid and press `Shift+Enter`, or right-click it and choose
**Open with Default App**.

## Fonts

Font files open in a font viewer with the tabs **Preview**, **Waterfall**, **Glyphs** and
**Information**. You can edit the preview text. `Mod+=`, `Mod+-` and `Mod+0` change the size of
the preview.

## Flash

SWF files play in a built-in Flash player (Ruffle) that has a Play or Pause button and a Stop
button. `K` plays or pauses, `Up` and `Down` change the volume, and `M` mutes the sound.

## Separate window

**Open in New Window** opens the item in a window of its own. You can keep several such windows
open.

- `Left` and `Right` go through the items that were selected when you opened the window.
- On macOS and Windows, `Shift+T` or the pin button keeps the window on top of other windows. On
  Linux the window has no pin button and cannot be kept on top.
- `Mod+Alt+C` copies the file's path, and `Mod+C` copies the file.
- The zoom keys work as in the media view.

When you switch to another library, Picto closes these windows.

## Collections

Opening a collection shows its members in a reader that scrolls. See [Collections](collections.md).
