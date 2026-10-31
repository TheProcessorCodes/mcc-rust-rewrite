# Read-only probe: inflate an MCC Halo 2 (classic, header version 13) .map into
# a flat file under this project, and print the header fields
# docs/format/halo2.md section 1 describes. The game file is only read.
# Run: python -I h2_decompress.py [map name or path] [out path]
import os, struct, sys, zlib

MAPS = os.environ.get("HALO2_MAPS", "")
name = sys.argv[1] if len(sys.argv) > 1 else "coagulation"
path = name if name.lower().endswith(".map") else os.path.join(MAPS, name + ".map")
here = os.path.dirname(os.path.abspath(__file__))
out = sys.argv[2] if len(sys.argv) > 2 else os.path.join(here, "..", "..", "..", ".scratch", "h2", os.path.basename(path))
d = open(path, "rb").read()

def i32(o): return struct.unpack_from("<i", d, o)[0]
def u16(o): return struct.unpack_from("<H", d, o)[0]
def cstr(o, n): return d[o:o + n].split(b"\0")[0].decode("latin1")

print("file", path, "on disk 0x%X" % len(d))
print("magic", d[0:4][::-1], "version", i32(4), "stated size 0x%X" % i32(8))
print("index address 0x%X size 0x%X flags 0x%X" % (i32(16), i32(20), u16(28)))
print("build", cstr(144, 32), "scenario", cstr(208, 256))
chunk_size, data_off, table, count = i32(776), i32(780), i32(784), i32(788)
print("chunk size 0x%X data 0x%X table 0x%X count %d" % (chunk_size, data_off, table, count))
if not u16(28) & 1:
    sys.exit("not compressed")

flat = bytearray(d[:0x380])  # the 896-byte header is stored as is
plain = 0
for i in range(count):
    size, addr = i32(table + 8 * i), i32(table + 8 * i + 4)
    if size < 0:  # stored: -size bytes after a two-byte chunk header
        flat += d[addr + 2:addr + 2 - size]
        plain += 1
    else:
        flat += zlib.decompressobj().decompress(d[addr:addr + size])
print("inflated 0x%X bytes (%d stored chunks); stated 0x%X" % (len(flat), plain, i32(8)))
os.makedirs(os.path.dirname(out), exist_ok=True)
open(out, "wb").write(flat)
print("wrote", os.path.normpath(out))
