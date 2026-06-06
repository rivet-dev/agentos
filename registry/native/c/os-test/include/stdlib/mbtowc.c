#include <stdlib.h>
#ifdef mbtowc
#undef mbtowc
#endif
int (*foo)(wchar_t *restrict, const char *restrict, size_t) = mbtowc;
int main(void) { return 0; }
