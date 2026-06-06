#include <wchar.h>
#ifdef wmemcmp
#undef wmemcmp
#endif
int (*foo)(const wchar_t *, const wchar_t *, size_t) = wmemcmp;
int main(void) { return 0; }
