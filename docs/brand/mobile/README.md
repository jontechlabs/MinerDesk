# MinerDesk Mobile identity

The mobile identity extends MinerDesk's existing **M**, mint, cyan and midnight palette. A continuous, geometric M and three small node cut-outs suggest the connected rigs managed from your phone. The mark stays legible at launcher size and does not depend on text, coin symbols or a version badge.

## Native icon sources

`icon-manifest.json` is the Tauri icon pipeline input. Its paths are relative to this directory:

```sh
cd apps/mobile
npm run tauri -- icon ../../docs/brand/mobile/icon-manifest.json --output src-tauri/icons
```

- `launcher-source.png`: 1024 × 1024 RGB, fully opaque square master. No baked-in rounded corners, border, text or drop shadow. The operating system applies its own mask.
- `ios-source.png`: identical opaque master, suitable for the iOS app icon set. This supplies artwork only; it does not provide Apple signing or App Store approval.
- `adaptive-background.png`: opaque midnight layer for Android adaptive icons.
- `adaptive-foreground.png`: 432 × 432 RGBA with only the centered M. Every non-transparent pixel is inside Android's centered 66/108 safe circle. The image already includes the required breathing room; `android_fg_scale` is 100 to avoid padding it twice.
- `monochrome.png` / `monochrome.svg`: a white alpha mask for Android's themed launcher icon. The operating system supplies the themed colors.
- `launcher-source.svg` / `logomark.svg`: editable original geometry, with square and transparent treatments respectively.
- `wordmark-dark.svg` / `wordmark-light.svg`: editable horizontal lockups, with corresponding PNG exports. SVG text uses the existing product's Inter / Segoe UI / sans-serif stack; no proprietary font is embedded or redistributed.
- `favicon.svg`: the same mark with a rounded browser-only tile. Do not substitute this browser treatment for the unmasked iOS source.
- `icon-32.png` through `icon-512.png`: convenient opaque raster exports.
- `geometry.json`: the same path geometry and palette used by the source SVG and the in-app React component.

## Usage

Use the gradient mark on midnight or similarly dark surfaces. On light backgrounds, prefer a dark tile or the light wordmark lockup. Preserve the aspect ratio, three nodes and mint-to-cyan direction. Keep clear space of at least one stem width around the mark; do not append coins, profit promises or platform badges to the icon.

| Color | Value | Use |
| --- | --- | --- |
| Mint | `#70F6A0` | Left side / connection accent |
| Cyan | `#48D5F7` | Right side / mobile accent |
| Midnight | `#0C1423` | Native icon background |
| Light | `#EDF4FF` | Dark-surface wordmark |

## Regeneration and license

The assets are original repository-native vector geometry. Regenerate the PNGs with Python and Pillow:

```sh
python3 scripts/mobile-brand-assets.py --preview /absolute/path/to/brand-preview.png
```

All identity files and the generator are covered by the repository's [MIT license](../../../LICENSE), copyright © 2026 MinerDesk. There are no stock images, downloaded icon packs or embedded font files. Raster icon geometry is deterministic across platforms; the optional brand preview's text uses an installed system font.
