#include <wchar.h>
#ifdef wcpncpy
#undef wcpncpy
#endif
wchar_t *(*foo)(wchar_t *restrict, const wchar_t *restrict, size_t) = wcpncpy;
int main(void) { return 0; }
