#include <wchar.h>
#ifdef wcstok
#undef wcstok
#endif
wchar_t *(*foo)(wchar_t *restrict, const wchar_t *restrict, wchar_t **restrict) = wcstok;
int main(void) { return 0; }
