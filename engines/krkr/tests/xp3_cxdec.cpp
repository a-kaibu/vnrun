#include "../xp3_cxdec.h"
#include <cassert>
#include <iostream>
#include <algorithm>

int main() {
    std::array<std::uint8_t,22> params{{0,1,2,3,4,5,6,7,0,1,2,3,4,5,0,1,2,0,0x55,1,0x23,1}};
    vnrun::CxScheme parsed{};
    assert(vnrun::CxFromParams(params.data(),parsed));
    assert(parsed.mask==0x155 && parsed.offset==0x123);
    assert((parsed.even==std::array<unsigned,8>{{0,2,3,1,5,6,7,4}}));
    assert((parsed.odd==std::array<unsigned,6>{{2,5,3,4,1,0}}));
    params[17]=1;assert(!vnrun::CxFromParams(params.data(),parsed));
    params[17]=0;params[1]=0;assert(!vnrun::CxFromParams(params.data(),parsed));
    std::array<std::uint8_t,4096> block{};
    for(unsigned i=0;i<block.size();++i) block[i]=(i*73+(i>>3)*19)&255;
    const vnrun::CxScheme schemes[]={vnrun::CxYuyukana,vnrun::CxLavender};
    // Independent oracle: the original TPM arithmetic generators, executed in an
    // isolated x86 emulator with the synthetic control block above. Covers all
    // 128 programs, complemented arguments, and both sides of the file split.
    const std::uint64_t expected[]={0x5d3c85f555d77c49ull,0x23cb4287df969461ull};
    for(unsigned s=0;s<2;++s) {
        vnrun::Cxdec cx(schemes[s],block.data());
        std::uint64_t sum=14695981039346656037ull;
        for(unsigned i=0;i<128;++i) {
            auto hash=(0x12345u<<7)|i;
            std::vector<std::uint8_t> all(66000),parts(all.size());
            cx.decrypt(hash,0,all.data(),all.size());
            // Deliberately split through the base offset and exceptional bytes.
            for(std::size_t at=0;at<parts.size();at+=137) {
                auto n=std::min<std::size_t>(137,parts.size()-at);
                cx.decrypt(hash,at,parts.data()+at,n);
            }
            assert(all==parts);
            for(auto b:all) sum=(sum^b)*1099511628211ull;
        }
        std::cout << "cxdec " << s << ": " << std::hex << sum << '\n';
        assert(sum==expected[s]);
        std::array<std::uint8_t,64> high{},split{};
        cx.decrypt(0xffffffff,0x100000003ull,high.data(),high.size());
        cx.decrypt(0xffffffff,0x100000003ull,split.data(),17);
        cx.decrypt(0xffffffff,0x100000014ull,split.data()+17,47);
        assert(high==split);
    }
}
