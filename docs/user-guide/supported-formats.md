# Supported File Formats

[← User Guide](README.md)

Picto separates **acceptance** from **preview support**. Every extension below can be imported and
preserved in the library. A format may still open through its default application when Picto does
not yet have an internal thumbnail or viewer adapter.

The executable source of truth is `core/src/media_processing/formats.rs`; preview capabilities are
owned separately by `core/src/media_capabilities.rs`. This page must change with those registries.

## Images and textures

`.jpg`, `.jpeg`, `.jpe`, `.jfif`, `.png`, `.apng`, `.gif`, `.webp`, `.bmp`, `.dib`, `.tif`,
`.tiff`, `.svg`, `.svgz`, `.ico`, `.icns`, `.cur`, `.heic`, `.heics`, `.heif`, `.heifs`, `.hif`,
`.avif`, `.avifs`, `.jxl`, `.qoi`, `.base64`, `.insp`, `.svga`, `.pag`, `.lottie`, `.dds`, `.exr`,
`.hdr`, `.tga`, `.iff`, `.lbm`.

## Video

`.mp4`, `.m4v`, `.webm`, `.mkv`, `.mov`, `.qt`, `.avi`, `.flv`, `.f4v`, `.wmv`, `.ogv`, `.mpeg`,
`.mpg`, `.mpe`, `.rm`, `.rmvb`, `.3gp`, `.3g2`, `.ts`, `.mts`, `.m2ts`.

Video thumbnails and the internal viewer use FFmpeg-compatible decoding. Whether a particular file
plays depends on its codecs as well as its container extension.

## Audio

`.aac`, `.flac`, `.m4a`, `.mp3`, `.ogg`, `.oga`, `.opus`, `.wav`, `.wave`, `.wma`, `.mka`, `.wv`,
`.tta`.

Audio files are accepted and preserved. They currently use metadata/file presentation rather than
inventing a still-image thumbnail.

## 3D

`.fbx`, `.obj`, `.3ds`, `.3mf`, `.dae`, `.ifc`, `.ply`, `.stl`, `.glb`, `.gltf`, `.vrm`.

## Creative and project files

`.af`, `.afdesign`, `.afphoto`, `.afpub`, `.ai`, `.c4d`, `.cdr`, `.clip`, `.dwg`, `.graffle`,
`.idml`, `.indd`, `.indt`, `.mindnode`, `.psd`, `.psb`, `.psdt`, `.pxd`, `.principle`, `.sketch`,
`.skt`, `.skp`, `.xd`, `.xmind`, `.sai2`, `.kra`, `.xcf`, `.procreate`, `.pdn`.

Picto currently extracts internal thumbnails for PSD, Clip Studio, Krita, Paint.NET and Procreate
files where a valid embedded preview is available. Other project files remain preserved and can be
opened externally.

## Fonts and camera RAW

Fonts: `.ttf`, `.ttc`, `.otf`, `.woff`.

Camera RAW: `.3fr`, `.arw`, `.cr2`, `.cr3`, `.crw`, `.dng`, `.erf`, `.mrw`, `.nef`, `.nrw`, `.orf`,
`.pef`, `.raf`, `.raw`, `.rw2`, `.sr2`, `.srw`, `.x3f`.

## Documents and web files

`.txt`, `.md`, `.markdown`, `.json`, `.rtf`, `.key`, `.numbers`, `.pages`, `.pdf`, `.potx`, `.ppt`,
`.pptx`, `.xls`, `.xlsx`, `.doc`, `.docx`, `.eddx`, `.emmx`, `.html`, `.htm`, `.mhtml`, `.url`,
`.epub`, `.cbz`, `.djvu`, `.djv`, `.swf`, `.eps`.

Generic `.zip` archives are not imported or opened by Picto. Extract the files outside Picto before
importing them. Format-specific containers such as EPUB and CBZ remain supported.

PDF, DOCX, PPTX, plain text, Markdown, JSON, RTF, EPUB, CBZ and DjVu have read-only internal
viewers built on the same document shell. JPEG XL has native decode, thumbnails, image analysis and
the standard image viewer. DOCX/PPTX use embedded thumbnails when the file supplies one; EPUB/CBZ
use archive cover/page previews. Legacy DOC/PPT and spreadsheet formats remain accepted library
files and open externally rather than being misrouted through the image viewer.

## Additional production formats

`.cube`, `.3dl`, `.ies`, `.livp`.

## Current preview tiers

| Tier | Current behavior |
| --- | --- |
| Native raster | Internal preview, thumbnail, dominant colors and perceptual hash for supported static raster images |
| Animated/video | Internal viewer and FFmpeg thumbnail where the installed decoder supports the contained codec |
| Embedded-preview adapter | Thumbnail extracted from supported SVG/project/document/archive formats |
| Read-only document | Internal selectable/pageable viewer with shared Picto navigation chrome |
| Accepted file | Preserved in the library, with file actions and external opening; no fabricated preview |

## Export formats

Picto can export the original file or convert supported raster input to PNG, JPEG, WebP, or AVIF.
JPEG, WebP, and AVIF expose quality controls.
