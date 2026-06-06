#include <wchar.h>
#ifdef wcsnrtombs
#undef wcsnrtombs
#endif
size_t (*foo)(char *restrict, const wchar_t **restrict, size_t, size_t, mbstate_t *restrict) = wcsnrtombs;
int main(void) { return 0; }
