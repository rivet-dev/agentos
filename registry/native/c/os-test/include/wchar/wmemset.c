#include <wchar.h>
#ifdef wmemset
#undef wmemset
#endif
wchar_t *(*foo)(wchar_t *, wchar_t, size_t) = wmemset;
int main(void) { return 0; }
