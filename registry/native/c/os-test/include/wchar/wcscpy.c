#include <wchar.h>
#ifdef wcscpy
#undef wcscpy
#endif
wchar_t *(*foo)(wchar_t *restrict, const wchar_t *restrict) = wcscpy;
int main(void) { return 0; }
