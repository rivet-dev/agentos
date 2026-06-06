#include <wchar.h>
#ifdef swprintf
#undef swprintf
#endif
int (*foo)(wchar_t *restrict, size_t, const wchar_t *restrict, ...) = swprintf;
int main(void) { return 0; }
