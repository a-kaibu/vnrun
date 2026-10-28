"""msinames.py <msi streams dir> <cab dir> <outdir>: moves the cab's files (named by File key) to
their install paths, read from the MSI's File, Component and Directory tables (7z-extracted streams)."""
import os, shutil, struct, sys
m, cab, out = sys.argv[1:4]
rd = lambda n: open(os.path.join(m, n), "rb").read()
pool, data = rd("!_StringPool"), rd("!_StringData")
cp = struct.unpack_from("<I", pool, 0)[0]
wide = bool(cp & 0x80000000)
strings, off = [None], 0
i = 4
while i + 4 <= len(pool):
    ln, refs = struct.unpack_from("<HH", pool, i)
    i += 4
    if ln == 0 and refs != 0:  # long string: real length in the next entry
        ln = struct.unpack_from("<I", pool, i)[0]
        i += 4
    strings.append(data[off:off + ln].decode("cp932", "replace"))
    off += ln
sw = 3 if wide else 2

def table(name, cols):
    """cols: 's' string, 'i2', 'i4'. Stored column by column."""
    raw = rd(name)
    size = sum(sw if c == "s" else int(c[1]) for c in cols)
    n = len(raw) // size
    assert n * size == len(raw), (name, len(raw), size)
    res, p = [[None] * len(cols) for _ in range(n)], 0
    for j, c in enumerate(cols):
        for r in range(n):
            if c == "s":
                v = int.from_bytes(raw[p:p + sw], "little"); p += sw
                res[r][j] = strings[v] if v else None
            elif c == "i2":
                res[r][j] = struct.unpack_from("<H", raw, p)[0] - 0x8000; p += 2
            else:
                res[r][j] = struct.unpack_from("<I", raw, p)[0] - 0x80000000; p += 4
    return res

def long_name(s):
    s = s.split("|")[-1]
    return s

for seq in ("i2", "i4"):
    try:
        files = table("!File", ["s", "s", "s", "i4", "s", "s", "i2", seq]); break
    except AssertionError:
        continue
comps = {r[0]: r[2] for r in table("!Component", ["s", "s", "s", "i2", "s", "s"])}
dirs = {r[0]: (r[1], r[2]) for r in table("!Directory", ["s", "s", "s"])}

def path(d):
    parts = []
    while d in dirs:
        parent, default = dirs[d]
        target = default.split(":")[0]
        name = long_name(target)
        if parent is None or parent == d:
            break
        if name not in (".", ""):
            parts.append(name)
        d = parent
    return list(reversed(parts))

for key, comp, fname, size, *_ in files:
    rel = os.path.join(*path(comps[comp]), long_name(fname)) if path(comps[comp]) else long_name(fname)
    src = os.path.join(cab, key)
    if not os.path.exists(src):
        print("missing", key, rel); continue
    dst = os.path.join(out, rel)
    os.makedirs(os.path.dirname(dst) or out, exist_ok=True)
    shutil.move(src, dst)
    print(rel, size)
