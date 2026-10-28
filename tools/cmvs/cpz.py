# CMVS CPZ5 reader (Purple software), a prototype for vnrun.
# The archive and PS2A steps follow GARbro's ArcCPZ.cs (MIT, (C) morkt); the
# MD5 variant and secret here are the ones cmvs32.exe of 未来ノスタルジア web
# 体験版 v2 uses (its 4th CPZ5 reader class), which GARbro does not list.
# usage: python3 -I cpz.py X.cpz            list entries
#        python3 -I cpz.py X.cpz NAME > out  extract (PS2A is unpacked)
import struct, sys, hashlib, math
M=0xffffffff
S=[[7,12,17,22],[5,9,14,20],[4,11,16,23],[6,10,15,21]]
K=[int(abs(math.sin(i+1))*2**32)&M for i in range(64)]
def transform2(state,buf):
    a,b,c,d=state
    for i in range(64):
        if i<16: f=(b&c)|((~b&M)&d); g=i
        elif i<32: f=(d&b)|((~d&M)&c); g=(5*i+1)&15
        elif i<48: f=b^c^d; g=(3*i+5)&15
        else: f=c^(b|(~d&M)); g=(7*i)&15
        t=d; d=c; c=b; b=(b+rotl((a+f+K[i]+buf[g])&M,S[i>>4][i&3]))&M; a=t
    return [(state[0]+a)&M,(state[1]+b)&M,(state[2]+c)&M,(state[3]+d)&M]
V={'Sukima':([0x64FE9A8C,0xC2F93EA8,0xEF81BA5D,0x92C8A72F],lambda s:[s[1]^0x49876325,(s[2]+0x54F46D7C)&M,s[3]^0xAC7958B7,(s[0]+0x1C0638BD)&M])}
def md5_compute(variant,data):
    init,res=V[variant]
    buf=list(data)+[0x80]+[0]*9+[0x80,0]
    return res(transform2(list(init),buf))
def rotr(x,n): x&=M; return ((x>>n)|(x<<(32-n)))&M
def rotl(x,n): x&=M; return ((x<<n)|(x>>(32-n)))&M
SECRET=[0xCD90F089,0xE982B782,0xA282AB88,0xCD82718E,0x52838A83,0xA882AA82,0x7592648E,0xB582AB82,0xE182BF82,0xDC82A282,0x4281B782,0x62838183,0xA9824981,0xA282ED82,0xC682A282,0xBE8CA982,0xC482C182,0x968BE082,0xC482B582,0xB082A082,0xA282C882,0xBE82F182,0xE782A982,0x49819F82]
FACTOR=0x1A743125; ENTRY_INIT=0x2547A39E; ENTRY_TAIL=0xBC; ENTRY_SUB=0x5C29E87B; ENTRY_POS=9
IDX_SEED=0x2A65CB4E; IDX_ADD=0x784C5962; IDX_SUB=0x79; DIRADD=[0,0x00112233,0,0x34258765]
def u32(b,o): return struct.unpack_from('<I',b,o)[0]
class Table:
    def __init__(s,key,summ): s.init(key,summ)
    def init(s,key,summ):
        t=list(range(256))
        for i in range(256):
            i0=(key>>16)&0xff; i1=key&0xff; t[i0],t[i1]=t[i1],t[i0]
            i0=(key>>8)&0xff; i1=(key>>24)&0xff; t[i0],t[i1]=t[i1],t[i0]
            key=(summ+FACTOR*rotr(key,2))&M
        s.t=t
    def decode(s,d,off,n,k):
        t=s.t
        for i in range(off,off+n): d[i]=t[k^d[i]]
