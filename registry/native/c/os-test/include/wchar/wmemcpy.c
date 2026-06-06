#include <wchar.h>
#ifdef wmemcpy
#undef wmemcpy
#endif
wchar_t *(*foo)(wchar_t *restrict, const wchar_t *restrict, size_t) = wmemcpy;
int main(void) { return 0; }
