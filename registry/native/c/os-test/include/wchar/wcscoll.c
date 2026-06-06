#include <wchar.h>
#ifdef wcscoll
#undef wcscoll
#endif
int (*foo)(const wchar_t *, const wchar_t *) = wcscoll;
int main(void) { return 0; }
