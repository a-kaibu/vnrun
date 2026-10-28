# Operand signatures of opcode handlers: the operand-reading calls along the straight path.
import re, sys
asm = {}; order = []
for line in open(sys.argv[3] if len(sys.argv) > 3 else sys.argv[1]):
    m = re.match(r'\s+([0-9a-f]+):\s+(.*)$', line.rstrip())
    if m:
        a = int(m.group(1), 16); asm[a] = m.group(2).strip(); order.append(a)
nxt = {order[i]: order[i + 1] for i in range(len(order) - 1)}
NEXT = 0x42a3d4
CALLS = {0x415690: 'v', 0x415190: 'w', 0x415500: 'a', 0x4150f0: 'push', 0x415140: 'pop'}
BORING = set()
def sig(h, depth=0):
    out = []; pc = h; branches = 0
    for _ in range(400):
        ins = asm.get(pc)
        if ins is None: out.append('?'); break
        m = re.match(r'call\s+0x([0-9a-f]+)', ins)
        if m:
            t = int(m.group(1), 16)
            if t in CALLS:
                out.append(CALLS[t])
            elif depth < 3 and t in asm and t not in BORING:
                sub, _ = sig(t, depth + 1)
                ops = [x for x in sub if x in ('v', 'w', 'a', 'raw', 'push', 'pop')]
                out.append('f%x' % t)
                if ops:
                    out += ['[' ] + ops + [']']
            else:
                out.append('f%x' % t)
        elif 'PTR [ebx+0x10]' in ins or 'PTR [esi+0x10]' in ins:
            out.append('raw')
        m = re.match(r'jmp\s+0x([0-9a-f]+)$', ins)
        if m:
            t = int(m.group(1), 16)
            if t == NEXT: out.append('.'); break
            if t in asm and t > pc - 0x4000:
                pc = t; continue
            out.append('->%x' % t); break
        if ins.startswith('j') and not ins.startswith('jmp'): branches += 1
        if ins.startswith('ret'):
            out.append('ret'); break
        pc = nxt.get(pc)
    return out, branches
for line in open(sys.argv[2]):
    h, *ops = line.split()
    s, b = sig(int(h, 16))
    keep = [x for x in s if x in ('v','w','a','raw','push','pop','.','ret','?','[',']') or x.startswith('->')]
    print(' '.join(ops[:4]), '|', ' '.join(keep), '| br', b, '|', ' '.join(x for x in s if x.startswith('f'))[:80])
