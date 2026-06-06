#include <wchar.h>
#ifdef mbsrtowcs
#undef mbsrtowcs
#endif
size_t (*foo)(wchar_t *restrict, const char **restrict, size_t, mbstate_t *restrict) = mbsrtowcs;
int main(void) { return 0; }
