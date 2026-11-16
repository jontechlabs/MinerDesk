"""Regression checks for default Tauri icons surviving native initialization."""
import importlib.util
import io
import json
from pathlib import Path
import shutil
import tempfile
import unittest
import zipfile

from PIL import Image, ImageDraw

SOURCE = Path(__file__).resolve().parents[1] / 'scripts/mobile-native-assets.py'
spec = importlib.util.spec_from_file_location('native_brand_production', SOURCE)
native_brand = importlib.util.module_from_spec(spec)
spec.loader.exec_module(native_brand)


def android_fixture(root):
    """Synthetic colored TEST DATA, deliberately not the production artwork."""
    tauri = root / 'apps/mobile/src-tauri'
    fallback = tauri / 'icons/android'
    native = tauri / 'gen/android/app/src/main/res'
    sizes = {'mdpi': (48, 108), 'hdpi': (72, 162), 'xhdpi': (96, 216),
             'xxhdpi': (144, 324), 'xxxhdpi': (192, 432)}
    kinds = ['ic_launcher', 'ic_launcher_round', 'ic_launcher_foreground',
             'ic_launcher_background', 'ic_launcher_monochrome']
    for density, (legacy, adaptive) in sizes.items():
        for index, kind in enumerate(kinds):
            size = legacy if index < 2 else adaptive
            image = Image.new('RGBA', (size, size), (18 + index * 31, 100, 180, 255))
            ImageDraw.Draw(image).rectangle((size // 3, size // 3, size // 2, size // 2), fill=(255, 40, 80, 255))
            directory = fallback / ('mipmap-' + density)
            directory.mkdir(parents=True, exist_ok=True)
            image.save(directory / (kind + '.png'))
    xml = fallback / 'mipmap-anydpi-v26/ic_launcher.xml'
    xml.parent.mkdir(parents=True)
    xml.write_text('<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">'
                   '<foreground android:drawable="@mipmap/ic_launcher_foreground"/>'
                   '<background android:drawable="@mipmap/ic_launcher_background"/>'
                   '<monochrome android:drawable="@mipmap/ic_launcher_monochrome"/></adaptive-icon>')
    shutil.copytree(fallback, native)
    manifest = native.parent / 'AndroidManifest.xml'
    manifest.write_text('<manifest xmlns:android="http://schemas.android.com/apk/res/android">'
                        '<application android:icon="@mipmap/ic_launcher"/></manifest>')
    return fallback, native


def ios_fixture(root):
    tauri = root / 'apps/mobile/src-tauri'
    fallback = tauri / 'icons/ios'
    native = tauri / 'gen/apple/Assets.xcassets/AppIcon.appiconset'
    fallback.mkdir(parents=True)
    for name, size in native_brand.IOS_ICONS.items():
        Image.new('RGB', (size, size), '#234567').save(fallback / name)
    shutil.copytree(fallback, native)
    (native / 'Contents.json').write_text(json.dumps({'images': [
        {'filename': name, 'idiom': 'fixture', 'scale': '1x'} for name in native_brand.IOS_ICONS],
        'info': {'version': 1, 'author': 'TEST DATA'}}))
    return fallback, native


def append_apk_brand(archive, native, omit=None, replace=False, optimize_alpha=False):
    for index, source in enumerate(sorted(native.rglob('*.png'))):
        if omit and omit in source.name:
            continue
        with Image.open(source) as image:
            if replace:
                image = Image.new('RGBA', image.size, '#fff000')
            if optimize_alpha:
                image = image.convert('RGB')
            data = io.BytesIO()
            image.save(data, format='PNG', optimize=True)
        archive.writestr(f'res/obfuscated-{index}.png', data.getvalue())


class NativeBrand(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='minerdesk-native-brand-test-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()

    def test_android_verifies_actual_native_mipmaps_and_adaptive_references(self):
        _, native = android_fixture(self.root)
        report = native_brand.verify_native_assets('android', self.root)
        self.assertEqual(report['resource_count'], 25)
        self.assertEqual(sum(item['required_in_apk'] for item in report['resources']), 20)
        self.assertEqual(report['application_icon'], '@mipmap/ic_launcher')
        self.assertEqual(json.loads((native.parents[3] / 'NATIVE-BRAND.json').read_text()), report)

    def test_native_default_placeholder_is_rejected_even_when_fallback_has_correct_names(self):
        _, native = android_fixture(self.root)
        Image.new('RGBA', (192, 192), '#ff0000').save(native / 'mipmap-xxxhdpi/ic_launcher.png')
        with self.assertRaisesRegex(RuntimeError, 'still differs'):
            native_brand.verify_native_assets('android', self.root)
        self.assertFalse((native.parents[3] / 'NATIVE-BRAND.json').exists())

    def test_missing_native_layer_does_not_pass_on_the_fallback_alone(self):
        _, native = android_fixture(self.root)
        (native / 'mipmap-mdpi/ic_launcher_monochrome.png').unlink()
        with self.assertRaisesRegex(RuntimeError, 'Missing generated/native'):
            native_brand.verify_native_assets('android', self.root)

    def test_adaptive_xml_cannot_keep_the_template_drawable(self):
        _, native = android_fixture(self.root)
        path = native / 'mipmap-anydpi-v26/ic_launcher.xml'
        path.write_text(path.read_text().replace('@mipmap/ic_launcher_foreground', '@drawable/ic_launcher_foreground'))
        with self.assertRaisesRegex(RuntimeError, 'brand layers'):
            native_brand.verify_native_assets('android', self.root)

    def test_android_manifest_must_use_the_verified_launcher(self):
        _, native = android_fixture(self.root)
        path = native.parent / 'AndroidManifest.xml'
        path.write_text(path.read_text().replace('@mipmap/ic_launcher', '@mipmap/template'))
        with self.assertRaisesRegex(RuntimeError, 'does not reference'):
            native_brand.verify_native_assets('android', self.root)

    def test_ios_checks_the_real_catalog_and_preserves_its_contents(self):
        _, native = ios_fixture(self.root)
        contents = (native / 'Contents.json').read_bytes()
        report = native_brand.verify_native_assets('ios', self.root)
        self.assertEqual(report['resource_count'], 18)
        self.assertEqual((native / 'Contents.json').read_bytes(), contents)

    def test_ios_template_icon_is_rejected(self):
        _, native = ios_fixture(self.root)
        Image.new('RGB', (1024, 1024), '#ff0000').save(native / 'AppIcon-512@2x.png')
        with self.assertRaisesRegex(RuntimeError, 'still differs'):
            native_brand.verify_native_assets('ios', self.root)

    def test_ios_catalog_cannot_redirect_to_other_artwork(self):
        _, native = ios_fixture(self.root)
        path = native / 'Contents.json'
        path.write_text(path.read_text().replace('AppIcon-512@2x.png', 'template.png'))
        with self.assertRaisesRegex(RuntimeError, 'Unexpected iOS native'):
            native_brand.verify_native_assets('ios', self.root)

    def test_apk_png_crunching_and_renaming_does_not_hide_correct_artwork(self):
        _, native = android_fixture(self.root)
        report = native_brand.verify_native_assets('android', self.root)
        apk = self.root / 'fixture.apk'
        with zipfile.ZipFile(apk, 'w') as archive:
            append_apk_brand(archive, native, omit='ic_launcher_round', optimize_alpha=True)
        self.assertTrue(native_brand.verify_apk_brand(apk, report)['verified'])

    def test_actual_apk_default_artwork_is_rejected_even_if_native_source_was_correct(self):
        _, native = android_fixture(self.root)
        report = native_brand.verify_native_assets('android', self.root)
        apk = self.root / 'fixture.apk'
        with zipfile.ZipFile(apk, 'w') as archive:
            append_apk_brand(archive, native, replace=True)
        with self.assertRaisesRegex(RuntimeError, 'APK native launcher pixels'):
            native_brand.verify_apk_brand(apk, report)

    def test_missing_required_apk_monochrome_layer_is_rejected(self):
        _, native = android_fixture(self.root)
        report = native_brand.verify_native_assets('android', self.root)
        apk = self.root / 'fixture.apk'
        with zipfile.ZipFile(apk, 'w') as archive:
            append_apk_brand(archive, native, omit='ic_launcher_monochrome')
        with self.assertRaisesRegex(RuntimeError, 'APK native launcher pixels'):
            native_brand.verify_apk_brand(apk, report)

    def test_pixel_normalization_changes_only_invisible_rgb(self):
        a = Image.new('RGBA', (2, 1), (255, 0, 100, 0))
        b = Image.new('RGBA', (2, 1), (0, 0, 0, 0))
        self.assertEqual(native_brand.pixel_identity(a), native_brand.pixel_identity(b))
        a.putpixel((0, 0), (255, 0, 100, 1))
        self.assertNotEqual(native_brand.pixel_identity(a), native_brand.pixel_identity(b))


if __name__ == '__main__':
    unittest.main()
