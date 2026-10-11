# Supported File Formats

[← User Guide](README.md)

Picto separates **acceptance** from **preview support**. Every extension below can be imported and
preserved in the library. A format may still open through its default application when Picto does
not yet have an internal thumbnail or viewer adapter.

## Images and textures

`.jpg`, `.jpeg`, `.jpe`, `.jfif`, `.png`, `.apng`, `.gif`, `.webp`, `.bmp`, `.dib`, `.tif`,
`.tiff`, `.svg`, `.svgz`, `.ico`, `.icns`, `.cur`, `.heic`, `.heics`, `.heif`, `.heifs`, `.hif`,
`.avif`, `.avifs`, `.jxl`, `.qoi`, `.base64`, `.insp`, `.svga`, `.pag`, `.lottie`, `.dds`, `.exr`,
`.hdr`, `.tga`, `.iff`, `.lbm`, `.ugoira`.

## Video

`.mp4`, `.m4v`, `.webm`, `.mkv`, `.mov`, `.qt`, `.avi`, `.flv`, `.f4v`, `.wmv`, `.ogv`, `.mpeg`,
`.mpg`, `.mpe`, `.rm`, `.rmvb`, `.3gp`, `.3g2`, `.ts`, `.mts`, `.m2ts`.

Video thumbnails and the internal viewer use FFmpeg-compatible decoding. Whether a particular file
plays depends on its codecs as well as its container extension.

## Audio

`.aac`, `.flac`, `.m4a`, `.mp3`, `.ogg`, `.oga`, `.opus`, `.wav`, `.wave`, `.wma`, `.mka`, `.wv`,
`.tta`.

Audio files get a waveform thumbnail and play in the internal player, with an optional
visualization (**Settings → Preview → Audio**).

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

Fonts: `.ttf`, `.ttc`, `.otf`, `.woff`. Fonts open in a font viewer with Preview (editable sample
text), Waterfall, Glyphs and Information tabs.

Camera RAW: `.3fr`, `.arw`, `.cr2`, `.cr3`, `.crw`, `.dng`, `.erf`, `.mrw`, `.nef`, `.nrw`, `.orf`,
`.pef`, `.raf`, `.raw`, `.rw2`, `.sr2`, `.srw`, `.x3f`.

## Documents and web files

`.txt`, `.md`, `.markdown`, `.json`, `.rtf`, `.key`, `.numbers`, `.pages`, `.pdf`, `.potx`, `.ppt`,
`.pptx`, `.xls`, `.xlsx`, `.doc`, `.docx`, `.eddx`, `.emmx`, `.html`, `.htm`, `.mhtml`, `.url`,
`.epub`, `.cbz`, `.djvu`, `.djv`, `.swf`, `.eps`.

Generic `.zip` archives explicitly selected for import or downloaded by a subscription are safely
expanded. The ZIP file and its accepted expanded contents are each capped at 1 GiB, with a 512 MiB
per-entry limit. Accepted members are ingested as a collection and the container is discarded;
nested archives are not expanded. ZIPs encountered while importing or watching a folder are
ignored. Format-specific containers such as EPUB and CBZ remain supported.

PDF, DOCX, PPTX, plain text, Markdown, JSON, RTF, EPUB, CBZ and DjVu open in read-only internal
viewers; use `J` and `L` for the previous and next page. CBZ comics and EPUB books open as paged
documents. DOCX/PPTX use embedded thumbnails when the file supplies one; EPUB/CBZ use their cover or
first page. JPEG XL has native decoding, thumbnails and the standard image viewer.

Flash (`.swf`) files play in a built-in Flash player (Ruffle) with play, pause and stop controls.

Legacy DOC/PPT and spreadsheet formats are accepted and open in their default application.

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
