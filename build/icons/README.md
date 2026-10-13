# Picto application icon sources

`picto-flat.svg` is the Windows and Linux master. `picto-macos-book.svg` is the same book geometry as one macOS layer, so Icon Composer treats the strokes as one mark set into the face rather than separate floating pieces.

The macOS layer uses the 1024×1024 canvas and stays flat and opaque: no rounded-square mask, baked blur, highlight, refraction, or shadow belongs in the source artwork. `npm run generate:app-icons` renders it and writes `build/Picto.icon`, whose single group applies Liquid Glass and specular lighting with combined lighting and no drop shadow. Icon Composer's tile fills the whole canvas while the flat master's tile is 896 px, so the layer is scaled by 1024/896 to keep the book the same size relative to its tile on every platform.

The macOS package points directly at `build/Picto.icon`. Its PNG layer is generated, not tracked. Both `npm run dist:electron` and the alpha packaging script regenerate it from the SVG before packaging. Electron Builder compiles it with Xcode 26 or newer `actool`, embeds the resulting `Assets.car`, and also generates the legacy ICNS representation. Do not replace this with a manually composited PNG or ICNS fallback.

`picto-library-folder.svg` is the neutral folder artwork for Picto `.library` packages. The generator produces `library.icns` for macOS Finder, `library.ico` for Windows library folders, and a PNG preview used by release audits. Linux has no portable per-directory icon contract across file managers, so the source remains available to Picto without writing desktop-specific metadata.

`picto-pack.svg` is the portable-package artwork for `.picto-pack` files. The generator produces `picto-pack.icns`, `picto-pack.ico`, and a PNG preview. Its orthogonal front, perspective lid, and center seam form a softly lit box, while the Picto book mark identifies its contents.

Verification:

1. Run `npm run generate:app-icons` and confirm `build/Picto.icon/Assets/01-book.png` is a 1024×1024 RGBA PNG.
2. Open `build/Picto.icon` in Icon Composer when visually tuning material properties. Do not edit the generated geometry there; update `build/icons/picto-macos-book.svg` instead.
3. Preview Default, Dark, and Mono appearances on macOS in Icon Composer.
4. Run the macOS `release/**` CI package job. It uses the Apple Silicon `macos-26` runner so Xcode can compile the native icon asset.
5. Inspect the packaged app for `Contents/Resources/Assets.car` and `Contents/Resources/icon.icns`.

Apple’s source guidance: keep source art simple, consistently sized, and layered; let Icon Composer own dynamic effects and deliver a `.icon` file.
