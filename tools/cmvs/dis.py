# CMVS PS2 script disassembler (vnrun prototype). Input is an unpacked PS2A
# file (cpz.py does that). Layout: 0x30 header, u32 labels [h+0x10], code
# [h+0x14], data [h+0x18], strings [h+0x1c]; h+0x20 is the scenario entry.
# usage: python3 -I dis.py X.ps2
import struct,sys
def u16(d,o): return struct.unpack_from('<H',d,o)[0]
def u32(d,o): return struct.unpack_from('<I',d,o)[0]
OPN={0x160:'*',0x161:'/',0x162:'%',0x163:'+',0x164:'-',0x165:'&',0x166:'|',0x167:'^',0x168:'<<',0x169:'>>',0x16a:'>',0x16b:'>=',0x16c:'<',0x16d:'<=',0x16e:'&&',0x16f:'||',0x170:'=',0x171:'==',0x172:'!=',0x173:'++',0x174:'--'}
for i,n in enumerate(['+=','-=','*=','/=','&=','|=','%=','^=','<<=','>>=']): OPN[0x175+i]=n
def expr(c,p,mode=0):
    out=[]
    while True:
        t=u16(c,p)
        if t==0x20f: return p+2,out
        if t in (0x101,0x102,0x12b,0x200) or (mode==1 and t==0x121):
            p,sub=expr(c,p+(2 if t==0x200 else 4)); out.append('%x(%s)'%(t,' '.join(sub)))
        elif t in (0x105,0x109,0x110,0x112,0x12d,0x12f) or (mode==2 and t in (0x134,0x136)):
            v=u32(c,p+2); p,sub=expr(c,p+8); out.append('%x:%x[%s]'%(t,v,' '.join(sub)))
        elif t in (0x107,0x10b) or (mode==2 and t in (0x131,0x133)):
            v=u32(c,p+2); st=u16(c,p+6); p,sub=expr(c,p+10); out.append('%x:%x*%d[%s]'%(t,v,st,' '.join(sub)))
        elif t in (0x111,0x113) or (mode==2 and t in (0x135,0x137)):
            v=u32(c,p+2); st=u16(c,p+6); p,s1=expr(c,p+10); p,s2=expr(c,p+2); out.append('%x:%x*%d[%s][%s]'%(t,v,st,' '.join(s1),' '.join(s2)))
        elif t==0x12a:
            out.append('f%g'%struct.unpack_from('<f',c,p+2)[0]); p+=6
        elif 0x160<=t<=0x17e:
            out.append(OPN.get(t,'op%x'%t)); p+=2
        else:
            v=u32(c,p+2); out.append('%x:%d'%(t,v) if t!=0x100 else '#%d'%struct.unpack_from('<i',c,p+2)[0]); p+=6
SZ={0x400:6,0x401:6,0x402:6,0x403:10,0x405:4,0x407:10,0x410:4,0x411:4,0x412:4,0x413:2,0x414:2,0x416:2,0x430:2,0x440:8,0x442:8}
def load(fn):
    d=open(fn,'rb').read(); h=struct.unpack_from('<12I',d,0)
    nl=h[4]; cs=h[5]; ds=h[6]; ss=h[7]
    lab=list(struct.unpack_from('<%dI'%nl,d,0x30))
    co=0x30+nl*4; code=d[co:co+cs]; data=d[co+cs:co+cs+ds]; strs=d[co+cs+ds:co+cs+ds+ss]
    return h,lab,code,data,strs
def sz(strs,o):
    e=strs.find(b'\0',o); return strs[o:e].decode('cp932','replace')
def dis(fn,start=0,end=None,out=sys.stdout):
    h,lab,c,data,strs=load(fn)
    labs={}
    for i,a in enumerate(lab): labs.setdefault(a,[]).append(i)
    p=start; end=end or len(c)
    while p<end:
        op=u16(c,p); pre=('L%s: '%','.join(map(str,labs[p]))) if p in labs else ''
        if op in (0x200,0x201,0x202):
            q,e=expr(c,p+2,op&3); s='expr%x %s'%(op&3,' '.join(e))
        elif op in SZ:
            q=p+SZ[op]
            if op in (0x400,0x401,0x402): s='%s %x'%({0x400:'jmp',0x401:'jz',0x402:'jnz'}[op],u32(c,p+2))
            elif op==0x403: s='case %d -> %x'%(u32(c,p+2),u32(c,p+6))
            elif op==0x405: s='jmpL %d'%u16(c,p+2)
            elif op==0x407: s='jmp7 %x %x'%(u32(c,p+2),u32(c,p+6))
            elif op==0x410: s='callL %d'%u16(c,p+2)
            elif op==0x411: s='ret %d'%u16(c,p+2)
            elif op==0x412: s='pop %d'%u16(c,p+2)
            elif op==0x440: s='push %d'%struct.unpack_from('<i',c,p+4)[0]
            elif op==0x442: s='pushf %g'%struct.unpack_from('<f',c,p+4)[0]
            else: s='op%x'%op
        elif 0x2000<=op<0x2800:
            q=p+2; s='CMD %x'%(op&0x7ff)
        else:
            q=p+2; s='?? %04x'%op
        print('%06x %s%s'%(p,pre,s),file=out); p=q
if __name__=='__main__':
    dis(sys.argv[1])
