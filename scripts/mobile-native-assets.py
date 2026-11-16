#!/usr/bin/env python3
"""Verify the icons in the actual initialized Tauri native projects.

Run `tauri icon` both before and after `android init`/`ios init`, then run
this check. Tauri 2.12.1 first falls back to icons/android or icons/ios when
the native project does not exist; initialization otherwise installs its own
placeholder artwork. This script verifies assets, never copies or edits them.
"""
import argparse
import hashlib
import io
import json
from pathlib import Path
import xml.etree.ElementTree as ET
import zipfile

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
ANDROID_NS = '{http://schemas.android.com/apk/res/android}'
DENSITIES = {'mdpi': (48, 108), 'hdpi': (72, 162), 'xhdpi': (96, 216),
             'xxhdpi': (144, 324), 'xxxhdpi': (192, 432)}
ANDROID_KINDS = ('ic_launcher', 'ic_launcher_round', 'ic_launcher_foreground',
                 'ic_launcher_background', 'ic_launcher_monochrome')
IOS_ICONS = {
    'AppIcon-20x20@1x.png': 20, 'AppIcon-20x20@2x.png': 40,
    'AppIcon-20x20@2x-1.png': 40, 'AppIcon-20x20@3x.png': 60,
    'AppIcon-29x29@1x.png': 29, 'AppIcon-29x29@2x.png': 58,
    'AppIcon-29x29@2x-1.png': 58, 'AppIcon-29x29@3x.png': 87,
    'AppIcon-40x40@1x.png': 40, 'AppIcon-40x40@2x.png': 80,
    'AppIcon-40x40@2x-1.png': 80, 'AppIcon-40x40@3x.png': 120,
    'AppIcon-60x60@2x.png': 120, 'AppIcon-60x60@3x.png': 180,
    'AppIcon-76x76@1x.png': 76, 'AppIcon-76x76@2x.png': 152,
    'AppIcon-83.5x83.5@2x.png': 167, 'AppIcon-512@2x.png': 1024,
}


def pixel_identity(image):
    """Match aapt2 PNG crunching without treating invisible RGB as artwork.

    aapt2 clears RGB of fully transparent pixels and may change PNG palette,
    filters and compression. Compare decoded RGBA with only alpha-zero RGB
    normalized; visible and partially transparent pixels must remain exact.
    """
    rgba = bytearray(image.convert('RGBA').tobytes())
    for offset in range(0, len(rgba), 4):
        if rgba[offset + 3] == 0:
            rgba[offset:offset + 3] = b'\0\0\0'
    return image.width, image.height, hashlib.sha256(rgba).hexdigest()


def image_record(source, native, size, root):
    if not source.is_file() or not native.is_file():
        raise RuntimeError(f'Missing generated/native brand icon: {native.name}')
    with Image.open(source) as source_image, Image.open(native) as native_image:
        source_image.load()
        native_image.load()
        if source_image.size != (size, size) or native_image.size != (size, size):
            raise RuntimeError(f'Unexpected native brand dimensions: {native.name}')
        identity = pixel_identity(native_image)
        if pixel_identity(source_image) != identity:
            raise RuntimeError(f'Native icon still differs from generated MinerDesk brand: {native.name}')
    return {'file': native.relative_to(root).as_posix(),
            'generated_source': source.relative_to(root).as_posix(),
            'sha256': hashlib.sha256(native.read_bytes()).hexdigest(),
            'width': identity[0], 'height': identity[1], 'pixel_sha256': identity[2]}


def adaptive_references(path):
    node = ET.parse(path).getroot()
    if node.tag != 'adaptive-icon':
        raise RuntimeError('Android launcher is not the expected adaptive icon')
    references = {child.tag: child.attrib.get(ANDROID_NS + 'drawable') for child in node}
    expected = {kind: '@mipmap/ic_launcher_' + kind for kind in ('foreground', 'background', 'monochrome')}
    if references != expected or len(node) != len(expected):
        raise RuntimeError('Adaptive launcher does not reference the generated brand layers')
    return references


