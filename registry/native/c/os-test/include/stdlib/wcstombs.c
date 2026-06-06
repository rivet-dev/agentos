#include <stdlib.h>
#ifdef wcstombs
#undef wcstombs
#endif
size_t (*foo)(char *restrict, const wchar_t *restrict, size_t) = wcstombs;
int main(void) { return 0; }
