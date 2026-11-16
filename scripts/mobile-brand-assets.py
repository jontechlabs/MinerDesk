#!/usr/bin/env python3
"""Render MinerDesk Mobile's editable vector identity with Pillow.

Usage: python3 scripts/mobile-brand-assets.py [--preview PATH]
The native icon pipeline consumes the checked-in icon-manifest.json and PNGs;
Pillow is needed only when regenerating this source asset family.
"""
import argparse
import json
from pathlib import Path
import shutil

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'docs/brand/mobile'
DARK = '#0c1423'
MINT = '#70f6a0'
CYAN = '#48d5f7'
INK = '#09131f'
WHITE = '#edf4ff'

# One continuous M trace, three circular cut-outs suggesting connected rigs.
# This geometry is shared by SVG, raster masks, the Android themed icon and UI.
COMMANDS = [
    ('M', 302, 692), ('L', 302, 360), ('Q', 302, 320, 339, 320),
    ('Q', 354, 320, 365, 333), ('L', 512, 485), ('L', 659, 333),
    ('Q', 670, 320, 685, 320), ('Q', 722, 320, 722, 360),
    ('L', 722, 692), ('L', 638, 692), ('L', 638, 466),
    ('L', 542, 564), ('Q', 512, 594, 482, 564),
    ('L', 386, 466), ('L', 386, 692), ('Z',),
]
NODES = [(344, 650, 11), (512, 537, 11), (680, 650, 11)]
PATH = ' '.join(command[0] + ' '.join(str(n) for n in command[1:]) for command in COMMANDS)
HOLES = ' '.join(f'M{x-r} {y}a{r} {r} 0 1 0 {r*2} 0a{r} {r} 0 1 0 {-r*2} 0Z' for x, y, r in NODES)
SHAPE = PATH + ' ' + HOLES


def rgb(hex_value):
    return tuple(int(hex_value[i:i+2], 16) for i in (1, 3, 5))


def mark_mask(size):
    # Supersampling applies only to geometry; no font-dependent drawing is used.
    aa = 4
    scale = size * aa / 1024
    mask = Image.new('L', (size * aa, size * aa))
    points, current = [], None
    for command in COMMANDS:
        kind, *values = command
        if kind in ('M', 'L'):
            current = tuple(values)
            points.append(current)
        elif kind == 'Q':
            control, end = tuple(values[:2]), tuple(values[2:])
            start = current
            for step in range(1, 41):
                t = step / 40
                points.append(((1-t)**2*start[0] + 2*(1-t)*t*control[0] + t*t*end[0],
                               (1-t)**2*start[1] + 2*(1-t)*t*control[1] + t*t*end[1]))
            current = end
    draw = ImageDraw.Draw(mask)
    draw.polygon([(round(x*scale), round(y*scale)) for x, y in points], fill=255)
    for x, y, r in NODES:
        draw.ellipse(tuple(round(v*scale) for v in (x-r, y-r, x+r, y+r)), fill=0)
    return mask.resize((size, size), Image.Resampling.LANCZOS)


def gradient(size, start, end, left=0, right=1):
    # Match SVG object-bounding-box gradients without altering their geometry.
    a, b = rgb(start), rgb(end)
    horizontal = Image.new('RGB', (size, 1))
    horizontal.putdata([tuple(round(a[c]*(1-t) + b[c]*t) for c in range(3))
                        for t in [max(0, min(1, (x / max(size-1, 1) - left) / (right-left))) for x in range(size)]])
    return horizontal.resize((size, size))


def mark(size, mono=False):
    result = Image.new('RGB', (size, size), 'white') if mono else gradient(size, MINT, CYAN, 302/1024, 722/1024)
    result.putalpha(mark_mask(size))
    return result


def launcher(size):
    result = gradient(size, '#111f32', DARK).convert('RGBA')
    result.alpha_composite(mark(size))
    return result.convert('RGB')


def defs():
    return ('<defs><linearGradient id="brand" x1="0" y1="0" x2="1" y2="0">'
            f'<stop stop-color="{MINT}"/><stop offset="1" stop-color="{CYAN}"/>'
            '</linearGradient></defs>')


