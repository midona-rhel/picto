# Duplicates

[← User Guide](README.md)

Picto finds near-duplicate images: the same picture saved at another size, in another format or
with another level of compression. You look at each pair side by side and decide what to keep.

## How pairs are found

Picto computes a perceptual hash for every still image in a format that it can decode itself.
Videos, audio files and documents are not compared.

Picto scans by itself: after the perceptual hashes of new images are computed, it scans the
library in the background. To start a scan by hand, open **Duplicates** in the sidebar while the
queue is empty and click **Scan library**. A notification says how many new pairs the scan found.

The similarity that makes two images a pair is fixed. No setting changes it.

## Review a pair

Open **Duplicates** in the sidebar. The screen shows one pair at a time.

- The two pictures are side by side. A picture that is narrower than its half of the screen is
  drawn against the middle of the screen, so that the two pictures are next to each other.
- Above each picture is a row with the file's name on the left and the **Keep left** or **Keep
  right** button on the right.
- Under each picture is a list of facts about the file: Dimensions, Size, Type, Rating, Tags, Date
  Created, Date Imported, Group and Occurrences. The Group row is shown for both files when either
  file is in a collection. A row whose value is the same for both files is drawn in the dimmer
  text color. A row whose value differs is drawn in the normal text color for both files.

The title bar holds the other controls, from left to right:

- The **Previous pair** button, the position in the queue (for example "1 / 500") and the **Next
  pair** button.
- One line of small text with the similarity and the pixel measurement, for example "100% similar
  · 0% of pixels differ".
- The **Smart merge**, **Not duplicates**, **Keep both** and **Show Difference** buttons.
- The **Zoom to fit** and **Actual pixels** buttons.

The similarity is a perceptual measure, and two pictures that are 100% similar can still have
different pixels. The pixel measurement says what share of the pixels of the two original files
differ. It appears when Picto has finished comparing the two files. When the two files cannot be
compared, because one of them is not a still image or could not be decoded, the line shows only
the similarity. Look at the pictures before you merge.

When the title bar is too narrow, Picto first cuts the pixel measurement short, then hides the
labels of the four buttons from **Smart merge** to **Show Difference**, then removes the line of
text, and then removes the position, the two zoom buttons and the **Previous pair** and **Next
pair** buttons. A button without its label shows the label as its tooltip.

### Compare

- The two pictures are zoomed and panned together. Turn the mouse wheel to zoom and drag to pan on
  either picture. The other picture follows, so both always show the same part. The mouse wheel
  never goes to another pair.
- **Zoom to fit** (`F`, or a double-click on a picture) shows the whole pictures. **Actual pixels**
  (`Mod+0`) shows them at their real size.
- **Show Difference** replaces both pictures with a picture of the pixels that differ. It does so
  while the pointer is on the button, while the `D` key is held down, and after a click on the
  button until the next click anywhere.
- While the pointer is on **Smart merge**, or the button has the keyboard, the picture that Smart
  merge would keep has the outline that the grid draws around a selected tile. No outline is
  drawn when Picto cannot rate one file as the better one.

Right-click either picture to open the file in its default application or in a new window.

## Resolve a pair

| Key | Button | Result |
|---|---|---|
| `S` | Smart merge | Picto keeps the better file (see below). |
| `Z` | Keep left | Picto keeps the left file and removes the right file. |
| `X` | Keep right | Picto keeps the right file and removes the left file. |
| `B` | Keep both | Picto keeps both files, and the pair leaves the queue. |
| `N` | Not duplicates | Picto records that the two files are different media, and the pair leaves the queue. |
| `←` and `→` | The arrows in the title bar | They go to the previous and to the next pair. |

When you keep one file, the library entries of the other file are changed to use the kept file, so
their tags, folders, ratings and collections stay. The better of the two names is kept. The
removed file is deleted from the library's storage once the decision can no longer be undone.

After each decision Picto shows the next pair. Every decision can be undone with `Mod+Z`.

## Smart Merge

Smart Merge chooses the file to keep by the quality of the two files:

- When the two files are identical, it keeps either one.
- When the two images are nearly identical and one is larger in both dimensions, it keeps the
  larger one.
- When the two images have the same pixels, it keeps a lossless file over a lossy one. Between two
  files of the same lossy format, it keeps the larger file.

When neither file is clearly better, Picto says **Smart merge needs a choice** and leaves the
pair. Resolve it with **Keep left**, **Keep right** or **Keep both**.
