# Duplicates

[← User Guide](README.md)

Picto finds near-duplicate images: the same picture saved at another size, format or compression
level. You review each pair side by side and decide what to keep.

## How pairs are found

Picto computes a perceptual hash for each static image (formats Picto can decode and preview
itself). Videos, audio and documents are not compared.

Scanning happens automatically: after new images finish their perceptual hash, Picto scans the
library in the background. To scan by hand, open **Duplicates** in the sidebar when the queue is
empty and click **Scan library**. A notification reports how many new pairs were found.

The similarity threshold is fixed; there is no setting for it.

## Review a pair

Open **Duplicates** in the sidebar. The two candidates appear side by side, each with its keep
button and details: name, resolution, size, format, rating, tag count, created and added dates,
group (if the image is part of a collection) and occurrences. The footer shows the similarity
percentage. Perceptual similarity is not pixel equality, so check the images before you merge.

### Compare

- **Zoom and pan are linked.** Scroll to zoom and drag to pan on either side; the other side follows
  so you always see the same region.
- **Zoom to fit** (`F`, or double-click a preview) and **Actual pixels** (`Mod+0`) are in the title
  bar, next to the pair counter and the previous/next arrows.
- **Show Difference** — hold the pointer over the button (or focus it) to replace both previews with
  a difference overlay. Release to return to the images.
- Hover **Smart merge** to highlight the file that Smart Merge would keep.

Right-click either preview to open the file in its default app or in a new window.

## Resolve a pair

| Key | Button | Result |
|---|---|---|
| `S` | Smart merge | Keeps the better file automatically (see below) |
| `Z` | Keep left | Keeps the left file; the right file is removed |
| `X` | Keep right | Keeps the right file; the left file is removed |
| `B` | Keep both | Keeps both files; the pair leaves the queue |
| `N` | Not duplicates | Marks them as different media; the pair leaves the queue |
| `←` / `→` | Arrows in the title bar | Previous / next pair |

When you keep one file, the other file's library entries are pointed at the kept file, so their
tags, folders, ratings and collections stay. The better of the two names is kept. The removed
file is deleted from the library's storage once the decision can no longer be undone.

After each decision the next pair loads. Every decision can be undone with `Mod+Z`.

## Smart Merge

Smart Merge picks the winner from file quality:

- identical files: either one
- near-identical images where one is larger in both dimensions: the larger one
- same pixels: a lossless file over a lossy one; for the same lossy format, the larger file

If there is no clear winner, Picto says **Smart merge needs a choice** and leaves the pair for you
to resolve with Keep left, Keep right or Keep both.