def svg(filename, body, viewbox='0 0 1024 1024', label='MinerDesk Mobile'):
    (OUT / filename).write_text(
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{viewbox}" role="img" aria-label="{label}">'
        f'<title>{label}</title>{body}</svg>\n', encoding='utf-8')


def font(size, bold=False):
    candidates = ([Path('C:/Windows/Fonts/segoeuib.ttf')] if bold else [Path('C:/Windows/Fonts/segoeui.ttf')])
    candidates += [Path('/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf' if bold else '/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf')]
    for candidate in candidates:
        if candidate.is_file():
            return ImageFont.truetype(str(candidate), size)
    return ImageFont.load_default(size=size)


def wordmark(dark_background):
    foreground = WHITE if dark_background else '#132238'
    background = DARK if dark_background else '#f6f9fc'
    return (defs() + f'<rect width="840" height="256" rx="28" fill="{background}"/>'
            f'<g transform="translate(0 -1) scale(.25)"><path fill="url(#brand)" fill-rule="evenodd" d="{SHAPE}"/></g>'
            f'<text x="266" y="132" fill="{foreground}" font-family="Inter, Segoe UI, sans-serif" '
            'font-size="82" font-weight="700" letter-spacing="-3">MinerDesk</text>'
            f'<text x="271" y="181" fill="{("#92a4bf" if dark_background else "#53677f")}" '
            'font-family="Inter, Segoe UI, sans-serif" font-size="27" font-weight="500" '
            'letter-spacing="7">MOBILE</text>')


def wordmark_png(dark_background):
    foreground = WHITE if dark_background else '#132238'
    background = DARK if dark_background else '#f6f9fc'
    result = Image.new('RGBA', (840, 256))
    draw = ImageDraw.Draw(result)
    draw.rounded_rectangle((0, 0, 839, 255), radius=28, fill=background)
    result.alpha_composite(mark(256), (0, -1))
    draw.text((266, 132), 'MinerDesk', font=font(82, True), fill=foreground, anchor='ls', stroke_width=0)
    x, subtitle = 271, font(27)
    for letter in 'MOBILE':
        draw.text((x, 181), letter, font=subtitle, fill=('#92a4bf' if dark_background else '#53677f'), anchor='ls')
        x += draw.textlength(letter, font=subtitle) + 7
    return result


