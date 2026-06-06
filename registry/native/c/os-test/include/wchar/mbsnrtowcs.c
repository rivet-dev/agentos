#include <wchar.h>
#ifdef mbsnrtowcs
#undef mbsnrtowcs
#endif
size_t (*foo)(wchar_t *restrict, const char **restrict, size_t, size_t, mbstate_t *restrict) = mbsnrtowcs;
int main(void) { return 0; }
