#include <wchar.h>
#ifdef wcscmp
#undef wcscmp
#endif
int (*foo)(const wchar_t *, const wchar_t *) = wcscmp;
int main(void) { return 0; }
