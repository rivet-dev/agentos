#include <wchar.h>
#ifdef wcsncasecmp
#undef wcsncasecmp
#endif
int (*foo)(const wchar_t *, const wchar_t *, size_t) = wcsncasecmp;
int main(void) { return 0; }
