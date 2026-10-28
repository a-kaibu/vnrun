import struct, sys
class PE:
    def __init__(self, path):
        self.b = open(path, 'rb').read()
        b = self.b
        pe = struct.unpack_from('<I', b, 0x3c)[0]
        n = struct.unpack_from('<H', b, pe + 6)[0]
        opt = struct.unpack_from('<H', b, pe + 20)[0]
        self.base = struct.unpack_from('<I', b, pe + 24 + 28)[0]
        self.secs = []
        at = pe + 24 + opt
        for i in range(n):
            name, vsize, va, rsize, raw = struct.unpack_from('<8sIIII', b, at + i * 40)
            self.secs.append((va + self.base, vsize, raw, rsize))
    def off(self, va):
        for s, vs, raw, rs in self.secs:
            if s <= va < s + max(vs, rs):
                return raw + va - s
        raise KeyError(hex(va))
    def u32(self, va): return struct.unpack_from('<I', self.b, self.off(va))[0]
    def u8(self, va): return self.b[self.off(va)]
    def table(self, va, n): return [self.u32(va + 4 * i) for i in range(n)]
    def cstr(self, va):
        o = self.off(va); e = self.b.index(b'\0', o); return self.b[o:e].decode('cp932', 'replace')
if __name__ == '__main__':
    p = PE(sys.argv[1])
    cmd = sys.argv[2]
    if cmd == 'table':
        va, n = int(sys.argv[3], 16), int(sys.argv[4])
        for i, t in enumerate(p.table(va, n)): print(i, hex(t))
    elif cmd == 'btable':
        va, n, tv = int(sys.argv[3], 16), int(sys.argv[4]), int(sys.argv[5], 16)
        for i in range(n):
            k = p.u8(va + i); print(hex(i), k, hex(p.u32(tv + 4 * k)))
    elif cmd == 'str':
        print(p.cstr(int(sys.argv[3], 16)))
