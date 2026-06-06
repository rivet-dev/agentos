#include <wchar.h>
#ifdef wcpcpy
#undef wcpcpy
#endif
wchar_t *(*foo)(wchar_t *restrict, const wchar_t *restrict) = wcpcpy;
int main(void) { return 0; }
