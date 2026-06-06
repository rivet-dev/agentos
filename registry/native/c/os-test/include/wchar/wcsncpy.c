#include <wchar.h>
#ifdef wcsncpy
#undef wcsncpy
#endif
wchar_t *(*foo)(wchar_t *restrict, const wchar_t *restrict, size_t) = wcsncpy;
int main(void) { return 0; }
