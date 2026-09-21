"""Regenerate the Windows app icon from the SVG.

    python scripts/make-icon.py resources/mailclient.svg resources/mailclient.ico

`resources/mailclient.svg` is the one source of truth for the app icon; the
`.ico` is a checked-in build input (crates/mailapp/build.rs links it into the
exe) so that building on Windows needs no rasteriser. Run this after editing
the SVG and commit the result.

Small sizes are stored as 32bpp BMP/DIB entries, which every Windows icon
consumer understands; the largest are stored as PNG, which is the only way to
keep the file from bloating. Pure stdlib apart from `rsvg-convert` (librsvg),
which does the rasterising; zlib does the PNG decoding.
"""
import struct, subprocess, sys, zlib
from pathlib import Path

SVG, OUT = Path(sys.argv[1]), Path(sys.argv[2])
SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]
PNG_FROM = 128  # store these and larger as PNG


def render(size):
    return subprocess.run(
        ["rsvg-convert", "-w", str(size), "-h", str(size), "-f", "png", str(SVG)],
        check=True, capture_output=True).stdout


def png_to_rgba(blob):
    """Decode a non-interlaced 8-bit RGBA/RGB PNG into (w, h, rgba bytes)."""
    assert blob[:8] == b"\x89PNG\r\n\x1a\n"
    pos, idat, w = 8, b"", None
    while pos < len(blob):
        ln, typ = struct.unpack(">I4s", blob[pos:pos + 8])
        data = blob[pos + 8:pos + 8 + ln]
        pos += 12 + ln
        if typ == b"IHDR":
            w, h, depth, color, _, _, interlace = struct.unpack(">IIBBBBB", data)
            assert depth == 8 and interlace == 0 and color in (2, 6), (depth, color, interlace)
            channels = 4 if color == 6 else 3
        elif typ == b"IDAT":
            idat += data
        elif typ == b"IEND":
            break
    raw, stride, out, prev = zlib.decompress(idat), w * channels, bytearray(), bytes(w * channels)
    for y in range(h):
        base = y * (stride + 1)
        filt, line = raw[base], bytearray(raw[base + 1:base + 1 + stride])
        for i in range(stride):
            a = line[i - channels] if i >= channels else 0
            b = prev[i]
            c = prev[i - channels] if i >= channels else 0
            if filt == 1: line[i] = (line[i] + a) & 0xFF
            elif filt == 2: line[i] = (line[i] + b) & 0xFF
            elif filt == 3: line[i] = (line[i] + (a + b) // 2) & 0xFF
            elif filt == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 0xFF
            elif filt: raise ValueError(f"filter {filt}")
        prev = bytes(line)
        if channels == 3:
            for x in range(w):
                out += line[x * 3:x * 3 + 3] + b"\xff"
        else:
            out += line
    return w, h, bytes(out)


def to_dib(w, h, rgba):
    """32bpp bottom-up BGRA DIB plus the (unused but mandatory) AND mask."""
    header = struct.pack("<IiiHHIIiiII", 40, w, h * 2, 1, 32, 0, w * h * 4, 0, 0, 0, 0)
    body = bytearray()
    for y in range(h - 1, -1, -1):
        row = rgba[y * w * 4:(y + 1) * w * 4]
        for x in range(w):
            r, g, b, a = row[x * 4:x * 4 + 4]
            body += bytes((b, g, r, a))
    mask_stride = ((w + 31) // 32) * 4
    return header + bytes(body) + bytes(mask_stride * h)


images = []
for size in SIZES:
    png = render(size)
    images.append(png if size >= PNG_FROM else to_dib(*png_to_rgba(png)))

offset = 6 + 16 * len(images)
out = bytearray(struct.pack("<HHH", 0, 1, len(images)))
for size, blob in zip(SIZES, images):
    out += struct.pack("<BBBBHHII", size & 0xFF, size & 0xFF, 0, 0, 1, 32, len(blob), offset)
    offset += len(blob)
for blob in images:
    out += blob
OUT.write_bytes(out)
print(f"{OUT}: {len(out)} bytes, {len(images)} sizes {SIZES}")