def words(d,off,n): return list(struct.unpack_from('<%dI'%n,d,off))
def put(d,off,ws): struct.pack_into('<%dI'%len(ws),d,off,*ws)
class Cpz:
    def __init__(s,path,variant='Sukima'):
        s.f=open(path,'rb'); h=s.f.read(0x40)
        assert h[:4]==b'CPZ5', h[:4]
        s.dircount=(0xFE3A53D9 ^ u32(h,4))&M
        s.dirsize=0x37F298E7^u32(h,8); s.filesize=0x7A6F3A2C^u32(h,12)
        s.master=0xAE7D39BF^u32(h,0x30); s.enc=(0xFB73A955^u32(h,0x34))!=0
        md=[0x43DE7C19^u32(h,0x20),0xCC65F415^u32(h,0x24),0xD016A93C^u32(h,0x28),0x97A3BA9A^u32(h,0x2C)]
        idx=bytearray(s.f.read(s.dirsize+s.filesize))
        assert hashlib.md5(idx).digest()==h[0x10:0x20], 'index md5'
        md=md5_compute(variant,md); s.md=md
        key=s.master^0x3795B39A
        sk=[(x-key)&M for x in SECRET]
        shift=(((key>>24)^(key>>16)^(key>>8)^key^0xB)&0xF)+7
        n=len(idx)//4; ws=words(idx,0,n); i=5
        for j in range(n):
            ws[j]=(rotr(((sk[i]^ws[j])+IDX_ADD)&M,shift)+0x01010101)&M; i=(i+1)%24
        put(idx,0,ws)
        for k in range(len(idx)&3,0,-1):
            p=n*4+(len(idx)&3)-k
            idx[p]=((idx[p]^((sk[i]>>(k*4))&0xff))-IDX_SUB)&0xff; i=(i+1)%24
        tab=Table(s.master,md[1]); tab.decode(idx,0,s.dirsize,0x3A)
        key=[md[0]^((s.master+0x76A3BF29)&M), md[1]^s.master, md[2]^((s.master+0x10000000)&M), md[3]^s.master]
        seed=0x76548AEF; n=s.dirsize//4; ws=words(idx,0,n)
        for j in range(n):
            ws[j]=(rotl(((ws[j]^key[j&3])-0x4A91C262)&M,3)-seed)&M; seed=(seed+0x10FB562A)&M
        put(idx,0,ws)
        for k in range(s.dirsize&3):
            p=n*4+k; idx[p]=((idx[p]^((key[(n+k)&3]>>6)&0xff))+0x37)&0xff
        tab.init(s.master,md[2])
        base=0x40+len(idx)
        s.entries={}; do=0
        for di in range(s.dircount):
            dsz,fcount,eoff,dkey=struct.unpack_from('<4I',idx,do)
            if dsz>0x1000 or fcount>0x10000: raise ValueError("bad dir")
            dname=idx[do+0x10:do+dsz].split(b"\0")[0].decode("cp932","replace")
            nxt=s.filesize if di+1==s.dircount else u32(idx,do+dsz+8)
            size=nxt-eoff; cur=s.dirsize+eoff
            tab.decode(idx,cur,size,0x7E)
            key=[md[j]^((dkey+DIRADD[j])&M) for j in range(4)]
            seed=IDX_SEED; n=size//4; ws=words(idx,cur,n)
            for j in range(n):
                ws[j]=(rotl(((ws[j]^key[j&3])-seed)&M,2)+0x37A19E8B)&M; seed=(seed-0x139FA9B)&M
            put(idx,cur,ws)
            for k in range(size&3):
                p=cur+n*4+k; idx[p]=((idx[p]^((key[(n+k)&3]>>4)&0xff))+5)&0xff
            for j in range(fcount):
                esz=u32(idx,cur); off=struct.unpack_from('<Q',idx,cur+4)[0]+base; sz=u32(idx,cur+12)
                ek=(u32(idx,cur+0x14)+dkey)&M
                name=idx[cur+0x18:cur+esz].split(b'\0')[0].decode('cp932')
                full=name if dname=='root' else dname+'/'+name
                s.entries[full.lower()]=(full,off,sz,ek)
                cur+=esz
            do+=dsz
        if s.enc: tab.init(md[3],s.master)
        s.tab=tab
    def read(s,name):
        full,off,sz,ek=s.entries[name.lower()]
        s.f.seek(off); d=bytearray(s.f.read(sz))
        if s.enc:
            key=(((s.master^ek)+s.dircount)-ENTRY_SUB)&M
            s.decrypt(d,key)
        return bytes(d)
    def decrypt(s,d,seed):
        md=s.md; t=s.tab.t
        k=md[1]>>2
        kb=bytearray(struct.pack('<16I',*SECRET[:16]))
        for i in range(64): kb[i]=(k^t[kb[i]])&0xff
        sk=[x^seed for x in struct.unpack('<16I',kb)]
        key=ENTRY_INIT; p=ENTRY_POS; n=len(d)//4; ws=words(d,0,n)
        for i in range(n):
            w=ws[i]
            v=(md[key&3]^(((w^sk[(key>>6)&0xf]^(sk[p]>>1))-seed)&M))&M
            ws[i]=v; p=(p+1)&0xf; key=(key+seed+v)&M
        put(d,0,ws)
        for i in range(n*4,len(d)): d[i]=t[d[i]^ENTRY_TAIL]
def unpack_ps2(d):
    d=bytearray(d)
    key=u32(d,12); shift=(key>>20)%5+1; key=((key>>24)+(key>>3))&0xff
    for i in range(0x30,len(d)):
        b=(key^((d[i]-0x7C)&0xff))&0xff; d[i]=((b>>shift)|(b<<(8-shift)))&0xff
    n=u32(d,0x28); out=bytearray(d[:0x30]); fr=bytearray(0x800); fp=0x7DF; src=0x30; ctl=1
    while len(out)<0x30+n and src<len(d):
        if ctl==1: ctl=d[src]|0x100; src+=1
        if ctl&1:
            b=d[src]; src+=1; out.append(b); fr[fp]=b; fp=(fp+1)&0x7ff
        else:
            lo=d[src]; hi=d[src+1]; src+=2; o=lo|((hi&0xe0)<<3); c=(hi&0x1f)+2
            for i in range(c):
                b=fr[(o+i)&0x7ff]; out.append(b); fr[fp]=b; fp=(fp+1)&0x7ff
        ctl>>=1
    return bytes(out)
if __name__=='__main__':
    c=Cpz(sys.argv[1])
    if len(sys.argv)==2:
        for k,(f,o,sz,ek) in c.entries.items(): print(f,sz)
    else:
        d=c.read(sys.argv[2])
        if d[:4]==b'PS2A': d=unpack_ps2(d)
        sys.stdout.buffer.write(d)
