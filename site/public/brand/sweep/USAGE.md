# kuizer / Sweep

The selected identity is Sweep from Round 01. The original app tile is preserved in `source/app-icon.png`.

| Folder | Use |
| --- | --- |
| `marks` | Transparent color mark and single-color black, white and orange marks |
| `wordmarks` | Outlined IBM Plex Mono typography in black, white and orange |
| `logos` | Horizontal and stacked logos for light and dark backgrounds |
| `icons` | App PNGs from 16 to 1024 pixels, maskable web icons and a macOS ICNS |
| `favicons` | Multi-resolution ICO, PNGs, theme-specific SVGs and Apple touch icon |
| `social` | Open Graph 1200 × 630, square 1200 × 1200, GitHub 1280 × 640, banner 1500 × 500 and README 1280 × 320 |
| `email` | Transparent email signatures at 600 × 180 |
| `source` | Original tile, generated derivatives, outlined typography, prompts and font license |

Names ending in `light` use dark lettering for a light background. Names ending in `dark` use white lettering for a dark background. PNG and WebP logos have transparent backgrounds. Social images have an opaque background.

Use the original full-color tile for app icons and avatars. Use the detached color mark in larger compositions. Use a single-color mark at small sizes or when color is unavailable. Keep clear space of at least one detached block around the mark. Preserve its proportions and three blocks.

Single-color SVGs and wordmarks contain vector paths. Full-color SVG logos embed the raster mark alongside outlined vector typography. The color artwork is raster, with a 1254-pixel original app tile and a 1024-pixel detached mark. Do not treat these full-color SVGs as infinitely scalable vector artwork.

`manifest.json` lists file dimensions, formats and SHA-256 hashes. Regenerate exports from the site directory with `bun scripts/build-brand.mjs`. The exporter uses the existing Sharp dependency and the system `zip` command. The selected artwork and outlined typography are bundled, so regeneration needs no image API or installed fonts.
