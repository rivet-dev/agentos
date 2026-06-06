#include <wchar.h>
#ifdef wcstoull
#undef wcstoull
#endif
unsigned long long (*foo)(const wchar_t *restrict, wchar_t **restrict, int) = wcstoull;
int main(void) { return 0; }
