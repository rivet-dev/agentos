#include <wchar.h>
#ifdef wcsftime
#undef wcsftime
#endif
size_t (*foo)(wchar_t *restrict, size_t, const wchar_t *restrict, const struct tm *restrict) = wcsftime;
int main(void) { return 0; }
