#include <string>
#include <cstddef>
#include "oniguruma.h"
bool IsSJISLeadByte(char) { return false; }
size_t SJISToUnicodeString(const char *, char16_t *) { return 0; }
size_t SJISToUnicodeString(const char *, char16_t *, size_t) { return 0; }
char16_t UnicodeToSJIS(char16_t) { return 0; }
size_t UnicodeToSJISString(const char16_t *, char *) { return 0; }
size_t UnicodeToSJISString(const char16_t *, char *, size_t) { return 0; }
bool TVPUtf16ToUtf8(std::string &, const std::u16string &) { return false; }
bool TVPUtf8ToUtf16(std::u16string &, const std::string &) { return false; }
extern "C" {
OnigEncodingType OnigEncodingUTF16_LE; OnigSyntaxType OnigSyntaxPerl;
int onig_end() { return 0; } int onig_error_code_to_str(OnigUChar *, int, ...) { return 0; }
void onig_free(OnigRegex) {} int onig_new(OnigRegex *, const OnigUChar *, const OnigUChar *, OnigOptionType, OnigEncoding, OnigSyntaxType *, OnigErrorInfo *) { return -1; }
void onig_region_clear(OnigRegion *) {} void onig_region_free(OnigRegion *, int) {} OnigRegion *onig_region_new() { return 0; }
int onig_search(OnigRegex, const OnigUChar *, const OnigUChar *, const OnigUChar *, const OnigUChar *, OnigRegion *, OnigOptionType) { return -1; }
}
