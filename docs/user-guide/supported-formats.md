# Supported File Formats

[← User Guide](README.md)

Picto supports a wide range of media formats. Thumbnails are generated automatically for all supported types.

## Images

| Format | Extensions | Notes |
|--------|-----------|-------|
| JPEG | `.jpg`, `.jpeg` | Most common photo format |
| PNG | `.png` | Lossless with transparency |
| GIF | `.gif` | Static and animated |
| WebP | `.webp` | Static and animated, modern compression |
| AVIF | `.avif` | Next-gen format, best compression |
| JPEG XL | `.jxl` | Modern JPEG successor, static and animated |
| TIFF | `.tiff`, `.tif` | High-quality archival format |
| BMP | `.bmp` | Uncompressed bitmap |
| ICO | `.ico` | Icon format |
| QOI | `.qoi` | Fast lossless format |
| SVG | `.svg` | Vector graphics (rasterized for display) |
| HEIF/HEIC | `.heif`, `.heic` | Apple's high-efficiency format |
| APNG | `.apng` | Animated PNG |

## Video

| Format | Extensions | Notes |
|--------|-----------|-------|
| MP4 | `.mp4` | Most widely supported video format |
| WebM | `.webm` | Web-optimized video |
| MKV | `.mkv` | Matroska container, supports many codecs |
| MOV | `.mov` | Apple QuickTime |
| AVI | `.avi` | Legacy Windows video |
| FLV | `.flv` | Flash video |
| OGV | `.ogv` | Ogg video |
| MPEG | `.mpeg`, `.mpg` | Standard MPEG video |
| WMV | `.wmv` | Windows Media Video |

Video thumbnails are generated from a representative frame using FFmpeg.

## Creative / Project Files

| Format | Extensions | Notes |
|--------|-----------|-------|
| PSD | `.psd` | Adobe Photoshop |
| CLIP | `.clip` | Clip Studio Paint |
| Krita | `.kra` | Krita |
| Paint.NET | `.pdn` | Paint.NET |
| XCF | `.xcf` | GIMP |
| SAI2 | `.sai2` | Paint Tool SAI v2 |

Thumbnails are extracted from embedded previews in these files.

## Documents

| Format | Extensions | Notes |
|--------|-----------|-------|
| PDF | `.pdf` | Portable Document Format |
| EPUB | `.epub` | E-book format |
| DjVu | `.djvu` | Scanned document format |
| CBZ | `.cbz` | Comic book archive (ZIP) |

## Audio

| Format | Extensions | Notes |
|--------|-----------|-------|
| MP3 | `.mp3` | Standard audio |
| OGG | `.ogg` | Ogg Vorbis |
| FLAC | `.flac` | Lossless audio |
| WAV | `.wav` | Uncompressed audio |
| WMA | `.wma` | Windows Media Audio |
| M4A | `.m4a` | AAC audio |

## Animation Formats

| Format | Extensions | Notes |
|--------|-----------|-------|
| Animated GIF | `.gif` | Classic animation format |
| Animated WebP | `.webp` | Modern animated format |
| Animated APNG | `.apng` | Animated PNG |
| Animated JXL | `.jxl` | JPEG XL animation |
| Ugoira | `.ugoira` | Pixiv animation format |

## Export Formats

When [exporting](exporting.md), you can convert to:
- PNG (lossless)
- JPEG (lossy, quality 1-100)
- WebP (lossy, quality 1-100)
- AVIF (lossy, quality 1-100)
