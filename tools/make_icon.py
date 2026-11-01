"""Renders icon.png (python tools/make_icon.py icon.png). Renders the Horizonmatica icon: a half-printed isometric build (solid blocks below, schematic ghost
blocks above) in front of a horizon sunrise. Supersampled, then scaled down to 256x256."""
import math
import random
import sys

from PIL import Image, ImageDraw, ImageFilter

SS = 4
N = 256
S = N * SS
random.seed(7)


def lerp(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(len(a)))


# --- background: rounded square, night-to-dawn gradient, horizon glow ---
bg = Image.new("RGBA", (S, S))
px = bg.load()
top, mid, low = (14, 18, 46), (46, 30, 92), (255, 128, 64)
hy = 0.74
for y in range(S):
    t = y / S
    if t < hy:
        c = lerp(top, mid, (t / hy) ** 1.6)
    else:
        c = lerp((255, 150, 80), (120, 40, 70), min(1, (t - hy) / (1 - hy)) ** 0.8)
    for x in range(S):
        px[x, y] = c + (255,)

glow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
gd = ImageDraw.Draw(glow)
cx, cy = S // 2, int(S * hy)
for i in range(40, 0, -1):
    r = int(S * 0.012 * i)
    a = int(10 + 3 * (40 - i))
    gd.ellipse((cx - r * 1.6, cy - r, cx + r * 1.6, cy + r), fill=(255, 170, 90, min(255, a // 2)))
glow = glow.filter(ImageFilter.GaussianBlur(S * 0.02))
bg = Image.alpha_composite(bg, glow)
d = ImageDraw.Draw(bg)
d.line((0, cy, S, cy), fill=(255, 220, 160, 255), width=int(S * 0.006))
# stars
for _ in range(45):
    x, y = random.randrange(S), random.randrange(int(S * 0.45))
    r = random.choice((1, 1, 2)) * SS
    d.ellipse((x - r, y - r, x + r, y + r), fill=(255, 255, 255, random.randrange(90, 220)))

# --- isometric blocks ---
u = S * 0.106  # cube edge in pixels
ox, oy = S * 0.5, S * 0.53
cos30 = math.cos(math.radians(30))


def proj(x, y, z):
    return (ox + (x - z) * u * cos30, oy + (x + z) * u * 0.5 - y * u)


layer = Image.new("RGBA", (S, S), (0, 0, 0, 0))
ld = ImageDraw.Draw(layer, "RGBA")

GRASS_TOP = (98, 176, 66)
DIRT = (134, 96, 67)
STONE = (150, 152, 164)
PLANK = (176, 136, 84)
GHOST = (80, 220, 255)

# (x, y, z) -> kind; x,z in 0..2, y up
blocks = {}
for x in range(3):
    for z in range(3):
        blocks[(x, 0, z)] = "grass"
for (x, z) in [(0, 0), (1, 0), (2, 0), (0, 1), (0, 2)]:
    blocks[(x, 1, z)] = "stone"
for (x, z) in [(2, 1), (1, 2), (2, 2), (1, 1)]:
    blocks[(x, 1, z)] = "ghost"
for (x, z) in [(0, 0), (1, 0), (0, 1)]:
    blocks[(x, 2, z)] = "ghost"
blocks[(0, 3, 0)] = "ghost"


def solid(p):
    return blocks.get(p) not in (None, "ghost")


def face_quads(corners, sub, base, jitter):
    """Splits a parallelogram face into sub x sub texels with random brightness jitter."""
    a, b, c, dd = corners  # a->b is one edge, a->dd the other
    out = []
    for i in range(sub):
        for j in range(sub):
            def pt(fi, fj):
                return (a[0] + (b[0] - a[0]) * fi + (dd[0] - a[0]) * fj,
                        a[1] + (b[1] - a[1]) * fi + (dd[1] - a[1]) * fj)
            q = [pt(i / sub, j / sub), pt((i + 1) / sub, j / sub),
                 pt((i + 1) / sub, (j + 1) / sub), pt(i / sub, (j + 1) / sub)]
            k = 1 + random.uniform(-jitter, jitter)
            out.append((q, tuple(max(0, min(255, int(ch * k))) for ch in base)))
    return out


def shade(c, f):
    return tuple(int(ch * f) for ch in c)


def cube_faces(x, y, z):
    P = lambda dx, dy, dz: proj(x + dx, y + dy, z + dz)
    top = [P(0, 1, 0), P(1, 1, 0), P(1, 1, 1), P(0, 1, 1)]
    right = [P(1, 1, 0), P(1, 1, 1), P(1, 0, 1), P(1, 0, 0)]  # +x face
    left = [P(0, 1, 1), P(1, 1, 1), P(1, 0, 1), P(0, 0, 1)]  # +z face
    return top, right, left


order = sorted(blocks, key=lambda p: (p[0] + p[2], p[1]))
for p in order:
    kind = blocks[p]
    x, y, z = p
    top, right, left = cube_faces(x, y, z)
    show_top = not solid((x, y + 1, z))
    show_right = not solid((x + 1, y, z))
    show_left = not solid((x, y, z + 1))
    if kind == "ghost":
        for face, f, show in ((left, 0.75, show_left), (right, 0.6, show_right), (top, 1.0, show_top)):
            ld.polygon(face, fill=shade(GHOST, f) + (48,))
        edge = (170, 245, 255, 255)
        w = int(SS * 2.2)
        for face in (top, right, left):
            ld.line(face + [face[0]], fill=edge, width=w, joint="curve")
        continue
    if kind == "grass":
        faces = [(top, GRASS_TOP, 1.0, show_top), (left, DIRT, 0.8, show_left), (right, DIRT, 0.6, show_right)]
    elif kind == "plank":
        faces = [(top, PLANK, 1.0, show_top), (left, PLANK, 0.8, show_left), (right, PLANK, 0.6, show_right)]
    else:
        faces = [(top, STONE, 1.0, show_top), (left, STONE, 0.8, show_left), (right, STONE, 0.6, show_right)]
    for face, col, f, show in faces:
        if not show:
            continue
        a, b, c, dd = face
        for q, colr in face_quads((a, b, c, dd), 4, shade(col, f), 0.09):
            ld.polygon(q, fill=colr + (255,))
        if kind == "grass" and face is not top:  # grass fringe on the side faces
            fr = [a, b, (b[0] + (c[0] - b[0]) * 0.22, b[1] + (c[1] - b[1]) * 0.22),
                  (a[0] + (dd[0] - a[0]) * 0.22, a[1] + (dd[1] - a[1]) * 0.22)]
            ld.polygon(fr, fill=shade(GRASS_TOP, f * 0.95) + (255,))
        ld.line(face + [face[0]], fill=(20, 20, 30, 110), width=SS)

# printer beam: from a nozzle point into the ghost block being placed
tx, ty = proj(1.5, 2.0, 1.5)  # top centre of the ghost at (1,1,1)
nx, ny = tx + S * 0.20, ty - S * 0.26
beam = Image.new("RGBA", (S, S), (0, 0, 0, 0))
bd = ImageDraw.Draw(beam)
bd.polygon([(nx - S * 0.012, ny), (nx + S * 0.012, ny), (tx + S * 0.05, ty), (tx - S * 0.05, ty)],
           fill=(255, 230, 140, 120))
beam = beam.filter(ImageFilter.GaussianBlur(S * 0.006))
bd = ImageDraw.Draw(beam)
bd.line((nx, ny, tx, ty), fill=(255, 245, 200, 255), width=int(SS * 2))
r = S * 0.03
bd.ellipse((nx - r, ny - r, nx + r, ny + r), fill=(255, 210, 110, 255), outline=(255, 250, 230, 255), width=SS * 2)
# sparkle at the target
for ang in range(0, 360, 45):
    L = S * (0.05 if ang % 90 == 0 else 0.028)
    bd.line((tx, ty, tx + L * math.cos(math.radians(ang)), ty + L * math.sin(math.radians(ang))),
            fill=(255, 250, 220, 255), width=int(SS * 1.8))

img = Image.alpha_composite(bg, layer)
img = Image.alpha_composite(img, beam)

# rounded-square mask
mask = Image.new("L", (S, S), 0)
ImageDraw.Draw(mask).rounded_rectangle((0, 0, S - 1, S - 1), radius=int(S * 0.2), fill=255)
img.putalpha(mask)
img = img.resize((N, N), Image.LANCZOS)
img.save(sys.argv[1], optimize=True)
print("ok")
