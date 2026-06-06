#include <wchar.h>
#ifdef wcslcpy
#undef wcslcpy
#endif
size_t (*foo)(wchar_t *restrict, const wchar_t *restrict, size_t) = wcslcpy;
int main(void) { return 0; }
