# isx.py <setup.exe> <outdir>: extracts the files an InstallShield setup.exe appends after its PE image.
# Two layouts: entries of name\0 path\0 version\0 decimal size\0 then the raw bytes, or an
# "InstallShield\0" stream (fixed 260-byte names, data XORed with a key made from the name).
# The .msi it holds opens with 7z; tools/verify/msinames.py names the files of its cab.
import os, struct, sys
src, out = sys.argv[1], sys.argv[2]
d = open(src, 'rb').read()
pe = struct.unpack_from('<I', d, 0x3c)[0]
ns, oh = struct.unpack_from('<H', d, pe + 6)[0], struct.unpack_from('<H', d, pe + 20)[0]
p = max(sum(struct.unpack_from('<II', d, pe + 24 + oh + 40 * i + 16)) for i in range(ns))
if d[p:p + 14] == b'InstallShield\0':
    n = struct.unpack_from('<H', d, p + 14)[0]
    p += 0x2e
    magic = (0x13, 0x35, 0x86, 0x07)
    for _ in range(n):
        name = d[p:p + 260].split(b'\0', 1)[0]
        flags, _x, size = struct.unpack_from('<III', d, p + 260)
        p += 312
        data = bytearray(d[p:p + size]); p += size
        if flags & 6:
            key = [c ^ magic[i % 4] for i, c in enumerate(name)]
            block = 1024 if flags & 4 else max(len(data), 1)
            for i, b in enumerate(data):
                data[i] = ~(key[(i % block) % len(key)] ^ (((b << 4) | (b >> 4)) & 0xff)) & 0xff
        rel = name.decode('cp932'); os.makedirs(out, exist_ok=True)
        open(os.path.join(out, rel), 'wb').write(data); print(rel, size)
    sys.exit()
while p < len(d):
    f = []
    q = p
    for _ in range(4):
        e = d.index(b'\0', q); f.append(d[q:e]); q = e + 1
    if not f[3].isdigit(): print('stop at', hex(p), f[:2]); break
    n = int(f[3]); rel = f[1].decode('cp932').replace('\\', '/')
    t = os.path.join(out, rel); os.makedirs(os.path.dirname(t), exist_ok=True)
    open(t, 'wb').write(d[q:q + n]); print(rel, n); p = q + n
