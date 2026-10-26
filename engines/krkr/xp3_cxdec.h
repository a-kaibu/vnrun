// Portable cxdec evaluator. No plugin code is executed and no x86 code is generated.
// Algorithm adapted from GARbro's KiriKiriCx.cs (Copyright (C) 2014-2017 morkt).
// MIT: Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies
// of the Software, and to permit persons to whom the Software is furnished to do
// so, subject to the following conditions: The above copyright notice and this
// permission notice shall be included in all copies or substantial portions of
// the Software. THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO
// EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES
// OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE,
// ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.
#pragma once
#include <array>
#include <cstddef>
#include <cstdint>
#include <vector>

namespace vnrun {
struct CxScheme {
    std::uint32_t mask, offset;
    std::array<unsigned, 3> prolog;
    std::array<unsigned, 6> odd;
    std::array<unsigned, 8> even;
};
constexpr CxScheme CxLavender{0x1cf, 0x635, {{1,0,2}}, {{5,1,2,3,4,0}}, {{3,5,4,0,1,2,7,6}}};
constexpr CxScheme CxYuyukana{0x1c9, 0x1f3, {{2,0,1}}, {{1,2,0,4,3,5}}, {{5,4,1,0,3,2,6,7}}};

// Newer plugins serialize selector values for each operation, rather than
// selector-to-operation lookup arrays. Reject malformed permutations first.
inline bool CxFromParams(const std::uint8_t *p, CxScheme &s) {
    if(p[17]!=0) return false; // classic LCG only
    const unsigned evenMap[8]={0,2,3,1,5,6,7,4};
    const unsigned oddMap[6]={2,5,3,4,1,0};
    unsigned seen=0;
    for(unsigned i=0;i<8;++i) {
        if(p[i]>=8 || (seen&(1u<<p[i]))) return false;
        seen|=1u<<p[i];s.even[p[i]]=evenMap[i];
    }
    seen=0;
    for(unsigned i=0;i<6;++i) {
        if(p[i+8]>=6 || (seen&(1u<<p[i+8]))) return false;
        seen|=1u<<p[i+8];s.odd[p[i+8]]=oddMap[i];
    }
    seen=0;
    for(unsigned i=0;i<3;++i) {
        if(p[i+14]>=3 || (seen&(1u<<p[i+14]))) return false;
        seen|=1u<<p[i+14];s.prolog[p[i+14]]=i;
    }
    s.mask=p[18]|(std::uint32_t(p[19])<<8);
    s.offset=p[20]|(std::uint32_t(p[21])<<8);
    return true;
}

class Cxdec {
    enum Op { Arg, Imm, Table, Push, Pop, Save, Not, Dec, Neg, Inc,
              Swap, XorImm, AddImm, SubImm, Shr, Shl, Add, ReverseSub, Mul, Sub };
    struct Ins { Op op; std::uint32_t value; };
    struct Program { std::vector<Ins> code; };
    CxScheme scheme;
    std::array<std::uint32_t,1024> table;
    std::array<Program,128> programs;
    std::uint32_t seed = 0;
    unsigned length = 0;
    Program *current = nullptr;
    std::uint32_t random() {
        auto old = seed;
        seed = 1103515245u * seed + 12345u;
        return seed ^ (old << 16) ^ (old >> 16);
    }
    bool room(unsigned n) {
        if(length + n > 128) return false;
        length += n; return true;
    }
    bool emit(Op op, unsigned n, std::uint32_t value = 0) {
        if(!room(n)) return false;
        current->code.push_back({op,value}); return true;
    }
    bool immediate(Op op) {
        // The opcode must fit before consuming the random immediate, which must
        // still be consumed if its four bytes subsequently exceed the limit.
        if(!room(1)) return false;
        auto v = random();
        return emit(op,4,v);
    }
    bool prolog() {
        switch(scheme.prolog[random()%3]) {
        case 0: return immediate(Imm);
        case 1: return emit(Arg,2);
        default:
            if(!room(5) || !room(2)) return false;
            return emit(Imm,4,table[random()&1023]);
        }
    }
    bool even() {
        switch(scheme.even[random()&7]) {
        case 0: return emit(Not,2);
        case 1: return emit(Dec,1);
        case 2: return emit(Neg,2);
        case 3: return emit(Inc,1);
        case 4: return room(5) && room(1) && room(4) && emit(Table,3);
        case 5:
            return room(1) && room(2) && room(2) && room(4) && room(1) &&
                   room(4) && room(2) && room(2) && room(2) && emit(Swap,1);
        case 6: return immediate(XorImm);
        default: {
            auto op = (random()&1) ? AddImm : SubImm;
            return immediate(op);
        }
        }
    }
    bool odd() {
        switch(scheme.odd[random()%6]) {
        case 0: return room(1) && room(2) && room(3) && room(2) && emit(Shr,1);
        case 1: return room(1) && room(2) && room(3) && room(2) && emit(Shl,1);
        case 2: return emit(Add,2);
        case 3: return room(2) && emit(ReverseSub,2);
        case 4: return emit(Mul,3);
        default: return emit(Sub,2);
        }
    }
    bool branch(unsigned stage) { return (random()&1) ? body(stage) : body2(stage); }
    bool body(unsigned stage) {
        if(stage==1) return prolog();
        return emit(Push,1) && branch(stage-1) && emit(Save,2) &&
               branch(stage-1) && odd() && emit(Pop,1);
    }
    bool body2(unsigned stage) {
        if(stage==1) return prolog();
        return branch(stage-1) && even();
    }
    std::uint32_t execute(std::uint32_t key, std::uint32_t argument) const {
        std::uint32_t a=0,b=0;
        std::array<std::uint32_t,16> stack{};
        unsigned sp=0;
        for(const auto &i:programs[key&127].code) {
            switch(i.op) {
            case Arg:a=argument;break;
            case Imm:a=i.value;break;
            case Table:a=table[a&1023];break;
            case Push:stack[sp++]=b;break;
            case Pop:b=stack[--sp];break;
            case Save:b=a;break;
            case Not:a=~a;break;
            case Dec:--a;break;
            case Neg:a=0u-a;break;
            case Inc:++a;break;
            case Swap:a=((a&0x55555555u)<<1)|((a&0xaaaaaaaau)>>1);break;
            case XorImm:a^=i.value;break;
            case AddImm:a+=i.value;break;
            case SubImm:a-=i.value;break;
            case Shr:a>>=b&15;break;
            case Shl:a<<=b&15;break;
            case Add:a+=b;break;
            case ReverseSub:a=b-a;break;
            case Mul:a*=b;break;
            case Sub:a-=b;break;
            }
        }
        return a;
    }
    void decode(std::uint32_t key,std::uint64_t off,std::uint8_t *p,std::size_t n) const {
        auto a=execute(key,key>>7), b=execute(key,~(key>>7));
        std::uint32_t p1=b>>16,p2=b&65535;
        if(p1==p2) ++p2;
        auto x=static_cast<std::uint8_t>(a); if(!x) x=1;
        for(std::size_t i=0;i<n;++i) p[i]^=x;
        if(p1>=off && p1-off<n) p[p1-off]^=static_cast<std::uint8_t>(a>>8);
        if(p2>=off && p2-off<n) p[p2-off]^=static_cast<std::uint8_t>(a>>16);
    }
public:
    // The 4096-byte control block comes from the game's own TPM, starting with
    // " Encryption control block". Values are little endian, without inversion.
    Cxdec(CxScheme s,const std::uint8_t *block):scheme(s) {
        for(unsigned i=0;i<1024;++i) {
            auto p=block+4*i;
            table[i]=std::uint32_t(p[0])|(std::uint32_t(p[1])<<8)|
                     (std::uint32_t(p[2])<<16)|(std::uint32_t(p[3])<<24);
        }
        for(unsigned i=0;i<128;++i) {
            current=&programs[i];seed=i;
            for(unsigned stage=5;stage>0;--stage) {
                length=0;current->code.clear();
                if(room(5) && room(4) && body(stage) && room(5) && room(1)) break;
            }
        }
        current=nullptr;
    }
    void decrypt(std::uint32_t hash,std::uint64_t off,std::uint8_t *p,std::size_t n) const {
        auto split=(hash&scheme.mask)+scheme.offset;
        if(off<split) {
            auto count=static_cast<std::size_t>(split-off);
            if(count>n) count=n;
            decode(hash,off,p,count);off+=count;p+=count;n-=count;
        }
        if(n) decode(hash^(hash>>16),off,p,n);
    }
};
} // namespace vnrun
