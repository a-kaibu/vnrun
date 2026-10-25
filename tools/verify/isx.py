# isx.py <setup.exe> <outdir>: extracts the files an InstallShield setup.exe appends after its PE image
# (entries: name\0 path\0 version\0 decimal size\0 then the raw bytes).
import os, struct, sys
src, out = sys.argv[1], sys.argv[2]
d = open(src, 'rb').read()
pe = struct.unpack_from('<I', d, 0x3c)[0]
ns, oh = struct.unpack_from('<H', d, pe + 6)[0], struct.unpack_from('<H', d, pe + 20)[0]
p = max(sum(struct.unpack_from('<II', d, pe + 24 + oh + 40 * i + 16)) for i in range(ns))
while p < len(d):
    f = []
    q = p
    for _ in range(4):
        e = d.index(b'\0', q); f.append(d[q:e]); q = e + 1
    if not f[3].isdigit(): print('stop at', hex(p), f[:2]); break
    n = int(f[3]); rel = f[1].decode('cp932').replace('\\', '/')
    t = os.path.join(out, rel); os.makedirs(os.path.dirname(t), exist_ok=True)
    open(t, 'wb').write(d[q:q + n]); print(rel, n); p = q + n