def brand_sheet(path):
    canvas = Image.new('RGB', (1800, 1420), '#080e18')
    d = ImageDraw.Draw(canvas)
    d.text((88, 58), 'MinerDesk Mobile', font=font(68, True), fill=WHITE)
    d.text((91, 145), 'A connected identity. Built for your rigs, wherever you are.', font=font(27), fill='#91a5c0')
    # Primary launcher master and the platform masks are previews only.
    d.rounded_rectangle((88, 225, 750, 895), radius=36, fill='#101b2c', outline='#243650', width=2)
    icon = launcher(540)
    shape = Image.new('L', icon.size)
    ImageDraw.Draw(shape).rounded_rectangle((0, 0, 539, 539), radius=122, fill=255)
    canvas.paste(icon, (150, 263), shape)
    d.text((140, 824), 'PRIMARY ICON', font=font(23, True), fill=WHITE)
    d.text((140, 858), 'Connected M / three rig nodes', font=font(20), fill='#91a5c0')
    d.text((835, 238), 'One mark. Every platform.', font=font(34, True), fill=WHITE)
    for offset, label, mask_type in [(0, 'Android adaptive', 'circle'), (280, 'iOS app icon', 'squircle')]:
        small = launcher(220)
        mask = Image.new('L', (220, 220))
        md = ImageDraw.Draw(mask)
        if mask_type == 'circle':
            md.ellipse((0, 0, 219, 219), fill=255)
        else:
            md.rounded_rectangle((0, 0, 219, 219), radius=52, fill=255)
        canvas.paste(small, (840+offset, 320), mask)
        d.text((841+offset, 565), label, font=font(22), fill='#b6c6da')
    d.rounded_rectangle((840, 638, 1620, 790), radius=22, fill='#eaf5ed')
    themed = mark(144, True)
    themed.paste(rgb('#236e52'), (0, 0, 144, 144), themed.getchannel('A'))
    themed.putalpha(mark_mask(144))
    canvas.paste(themed, (840, 642), themed)
    d.text((994, 676), 'Themed. Recognizable. Clear.', font=font(27, True), fill='#163a2c')
    d.text((994, 722), 'Android monochrome / no extra badge', font=font(20), fill='#426c58')
    d.text((839, 842), '16', font=font(16), fill='#91a5c0')
    for size, x in [(16, 891), (24, 948), (32, 1017), (48, 1110), (64, 1220), (96, 1360)]:
        canvas.paste(launcher(size), (x, 820 + (96-size)//2))
    d.text((88, 977), 'MINERDESK', font=font(20, True), fill='#91a5c0')
    d.text((88, 1015), 'MinerDesk', font=font(72, True), fill=WHITE)
    d.text((93, 1103), 'M O B I L E', font=font(26), fill=CYAN)
    swatches = [(MINT, 'Mint', '#70F6A0'), (CYAN, 'Cyan', '#48D5F7'), (DARK, 'Midnight', '#0C1423')]
    for i, (color, name, value) in enumerate(swatches):
        x = 835 + i*260
        d.rounded_rectangle((x, 999, x+220, 1094), radius=18, fill=color, outline='#2a3b52', width=1)
        d.text((x, 1111), name, font=font(23, True), fill=WHITE)
        d.text((x, 1147), value, font=font(19), fill='#91a5c0')
    d.line((88, 1234, 1712, 1234), fill='#26374e', width=2)
    d.text((88, 1282), '1.0   /   VECTOR ORIGINALS + NATIVE ICONS', font=font(23, True), fill='#b4c6dc')
    d.text((88, 1324), 'Square, opaque source. Platform masks shown here are previews applied by the operating system.', font=font(20), fill='#7d93b1')
    path.parent.mkdir(parents=True, exist_ok=True)
    canvas.save(path)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--preview', type=Path)
    args = parser.parse_args()
    OUT.mkdir(parents=True, exist_ok=True)
    svg('logomark.svg', defs() + f'<path fill="url(#brand)" fill-rule="evenodd" d="{SHAPE}"/>')
    svg('launcher-source.svg', defs() + f'<rect width="1024" height="1024" fill="{DARK}"/>'
        + f'<path fill="url(#brand)" fill-rule="evenodd" d="{SHAPE}"/>')
    svg('monochrome.svg', f'<path fill="#fff" fill-rule="evenodd" d="{SHAPE}"/>')
    svg('wordmark-dark.svg', wordmark(True), '0 0 840 256')
    svg('wordmark-light.svg', wordmark(False), '0 0 840 256')
    wordmark_png(True).save(OUT / 'wordmark-dark.png')
    wordmark_png(False).save(OUT / 'wordmark-light.png')
    svg('favicon.svg', defs() + f'<rect width="1024" height="1024" rx="235" fill="{DARK}"/>'
        + f'<path fill="url(#brand)" fill-rule="evenodd" d="{SHAPE}"/>')
    launcher(1024).save(OUT / 'launcher-source.png')
    shutil.copyfile(OUT / 'launcher-source.png', OUT / 'ios-source.png')
    mark(432).save(OUT / 'adaptive-foreground.png')
    mark(432, True).save(OUT / 'monochrome.png')
    Image.new('RGB', (1024, 1024), DARK).save(OUT / 'adaptive-background.png')
    for size in (32, 64, 128, 256, 512):
        launcher(size).save(OUT / f'icon-{size}.png')
    manifest = {'default': 'launcher-source.png', 'bg_color': DARK,
                'android_bg': 'adaptive-background.png', 'android_fg': 'adaptive-foreground.png',
                'android_fg_scale': 100, 'android_monochrome': 'monochrome.png'}
    (OUT / 'icon-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    (OUT / 'geometry.json').write_text(json.dumps({'view_box': '0 0 1024 1024', 'path': SHAPE,
        'gradient': {'start': MINT, 'end': CYAN}, 'background': DARK,
        'mark_bounds': [302, 320, 722, 692], 'adaptive_foreground_scale': 100}, indent=2) + '\n')
    if args.preview:
        brand_sheet(args.preview)
    print(f'MinerDesk Mobile native brand assets written to {OUT}')


if __name__ == '__main__':
    main()
