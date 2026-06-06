#include <wchar.h>
#ifdef wcsncat
#undef wcsncat
#endif
wchar_t *(*foo)(wchar_t *restrict, const wchar_t *restrict, size_t) = wcsncat;
int main(void) { return 0; }
