# Linear disassembler for ShiinaRio .SCN, using sigs.txt operand lists (v value, w store, a address).
import re, sys, struct
sigs = {}
for line in open(sys.argv[1]):
    ops, s = line.split('|')[0], line.split('|')[1]
    args = [x for x in s.split() if x in ('v', 'w', 'a')]
    for o in ops.split():
        if o.startswith('0x'): sigs[int(o, 16)] = args
over = {}
if len(sys.argv) > 4:
    for line in open(sys.argv[4]):
        line = line.split('#')[0].strip()
        if line:
            o, *a = line.split(); over[int(o, 16)] = a
sigs.update(over)
b = open(sys.argv[2], 'rb').read()
limit = int(sys.argv[3]) if len(sys.argv) > 3 else 400
SIZE = {2: 2, 3: 2, 4: 4, 5: 4, 6: 2, 7: 2, 8: 2, 9: 2, 10: 2, 11: 2, 12: 2, 13: 2, 14: 2, 15: 2}
NAMES = {2: 'G8', 4: 'imm', 6: 'F', 8: 'L', 10: 'G', 12: 'A', 14: 'G4', 16: 'str', 17: 'num', 18: 'name'}
def cstr(p):
    e = b.index(b'\0', p); return b[p:e].decode('cp932', 'replace'), e + 1
def operand(p, kind):
    t = b[p]; p += 1
    if t & 0x40 and kind == 'v':
        tt = t & 0x3f; pre = '&'
    else:
        tt = t & 0x7f; pre = '*' if t & 0x80 else ''
    if tt in SIZE:
        n = SIZE[tt]
        v = int.from_bytes(b[p:p + n], 'little'); p += n
        name = NAMES.get(tt & ~1, '?%d' % tt)
        if tt & 1: name = '[' + name
        return '%s%s%s' % (pre, name, v if name != 'imm' else (v if v < 0x80000000 else v - (1 << 32))), p
    if tt == 16:
        s, p = cstr(p); return pre + repr(s), p
    if tt == 17:
        s, p = cstr(p); return pre + 'num:' + s, p
    if tt in (18, 19):
        e = p
        while b[e] not in (0, 0x7d): e += 1
        s = b[p:e].decode('cp932', 'replace'); return pre + '$' + s, e + 1
    raise ValueError('operand type %#x at %#x' % (t, p - 1))
p = 0
for _ in range(limit):
    if p >= len(b): break
    op = struct.unpack_from('<H', b, p)[0]
    start = p; p += 2
    if op not in sigs:
        print('%06x ??? %04x' % (start, op)); break
    try:
        args = []
        for k in sigs[op]:
            a, p = operand(p, k); args.append(a)
    except ValueError as e:
        print('%06x %04x ERR %s' % (start, op, e)); break
    print('%06x %04x %s' % (start, op, ' '.join(args)))
