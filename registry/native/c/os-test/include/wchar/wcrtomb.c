#include <wchar.h>
#ifdef wcrtomb
#undef wcrtomb
#endif
size_t (*foo)(char *restrict, wchar_t, mbstate_t *restrict) = wcrtomb;
int main(void) { return 0; }
