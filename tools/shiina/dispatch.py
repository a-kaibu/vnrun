# Follows the interpreter's opcode dispatch (compare trees and jump tables) for every opcode.
import re, sys
import os; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from pe import PE
pe = PE(sys.argv[1])
asm = {}
order = []
for line in open(sys.argv[2]):
    m = re.match(r'\s+([0-9a-f]+):\s+(.*)$', line.rstrip())
    if m:
        a = int(m.group(1), 16); asm[a] = m.group(2).strip(); order.append(a)
nxt = {order[i]: order[i + 1] for i in range(len(order) - 1)}
start = int(sys.argv[3], 16)
M = 0xffffffff
def s32(v): v &= M; return v - (1 << 32) if v >> 31 else v
seen_handlers = set()
def run(op):
    regs = {'eax': op, 'ecx': 0, 'edx': 0}
    flags = None
    pc = start
    for _ in range(200):
        ins = asm[pc]
        parts = re.split(r'\s+', ins, maxsplit=1)
        mn, args = parts[0], parts[1] if len(parts) > 1 else ''
        if mn == 'cmp' and args.split(',')[0] in regs and args.split(',')[1].startswith('0x'):
            r, v = [x.strip() for x in args.split(',')]
            flags = (regs[r] & M, int(v, 16) & M)
        elif mn in ('add', 'sub') and args.split(',')[0] in regs and args.split(',')[1].startswith('0x'):
            r, v = [x.strip() for x in args.split(',')]
            v = int(v, 16)
            regs[r] = (regs[r] + v if mn == 'add' else regs[r] - v) & M
        elif mn == 'xor' and args in ('ecx,ecx', 'edx,edx', 'eax,eax'):
            r = args.split(',')[0]; regs[r] = 0
        elif mn == 'dec':
            regs[args] = (regs[args] - 1) & M
        elif mn == 'mov' and args.startswith('cl,BYTE PTR'):
            m = re.search(r'\[(e..)\+0x([0-9a-f]+)\]', args)
            regs['ecx'] = pe.u8(int(m.group(2), 16) + regs[m.group(1)])
        elif mn == 'mov' and args.startswith('dl,BYTE PTR'):
            m = re.search(r'\[(e..)\+0x([0-9a-f]+)\]', args)
            regs['edx'] = pe.u8(int(m.group(2), 16) + regs[m.group(1)])
        elif mn == 'jmp' and 'PTR' in args:
            m = re.search(r'\[(e..)\*4\+0x([0-9a-f]+)\]', args)
            return pe.u32(int(m.group(2), 16) + 4 * regs[m.group(1)])
        elif mn == 'jmp':
            pc = int(args, 16); continue
        elif mn.startswith('j') and flags is not None and pc not in seen_handlers:
            a, b = flags
            t = {'je': a == b, 'jne': a != b, 'jg': s32(a) > s32(b), 'jge': s32(a) >= s32(b), 'jl': s32(a) < s32(b),
                 'jle': s32(a) <= s32(b), 'ja': a > b, 'jae': a >= b, 'jb': a < b, 'jbe': a <= b}[mn]
            if t:
                pc = int(args, 16); continue
        elif mn == 'and' and args == 'eax,0xffff':
            pass
        else:
            return pc  # a handler starts here
        pc = nxt[pc]
    return None
res = {}
for op in range(0, 0x10000):
    h = run(op)
    res.setdefault(h, []).append(op)
bad = int(sys.argv[4], 16) if len(sys.argv) > 4 else None
for h, ops in sorted(res.items(), key=lambda x: x[1][0]):
    if h == bad: continue
    print(hex(h), ' '.join(hex(o) for o in ops[:20]) + (' ...' if len(ops) > 20 else ''))
