#include <wchar.h>
#ifdef mbrlen
#undef mbrlen
#endif
size_t (*foo)(const char *restrict, size_t, mbstate_t *restrict) = mbrlen;
int main(void) { return 0; }
