#include <wchar.h>
#ifdef wcscat
#undef wcscat
#endif
wchar_t *(*foo)(wchar_t *restrict, const wchar_t *restrict) = wcscat;
int main(void) { return 0; }
