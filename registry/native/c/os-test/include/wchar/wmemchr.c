#include <wchar.h>
#ifdef wmemchr
#undef wmemchr
#endif
wchar_t *(*foo)(const wchar_t *, wchar_t, size_t) = wmemchr;
int main(void) { return 0; }
