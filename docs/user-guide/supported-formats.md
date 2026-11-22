# Supported File Formats

[← User Guide](README.md)

Picto imports every file type that is listed on this page and keeps the file in the library
unchanged. It shows only some of these types itself. For a type that it cannot show, the media
view says "Preview is not available for" followed by the file's type. You can open such a file in
its default application: select it in the grid and press `Shift+Enter`, or right-click it and
choose **Open with Default App**.

## Images and textures

`.jpg`, `.jpeg`, `.jpe`, `.jfif`, `.png`, `.apng`, `.gif`, `.webp`, `.bmp`, `.dib`, `.tif`,
`.tiff`, `.svg`, `.svgz`, `.ico`, `.icns`, `.cur`, `.heic`, `.heics`, `.heif`, `.heifs`, `.hif`,
`.avif`, `.avifs`, `.jxl`, `.qoi`, `.base64`, `.insp`, `.svga`, `.pag`, `.lottie`, `.dds`, `.exr`,
`.hdr`, `.tga`, `.iff`, `.lbm`, `.ugoira`.

Picto decodes JPEG XL files itself, makes thumbnails of them and shows them in the image viewer.

## Video

`.mp4`, `.m4v`, `.webm`, `.mkv`, `.mov`, `.qt`, `.avi`, `.flv`, `.f4v`, `.wmv`, `.ogv`, `.mpeg`,
`.mpg`, `.mpe`, `.rm`, `.rmvb`, `.3gp`, `.3g2`, `.ts`, `.mts`, `.m2ts`.

Picto makes the thumbnails of videos with FFmpeg, which is included in Picto. On macOS, Picto
plays QuickTime and MPEG-4 video with the system's own decoder, and it plays other video, for
example WebM and Matroska, with FFmpeg. On Windows and Linux it plays all video with FFmpeg.
Whether a file plays depends on the codecs inside it as well as on its extension.

## Audio

`.aac`, `.flac`, `.m4a`, `.mp3`, `.ogg`, `.oga`, `.opus`, `.wav`, `.wave`, `.wma`, `.mka`, `.wv`,
`.tta`.

An audio file gets a thumbnail that shows its waveform. It plays in Picto's own player, which can
show a visualization. Choose the visualization in **Settings → Preview → Audio**.

## 3D

`.fbx`, `.obj`, `.3ds`, `.3mf`, `.dae`, `.ifc`, `.ply`, `.stl`, `.glb`, `.gltf`, `.vrm`.

## Creative and project files

`.af`, `.afdesign`, `.afphoto`, `.afpub`, `.ai`, `.c4d`, `.cdr`, `.clip`, `.dwg`, `.graffle`,
`.idml`, `.indd`, `.indt`, `.mindnode`, `.psd`, `.psb`, `.psdt`, `.pxd`, `.principle`, `.sketch`,
`.skt`, `.skp`, `.xd`, `.xmind`, `.sai2`, `.kra`, `.xcf`, `.procreate`, `.pdn`.

Picto takes the thumbnail of a PSD, Clip Studio, Krita, Paint.NET or Procreate file from the
preview that is stored inside the file, when the file has a valid one. Picto keeps the other
project files in the library, and you open them in their own applications.

## Fonts and camera RAW

The font types are `.ttf`, `.ttc`, `.otf` and `.woff`. A font opens in a font viewer with the tabs
Preview, Waterfall, Glyphs and Information. The sample text of the Preview tab can be edited.

The camera RAW types are `.3fr`, `.arw`, `.cr2`, `.cr3`, `.crw`, `.dng`, `.erf`, `.mrw`, `.nef`,
`.nrw`, `.orf`, `.pef`, `.raf`, `.raw`, `.rw2`, `.sr2`, `.srw` and `.x3f`.

## Documents and web files

`.txt`, `.md`, `.markdown`, `.json`, `.rtf`, `.key`, `.numbers`, `.pages`, `.pdf`, `.potx`, `.ppt`,
`.pptx`, `.xls`, `.xlsx`, `.doc`, `.docx`, `.eddx`, `.emmx`, `.html`, `.htm`, `.mhtml`, `.url`,
`.epub`, `.cbz`, `.djvu`, `.djv`, `.swf`, `.eps`.

A `.zip` archive is expanded when you choose it for import yourself or when a subscription
downloads it. The ZIP file may be at most 1 GiB, the files that Picto takes out of it may be at
most 1 GiB together, and a single entry may be at most 512 MiB. The files that Picto accepts from
the archive are imported as one collection, and the ZIP file itself is not kept. Archives inside
the archive are not expanded. A ZIP file that Picto finds while it imports or watches a folder is
ignored. EPUB and CBZ files are not treated as ZIP archives and stay supported.

PDF, plain text, Markdown, JSON, RTF, EPUB, CBZ and DjVu files open in Picto's document viewers,
which show the document and do not change it. `J` and `L` go to the previous and the next page.
A PDF file, a DjVu file and a CBZ comic show all their pages in one column that scrolls. An EPUB
book shows one chapter at a time. Text cannot be selected in the document viewers. See
[Viewing Images and Video](viewing-images-and-video.md#documents-and-comics).

Word and PowerPoint files (`.doc`, `.docx`, `.ppt`, `.pptx`, `.potx`), spreadsheets and the other
types of this section have no viewer. Open them in their default application. A DOCX or PPTX file
gets the thumbnail that is stored inside it, when it has one. An EPUB or CBZ file gets its cover
or its first page as its thumbnail.

Flash (`.swf`) files play in a built-in Flash player (Ruffle) that has play, pause and stop
controls.

## Additional production formats

`.cube`, `.3dl`, `.ies`, `.livp`.

## Current preview tiers

| Tier | What Picto does |
| --- | --- |
| Native raster | For a still raster image that it can decode, Picto shows the image, makes a thumbnail, and computes the main colors and the perceptual hash. |
| Animated and video | Picto plays the file in its viewer and makes a thumbnail with FFmpeg, when the decoder supports the codec inside the file. |
| Embedded preview | Picto takes the thumbnail from the SVG, project, document or archive file itself, for the formats named above. |
| Document | Picto shows the document in a document viewer. The document cannot be changed there. |
| Accepted file | Picto keeps the file in the library and offers the file actions and opening in another application. It makes no preview. |

## Export formats

Picto can export the original file. It can convert a raster image that it can decode to PNG,
JPEG, WebP or AVIF, and JPEG, WebP and AVIF have a quality setting. It can also write the original
files into one ZIP archive. See [Exporting](exporting.md).
