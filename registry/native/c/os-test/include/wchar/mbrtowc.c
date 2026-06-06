#include <wchar.h>
#ifdef mbrtowc
#undef mbrtowc
#endif
size_t (*foo)(wchar_t *restrict, const char *restrict, size_t, mbstate_t *restrict) = mbrtowc;
int main(void) { return 0; }
