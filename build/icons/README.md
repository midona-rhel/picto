# Picto application icon sources

`scripts/generate-app-icons.mjs` (`npm run generate:app-icons`) draws every icon in `build/` from
the SVG files in this folder. It needs the repository's npm packages (`npm ci`).

## The app's icon

`picto-flat.svg` is the icon for Windows and Linux. The script draws `build/icon.ico` and
`build/icon-flat.png` from it.

`picto-macos-book.svg` holds the same book as one layer for macOS. Its canvas is 1024×1024 px. The
artwork is flat and opaque: it has no rounded-square mask, no blur, no highlight, no refraction and
no shadow, because Icon Composer adds those effects. The script draws the layer as
`build/Picto.icon/Assets/01-book.png` and writes `build/Picto.icon/icon.json`. That file describes
one group with the glass material, specular lighting, combined lighting and no drop shadow. The
tile of an Icon Composer icon fills the whole canvas, while the tile in `picto-flat.svg` is 896 px
wide, so the layer is scaled by 1024/896. The book then has the same size relative to its tile on
every system.

The picture of the layer is generated and is not tracked. `app/package.sh` runs the script before
it packages the app on macOS, and then compiles `build/Picto.icon` with `actool` of Xcode 26 or
later into `Assets.car` and `Icon.icns`, which it puts into the app bundle. Do not put a PNG or an
ICNS file that was composed by hand in its place.

## The icons of libraries and packs

`picto-library-folder.svg` is the folder artwork of a Picto `.library` folder. The script draws
`build/library.icns` for the macOS Finder, `build/library.ico` for Windows, and the preview
`build/library-folder.png`. Linux file managers share no way to give one folder its own icon, so
nothing is made for Linux.

`picto-pack.svg` is the artwork of a `.picto-pack` file: a box with the Picto book on its front.
The script draws `build/picto-pack.icns`, `build/picto-pack.ico` and the preview
`build/picto-pack.png`.

The `.icns` files are made with macOS's `iconutil`, so the script writes them only when it runs on
macOS.

## Checking a change

1. Run `npm run generate:app-icons`. `build/Picto.icon/Assets/01-book.png` must be a PNG of
   1024×1024 px with an alpha channel.
2. To adjust the material, open `build/Picto.icon` in Icon Composer. Do not change the book's shape
   there. Change `build/icons/picto-macos-book.svg` and run the script again.
3. Look at the Default, Dark and Mono appearances in Icon Composer.
4. Run the macOS job of the Package workflow. It runs on the Apple Silicon runner `macos-26`,
   whose Xcode compiles the icon.
5. Check that the packaged app holds `Contents/Resources/Assets.car` and
   `Contents/Resources/Icon.icns`.

Apple's guidance for the source artwork: keep it simple, keep its layers the same size, and leave
the effects that change with the system's appearance to Icon Composer.
