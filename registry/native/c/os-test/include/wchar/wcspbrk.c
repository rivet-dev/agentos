#include <wchar.h>
#ifdef wcspbrk
#undef wcspbrk
#endif
wchar_t *(*foo)(const wchar_t *, const wchar_t *) = wcspbrk;
int main(void) { return 0; }