def verify_native_assets(platform, root=None, write_receipt=True):
    root = Path(root or ROOT).resolve()
    tauri = root / 'apps/mobile/src-tauri'
    records = []
    report = {'platform': platform, 'verified': True,
              'pixel_comparison': 'Decoded RGBA; RGB normalized to zero only when alpha is zero',
              'resources': records}
    if platform == 'android':
        project = tauri / 'gen/android'
        native = project / 'app/src/main/res'
        fallback = tauri / 'icons/android'
        manifest = ET.parse(project / 'app/src/main/AndroidManifest.xml').getroot()
        application = manifest.find('application')
        if application is None or application.attrib.get(ANDROID_NS + 'icon') != '@mipmap/ic_launcher':
            raise RuntimeError('Android application does not reference the verified launcher')
        round_icon = application.attrib.get(ANDROID_NS + 'roundIcon')
        if round_icon not in (None, '@mipmap/ic_launcher_round'):
            raise RuntimeError('Android application uses an unexpected round launcher')
        for density, (legacy_size, adaptive_size) in DENSITIES.items():
            for kind in ANDROID_KINDS:
                relative = Path('mipmap-' + density) / (kind + '.png')
                size = legacy_size if kind in ('ic_launcher', 'ic_launcher_round') else adaptive_size
                record = image_record(fallback / relative, native / relative, size, root)
                record['kind'] = kind
                record['required_in_apk'] = kind != 'ic_launcher_round' or round_icon is not None
                records.append(record)
        adaptive = Path('mipmap-anydpi-v26/ic_launcher.xml')
        source_refs = adaptive_references(fallback / adaptive)
        if adaptive_references(native / adaptive) != source_refs:
            raise RuntimeError('Native adaptive icon references differ from the generated brand')
        report['adaptive_icon'] = {'file': (native / adaptive).relative_to(root).as_posix(),
                                   'references': source_refs,
                                   'sha256': hashlib.sha256((native / adaptive).read_bytes()).hexdigest()}
        report['application_icon'] = '@mipmap/ic_launcher'
        report['round_icon_required'] = round_icon is not None
    elif platform == 'ios':
        project = tauri / 'gen/apple'
        native = project / 'Assets.xcassets/AppIcon.appiconset'
        fallback = tauri / 'icons/ios'
        catalog = native / 'Contents.json'
        contents = json.loads(catalog.read_text())
        filenames = [item.get('filename') for item in contents.get('images', [])]
        if len(filenames) != len(IOS_ICONS) or set(filenames) != set(IOS_ICONS):
            raise RuntimeError('Unexpected iOS native AppIcon catalog; inspect its icon references')
        for name, size in IOS_ICONS.items():
            records.append(image_record(fallback / name, native / name, size, root))
        report['catalog_sha256'] = hashlib.sha256(catalog.read_bytes()).hexdigest()
    else:
        raise RuntimeError('Unsupported native mobile platform')
    report['resource_count'] = len(records)
    if write_receipt:
        (project / 'NATIVE-BRAND.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    return report


def verify_apk_brand(apk, report):
    if report.get('platform') != 'android' or report.get('verified') is not True:
        raise RuntimeError('A verified Android native brand receipt is required')
    required = {(item['width'], item['height'], item['pixel_sha256'])
                for item in report['resources'] if item.get('required_in_apk')}
    matches = {}
    # Release resource shrinking may rename or deduplicate resources. Pixel
    # identities remain stable even when a PNG is palette/compression optimized.
    with zipfile.ZipFile(apk) as archive:
        for item in archive.infolist():
            if not item.filename.startswith('res/') or item.file_size > 8 * 1024 * 1024:
                continue
            with archive.open(item) as stream:
                prefix = stream.read(8)
                if prefix != b'\x89PNG\r\n\x1a\n':
                    continue
                data = prefix + stream.read()
            with Image.open(io.BytesIO(data)) as image:
                if image.width > 2048 or image.height > 2048:
                    continue
                identity = pixel_identity(image)
            if identity in required:
                matches.setdefault(identity, []).append(item.filename)
    if set(matches) != required:
        raise RuntimeError('APK native launcher pixels do not match the verified MinerDesk brand')
    return {'verified': True, 'required_bitmap_identities': len(required),
            'matched_apk_resources': sorted(name for values in matches.values() for name in values),
            'resource_names_may_be_obfuscated': True,
            'pixel_comparison': report['pixel_comparison']}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('platform', choices=['android', 'ios'])
    args = parser.parse_args()
    report = verify_native_assets(args.platform)
    print(f"Verified {report['resource_count']} actual {args.platform} native brand resources")


if __name__ == '__main__':
    main()
