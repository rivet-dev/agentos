#include <wchar.h>
#ifdef wcsncasecmp_l
#undef wcsncasecmp_l
#endif
int (*foo)(const wchar_t *, const wchar_t *, size_t, locale_t) = wcsncasecmp_l;
int main(void) { return 0; }
