#include <wchar.h>
#ifdef wcsnlen
#undef wcsnlen
#endif
size_t (*foo)(const wchar_t *, size_t) = wcsnlen;
int main(void) { return 0; }
