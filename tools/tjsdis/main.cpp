// tjsdis FILE...: disassemble TJS2 bytecode files
#include "tjsCommHead.h"
#include "tjs.h"
#include "tjsByteCodeLoader.h"
#include "tjsScriptBlock.h"
#include <fstream>
#include <iterator>
using namespace TJS;
static std::string u8(const tjs_char *s) { std::string o; for (; *s; s++) { char32_t c = *s;
  if (c >= 0xd800 && c < 0xdc00 && s[1]) { c = 0x10000 + ((c - 0xd800) << 10) + (s[1] - 0xdc00); s++; }
  if (c < 0x80) o += (char)c; else if (c < 0x800) { o += 0xc0 | c >> 6; o += 0x80 | (c & 63); }
  else if (c < 0x10000) { o += 0xe0 | c >> 12; o += 0x80 | (c >> 6 & 63); o += 0x80 | (c & 63); }
  else { o += 0xf0 | c >> 18; o += 0x80 | (c >> 12 & 63); o += 0x80 | (c >> 6 & 63); o += 0x80 | (c & 63); } } return o; }
struct Out : iTJSConsoleOutput { void ExceptionPrint(const tjs_char *m) { printf("!%s\n", u8(m).c_str()); } void Print(const tjs_char *m) { printf("%s\n", u8(m).c_str()); } } out;
int main(int argc, char **argv) {
  tTJS *tjs = new tTJS(); tjs->SetConsoleOutput(&out);
  for (int i = 1; i < argc; i++) {
    std::ifstream f(argv[i], std::ios::binary); std::vector<char> b((std::istreambuf_iterator<char>(f)), {});
    printf("==== %s\n", argv[i]);
    try { tTJSByteCodeLoader l; tTJSScriptBlock *blk = l.ReadByteCode(tjs, TJS_W("x"), (const tjs_uint8 *)b.data(), b.size()); if (blk) blk->Dump(); }
    catch (eTJS &e) { printf("error %s\n", u8(e.GetMessage().c_str()).c_str()); }
    catch (...) { printf("error\n"); }
  }
}
