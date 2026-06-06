#include <wchar.h>
#ifdef wcsstr
#undef wcsstr
#endif
wchar_t *(*foo)(const wchar_t *restrict, const wchar_t *restrict) = wcsstr;
int main(void) { return 0; }
