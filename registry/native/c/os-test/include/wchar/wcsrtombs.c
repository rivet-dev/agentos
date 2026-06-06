#include <wchar.h>
#ifdef wcsrtombs
#undef wcsrtombs
#endif
size_t (*foo)(char *restrict, const wchar_t **restrict, size_t, mbstate_t *restrict) = wcsrtombs;
int main(void) { return 0; }
