"""Input-indicator icons for the TIP DLL, per Microsoft's IME icon guidelines
(learn.microsoft.com/windows/apps/develop/input/input-method-editor-requirements#ime-icons):
black-and-white only, sizes 16/20/24/32/40/48 with alpha;
- brand icon: a black glyph in a white box with a 1 px outer stroke in black at 50 % opacity;
- mode icon: a white glyph with a 1 px outer stroke in black at 50 % opacity.
Glyphs come from the project's own OFL fonts (Manrope, Kufam), never from Windows' fonts.

Run from the repo root:  python crates/t3a-tip/res/make_icons.py
"""
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parents[3]
FONTS = ROOT / "apps" / "settings" / "ui" / "fonts"
OUT = Path(__file__).resolve().parent
SIZES = [16, 20, 24, 32, 40, 48]
SS = 8  # supersampling


def glyph_mask(text: str, font_file: str, size: int, box: float, dy: float = 0.0) -> Image.Image:
    """Alpha mask of `text`, centred, fitted into `box` (fraction of the icon)."""
    big = size * SS
    font_px = big
    font = ImageFont.truetype(str(FONTS / font_file), font_px)
    l, t, r, b = font.getbbox(text)
    scale = min(box * big / (r - l), box * big / (b - t))
    font = ImageFont.truetype(str(FONTS / font_file), max(1, int(font_px * scale)))
    l, t, r, b = font.getbbox(text)
    m = Image.new("L", (big, big), 0)
    ImageDraw.Draw(m).text(((big - (r - l)) / 2 - l, (big - (b - t)) / 2 - t + dy * big), text, font=font, fill=255)
    return m.resize((size, size), Image.LANCZOS)


def stroke(alpha: Image.Image) -> Image.Image:
    """1 px outer stroke around `alpha`, at 50 % opacity."""
    grown = alpha.filter(ImageFilter.MaxFilter(3))
    return grown.point(lambda v: v // 2)


def mode_icon(text: str, font_file: str, size: int, box: float) -> Image.Image:
    g = glyph_mask(text, font_file, size, box)
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    img.putalpha(stroke(g))  # black outline
    white = Image.new("RGBA", (size, size), (255, 255, 255, 255))
    white.putalpha(g)
    return Image.alpha_composite(img, white)


def brand_icon(size: int) -> Image.Image:
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    big = size * SS
    box = Image.new("L", (big, big), 0)
    inset, radius = SS * 1, int(big * 0.18)
    ImageDraw.Draw(box).rounded_rectangle([inset, inset, big - inset - 1, big - inset - 1], radius=radius, fill=255)
    box = box.resize((size, size), Image.LANCZOS)
    img.putalpha(stroke(box))
    white = Image.new("RGBA", (size, size), (255, 255, 255, 255))
    white.putalpha(box)
    img = Image.alpha_composite(img, white)
    g = glyph_mask("t3", "manrope-latin-800-normal.woff2", size, 0.62)
    black = Image.new("RGBA", (size, size), (0, 0, 0, 255))
    black.putalpha(g)
    return Image.alpha_composite(img, black)


def save(frames, name):
    frames = sorted(frames, key=lambda f: -f.size[0])  # Pillow keeps only frames up to the first one's size
    frames[0].save(OUT / name, sizes=[f.size for f in frames], append_images=frames[1:])


save([brand_icon(s) for s in SIZES], "brand.ico")
save([mode_icon("ع", "kufam-arabic-700-normal.woff2", s, 0.86) for s in SIZES], "mode-ar.ico")
save([mode_icon("A", "manrope-latin-800-normal.woff2", s, 0.78) for s in SIZES], "mode-latin.ico")
print("wrote", ", ".join(p.name for p in sorted(OUT.glob("*.ico"))))
