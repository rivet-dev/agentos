#include <stdlib.h>
#ifdef mbstowcs
#undef mbstowcs
#endif
size_t (*foo)(wchar_t *restrict, const char *restrict, size_t) = mbstowcs;
int main(void) { return 0; }
