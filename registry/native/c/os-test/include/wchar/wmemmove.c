#include <wchar.h>
#ifdef wmemmove
#undef wmemmove
#endif
wchar_t *(*foo)(wchar_t *, const wchar_t *, size_t) = wmemmove;
int main(void) { return 0; }
