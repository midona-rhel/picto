# Picto application icon sources

`picto-flat.svg` is the Windows and Linux master. The three numbered macOS SVGs are the accepted book geometry split into native material layers:

1. `picto-macos-01-spine.svg`
2. `picto-macos-02-under-pages.svg`
3. `picto-macos-03-open-book.svg`

All macOS layers use the same 1024×1024 canvas. They remain flat and opaque: no rounded-square mask, baked blur, highlight, refraction, or shadow belongs in the source artwork. `npm run generate:app-icons` renders those layers and writes `build/Picto.icon`, whose groups apply Liquid Glass, specular lighting, and neutral shadows. This follows Apple’s Icon Composer model while keeping the accepted geometry reproducible.

The macOS package points directly at `build/Picto.icon`. Electron Builder compiles it with Xcode 26 or newer `actool`, embeds the resulting `Assets.car`, and also generates the legacy ICNS representation. Do not replace this with a manually composited PNG or ICNS fallback.

`picto-library-folder.svg` is the neutral folder artwork for Picto `.library` packages. The generator produces `library.icns` for macOS Finder, `library.ico` for Windows library folders, and a PNG preview used by release audits. Linux has no portable per-directory icon contract across file managers, so the source remains available to Picto without writing desktop-specific metadata.

Verification:

1. Run `npm run generate:app-icons` and confirm the three generated assets are 1024×1024 RGBA PNGs.
2. Open `build/Picto.icon` in Icon Composer when visually tuning material properties. Do not edit the generated geometry there; update the numbered SVG master instead.
3. Preview Default, Dark, and Mono appearances on macOS in Icon Composer.
4. Run the macOS `release/**` CI package job. It uses the Apple Silicon `macos-26` runner so Xcode can compile the native icon asset.
5. Inspect the packaged app for `Contents/Resources/Assets.car` and `Contents/Resources/icon.icns`.

Apple’s source guidance: keep source art simple, consistently sized, and layered; let Icon Composer own dynamic effects and deliver a `.icon` file.
