#include <wchar.h>
#ifdef wcsncmp
#undef wcsncmp
#endif
int (*foo)(const wchar_t *, const wchar_t *, size_t) = wcsncmp;
int main(void) { return 0; }
