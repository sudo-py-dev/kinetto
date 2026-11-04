"""Draws Kinetto's newer sample illustrations as both an editable SVG and a
1600 x 1200 PNG from one list of shapes, so the two always match.

    python3 assets/samples/draw.py

Needs Pillow. Each PNG is drawn at twice the size and scaled down, which
smooths the edges.
"""
import math
import random
from pathlib import Path

from PIL import Image, ImageDraw

W, H = 1600, 1200
SS = 2  # supersampling factor for the PNG
HERE = Path(__file__).parent


def rgb(hex_colour):
    h = hex_colour.lstrip('#')
    return tuple(int(h[i:i + 2], 16) for i in (0, 2, 4))


class Scene:
    """Collects shapes, then writes them as SVG and PNG."""

    def __init__(self):
        self.svg = []
        self.png = Image.new('RGBA', (W * SS, H * SS), (255, 255, 255, 255))
        self.gradients = 0

    # --- shapes -----------------------------------------------------------
    def gradient(self, top, bottom, box=(0, 0, W, H)):
        """A vertical gradient filling `box`."""
        x, y, w, h = box
        self.gradients += 1
        gid = f'g{self.gradients}'
        self.svg.append(
            f'<defs><linearGradient id="{gid}" x2="0" y2="1"><stop stop-color="{top}"/>'
            f'<stop offset="1" stop-color="{bottom}"/></linearGradient></defs>'
            f'<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="url(#{gid})"/>')
        a, b = rgb(top), rgb(bottom)
        draw = ImageDraw.Draw(self.png)
        for row in range(int(h * SS)):
            t = row / max(1, h * SS - 1)
            c = tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3))
            draw.line([(x * SS, y * SS + row), ((x + w) * SS, y * SS + row)], fill=c)

    def _layer(self, opacity, paint):
        layer = Image.new('RGBA', self.png.size, (0, 0, 0, 0))
        paint(ImageDraw.Draw(layer))
        if opacity < 1:
            alpha = layer.getchannel('A').point(lambda v: round(v * opacity))
            layer.putalpha(alpha)
        self.png.alpha_composite(layer)

    @staticmethod
    def _op(opacity):
        return f' opacity="{opacity}"' if opacity < 1 else ''

    def rect(self, x, y, w, h, fill, r=0, opacity=1.0):
        self.svg.append(f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{r}" fill="{fill}"{self._op(opacity)}/>')
        self._layer(opacity, lambda d: d.rounded_rectangle(
            [x * SS, y * SS, (x + w) * SS, (y + h) * SS], radius=r * SS, fill=rgb(fill)))

    def ellipse(self, cx, cy, rx, ry, fill, opacity=1.0):
        self.svg.append(f'<ellipse cx="{cx}" cy="{cy}" rx="{rx}" ry="{ry}" fill="{fill}"{self._op(opacity)}/>')
        self._layer(opacity, lambda d: d.ellipse(
            [(cx - rx) * SS, (cy - ry) * SS, (cx + rx) * SS, (cy + ry) * SS], fill=rgb(fill)))

    def circle(self, cx, cy, r, fill, opacity=1.0):
        self.ellipse(cx, cy, r, r, fill, opacity)

    def poly(self, points, fill, opacity=1.0):
        pts = ' '.join(f'{x:.1f},{y:.1f}' for x, y in points)
        self.svg.append(f'<polygon points="{pts}" fill="{fill}"{self._op(opacity)}/>')
        self._layer(opacity, lambda d: d.polygon([(x * SS, y * SS) for x, y in points], fill=rgb(fill)))

    def line(self, points, stroke, width, opacity=1.0):
        pts = ' '.join(f'{x:.1f},{y:.1f}' for x, y in points)
        self.svg.append(f'<polyline points="{pts}" fill="none" stroke="{stroke}" stroke-width="{width}" '
                        f'stroke-linecap="round" stroke-linejoin="round"{self._op(opacity)}/>')

        def paint(d):
            scaled = [(x * SS, y * SS) for x, y in points]
            d.line(scaled, fill=rgb(stroke), width=round(width * SS), joint='curve')
            for x, y in (scaled[0], scaled[-1]):
                rr = width * SS / 2
                d.ellipse([x - rr, y - rr, x + rr, y + rr], fill=rgb(stroke))
        self._layer(opacity, paint)

    # --- output -----------------------------------------------------------
    def save(self, name):
        svg = (f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" '
               f'viewBox="0 0 {W} {H}">' + ''.join(self.svg) + '</svg>\n')
        (HERE / f'{name}.svg').write_text(svg)
        self.png.resize((W, H), Image.LANCZOS).convert('RGB').save(HERE / f'{name}.png', optimize=True)
        print('drew', name)


def curve(a, b, bend, steps=24):
    """Points along a gentle arc from a to b, bowed sideways by `bend`."""
    (x0, y0), (x1, y1) = a, b
    nx, ny = -(y1 - y0), x1 - x0
    length = math.hypot(nx, ny) or 1
    nx, ny = nx / length, ny / length
    return [(x0 + (x1 - x0) * t + nx * bend * math.sin(math.pi * t),
             y0 + (y1 - y0) * t + ny * bend * math.sin(math.pi * t)) for t in (i / steps for i in range(steps + 1))]


def wave(x0, x1, y, amplitude, period, steps=80):
    return [(x0 + (x1 - x0) * i / steps,
             y + amplitude * math.sin((x0 + (x1 - x0) * i / steps) / period * 2 * math.pi)) for i in range(steps + 1)]


def flame(s, cx, base):
    s.circle(cx, base - 30, 34, '#ffd166', 0.28)
    s.poly([(cx, base - 64), (cx + 14, base - 30), (cx + 10, base - 12), (cx, base - 6), (cx - 10, base - 12), (cx - 14, base - 30)], '#ffb703')
    s.poly([(cx, base - 44), (cx + 6, base - 24), (cx, base - 12), (cx - 6, base - 24)], '#fff3b0')


# ---------------------------------------------------------------------------
def birthday_cake():
    s = Scene()
    rnd = random.Random(7)
    s.gradient('#ffe1e8', '#ffefd6')
    # Bunting across the top.
    string = curve((-20, 120), (1620, 120), -70)
    s.line(string, '#c98aa0', 4)
    colours = ['#ff6f91', '#ffc75f', '#4fc1e9', '#a78bfa', '#62d2a2']
    for i in range(1, len(string) - 1, 2):
        (x0, y0), (x1, y1) = string[i - 1], string[i + 1]
        s.poly([(x0, y0), (x1, y1), ((x0 + x1) / 2, (y0 + y1) / 2 + 95)], colours[(i // 2) % len(colours)])
    # Confetti.
    for _ in range(60):
        x, y = rnd.uniform(40, 1560), rnd.uniform(220, 1000)
        if 420 < x < 1180 and y > 300:
            continue
        s.circle(x, y, rnd.uniform(6, 13), rnd.choice(colours), 0.85)
    # Table and plate.
    s.rect(0, 1000, 1600, 200, '#f4c7a8')
    s.ellipse(800, 1000, 420, 46, '#ffffff')
    s.ellipse(800, 994, 380, 34, '#f1e4ea')
    # Bottom tier with drips.
    s.rect(450, 690, 700, 300, '#fff4ea', r=34)
    drip = [(450, 724)] + [(450 + i * 35, 724 + (60 if i % 2 else 18) + (i % 3) * 8) for i in range(1, 20)] + [(1150, 724), (1150, 700), (450, 700)]
    s.rect(450, 688, 700, 52, '#ff8fab', r=26)
    s.poly(drip, '#ff8fab')
    # Middle tier.
    s.rect(560, 470, 480, 240, '#ffd9e6', r=30)
    s.rect(560, 468, 480, 46, '#ff6f91', r=23)
    s.poly([(560, 500)] + [(560 + i * 32, 500 + (50 if i % 2 else 14)) for i in range(1, 15)] + [(1040, 500), (1040, 480), (560, 480)], '#ff6f91')
    # Sprinkles.
    for _ in range(40):
        x, y = rnd.uniform(480, 1120), rnd.uniform(800, 960)
        s.rect(x, y, 18, 6, rnd.choice(colours), r=3)
    for _ in range(18):
        s.rect(rnd.uniform(590, 1010), rnd.uniform(580, 690), 16, 6, rnd.choice(colours), r=3)
    # Candles.
    for i, cx in enumerate([680, 800, 920]):
        s.rect(cx - 13, 340, 26, 135, ['#4fc1e9', '#ffc75f', '#a78bfa'][i], r=6)
        for k in range(4):
            s.rect(cx - 13, 352 + k * 32, 26, 9, '#ffffff', opacity=0.7)
        flame(s, cx, 340)
    s.save('birthday-cake')


def party_balloons():
    s = Scene()
    rnd = random.Random(11)
    s.gradient('#2b2d6e', '#6a3d8f')
    for _ in range(70):
        s.circle(rnd.uniform(0, 1600), rnd.uniform(0, 1200), rnd.uniform(2, 5), '#fff6d8', rnd.uniform(0.35, 0.9))
    # Streamers.
    for i, colour in enumerate(['#ffc75f', '#4fc1e9', '#ff6f91']):
        s.line(wave(-20, 1620, 1010 + i * 55, 22, 300 + i * 40), colour, 10, 0.85)
    balloons = [(420, 430, '#ff6f91'), (640, 330, '#ffc75f'), (860, 420, '#4fc1e9'), (1080, 320, '#a78bfa'), (1260, 450, '#62d2a2'), (530, 610, '#ff9f68'), (1000, 600, '#f78fb3')]
    for cx, cy, colour in balloons:
        rx, ry = 120, 150
        s.line(curve((cx, cy + ry + 14), (800 + (cx - 800) * 0.15, 1120), 40 * (1 if cx < 800 else -1)), '#e8e2f5', 3, 0.8)
        s.ellipse(cx, cy, rx, ry, colour)
        s.poly([(cx, cy + ry - 6), (cx + 16, cy + ry + 20), (cx - 16, cy + ry + 20)], colour)
        s.ellipse(cx - 45, cy - 60, 26, 46, '#ffffff', 0.35)
    s.save('party-balloons')


def seaside():
    s = Scene()
    s.gradient('#ffd6a5', '#ff9e7a', (0, 0, W, 640))
    s.circle(1060, 560, 150, '#fff1c1')
    s.gradient('#3d8fb5', '#1d5f86', (0, 620, W, 380))
    for i in range(7):
        y = 650 + i * 36
        w = 300 - i * 30
        s.rect(1060 - w / 2, y, w, 8, '#ffe8b0', r=4, opacity=0.7)
    for i in range(4):
        s.line(wave(-20, 1620, 760 + i * 60, 10, 160 + i * 30), '#e6f4fb', 5, 0.55)
    s.poly([(0, 930)] + wave(0, 1600, 950, 18, 700)[1:] + [(1600, 1200), (0, 1200)], '#f2d0a4')
    s.poly([(0, 1050)] + wave(0, 1600, 1060, 14, 520)[1:] + [(1600, 1200), (0, 1200)], '#e8bd8a')
    # Umbrella.
    s.line([(420, 640), (470, 1060)], '#6b4f3a', 12)
    top = (415, 620)
    for k in range(6):
        a0, a1 = math.pi * (1 + k / 6), math.pi * (1 + (k + 1) / 6)
        p0 = (top[0] + 330 * math.cos(a0), top[1] + 170 * math.sin(a0) + 120)
        p1 = (top[0] + 330 * math.cos(a1), top[1] + 170 * math.sin(a1) + 120)
        s.poly([top, p0, p1], '#e63946' if k % 2 == 0 else '#fff7ec')
    # Towel and ball.
    s.rect(560, 1010, 300, 110, '#4fc1e9', r=14)
    for k in range(3):
        s.rect(560, 1030 + k * 32, 300, 12, '#ffffff', opacity=0.8)
    s.circle(1220, 1040, 58, '#ffffff')
    s.poly([(1220, 982), (1270, 1010), (1220, 1040)], '#ffc75f')
    s.poly([(1220, 1040), (1170, 1070), (1220, 1098)], '#ff6f91')
    for x, y in [(250, 200), (330, 250), (1350, 180)]:
        s.line([(x - 30, y), (x - 12, y - 14), (x, y), (x + 12, y - 14), (x + 30, y)], '#5c4b51', 5)
    s.save('seaside')


def city_lights():
    s = Scene()
    rnd = random.Random(3)
    s.gradient('#0b1026', '#3a2a63')
    for _ in range(90):
        s.circle(rnd.uniform(0, 1600), rnd.uniform(0, 620), rnd.uniform(1.5, 4), '#ffffff', rnd.uniform(0.3, 0.9))
    s.circle(1250, 230, 95, '#f6f1d5')
    s.circle(1290, 205, 82, '#141532')  # the sky colour there, cutting a crescent
    s.ellipse(800, 900, 900, 260, '#ff9e7a', 0.12)
    x = -20
    layers = [('#1b2140', 0.9, 1.0), ('#262e5a', 1.0, 0.75)]
    for colour, scale, lit in layers:
        x = -20
        while x < 1620:
            w = rnd.choice([110, 140, 170, 200])
            h = rnd.uniform(260, 620) * scale
            top = 1000 - h
            s.rect(x, top, w, h + 200, colour)
            if rnd.random() < 0.3:
                s.rect(x + w / 2 - 4, top - 60, 8, 60, colour)
            for wy in range(int(top + 30), 960, 44):
                for wx in range(int(x + 18), int(x + w - 26), 36):
                    if rnd.random() < 0.38 * lit:
                        s.rect(wx, wy, 18, 24, rnd.choice(['#ffd166', '#ffe8a3', '#ffb86b']), r=3)
            x += w + rnd.choice([0, 8, 16])
    s.gradient('#1f2a52', '#0b1026', (0, 1000, W, 200))
    for _ in range(26):
        y = rnd.uniform(1015, 1180)
        cx = rnd.uniform(60, 1540)
        s.rect(cx, y, rnd.uniform(40, 120), 5, '#ffd166', r=3, opacity=0.35)
    s.save('city-lights')


def wildflowers():
    s = Scene()
    rnd = random.Random(5)
    s.gradient('#fdf6e3', '#e4f1e1')
    s.circle(1300, 260, 120, '#ffe29a', 0.8)
    s.ellipse(400, 1050, 900, 360, '#a8cfa0')
    s.ellipse(1300, 1080, 800, 330, '#8fbf8a')
    s.ellipse(800, 1200, 1100, 330, '#6fa577')
    petals = ['#f28482', '#f6bd60', '#cdb4db', '#ffafcc', '#84a59d', '#ffffff']
    flowers = []
    for _ in range(26):
        flowers.append((rnd.uniform(80, 1520), rnd.uniform(420, 980), rnd.uniform(26, 48), rnd.choice(petals)))
    flowers.sort(key=lambda f: f[1])
    for x, y, size, colour in flowers:
        bend = rnd.uniform(-40, 40)
        s.line(curve((x, y), (x + bend * 0.4, 1200), bend), '#4a7c59', 6)
        s.ellipse(x + bend * 0.2 + 26, y + 160, 30, 12, '#5e9a6a')
        for k in range(6):
            a = k / 6 * 2 * math.pi
            s.circle(x + math.cos(a) * size * 0.7, y + math.sin(a) * size * 0.7, size * 0.55, colour)
        s.circle(x, y, size * 0.42, '#6d4c41' if colour != '#f6bd60' else '#9c6644')
    for x, y in [(520, 300), (980, 380)]:
        s.ellipse(x - 22, y, 24, 16, '#9d79bc')
        s.ellipse(x + 22, y, 24, 16, '#9d79bc')
        s.rect(x - 4, y - 14, 8, 30, '#4a3f55', r=4)
    s.save('wildflowers')


if __name__ == '__main__':
    for draw in (birthday_cake, party_balloons, seaside, city_lights, wildflowers):
        draw()
