#!/usr/bin/env python3
"""Reproduce the approved website wordmark; requires fonttools[woff] and Pillow.

The website uses Space Grotesk 500/24 for the g inside a 32px purple square,
Space Grotesk 700/30 and -1.5px tracking for glimdock., with a 10px gap. Its
1.55 line height and 4px bottom padding on the mark determine the baselines.
No network access or external assets are required.
"""
from pathlib import Path
import math
import tempfile
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from PIL import Image, ImageDraw, ImageFont

HERE = Path(__file__).resolve().parent
SOURCE = HERE / 'Space-Grotesk.woff2'
FIRMWARE = HERE.parent / 'firmware' / 'src'
SCALE = 0.55
SUPER = 20
PURPLE = '#6644dd'


def font(weight):
    result = instantiateVariableFont(TTFont(SOURCE), {'wght': weight})
    result.flavor = None
    return result


def kern(f, left, right):
    # CSS enables the font's default horizontal kern feature.
    gpos = f.get('GPOS')
    if gpos is None:
        return 0
    indices = []
    for feature in gpos.table.FeatureList.FeatureRecord:
        if feature.FeatureTag == 'kern':
            indices.extend(feature.Feature.LookupListIndex)
    value = 0
    for index in sorted(set(indices)):
        lookup = gpos.table.LookupList.Lookup[index]
        for sub in lookup.SubTable:
            if lookup.LookupType == 9:
                sub = sub.ExtSubTable
            if not hasattr(sub, 'Coverage') or left not in sub.Coverage.glyphs:
                continue
            record = None
            if sub.Format == 1:
                pairs = sub.PairSet[sub.Coverage.glyphs.index(left)].PairValueRecord
                record = next((p for p in pairs if p.SecondGlyph == right), None)
            elif sub.Format == 2:
                a = sub.ClassDef1.classDefs.get(left, 0)
                b = sub.ClassDef2.classDefs.get(right, 0)
                record = sub.Class1Record[a].Class2Record[b]
            if record and record.Value1:
                value += getattr(record.Value1, 'XAdvance', 0)
    return value


def outline(f, char, size, x, baseline):
    gs = f.getGlyphSet()
    pen = SVGPathPen(gs)
    scale = size / f['head'].unitsPerEm
    gs[f.getBestCmap()[ord(char)]].draw(TransformPen(pen, (scale, 0, 0, -scale, x, baseline)))
    return pen.getCommands()


def positions(f, text, size, tracking):
    cmap = f.getBestCmap()
    result = []
    x = 0.0
    for i, char in enumerate(text):
        glyph = cmap[ord(char)]
        if i:
            x += kern(f, cmap[ord(text[i - 1])], glyph) * size / f['head'].unitsPerEm
        result.append(x)
        x += f['hmtx'][glyph][0] * size / f['head'].unitsPerEm + tracking
    return result, x


def baseline(f, size, line_height):
    units = f['head'].unitsPerEm
    asc = f['OS/2'].sTypoAscender * size / units
    desc = -f['OS/2'].sTypoDescender * size / units
    return asc + (line_height - asc - desc) / 2


def write_mask(name, image, lines):
    # Crop static A8 data; offsets preserve the exact lockup placement.
    bounds = image.getbbox()
    if bounds is None:
        raise ValueError('empty logo glyph')
    data = image.crop(bounds)
    raw = data.tobytes()
    lines += [f'inline constexpr int {name}_X = {bounds[0]}, {name}_Y = {bounds[1]};',
              f'inline constexpr uint8_t {name}_DATA[] = {{']
    lines += ['  ' + ','.join(str(v) for v in raw[i:i + 24]) + ',' for i in range(0, len(raw), 24)]
    lines += ['};', f'inline constexpr lv_image_dsc_t {name} = {{',
              f'  {{LV_IMAGE_HEADER_MAGIC, LV_COLOR_FORMAT_A8, 0, {data.width}, {data.height}, {data.width}, 0}},',
              f'  sizeof({name}_DATA), {name}_DATA, nullptr, nullptr', '};', '']


