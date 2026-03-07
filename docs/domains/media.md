# Media Domain

## Purpose

The media domain handles file import, processing, and blob storage. It covers the full lifecycle from raw file paths to stored, thumbnailed, and indexed media.

## Import Pipeline

1. **Path resolution** — resolve input paths, detect duplicates via SHA256 hash.
2. **File processing** — extract metadata (dimensions, duration, EXIF), generate thumbnails, compute blurhash, extract dominant colors.
3. **Blob storage** — copy file to content-addressed blob store (`{library}/blobs/{hash[0..2]}/{hash}`).
4. **DB insertion** — single-transaction insert of file record, tags, and blob reference.
5. **Compiler notification** — `FileInserted` event triggers bitmap/projection rebuild.

## File Processing Modules

| Module | Purpose |
|--------|---------|
| `media_processing/mod.rs` | Shared utilities, MIME detection, metadata extraction |
| `archive.rs` | CBZ, EPUB, ZIP extraction and thumbnail generation |
| `blurhash.rs` | BlurHash encoding for progressive image loading |
| `colors.rs` | Dominant color extraction via k-means clustering |
| `ffmpeg.rs` | Video/audio metadata and thumbnail extraction via CLI subprocess |
| `ffmpeg_path.rs` | Bundled ffmpeg/ffprobe binary resolution |
| `gallery_dl_path.rs` | Bundled gallery-dl binary resolution |
| `office.rs` | OOXML and OLE document handling |
| `pdf.rs` | PDF page extraction and thumbnailing |
| `specialty.rs` | Specialty format handlers |
| `svg.rs` | SVG rasterization |

## Blob Store

Content-addressed storage under `{library_root}/blobs/`. Files are stored by their SHA256 hash with a two-character directory prefix for filesystem distribution: `blobs/ab/abcdef1234...`.

Thumbnails are stored separately under `{library_root}/thumbnails/`.

## Key Files

- `core/src/import.rs` — `ImportPipeline`, `ImportOptions`, file processing orchestration
- `core/src/import_controller.rs` — dispatch-layer orchestration
- `core/src/blob_store.rs` — content-addressed blob storage
- `core/src/media_processing/` — file processing modules
- `core/src/sqlite/import.rs` — single-transaction DB insertion
- `core/src/sqlite/files.rs` — file CRUD
- `core/src/dispatch/typed/media_lifecycle.rs` — import, status change, delete commands
- `core/src/dispatch/typed/media_io.rs` — path resolution, thumbnails, blurhash, color search
- `core/src/dispatch/typed/media_metadata.rs` — rating, notes, source URLs