def main():
    regular, bold = font(500), font(700)
    text = 'glimdock.'
    xs, width = positions(bold, text, 30, -1.5)
    line_height = 30 * 1.55
    word_baseline = baseline(bold, 30, line_height)
    g_width = regular['hmtx'][regular.getBestCmap()[ord('g')]][0] * 24 / regular['head'].unitsPerEm
    # The mark's CSS letter-spacing is -1px, and padding-bottom is 4px.
    g_x = (32 - (g_width - 1)) / 2
    g_baseline = (28 - 24 * 1.55) / 2 + baseline(regular, 24, 24 * 1.55)
    g_path = outline(regular, 'g', 24, g_x, g_baseline)
    mark = f'<rect width="32" height="32" rx="9" fill="{PURPLE}"/><path d="{g_path}" fill="#fff"/>'
    (HERE / 'glimdock-mark.svg').write_text('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" role="img" aria-labelledby="title"><title id="title">Glimdock mark</title>' + mark + '</svg>\n')
    portal = None
    for suffix, ink in (('', '#171323'), ('-dark', '#eaf2fb')):
        svg = f'<svg xmlns="http://www.w3.org/2000/svg" width="{42 + width:.2f}" height="{line_height:.2f}" viewBox="0 0 {42 + width:.2f} {line_height:.2f}" role="img" aria-labelledby="glimdock-title"><title id="glimdock-title">Glimdock</title><g transform="translate(0 {(line_height - 32) / 2:.3f})">{mark}</g>'
        for i, char in enumerate(text):
            svg += f'<path d="{outline(bold, char, 30, 42 + xs[i], word_baseline)}" fill="{PURPLE if char == "." else ink}"/>'
        svg += '</svg>'
        (HERE / f'glimdock-logotype{suffix}.svg').write_text(svg + '\n')
        if suffix:
            portal = svg
    (FIRMWARE / 'brand_portal.h').write_text('#pragma once\n// Generated by branding/generate.py from the approved website letterforms.\nnamespace glimdock {\ninline constexpr char PORTAL_LOGOTYPE[] = R"glim(' + portal + ')glim";\n}\n')

    mark_size = round(32 * SCALE)
    word_w = math.ceil(width * SCALE)
    word_h = math.ceil(line_height * SCALE)
    lines = ['#pragma once', '#include <lvgl.h>', '// Generated by branding/generate.py; Space Grotesk glyphs, SIL OFL 1.1.',
             '// Alpha masks are static flash data. The telemetry UI keeps its existing fonts.', 'namespace glimdock {',
             f'inline constexpr int MARK_SIZE = {mark_size};', f'inline constexpr int WORDMARK_W = {word_w}, WORDMARK_H = {word_h};', '']
    with tempfile.TemporaryDirectory() as temp:
        mark_ttf = Path(temp) / 'mark.ttf'; word_ttf = Path(temp) / 'word.ttf'
        regular.save(mark_ttf); bold.save(word_ttf)
        mark_font = ImageFont.truetype(str(mark_ttf), round(24 * SCALE * SUPER))
        word_font = ImageFont.truetype(str(word_ttf), round(30 * SCALE * SUPER))
        g = Image.new('L', (mark_size * SUPER, mark_size * SUPER))
        ImageDraw.Draw(g).text((g_x * SCALE * SUPER, g_baseline * SCALE * SUPER), 'g', font=mark_font, fill=255, anchor='ls')
        write_mask('MARK_G', g.resize((mark_size, mark_size), Image.Resampling.LANCZOS), lines)
        for name, include in [('WORDMARK_INK', text[:-1]), ('WORDMARK_DOT', '.')]:
            mask = Image.new('L', (word_w * SUPER, word_h * SUPER))
            draw = ImageDraw.Draw(mask)
            for i, char in enumerate(text):
                if char in include:
                    draw.text((xs[i] * SCALE * SUPER, word_baseline * SCALE * SUPER), char, font=word_font, fill=255, anchor='ls')
            write_mask(name, mask.resize((word_w, word_h), Image.Resampling.LANCZOS), lines)
    lines.append('}')
    (FIRMWARE / 'brand_assets.h').write_text('\n'.join(lines) + '\n')
    print(f'Website lockup: {42 + width:.2f} × {line_height:.2f}; device wordmark {word_w} × {word_h}, mark {mark_size} × {mark_size}')


if __name__ == '__main__':
    main()
